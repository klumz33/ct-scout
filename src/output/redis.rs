//! Redis output handler - publishes matches to Redis pub/sub

use crate::output::OutputHandler;
use crate::redis_publisher::{CTEventMessage, RedisPublisher};
use crate::types::MatchResult;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::warn;

/// Redis output handler
pub struct RedisOutput {
    publisher: Arc<RedisPublisher>,
}

impl RedisOutput {
    /// Create a new RedisOutput
    pub fn new(publisher: Arc<RedisPublisher>) -> Self {
        Self { publisher }
    }
}

#[async_trait]
impl OutputHandler for RedisOutput {
    async fn emit_match(&self, result: &MatchResult) -> anyhow::Result<()> {
        // Build the CT event message from the match result
        let event = CTEventMessage::from_match(
            result.matched_domain.clone(),
            result.all_domains.clone(),
            result.cert_index.unwrap_or(0),
            result.not_before.unwrap_or(0) as i64,
            result.not_after.unwrap_or(0) as i64,
            result.fingerprint.clone().unwrap_or_default(),
            result.program_name.clone(),
            "unknown".to_string(), // CT log URL - could be added to MatchResult later
            None,                   // Issuer - could be added to MatchResult later
            false,                  // is_precert - could be added to MatchResult later
        );

        // Publish with retry (fire and forget, don't block)
        let publisher = self.publisher.clone();
        tokio::spawn(async move {
            if !publisher.publish_with_retry(event, 3).await {
                warn!("Failed to publish CT event to Redis after retries");
            }
        });

        Ok(())
    }

    async fn flush(&self) -> anyhow::Result<()> {
        // Redis publish is already fire-and-forget, nothing to flush
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redis_publisher::RedisConfig;
    use crate::types::CertData;

    #[tokio::test]
    async fn test_redis_output_emit() {
        // Create a Redis publisher (won't actually connect in test)
        let redis_config = RedisConfig::default();
        let publisher = Arc::new(RedisPublisher::new(redis_config));
        let handler = RedisOutput::new(publisher);

        let cert_data = CertData {
            all_domains: Some(vec!["test.com".to_string()]),
            cert_index: Some(123),
            seen_unix: Some(1234567890.0),
            leaf_cert: None,
        };

        let result = MatchResult::from_cert_data(
            "test.com".to_string(),
            &cert_data,
            Some("Test Program".to_string()),
        );

        // Should not fail even without Redis connection (fire and forget)
        assert!(handler.emit_match(&result).await.is_ok());
        assert!(handler.flush().await.is_ok());
    }
}
