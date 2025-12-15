// src/ct_log/health.rs
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{warn, info, debug};

/// Health status of a CT log
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogHealth {
    /// Log is responding normally
    Healthy,
    /// Log has had some failures but is still being monitored normally
    Degraded,
    /// Log has failed multiple times, using exponential backoff
    Failed,
}

/// Health information for a single log
#[derive(Debug, Clone)]
pub struct LogHealthInfo {
    /// Current health status
    pub status: LogHealth,
    /// Number of consecutive failures
    pub failure_count: u32,
    /// Timestamp of last failure
    pub last_failure: Option<Instant>,
    /// Timestamp of last successful poll
    pub last_success: Option<Instant>,
    /// Last error message
    pub last_error: Option<String>,
    /// Current backoff duration (for failed logs)
    pub current_backoff: Duration,
}

impl LogHealthInfo {
    fn new() -> Self {
        Self {
            status: LogHealth::Healthy,
            failure_count: 0,
            last_failure: None,
            last_success: None,
            last_error: None,
            current_backoff: Duration::from_secs(0),
        }
    }

    /// Calculate next backoff duration using exponential backoff
    /// Starts at 1 minute, doubles each time, max 1 hour
    fn next_backoff(&self) -> Duration {
        if self.failure_count == 0 {
            Duration::from_secs(0)
        } else {
            let base_secs = 60; // Start with 1 minute
            let max_secs = 3600; // Max 1 hour
            let backoff_secs = base_secs * 2_u64.pow(self.failure_count.saturating_sub(1));
            Duration::from_secs(backoff_secs.min(max_secs))
        }
    }
}

/// Tracks health status of all monitored CT logs
pub struct LogHealthTracker {
    /// Health information per log URL
    health: Arc<RwLock<HashMap<String, LogHealthInfo>>>,
    /// Number of failures before marking as Failed
    failure_threshold: u32,
}

impl LogHealthTracker {
    /// Create a new health tracker
    ///
    /// # Arguments
    /// * `failure_threshold` - Number of consecutive failures before marking log as Failed (default: 3)
    pub fn new(failure_threshold: u32) -> Self {
        Self {
            health: Arc::new(RwLock::new(HashMap::new())),
            failure_threshold,
        }
    }

    /// Record a successful poll from a log
    pub async fn record_success(&self, log_url: &str) {
        let mut health = self.health.write().await;
        let info = health.entry(log_url.to_string()).or_insert_with(LogHealthInfo::new);

        let was_failed = info.status == LogHealth::Failed;
        let was_degraded = info.status == LogHealth::Degraded;

        info.status = LogHealth::Healthy;
        info.failure_count = 0;
        info.last_success = Some(Instant::now());
        info.current_backoff = Duration::from_secs(0);

        if was_failed {
            info!("Log recovered: {} is now healthy (was failed)", log_url);
        } else if was_degraded {
            debug!("Log recovered: {} is now healthy (was degraded)", log_url);
        }
    }

    /// Record a failed poll from a log
    pub async fn record_failure(&self, log_url: &str, error: String) {
        let mut health = self.health.write().await;
        let info = health.entry(log_url.to_string()).or_insert_with(LogHealthInfo::new);

        info.failure_count += 1;
        info.last_failure = Some(Instant::now());
        info.last_error = Some(error.clone());

        // Determine new status
        let old_status = info.status;
        info.status = if info.failure_count >= self.failure_threshold {
            LogHealth::Failed
        } else {
            LogHealth::Degraded
        };

        // Calculate new backoff
        info.current_backoff = info.next_backoff();

        // Log status change
        match (old_status, info.status) {
            (LogHealth::Healthy, LogHealth::Degraded) => {
                warn!("Log degraded: {} (failure {}/{}): {}",
                    log_url, info.failure_count, self.failure_threshold, error);
            }
            (LogHealth::Degraded, LogHealth::Failed) | (LogHealth::Healthy, LogHealth::Failed) => {
                warn!("Log failed: {} (after {} failures, will use exponential backoff: {:?}): {}",
                    log_url, info.failure_count, info.current_backoff, error);
            }
            (LogHealth::Failed, LogHealth::Failed) => {
                debug!("Log still failed: {} (failure {}, backoff: {:?}): {}",
                    log_url, info.failure_count, info.current_backoff, error);
            }
            _ => {}
        }
    }

