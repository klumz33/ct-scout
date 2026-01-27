// src/output/webhook.rs
//! Webhook output handler - sends HTTP POST notifications

use crate::config::WebhookConfig;
use crate::output::OutputHandler;
use crate::types::MatchResult;
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Serialize;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Webhook output handler
pub struct WebhookOutput {
    client: Client,
    config: WebhookConfig,
}

#[derive(Serialize)]
struct WebhookPayload<'a> {
    matched_domain: &'a str,
    all_domains: &'a [String],
    cert_index: Option<u64>,
    not_before: Option<u64>,
    not_after: Option<u64>,
    program_name: Option<&'a str>,
    timestamp: u64,
    fingerprint: Option<&'a str>,
}

impl WebhookOutput {
    /// Create a new WebhookOutput
    pub fn new(config: WebhookConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }
}

#[async_trait]
impl OutputHandler for WebhookOutput {
    async fn emit_match(&self, result: &MatchResult) -> anyhow::Result<()> {
        let payload = WebhookPayload {
            matched_domain: &result.matched_domain,
            all_domains: &result.all_domains,
            cert_index: result.cert_index,
            not_before: result.not_before,
            not_after: result.not_after,
            program_name: result.program_name.as_deref(),
            timestamp: result.timestamp,
            fingerprint: result.fingerprint.as_deref(),
        };

        let body = serde_json::to_vec(&payload)?;

        let timeout_secs = self.config.timeout_secs.unwrap_or(5);
        let mut req = self
            .client
            .post(&self.config.url)
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .body(body.clone())
            .header("Content-Type", "application/json");

        // Add HMAC signature if secret is configured
        if let Some(secret) = &self.config.secret {
            let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
                .map_err(|e| anyhow::anyhow!("HMAC init error: {:?}", e))?;
            mac.update(&body);
            let sig = mac.finalize().into_bytes();
            let sig_hex = hex::encode(sig);
            req = req.header("X-CTScout-Signature", sig_hex);
        }

        let resp = req.send().await?;
        resp.error_for_status()?;

        Ok(())
    }

    async fn flush(&self) -> anyhow::Result<()> {
        // HTTP requests are not buffered
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CertData;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_webhook_output() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .and(header("Content-Type", "application/json"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let config = WebhookConfig {
            url: mock_server.uri(),
            secret: None,
            timeout_secs: Some(5),
        };

        let handler = WebhookOutput::new(config);

        let cert_data = CertData {
            all_domains: Some(vec!["test.com".to_string()]),
            cert_index: Some(123),
            seen_unix: Some(1234567890.0),
            leaf_cert: None,
            is_precert: false,
            ct_log_url: None,
        };

        let result = MatchResult::from_cert_data(
            "test.com".to_string(),
            &cert_data,
            Some("Test Program".to_string()),
            None,
        );

        assert!(handler.emit_match(&result).await.is_ok());
    }

    #[tokio::test]
    async fn test_webhook_with_signature() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let config = WebhookConfig {
            url: mock_server.uri(),
            secret: Some("test_secret".to_string()),
            timeout_secs: Some(5),
        };

        let handler = WebhookOutput::new(config);

        let cert_data = CertData {
            all_domains: Some(vec!["test.com".to_string()]),
            cert_index: Some(123),
            seen_unix: Some(1234567890.0),
            leaf_cert: None,
            is_precert: false,
            ct_log_url: None,
        };

        let result = MatchResult::from_cert_data(
            "test.com".to_string(),
            &cert_data,
            None,
            None,
        );

        assert!(handler.emit_match(&result).await.is_ok());
    }
}
