// src/ct_log/monitor.rs
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, warn};

use super::client::CtLogClient;
use super::health::LogHealthTracker;
use crate::cert_parser::CertificateParser;
use crate::state::StateBackend;
use crate::types::CertData;

/// Configuration for single log monitor
#[derive(Debug, Clone)]
pub struct LogMonitorConfig {
    pub poll_interval_secs: u64,
    pub batch_size: u64,
    pub parse_precerts: bool,
}

/// Monitors a single CT log for new entries
pub struct LogMonitor {
    log_url: String,
    client: CtLogClient,
    state_manager: Arc<dyn StateBackend>,
    health_tracker: Arc<LogHealthTracker>,
    config: LogMonitorConfig,
}

impl LogMonitor {
    /// Create new log monitor
    pub fn new(
        log_url: String,
        state_manager: Arc<dyn StateBackend>,
        health_tracker: Arc<LogHealthTracker>,
        config: LogMonitorConfig,
    ) -> Result<Self> {
        let client = CtLogClient::new(log_url.clone())?;

        Ok(Self {
            log_url,
            client,
            state_manager,
            health_tracker,
            config,
        })
    }

    /// Main monitoring loop - continuously polls for new entries
    pub async fn run(
        &self,
        cert_tx: mpsc::Sender<CertData>,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        info!("Starting monitor for {}", self.log_url);

        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);

        loop {
            // Check shutdown signal
            if *shutdown_rx.borrow() {
                info!("Shutting down monitor for {}", self.log_url);
                break;
            }

            // Check if log should be polled (health-based backoff)
            if !self.health_tracker.should_poll(&self.log_url).await {
                debug!("{}: Skipping poll (health-based backoff)", self.log_url);
                tokio::time::sleep(poll_interval).await;
                continue;
            }

            // Poll for new entries
            match self.poll_once(&cert_tx).await {
                Ok(()) => {
                    // Record successful poll
                    self.health_tracker.record_success(&self.log_url).await;
                }
                Err(e) => {
                    // Record failure
                    self.health_tracker
                        .record_failure(&self.log_url, e.to_string())
                        .await;

                    error!(
                        "Error polling {} : {}. Will retry after {:?}",
                        self.log_url, e, poll_interval
                    );
                }
            }

            // Sleep until next poll or shutdown
            tokio::select! {
                _ = tokio::time::sleep(poll_interval) => {},
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Shutting down monitor for {}", self.log_url);
                        break;
                    }
                }
            }
        }

        info!("Monitor for {} stopped", self.log_url);
    }

    /// Poll once for new entries
    async fn poll_once(&self, cert_tx: &mpsc::Sender<CertData>) -> Result<()> {
        // Get current tree size
        let sth = self
            .client
            .get_sth_with_retry(3)
            .await
            .context("Failed to get STH")?;

        let tree_size = sth.tree_size;

        // Get last processed index
        let last_index = self
            .state_manager
            .get_last_index(&self.log_url)
            .await
            .unwrap_or(0);

        // Check if there are new entries
        if last_index >= tree_size {
            debug!(
                "{}: Up to date (last_index={}, tree_size={})",
                self.log_url, last_index, tree_size
            );
            return Ok(());
        }

        // Calculate batch end
        let end_index = std::cmp::min(last_index + self.config.batch_size, tree_size) - 1;

        debug!(
            "{}: Fetching entries {}-{} (tree_size={})",
            self.log_url, last_index, end_index, tree_size
        );

        // Fetch entries
        let entries = self
            .client
            .get_entries_with_retry(last_index, end_index, 3)
            .await
            .context("Failed to get entries")?;

        debug!(
            "{}: Processing {} entries",
            self.log_url,
            entries.len()
        );

        // Process each entry
        for (offset, entry) in entries.iter().enumerate() {
            let entry_index = last_index + offset as u64;

            // Parse certificate and extract full metadata (using both leaf_input and extra_data)
            let parsed_cert = match CertificateParser::parse_log_entry(&entry.leaf_input, &entry.extra_data, self.config.parse_precerts) {
                Ok(cert) => cert,
                Err(e) => {
                    // Only warn if not disabled precert parsing
                    if self.config.parse_precerts || !e.to_string().contains("Precertificate parsing disabled") {
                        warn!(
                            "{}: Failed to parse certificate at index {}: {}",
                            self.log_url, entry_index, e
                        );
                    }
                    continue;  // Skip this entry
                }
            };

            if parsed_cert.domains.is_empty() {
                debug!(
                    "{}: No domains found in certificate at index {}",
                    self.log_url, entry_index
                );
                continue;
            }

            // Create CertData with full certificate metadata
            let cert_data = CertData {
                all_domains: Some(parsed_cert.domains.clone()),
                cert_index: Some(entry_index),
                seen_unix: Some(chrono::Utc::now().timestamp() as f64),
                leaf_cert: Some(crate::types::LeafCert {
                    not_before: parsed_cert.not_before,
                    not_after: parsed_cert.not_after,
                    fingerprint: Some(parsed_cert.fingerprint),
                    issuer: parsed_cert.issuer,
                }),
                is_precert: parsed_cert.is_precert,
                ct_log_url: Some(self.log_url.clone()),
            };

            // Send to processing pipeline
            if let Err(e) = cert_tx.send(cert_data).await {
                warn!(
                    "{}: Failed to send cert_data to processing pipeline: {}",
                    self.log_url, e
                );
                // Channel closed, stop processing
                return Err(anyhow::anyhow!("Processing pipeline closed"));
            }

            // Update state periodically (every entry)
            self.state_manager
                .update_index(&self.log_url, entry_index + 1)
                .await;
        }

        info!(
            "{}: Processed entries {}-{} ({} entries)",
            self.log_url,
            last_index,
            end_index,
            entries.len()
        );

        Ok(())
    }
}
