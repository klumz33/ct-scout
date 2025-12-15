// src/ct_log/client.rs
use anyhow::{Context, Result};
use std::time::Duration;
use tracing::{debug, warn};

use super::types::{GetEntriesResponse, LogEntry, SignedTreeHead};

/// HTTP client for Certificate Transparency log RFC 6962 API
pub struct CtLogClient {
    base_url: String,
    http_client: reqwest::Client,
}

impl CtLogClient {
    /// Create a new CT log client
    pub fn new(base_url: String) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .gzip(true)  // Enable compression
            // Don't force HTTP/2 - let reqwest negotiate automatically
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            base_url,
            http_client,
        })
    }

    /// Get Signed Tree Head (current log size and timestamp)
    /// Endpoint: GET {base_url}/ct/v1/get-sth
    pub async fn get_sth(&self) -> Result<SignedTreeHead> {
        let url = format!("{}/ct/v1/get-sth", self.base_url);

        debug!("Fetching STH from {}", url);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch STH")?;

        if !response.status().is_success() {
            anyhow::bail!(
                "STH request failed with status {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            );
        }

        let sth: SignedTreeHead = response
            .json()
            .await
            .context("Failed to parse STH JSON")?;

        debug!(
            "STH received: tree_size={}, timestamp={}",
            sth.tree_size, sth.timestamp
        );

        Ok(sth)
    }

    /// Get entries from CT log
    /// Endpoint: GET {base_url}/ct/v1/get-entries?start={start}&end={end}
    pub async fn get_entries(&self, start: u64, end: u64) -> Result<Vec<LogEntry>> {
        let url = format!(
            "{}/ct/v1/get-entries?start={}&end={}",
            self.base_url, start, end
        );

        debug!("Fetching entries {}-{} from {}", start, end, self.base_url);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch entries")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            // Handle rate limiting specifically
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                warn!("Rate limited by CT log: {}", self.base_url);
                anyhow::bail!("Rate limited (429)");
            }

            anyhow::bail!(
                "Get entries request failed with status {}: {}",
                status,
                body
            );
        }

        let entries_response: GetEntriesResponse = response
            .json()
            .await
            .context("Failed to parse entries JSON")?;

        debug!(
            "Received {} entries from {}",
            entries_response.entries.len(),
            self.base_url
        );

        Ok(entries_response.entries)
    }

    /// Get entries with retry logic and exponential backoff
    pub async fn get_entries_with_retry(
        &self,
        start: u64,
        end: u64,
        max_retries: u32,
    ) -> Result<Vec<LogEntry>> {
        let mut retries = 0;
        let mut backoff = Duration::from_secs(1);

        loop {
            match self.get_entries(start, end).await {
                Ok(entries) => return Ok(entries),
                Err(e) => {
                    retries += 1;

                    if retries >= max_retries {
                        return Err(e.context(format!(
                            "Failed after {} retries",
                            max_retries
                        )));
                    }

                    warn!(
                        "Error fetching entries (attempt {}/{}): {}. Retrying in {:?}",
                        retries, max_retries, e, backoff
                    );

                    tokio::time::sleep(backoff).await;

                    // Exponential backoff with max 60 seconds
                    backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                }
            }
        }
    }

    /// Get STH with retry logic
    pub async fn get_sth_with_retry(&self, max_retries: u32) -> Result<SignedTreeHead> {
        let mut retries = 0;
        let mut backoff = Duration::from_secs(1);

        loop {
            match self.get_sth().await {
                Ok(sth) => return Ok(sth),
                Err(e) => {
                    retries += 1;

                    if retries >= max_retries {
                        return Err(e.context(format!(
                            "Failed after {} retries",
                            max_retries
                        )));
                    }

                    warn!(
                        "Error fetching STH (attempt {}/{}): {}. Retrying in {:?}",
                        retries, max_retries, e, backoff
                    );

                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                }
            }
        }
    }
}
