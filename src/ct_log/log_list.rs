// src/ct_log/log_list.rs
use anyhow::{Context, Result};
use std::time::Duration;
use tracing::{info, debug};

use super::types::LogListV3;

/// Fetches and filters Google's CT log list
pub struct LogListFetcher {
    http_client: reqwest::Client,
}

impl LogListFetcher {
    pub fn new() -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .gzip(true)
            .build()
            .unwrap();

        Self { http_client }
    }

    /// Fetch CT logs from Google's log list
    /// Returns list of log URLs ready to monitor
    ///
    /// # Arguments
    /// * `list_url` - URL to Google's CT log list (usually v3/all_logs_list.json)
    /// * `include_readonly` - Whether to include readonly logs (frozen but may have recent entries)
    /// * `include_pending` - Whether to include pending logs (like gungnir does)
    /// * `include_all` - Whether to include ALL logs regardless of state (retired, rejected, etc.)
    pub async fn fetch_usable_logs(&self, list_url: &str, include_readonly: bool, include_pending: bool, include_all: bool) -> Result<Vec<String>> {
        info!("Fetching CT log list from {}", list_url);

        let response = self
            .http_client
            .get(list_url)
            .send()
            .await
            .context("Failed to fetch CT log list")?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to fetch log list: HTTP {}",
                response.status()
            );
        }

        let log_list: LogListV3 = response
            .json()
            .await
            .context("Failed to parse log list JSON")?;

        let mut acceptable_logs = Vec::new();

        for operator in &log_list.operators {
            for log in &operator.logs {
                // Determine if this log should be included
                let is_acceptable = if include_all {
                    // Include ALL logs with URLs
                    !log.url.is_empty()
                } else {
                    // Filter by state
                    match &log.state {
                        Some(state) => state.is_acceptable(include_readonly, include_pending),
                        None => false,  // No state means not acceptable
                    }
                };

                if is_acceptable && !log.url.is_empty() {
                    let state_desc = if let Some(state) = &log.state {
                        if state.is_usable() {
                            "usable"
                        } else if state.is_readonly() {
                            "readonly"
                        } else if state.is_pending() {
                            "pending"
                        } else if state.is_retired() {
                            "retired"
                        } else if state.is_rejected() {
                            "rejected"
                        } else {
                            "other"
                        }
                    } else {
                        "no-state"
                    };

                    debug!(
                        "Found {} log: {} ({})",
                        state_desc,
                        log.description,
                        log.url
                    );
                    acceptable_logs.push(log.url.clone());
                }
            }
        }

        info!(
            "Found {} acceptable CT logs (readonly={}, pending={}, all={})",
            acceptable_logs.len(),
            include_readonly,
            include_pending,
            include_all
        );

        Ok(acceptable_logs)
    }

    /// Fetch CT logs and merge with additional custom logs
    /// Returns deduplicated list of log URLs
    ///
    /// # Arguments
    /// * `list_url` - URL to Google's CT log list
    /// * `include_readonly` - Whether to include readonly logs
    /// * `include_pending` - Whether to include pending logs
    /// * `include_all` - Whether to include all logs regardless of state
    /// * `additional_logs` - Additional log URLs to add to the list
    pub async fn fetch_logs_with_additional(
        &self,
        list_url: &str,
        include_readonly: bool,
        include_pending: bool,
        include_all: bool,
        additional_logs: &[String],
    ) -> Result<Vec<String>> {
        // Fetch logs from Google's list
        let mut logs = self.fetch_usable_logs(list_url, include_readonly, include_pending, include_all).await?;

        // Add additional logs
        for log_url in additional_logs {
            if !log_url.is_empty() && !logs.contains(log_url) {
                info!("Adding additional log: {}", log_url);
                logs.push(log_url.clone());
            }
        }

        info!(
            "Total logs after merging: {} ({} from Google list + {} additional)",
            logs.len(),
            logs.len() - additional_logs.len(),
            additional_logs.len()
        );

        Ok(logs)
    }

    /// Fetch all logs regardless of state (for debugging/testing)
    pub async fn fetch_all_logs(&self, list_url: &str) -> Result<Vec<String>> {
        let response = self
            .http_client
            .get(list_url)
            .send()
            .await
            .context("Failed to fetch CT log list")?;

        let log_list: LogListV3 = response
            .json()
            .await
            .context("Failed to parse log list JSON")?;

        let mut all_logs = Vec::new();

        for operator in &log_list.operators {
            for log in &operator.logs {
                if !log.url.is_empty() {
                    all_logs.push(log.url.clone());
                }
            }
        }

        info!("Found {} total CT logs", all_logs.len());

        Ok(all_logs)
    }
}

impl Default for LogListFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]  // Requires internet connection
    async fn test_fetch_usable_logs() {
        let fetcher = LogListFetcher::new();
        let logs = fetcher
            .fetch_usable_logs("https://www.gstatic.com/ct/log_list/v3/all_logs_list.json", false, false, false)
            .await
            .unwrap();

        assert!(!logs.is_empty(), "Should find at least some usable logs");
        println!("Found {} usable logs (readonly=false, pending=false, all=false)", logs.len());

        // Test with readonly logs included
        let logs_with_readonly = fetcher
            .fetch_usable_logs("https://www.gstatic.com/ct/log_list/v3/all_logs_list.json", true, false, false)
            .await
            .unwrap();

        println!("Found {} logs (readonly=true, pending=false, all=false)", logs_with_readonly.len());
        assert!(logs_with_readonly.len() >= logs.len(), "Should find more logs when including readonly");

        // Test with pending logs included
        let logs_with_pending = fetcher
            .fetch_usable_logs("https://www.gstatic.com/ct/log_list/v3/all_logs_list.json", false, true, false)
            .await
            .unwrap();

        println!("Found {} logs (readonly=false, pending=true, all=false)", logs_with_pending.len());

        // Test with ALL logs
        let all_logs = fetcher
            .fetch_usable_logs("https://www.gstatic.com/ct/log_list/v3/all_logs_list.json", false, false, true)
            .await
            .unwrap();

        println!("Found {} logs (readonly=false, pending=false, all=true)", all_logs.len());
        assert!(all_logs.len() >= logs_with_readonly.len(), "Should find most logs when including all");
    }
}
