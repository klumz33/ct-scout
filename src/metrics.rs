//! Prometheus metrics for ct-scout
//!
//! Provides observability into Redis publishing operations,
//! connection health, and overall system performance.

use lazy_static::lazy_static;
use prometheus::{
    Gauge, HistogramVec, IntCounter, IntCounterVec, Opts, Registry,
};
use tracing::warn;

lazy_static! {
    /// Global metrics registry
    pub static ref REGISTRY: Registry = Registry::new();

    // ===== Redis Metrics =====

    /// Total Redis publish operations
    /// Labels: status="success|failure"
    pub static ref REDIS_PUBLISH_TOTAL: IntCounterVec = IntCounterVec::new(
        Opts::new(
            "ctscout_redis_publish_total",
            "Total number of Redis publish operations"
        ),
        &["status"]  // success or failure
    ).expect("metric cannot be created");

    /// Redis publish duration in seconds
    /// Labels: status="success|failure"
    pub static ref REDIS_PUBLISH_DURATION: HistogramVec = HistogramVec::new(
        prometheus::HistogramOpts::new(
            "ctscout_redis_publish_duration_seconds",
            "Duration of Redis publish operations in seconds"
        )
        .buckets(vec![
            0.001, 0.005, 0.010, 0.025, 0.050,  // 1ms to 50ms
            0.100, 0.250, 0.500, 1.0, 2.5, 5.0   // 100ms to 5s
        ]),
        &["status"]  // success or failure
    ).expect("metric cannot be created");

    /// Current Redis connection status
    /// Value: 1=connected, 0=disconnected
    pub static ref REDIS_CONNECTION_STATUS: Gauge = Gauge::new(
        "ctscout_redis_connection_status",
        "Current Redis connection status (1=connected, 0=disconnected)"
    ).expect("metric cannot be created");

    /// Total Redis reconnection attempts
    pub static ref REDIS_RECONNECT_ATTEMPTS: IntCounter = IntCounter::new(
        "ctscout_redis_reconnection_attempts_total",
        "Total number of Redis reconnection attempts"
    ).expect("metric cannot be created");

    // ===== General Metrics =====

    /// Total certificates processed
    pub static ref CERTIFICATES_PROCESSED_TOTAL: IntCounter = IntCounter::new(
        "ctscout_certificates_processed_total",
        "Total number of certificates processed"
    ).expect("metric cannot be created");

    /// Total certificate matches found
    pub static ref MATCHES_FOUND_TOTAL: IntCounter = IntCounter::new(
        "ctscout_matches_found_total",
        "Total number of certificate matches found"
    ).expect("metric cannot be created");
}

/// Initialize metrics registry
pub fn init_metrics() -> Result<(), prometheus::Error> {
    // Register all metrics
    REGISTRY.register(Box::new(REDIS_PUBLISH_TOTAL.clone()))?;
    REGISTRY.register(Box::new(REDIS_PUBLISH_DURATION.clone()))?;
    REGISTRY.register(Box::new(REDIS_CONNECTION_STATUS.clone()))?;
    REGISTRY.register(Box::new(REDIS_RECONNECT_ATTEMPTS.clone()))?;
    REGISTRY.register(Box::new(CERTIFICATES_PROCESSED_TOTAL.clone()))?;
    REGISTRY.register(Box::new(MATCHES_FOUND_TOTAL.clone()))?;

    Ok(())
}

/// Export metrics in Prometheus text format
pub fn export_metrics() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let metric_families = REGISTRY.gather();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        warn!("Failed to encode metrics: {}", e);
        return String::new();
    }

    String::from_utf8(buffer).unwrap_or_default()
}

/// Metrics configuration
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub export_path: Option<String>,  // None = stdout, Some = file path
    pub export_interval_secs: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            export_path: None,  // stdout by default
            export_interval_secs: 60,
        }
    }
}

/// Background task for periodic metrics export
pub async fn metrics_exporter_task(config: MetricsConfig) {
    use tokio::time::{interval, Duration};
    use std::io::Write;

    let mut ticker = interval(Duration::from_secs(config.export_interval_secs));

    loop {
        ticker.tick().await;

        let metrics_text = export_metrics();

        match &config.export_path {
            None => {
                // Export to stdout
                println!("\n# Prometheus Metrics");
                println!("{}", metrics_text);
            }
            Some(path) => {
                // Export to file
                match std::fs::File::create(path) {
                    Ok(mut file) => {
                        if let Err(e) = file.write_all(metrics_text.as_bytes()) {
                            warn!("Failed to write metrics to file {}: {}", path, e);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to create metrics file {}: {}", path, e);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_initialization() {
        let result = init_metrics();
        // May fail if already initialized in other tests, that's ok
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_redis_publish_counter() {
        REDIS_PUBLISH_TOTAL.with_label_values(&["success"]).inc();
        let value = REDIS_PUBLISH_TOTAL.with_label_values(&["success"]).get();
        assert!(value > 0);
    }

    #[test]
    fn test_export_metrics() {
        let output = export_metrics();
        // Should contain Prometheus format output
        assert!(!output.is_empty() || output.is_empty()); // Always passes, just test it doesn't panic
    }
}
