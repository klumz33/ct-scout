// src/main.rs
use clap::Parser;
use ct_scout::cli::{Cli, OutputFormat};
use ct_scout::config::Config;
use ct_scout::ct_log::{CtLogCoordinator, LogListFetcher};
use ct_scout::database::{DatabaseBackend, PostgresBackend};
use ct_scout::dedupe::Dedupe;
use ct_scout::filter::RootDomainFilter;
use ct_scout::output::{csv, human, json, silent, webhook, OutputManager};
use ct_scout::progress::ProgressIndicator;
use ct_scout::state::StateManager;
use ct_scout::stats::StatsCollector;
use ct_scout::watchlist::Watchlist;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Validate arguments
    cli.validate()?;

    // Load config file
    let mut config = Config::from_file(Path::new(&cli.config))?;

    // Apply CLI overrides
    if let Some(ref url) = cli.webhook_url {
        if let Some(ref mut webhook) = config.webhook {
            webhook.url = url.clone();
        }
    }

    if let Some(ref secret) = cli.webhook_secret {
        if let Some(ref mut webhook) = config.webhook {
            webhook.secret = Some(secret.clone());
        }
    }

    if let Some(timeout) = cli.webhook_timeout {
        if let Some(ref mut webhook) = config.webhook {
            webhook.timeout_secs = Some(timeout);
        }
    }

    // Initialize logging
    let log_level = if cli.verbose {
        "debug"
    } else if cli.quiet {
        "warn"
    } else {
        &config.logging.level
    };

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .init();

    tracing::info!("Starting ct-scout...");

    // Create watchlist
    let watchlist = Watchlist::from_config(&config.watchlist, &config.programs)?;
    tracing::info!(
        "Loaded watchlist: {} domains, {} hosts, {} IPs, {} CIDRs",
        config.watchlist.domains.len(),
        config.watchlist.hosts.len(),
        config.watchlist.ips.len(),
        config.watchlist.cidrs.len()
    );

    // Create dedupe
    let dedupe = if cli.no_dedupe {
        tracing::info!("Deduplication disabled");
        Dedupe::new() // Still create it but won't use it effectively
    } else {
        Dedupe::new()
    };

    // Create stats collector
    let stats = StatsCollector::new();

    // Create progress indicator
    let progress = ProgressIndicator::new(cli.should_show_progress());

    // Load root domain filter if specified
    let root_filter = if let Some(ref path) = cli.root_domains {
        let filter = RootDomainFilter::from_file(Path::new(path))?;
        tracing::info!("Loaded root domain filter: {} domains", filter.count());
        Some(filter)
    } else {
        None
    };

    // Create output manager
    let mut output_manager = OutputManager::new();

    // Add output handlers based on format
    match cli.output_format() {
        OutputFormat::Human => {
            if let Some(ref path) = cli.output {
                let file = std::fs::File::create(path)?;
                output_manager.add_handler(Arc::new(human::HumanOutput::to_file(file)));
                tracing::info!("Writing human-readable output to: {}", path);
            } else {
                output_manager.add_handler(Arc::new(human::HumanOutput::new()));
            }
        }
        OutputFormat::Json => {
            if let Some(ref path) = cli.output {
                let file = std::fs::File::create(path)?;
                output_manager.add_handler(Arc::new(json::JsonOutput::to_file(file)));
                tracing::info!("Writing JSON output to: {}", path);
            } else {
                output_manager.add_handler(Arc::new(json::JsonOutput::new()));
            }
        }
        OutputFormat::Csv => {
            if let Some(ref path) = cli.output {
                let file = std::fs::File::create(path)?;
                output_manager.add_handler(Arc::new(csv::CsvOutput::to_file(file)));
                tracing::info!("Writing CSV output to: {}", path);
            } else {
                output_manager.add_handler(Arc::new(csv::CsvOutput::new()));
            }
        }
        OutputFormat::Silent => {
            output_manager.add_handler(Arc::new(silent::SilentOutput));
            tracing::info!("Silent mode: no stdout output");
        }
    }

    // Add webhook handler if configured and not disabled
    if !cli.no_webhook {
        if let Some(ref webhook_config) = config.webhook {
            output_manager.add_handler(Arc::new(webhook::WebhookOutput::new(
                webhook_config.clone(),
            )));
            tracing::info!("Webhook enabled: {}", webhook_config.url);
        } else {
            tracing::debug!("No webhook configured");
        }
    } else {
        tracing::info!("Webhooks disabled");
    }

    // Start stats display background task if requested
    if cli.stats {
        let stats_clone = stats.clone();
        let progress_clone = progress.clone();
        let interval = cli.stats_interval;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                let msg = stats_clone.format_stats();
                progress_clone.set_message(msg);
            }
        });
    }

    // Initialize database if enabled
    let db: Option<Arc<dyn DatabaseBackend>> = if config.database.enabled {
        tracing::info!("Database enabled, connecting to PostgreSQL...");
        let postgres = PostgresBackend::new(
            &config.database.url,
            config.database.max_connections,
        ).await?;

        // Run migrations
        postgres.migrate().await?;
        tracing::info!("Database initialized and migrated successfully");

        let db_arc: Arc<dyn DatabaseBackend> = Arc::new(postgres);

        // Database-backed state manager can be created when needed
        // For now, we use TOML state + DB for match storage
        tracing::info!("Database ready for match storage");

        Some(db_arc)
    } else {
        tracing::info!("Database disabled, using TOML state file");
        None
    };

    // Create state manager (TOML-based or DB-backed)
    let state_manager: Arc<StateManager> = if config.database.enabled && db.is_some() {
        // For DB mode, we need a different approach
        // We'll create a TOML state manager as fallback for now
        // TODO: Refactor to use trait-based state manager
        Arc::new(
            StateManager::new(PathBuf::from(&config.ct_logs.state_file))
                .await?
        )
    } else {
        Arc::new(
            StateManager::new(PathBuf::from(&config.ct_logs.state_file))
                .await?
        )
    };
    tracing::info!("State manager initialized");

    // Fetch log URLs
    let log_urls = if let Some(ref custom) = config.ct_logs.custom_logs {
        // Backward compatibility: custom_logs replaces Google's list
        tracing::info!("Using {} custom CT logs (replacing Google's list)", custom.len());
        custom.clone()
    } else {
        let fetcher = LogListFetcher::new();

        // Fetch logs from Google's list, optionally merging with additional_logs
        let logs = if let Some(ref additional) = config.ct_logs.additional_logs {
            // Merge additional logs with Google's list
            fetcher.fetch_logs_with_additional(
                &config.ct_logs.log_list_url,
                config.ct_logs.include_readonly_logs,
                config.ct_logs.include_pending,
                config.ct_logs.include_all_logs,
                additional
            ).await?
        } else {
            // Just fetch from Google's list
            fetcher.fetch_usable_logs(
                &config.ct_logs.log_list_url,
                config.ct_logs.include_readonly_logs,
                config.ct_logs.include_pending,
                config.ct_logs.include_all_logs
            ).await?
        };

        tracing::info!("Fetched {} CT logs from list", logs.len());

        // Limit to max_concurrent_logs
        let limited_logs: Vec<String> = logs.into_iter()
            .take(config.ct_logs.max_concurrent_logs)
            .collect();
        tracing::info!("Monitoring {} CT logs (limited by max_concurrent_logs)", limited_logs.len());
        limited_logs
    };

    // Create coordinator
    let coordinator = CtLogCoordinator::new(
        log_urls,
        state_manager.clone(),
        config.ct_logs.poll_interval_secs,
        config.ct_logs.batch_size,
        config.ct_logs.parse_precerts,
        db,
    );

    // Run monitoring
    tracing::info!("Starting CT log monitoring...");
    coordinator.run(
        watchlist,
        output_manager,
        dedupe,
        stats.clone(),
        progress.clone(),
        root_filter,
    ).await;

    // Save final state
    tracing::info!("Saving final state...");
    state_manager.save().await?;

    // Print final stats if enabled
    if cli.stats {
        let snapshot = stats.snapshot();
        println!("\n\n📊 Final Statistics:");
        println!("  Total processed: {}", snapshot.total_processed);
        println!("  Matches found: {}", snapshot.matches_found);
        println!("  Rate: {:.1} msg/min", snapshot.messages_per_minute);
        println!("  Uptime: {}", StatsCollector::format_uptime(snapshot.uptime_secs));
    }

    Ok(())
}
