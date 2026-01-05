//! Redis Pub/Sub publisher for ct-scout matches
//!
//! Publishes certificate matches directly to Redis channels,
//! enabling real-time integration with automation pipelines.

use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Redis publisher configuration
#[derive(Debug, Clone)]
pub struct RedisConfig {
    /// Redis URL (supports Upstash format: rediss://...)
    pub url: String,
    /// Optional auth token (for Upstash)
    pub token: Option<String>,
    /// Channel name for CT events
    pub channel: String,
    /// Also push to a list for persistence (optional)
    pub queue_name: Option<String>,
    /// Maximum queue size (older items evicted)
    pub max_queue_size: Option<i64>,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://localhost:6379".to_string(),
            token: None,
            channel: "bb:ct_events".to_string(),
            queue_name: Some("bb:ct_events_queue".to_string()),
            max_queue_size: Some(10000),
        }
    }
}

/// Message published to Redis
#[derive(Debug, Clone, Serialize)]
pub struct CTEventMessage {
    /// Event type (always "ct_match")
    pub event_type: String,
    /// Unix timestamp
    pub timestamp: i64,
    /// Primary matched domain
    pub matched_domain: String,
    /// All domains in the certificate (SANs + CN)
    pub all_domains: Vec<String>,
    /// Certificate index in the CT log
    pub cert_index: u64,
    /// Certificate validity start (Unix timestamp)
    pub not_before: i64,
    /// Certificate validity end (Unix timestamp)
    pub not_after: i64,
    /// SHA-256 fingerprint of the certificate
    pub fingerprint: String,
    /// Bug bounty program name (if configured)
    pub program_name: Option<String>,
    /// CT log URL where this was found
    pub ct_log: String,
    /// Issuer common name
    pub issuer: Option<String>,
    /// Is this a precertificate?
    pub is_precert: bool,
}

/// Redis publisher with automatic reconnection
pub struct RedisPublisher {
    config: RedisConfig,
    connection: Arc<RwLock<Option<ConnectionManager>>>,
    connected: Arc<RwLock<bool>>,
}

impl RedisPublisher {
    /// Create a new Redis publisher
    pub fn new(config: RedisConfig) -> Self {
        Self {
            config,
            connection: Arc::new(RwLock::new(None)),
            connected: Arc::new(RwLock::new(false)),
        }
    }

    /// Connect to Redis (with Upstash support)
    pub async fn connect(&self) -> Result<(), redis::RedisError> {
        let url = if let Some(ref token) = self.config.token {
            // Upstash format: rediss://default:TOKEN@host:port
            if self.config.url.contains("@") {
                self.config.url.clone()
            } else {
                // Insert token into URL
                self.config.url.replace("rediss://", &format!("rediss://default:{}@", token))
            }
        } else {
            self.config.url.clone()
        };

        info!("Connecting to Redis...");

        let client = redis::Client::open(url)?;
        let manager = ConnectionManager::new(client).await?;

        // Test connection
        let mut conn = manager.clone();
        redis::cmd("PING").query_async::<String>(&mut conn).await?;

        *self.connection.write().await = Some(manager);
        *self.connected.write().await = true;

        info!("Redis connected successfully");
        Ok(())
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Publish a CT match event
    pub async fn publish(&self, event: CTEventMessage) -> Result<(), redis::RedisError> {
        let conn_guard = self.connection.read().await;
        let conn = match conn_guard.as_ref() {
            Some(c) => c.clone(),
            None => {
                error!("Redis not connected");
                return Err(redis::RedisError::from((
                    redis::ErrorKind::IoError,
                    "Not connected",
                )));
            }
        };
        drop(conn_guard);

        let payload = serde_json::to_string(&event)
            .map_err(|e| redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Serialization failed",
                e.to_string(),
            )))?;

        let mut conn = conn;

        // Publish to channel (for real-time subscribers)
        let subscribers: i64 = conn.publish(&self.config.channel, &payload).await?;
        debug!(
            "Published to channel {} ({} subscribers)",
            self.config.channel, subscribers
        );

        // Also push to queue for persistence (if configured)
        if let Some(ref queue_name) = self.config.queue_name {
            conn.lpush::<_, _, ()>(queue_name, &payload).await?;

            // Trim queue to max size
            if let Some(max_size) = self.config.max_queue_size {
                conn.ltrim::<_, ()>(queue_name, 0, (max_size - 1) as isize).await?;
            }

            debug!("Pushed to queue {}", queue_name);
        }

        Ok(())
    }

    /// Publish with automatic retry
    pub async fn publish_with_retry(&self, event: CTEventMessage, max_retries: u32) -> bool {
        for attempt in 0..max_retries {
            match self.publish(event.clone()).await {
                Ok(_) => return true,
                Err(e) => {
                    warn!(
                        "Redis publish failed (attempt {}/{}): {}",
                        attempt + 1,
                        max_retries,
                        e
                    );

                    // Try to reconnect
                    if let Err(reconnect_err) = self.connect().await {
                        error!("Redis reconnection failed: {}", reconnect_err);
                    }

                    tokio::time::sleep(tokio::time::Duration::from_millis(
                        100 * 2_u64.pow(attempt),
                    )).await;
                }
            }
        }

        error!("Redis publish failed after {} retries", max_retries);
        false
    }
}

/// Builder for CTEventMessage from ct-scout's internal types
impl CTEventMessage {
    pub fn from_match(
        matched_domain: String,
        all_domains: Vec<String>,
        cert_index: u64,
        not_before: i64,
        not_after: i64,
        fingerprint: String,
        program_name: Option<String>,
        ct_log: String,
        issuer: Option<String>,
        is_precert: bool,
    ) -> Self {
        Self {
            event_type: "ct_match".to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            matched_domain,
            all_domains,
            cert_index,
            not_before,
            not_after,
            fingerprint,
            program_name,
            ct_log,
            issuer,
            is_precert,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_serialization() {
        let event = CTEventMessage::from_match(
            "test.example.com".to_string(),
            vec!["test.example.com".to_string(), "www.test.example.com".to_string()],
            12345,
            1704067200,
            1735689600,
            "abc123def456".to_string(),
            Some("Example Program".to_string()),
            "https://ct.googleapis.com/logs/us1/argon2024/".to_string(),
            Some("Let's Encrypt".to_string()),
            false,
        );

        let json = serde_json::to_string_pretty(&event).unwrap();
        println!("{}", json);

        assert!(json.contains("ct_match"));
        assert!(json.contains("test.example.com"));
    }
}
