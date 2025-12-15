// src/stats.rs
//! Statistics tracking for ct-scout

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Thread-safe statistics collector
#[derive(Clone)]
pub struct StatsCollector {
    total_processed: Arc<AtomicU64>,
    matches_found: Arc<AtomicU64>,
    start_time: Instant,
}

/// Snapshot of statistics at a point in time
#[derive(Debug, Clone)]
pub struct StatsSnapshot {
    pub total_processed: u64,
    pub matches_found: u64,
    pub messages_per_minute: f64,
    pub uptime_secs: u64,
}

impl StatsCollector {
    /// Create a new StatsCollector
    pub fn new() -> Self {
        Self {
            total_processed: Arc::new(AtomicU64::new(0)),
            matches_found: Arc::new(AtomicU64::new(0)),
            start_time: Instant::now(),
        }
    }

    /// Increment the count of processed certificates
    pub fn increment_processed(&self) {
        self.total_processed.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the count of matched certificates
    pub fn increment_matches(&self) {
        self.matches_found.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current statistics snapshot
    pub fn snapshot(&self) -> StatsSnapshot {
        let elapsed = self.start_time.elapsed();
        let processed = self.total_processed.load(Ordering::Relaxed);
        let matches = self.matches_found.load(Ordering::Relaxed);

        let rate = if elapsed.as_secs() > 0 {
            (processed as f64 / elapsed.as_secs() as f64) * 60.0
        } else {
            0.0
        };

        StatsSnapshot {
            total_processed: processed,
            matches_found: matches,
            messages_per_minute: rate,
            uptime_secs: elapsed.as_secs(),
        }
    }

    /// Format statistics as a human-readable string
    pub fn format_stats(&self) -> String {
        let snapshot = self.snapshot();
        format!(
            "{} processed | {} matches | {:.1} msg/min | uptime: {}",
            snapshot.total_processed,
            snapshot.matches_found,
            snapshot.messages_per_minute,
            Self::format_uptime(snapshot.uptime_secs)
        )
    }

    /// Format uptime duration
    pub fn format_uptime(secs: u64) -> String {
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        let seconds = secs % 60;

        if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    }
}

impl Default for StatsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_stats_collector_new() {
        let stats = StatsCollector::new();
        let snapshot = stats.snapshot();

        assert_eq!(snapshot.total_processed, 0);
        assert_eq!(snapshot.matches_found, 0);
    }

    #[test]
    fn test_increment_processed() {
        let stats = StatsCollector::new();

        stats.increment_processed();
        stats.increment_processed();
        stats.increment_processed();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_processed, 3);
        assert_eq!(snapshot.matches_found, 0);
    }

    #[test]
    fn test_increment_matches() {
        let stats = StatsCollector::new();

        stats.increment_matches();
        stats.increment_matches();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_processed, 0);
        assert_eq!(snapshot.matches_found, 2);
    }

    #[test]
    fn test_rate_calculation() {
        let stats = StatsCollector::new();

        // Wait longer to ensure rate calculation works
        thread::sleep(Duration::from_secs(1));
        for _ in 0..10 {
            stats.increment_processed();
        }

        let snapshot = stats.snapshot();
        assert!(snapshot.messages_per_minute > 0.0);
        assert!(snapshot.uptime_secs >= 1);
    }

    #[test]
    fn test_clone_shares_state() {
        let stats1 = StatsCollector::new();
        let stats2 = stats1.clone();

        stats1.increment_processed();
        stats2.increment_processed();

        let snapshot1 = stats1.snapshot();
        let snapshot2 = stats2.snapshot();

        assert_eq!(snapshot1.total_processed, 2);
        assert_eq!(snapshot2.total_processed, 2);
    }

    #[test]
    fn test_format_uptime() {
        assert_eq!(StatsCollector::format_uptime(30), "30s");
        assert_eq!(StatsCollector::format_uptime(90), "1m 30s");
        assert_eq!(StatsCollector::format_uptime(3661), "1h 1m 1s");
    }
}
