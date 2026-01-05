// src/main.rs
use clap::Parser;
use ct_scout::cli::{Cli, OutputFormat};
use ct_scout::config::Config;
use ct_scout::ct_log::{CtLogCoordinator, LogListFetcher};
use ct_scout::database::{DatabaseBackend, PostgresBackend};
use ct_scout::dedupe::Dedupe;
use ct_scout::filter::RootDomainFilter;
use ct_scout::output::{self, csv, human, json, silent, webhook, OutputManager};
use ct_scout::platforms::{HackerOneAPI, IntigritiAPI, PlatformAPI, PlatformSyncManager};
use ct_scout::redis_publisher;
use ct_scout::progress::ProgressIndicator;
use ct_scout::state::StateManager;
use ct_scout::stats::StatsCollector;
use ct_scout::watcher::ConfigWatcher;
use ct_scout::watchlist::Watchlist;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
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

    // Start config file watcher if enabled
    // Precedence: CLI flag overrides config
    let watch_config_enabled = if cli.watch_config {
        true
    } else {
        config.watch_config
    };

    if watch_config_enabled {
        let config_path = PathBuf::from(&cli.config);
        let watcher = ConfigWatcher::new(config_path.clone());
        let mut config_rx = watcher.watch()?;

        // Spawn task to handle config reloads
        tokio::spawn(async move {
            while let Some(new_config) = config_rx.recv().await {
                tracing::info!("Config file changed detected! New configuration loaded.");
                tracing::info!("Note: Dynamic config reload not yet implemented. Please restart ct-scout to apply changes.");
                // TODO: Implement dynamic reload of watchlist and settings
                // For now, just log that we detected the change
                let _ = new_config; // Silence unused variable warning
            }
        });
    } else {
        tracing::debug!("Config file watching disabled");
    }

    // Create watchlist wrapped in Arc<Mutex<>> for sharing with background tasks
    let watchlist = Arc::new(Mutex::new(Watchlist::from_config(&config.watchlist, &config.programs)?));
    tracing::info!(
        "Loaded watchlist: {} domains, {} hosts, {} IPs, {} CIDRs",
        config.watchlist.domains.len(),
        config.watchlist.hosts.len(),
        config.watchlist.ips.len(),
        config.watchlist.cidrs.len()
    );

    // Initialize and spawn platform sync manager if configured
    let (platform_shutdown_tx, platform_shutdown_rx) = tokio::sync::watch::channel(false);
    let mut platform_sync_handle = None;

    if config.platforms.hackerone.as_ref().map(|h| h.enabled).unwrap_or(false)
        || config.platforms.intigriti.as_ref().map(|i| i.enabled).unwrap_or(false)
    {
        tracing::info!("Platform API integration enabled, initializing sync manager...");

        let mut platforms: Vec<Box<dyn PlatformAPI>> = Vec::new();

        // Initialize HackerOne if configured
        if let Some(h1_config) = &config.platforms.hackerone {
            if h1_config.enabled {
                tracing::info!("Initializing HackerOne API integration");
                let h1_api = HackerOneAPI::new(
                    h1_config.username.clone(),
                    h1_config.api_token.clone(),
                )?;

                // Test connection
                match h1_api.test_connection().await {
                    Ok(true) => {
                        tracing::info!("HackerOne API connection successful");
                        platforms.push(Box::new(h1_api));
                    }
                    Ok(false) => {
                        tracing::warn!("HackerOne API connection failed (invalid credentials?)");
                    }
                    Err(e) => {
                        tracing::error!("HackerOne API connection error: {:?}", e);
                    }
                }
            }
        }

        // Initialize Intigriti if configured
        if let Some(intigriti_config) = &config.platforms.intigriti {
            if intigriti_config.enabled {
                tracing::info!("Initializing Intigriti API integration");
                let intigriti_api = IntigritiAPI::new(intigriti_config.api_token.clone())?;

                // Test connection
                match intigriti_api.test_connection().await {
                    Ok(true) => {
                        tracing::info!("Intigriti API connection successful");
                        platforms.push(Box::new(intigriti_api));
                    }
                    Ok(false) => {
                        tracing::warn!("Intigriti API connection failed (invalid credentials?)");
                    }
                    Err(e) => {
                        tracing::error!("Intigriti API connection error: {:?}", e);
                    }
                }
            }
        }

        if !platforms.is_empty() {
            // Create platform sync manager
            let sync_manager = PlatformSyncManager::new(
                platforms,
                watchlist.clone(),
                config.platforms.sync_interval_hours,
            );

            // Spawn platform sync manager as background task
            let shutdown_rx_clone = platform_shutdown_rx.clone();
            platform_sync_handle = Some(tokio::spawn(async move {
                sync_manager.run(shutdown_rx_clone).await;
            }));

            tracing::info!(
                "Platform sync manager started (sync interval: {} hours)",
                config.platforms.sync_interval_hours
            );
        }
    }

    // Handle --export-scope flag
    if cli.export_scope {
        tracing::info!("Exporting current scope to TOML format...");
        let watchlist_guard = watchlist.lock().await;
        let toml_output = watchlist_guard.export_to_toml();
        println!("{}", toml_output);
        tracing::info!("Export complete. Exiting.");
        return Ok(());
    }

    // Create dedupe
    // Precedence: CLI flags override config
    let dedupe_enabled = if cli.no_dedupe {
        false
    } else if cli.dedupe {
        true
    } else {
        config.ct_logs.dedupe
    };

    let dedupe = if dedupe_enabled {
        Dedupe::new()
    } else {
        tracing::info!("Deduplication disabled");
        Dedupe::new() // Still create it but won't use it effectively
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

    // Add Redis pub/sub handler if configured
    if config.redis.enabled {
        tracing::info!("Initializing Redis publisher...");
        let redis_config = redis_publisher::RedisConfig {
            url: config.redis.url.clone(),
            token: config.redis.token.clone(),
            channel: config.redis.channel.clone(),
            queue_name: config.redis.queue_name.clone(),
            max_queue_size: config.redis.max_queue_size,
        };

        let redis_pub = Arc::new(redis_publisher::RedisPublisher::new(redis_config));

        // Try to connect to Redis
        match redis_pub.connect().await {
            Ok(_) => {
                output_manager.add_handler(Arc::new(output::redis::RedisOutput::new(
                    redis_pub.clone(),
                )));
                tracing::info!("Redis publisher enabled: channel={}", config.redis.channel);
            }
            Err(e) => {
                tracing::error!("Failed to connect to Redis: {}", e);
                tracing::warn!("Continuing without Redis publishing");
            }
        }
    } else {
        tracing::debug!("Redis publishing disabled");
    }

    // Start stats display background task if requested
    // Precedence: CLI flags override config
    let stats_enabled = if cli.no_stats {
        false
    } else if cli.stats {
        true
    } else {
        config.stats.enabled
    };

    let stats_interval = if cli.stats_interval != 10 {
        // CLI provided non-default value
        cli.stats_interval
    } else {
        config.stats.interval_secs
    };

    if stats_enabled {
        let stats_clone = stats.clone();
        let progress_clone = progress.clone();
        let interval = stats_interval;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                let msg = stats_clone.format_stats();

                // If progress indicator is enabled, use it; otherwise print directly to stderr
                if progress_clone.is_enabled() {
                    progress_clone.set_message(msg);
                } else {
                    eprintln!("{}", msg);
                }
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

    // Create state manager based on configuration
    let state_manager: Arc<dyn ct_scout::state::StateBackend> = match config.ct_logs.state_backend.as_str() {
        "database" => {
            if !config.database.enabled || db.is_none() {
                anyhow::bail!(
                    "state_backend is set to 'database' but database is not enabled. \
                    Either enable database or use state_backend='file'"
                );
            }

            tracing::info!("Using database for CT log state storage");
            let db_state = ct_scout::database::DbStateManager::new(db.clone().unwrap());
            Arc::new(db_state)
        }
        "file" => {
            tracing::info!("Using file-based CT log state storage: {}", config.ct_logs.state_file);
            let file_state = StateManager::new(PathBuf::from(&config.ct_logs.state_file)).await?;
            Arc::new(file_state)
        }
        other => {
            anyhow::bail!(
                "Invalid state_backend '{}'. Must be 'file' or 'database'",
                other
            );
        }
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

    // Shutdown platform sync manager if it was running
    if let Some(handle) = platform_sync_handle {
        tracing::info!("Shutting down platform sync manager...");
        platform_shutdown_tx.send(true).ok();
        handle.await.ok();
    }

    // Save final state
    tracing::info!("Saving final state...");
    state_manager.save().await?;

    // Print final stats if enabled
    if stats_enabled {
        let snapshot = stats.snapshot();
        println!("\n\n📊 Final Statistics:");
        println!("  Total processed: {}", snapshot.total_processed);
        println!("  Matches found: {}", snapshot.matches_found);
        println!("  Rate: {:.1} msg/min", snapshot.messages_per_minute);
        println!("  Uptime: {}", StatsCollector::format_uptime(snapshot.uptime_secs));
    }

    Ok(())
}