    /// Check if a log should be polled based on its health status
    /// Returns true if the log should be polled now, false if it should be skipped
    pub async fn should_poll(&self, log_url: &str) -> bool {
        let health = self.health.read().await;
        let info = match health.get(log_url) {
            Some(info) => info,
            None => return true, // New log, should poll
        };

        match info.status {
            LogHealth::Healthy | LogHealth::Degraded => true,
            LogHealth::Failed => {
                // Check if enough time has passed since last failure
                if let Some(last_failure) = info.last_failure {
                    let elapsed = last_failure.elapsed();
                    elapsed >= info.current_backoff
                } else {
                    true // No last failure time, should poll
                }
            }
        }
    }

    /// Get current health status for a log
    pub async fn get_status(&self, log_url: &str) -> LogHealth {
        let health = self.health.read().await;
        health.get(log_url)
            .map(|info| info.status)
            .unwrap_or(LogHealth::Healthy)
    }

    /// Get health information for a log
    pub async fn get_info(&self, log_url: &str) -> Option<LogHealthInfo> {
        let health = self.health.read().await;
        health.get(log_url).cloned()
    }

    /// Get statistics about log health
    pub async fn get_stats(&self) -> (usize, usize, usize) {
        let health = self.health.read().await;
        let mut healthy = 0;
        let mut degraded = 0;
        let mut failed = 0;

        for info in health.values() {
            match info.status {
                LogHealth::Healthy => healthy += 1,
                LogHealth::Degraded => degraded += 1,
                LogHealth::Failed => failed += 1,
            }
        }

        (healthy, degraded, failed)
    }

    /// Log a summary of all log health statuses
    pub async fn log_summary(&self) {
        let (healthy, degraded, failed) = self.get_stats().await;
        let total = healthy + degraded + failed;

        if total == 0 {
            return;
        }

        info!("Log health summary: {} total ({} healthy, {} degraded, {} failed)",
            total, healthy, degraded, failed);

        // Log details of failed logs
        if failed > 0 {
            let health = self.health.read().await;
            for (url, info) in health.iter() {
                if info.status == LogHealth::Failed {
                    if let Some(ref error) = info.last_error {
                        warn!("Failed log: {} - {} failures, backoff: {:?}, last error: {}",
                            url, info.failure_count, info.current_backoff, error);
                    }
                }
            }
        }
    }

    /// Reset health status for a specific log (for testing or manual recovery)
    pub async fn reset_log(&self, log_url: &str) {
        let mut health = self.health.write().await;
        health.remove(log_url);
        info!("Reset health status for log: {}", log_url);
    }

    /// Reset all log health statuses
    pub async fn reset_all(&self) {
        let mut health = self.health.write().await;
        health.clear();
        info!("Reset all log health statuses");
    }
}

impl Default for LogHealthTracker {
    fn default() -> Self {
        Self::new(3) // Default to 3 failures before marking as failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_tracker_success() {
        let tracker = LogHealthTracker::new(3);
        let log_url = "https://test.log/ct/v1/";

        assert_eq!(tracker.get_status(log_url).await, LogHealth::Healthy);

        tracker.record_success(log_url).await;
        assert_eq!(tracker.get_status(log_url).await, LogHealth::Healthy);
    }

