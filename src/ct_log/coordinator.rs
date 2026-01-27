// src/ct_log/coordinator.rs
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use super::health::LogHealthTracker;
use super::monitor::{LogMonitor, LogMonitorConfig};
use crate::database::DatabaseBackend;
use crate::dedupe::Dedupe;
use crate::filter::RootDomainFilter;
use crate::output::OutputManager;
use crate::progress::ProgressIndicator;
use crate::state::StateBackend;
use crate::stats::StatsCollector;
use crate::types::{CertData, MatchResult};
use crate::watchlist::Watchlist;

/// CT Log Coordinator - Manages monitoring of all CT logs
pub struct CtLogCoordinator {
    monitors: Vec<JoinHandle<()>>,
    cert_rx: mpsc::Receiver<CertData>,
    shutdown_tx: watch::Sender<bool>,
    db: Option<Arc<dyn DatabaseBackend>>,
    health_tracker: Arc<LogHealthTracker>,
}

impl CtLogCoordinator {
    /// Create new coordinator for multiple CT logs
    pub fn new(
        log_urls: Vec<String>,
        state_manager: Arc<dyn StateBackend>,
        poll_interval_secs: u64,
        batch_size: u64,
        parse_precerts: bool,
        db: Option<Arc<dyn DatabaseBackend>>,
    ) -> Self {
        let (cert_tx, cert_rx) = mpsc::channel(1000);
        let (shutdown_tx, _) = watch::channel(false);
        let health_tracker = Arc::new(LogHealthTracker::default());

        let config = LogMonitorConfig {
            poll_interval_secs,
            batch_size,
            parse_precerts,
        };

        let mut monitors = Vec::new();

        info!("Starting {} CT log monitors", log_urls.len());

        // Spawn monitor for each log
        for log_url in log_urls {
            let log_monitor = match LogMonitor::new(
                log_url.clone(),
                Arc::clone(&state_manager),
                Arc::clone(&health_tracker),
                config.clone(),
            ) {
                Ok(monitor) => monitor,
                Err(e) => {
                    error!("Failed to create monitor for {}: {}", log_url, e);
                    continue;
                }
            };

            let cert_tx_clone = cert_tx.clone();
            let shutdown_rx = shutdown_tx.subscribe();

            let handle = tokio::spawn(async move {
                log_monitor.run(cert_tx_clone, shutdown_rx).await;
            });

            monitors.push(handle);
        }

        // Drop original sender so channel closes when all monitors finish
        drop(cert_tx);

        info!("Spawned {} monitor tasks", monitors.len());

        Self {
            monitors,
            cert_rx,
            shutdown_tx,
            db,
            health_tracker,
        }
    }

    /// Run the coordinator - processes certificates from all monitors
    pub async fn run(
        mut self,
        watchlist: Arc<tokio::sync::Mutex<Watchlist>>,
        output_manager: OutputManager,
        dedupe: Dedupe,
        stats: StatsCollector,
        progress: ProgressIndicator,
        root_filter: Option<RootDomainFilter>,
    ) {
        info!("CT Log Coordinator running");

        // Spawn background task for periodic health logging
        let health_tracker_clone = Arc::clone(&self.health_tracker);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // Every 5 minutes
            loop {
                interval.tick().await;
                health_tracker_clone.log_summary().await;
            }
        });

        // Process certificates from channel
        while let Some(cert_data) = self.cert_rx.recv().await {
            stats.increment_processed();

            // Process through existing handler chain
            self.handle_cert_entry(
                &cert_data,
                &watchlist,
                &output_manager,
                &dedupe,
                &stats,
                &progress,
                &root_filter,
            )
            .await;
        }

        info!("Certificate channel closed, coordinator shutting down");

        // Wait for all monitors to finish
        for handle in self.monitors {
            if let Err(e) = handle.await {
                error!("Monitor task failed: {}", e);
            }
        }

        info!("All monitor tasks stopped");
    }

    /// Handle a single certificate entry (same logic as certstream.rs)
    async fn handle_cert_entry(
        &self,
        data: &CertData,
        watchlist: &Arc<tokio::sync::Mutex<Watchlist>>,
        output_manager: &OutputManager,
        dedupe: &Dedupe,
        stats: &StatsCollector,
        progress: &ProgressIndicator,
        root_filter: &Option<RootDomainFilter>,
    ) {
        // Check dedupe first
        if !dedupe.should_emit(data).await {
            return;
        }

        let domains = match &data.all_domains {
            Some(d) if !d.is_empty() => d,
            _ => return,
        };

        // Lock watchlist once for all domains
        let watchlist_guard = watchlist.lock().await;

        for d in domains {
            if watchlist_guard.matches_domain(d) {
                // Apply root domain filter if specified
                if let Some(filter) = root_filter {
                    if !filter.should_emit(d) {
                        continue;
                    }
                }

                stats.increment_matches();

                let program = watchlist_guard.program_for_domain(d);
                let program_name = program.as_ref().map(|p| p.name.clone());
                let platform = program.as_ref().and_then(|p| p.platform.clone());

                // Create match result
                let result = MatchResult::from_cert_data(
                    d.to_string(),
                    data,
                    program_name,
                    platform,
                );

                // Emit to all output handlers
                // Suspend progress bar temporarily for clean output
                progress.suspend(|| {});

                if let Err(e) = output_manager.emit(&result).await {
                    warn!("Output error: {:?}", e);
                }

                // Save to database if enabled
                if let Some(ref db) = self.db {
                    if let Err(e) = db.save_match(&result).await {
                        warn!("Failed to save match to database: {:?}", e);
                    }
                }

                break;  // Only emit first match per certificate
            }
        }
    }

    /// Signal shutdown to all monitors
    pub async fn shutdown(&self) {
        info!("Signaling shutdown to all monitors");
        let _ = self.shutdown_tx.send(true);
    }
}