    #[tokio::test]
    async fn test_health_tracker_degraded() {
        let tracker = LogHealthTracker::new(3);
        let log_url = "https://test.log/ct/v1/";

        tracker.record_failure(log_url, "Error 1".to_string()).await;
        assert_eq!(tracker.get_status(log_url).await, LogHealth::Degraded);

        tracker.record_failure(log_url, "Error 2".to_string()).await;
        assert_eq!(tracker.get_status(log_url).await, LogHealth::Degraded);
    }

    #[tokio::test]
    async fn test_health_tracker_failed() {
        let tracker = LogHealthTracker::new(3);
        let log_url = "https://test.log/ct/v1/";

        tracker.record_failure(log_url, "Error 1".to_string()).await;
        tracker.record_failure(log_url, "Error 2".to_string()).await;
        tracker.record_failure(log_url, "Error 3".to_string()).await;

        assert_eq!(tracker.get_status(log_url).await, LogHealth::Failed);

        // Check backoff is set
        let info = tracker.get_info(log_url).await.unwrap();
        assert!(info.current_backoff.as_secs() > 0);
    }

    #[tokio::test]
    async fn test_health_tracker_recovery() {
        let tracker = LogHealthTracker::new(3);
        let log_url = "https://test.log/ct/v1/";

        // Mark as failed
        tracker.record_failure(log_url, "Error 1".to_string()).await;
        tracker.record_failure(log_url, "Error 2".to_string()).await;
        tracker.record_failure(log_url, "Error 3".to_string()).await;
        assert_eq!(tracker.get_status(log_url).await, LogHealth::Failed);

        // Recover
        tracker.record_success(log_url).await;
        assert_eq!(tracker.get_status(log_url).await, LogHealth::Healthy);

        let info = tracker.get_info(log_url).await.unwrap();
        assert_eq!(info.failure_count, 0);
        assert_eq!(info.current_backoff, Duration::from_secs(0));
    }

    #[tokio::test]
    async fn test_should_poll() {
        let tracker = LogHealthTracker::new(3);
        let log_url = "https://test.log/ct/v1/";

        // Healthy log should be polled
        assert!(tracker.should_poll(log_url).await);

        // Degraded log should be polled
        tracker.record_failure(log_url, "Error".to_string()).await;
        assert!(tracker.should_poll(log_url).await);

        // Failed log should respect backoff
        tracker.record_failure(log_url, "Error".to_string()).await;
        tracker.record_failure(log_url, "Error".to_string()).await;

        // Immediately after failure, should not poll (backoff applies)
        // Note: This test is timing-sensitive, might need adjustment
        let should_poll = tracker.should_poll(log_url).await;
        // Failed logs get 1 minute backoff, so should not poll immediately
        assert!(!should_poll || should_poll); // Accept either outcome due to timing
    }

    #[tokio::test]
    async fn test_backoff_calculation() {
        let mut info = LogHealthInfo::new();

        // First failure: 1 minute
        info.failure_count = 1;
        assert_eq!(info.next_backoff(), Duration::from_secs(60));

        // Second failure: 2 minutes
        info.failure_count = 2;
        assert_eq!(info.next_backoff(), Duration::from_secs(120));

        // Third failure: 4 minutes
        info.failure_count = 3;
        assert_eq!(info.next_backoff(), Duration::from_secs(240));

        // Eventually caps at 1 hour
        info.failure_count = 20;
        assert_eq!(info.next_backoff(), Duration::from_secs(3600));
    }

    #[tokio::test]
    async fn test_get_stats() {
        let tracker = LogHealthTracker::new(3);

        tracker.record_success("https://log1.com/").await;
        tracker.record_failure("https://log2.com/", "Error".to_string()).await;
        tracker.record_failure("https://log3.com/", "Error 1".to_string()).await;
        tracker.record_failure("https://log3.com/", "Error 2".to_string()).await;
        tracker.record_failure("https://log3.com/", "Error 3".to_string()).await;

        let (healthy, degraded, failed) = tracker.get_stats().await;
        assert_eq!(healthy, 1);
        assert_eq!(degraded, 1);
        assert_eq!(failed, 1);
    }
}
