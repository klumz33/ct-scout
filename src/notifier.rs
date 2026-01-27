// src/notifier.rs
use crate::config::WebhookConfig;
use crate::types::CertData;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Serialize;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct Notifier {
    client: Client,
    cfg: WebhookConfig,
}

#[derive(Serialize)]
pub struct NotificationPayload<'a> {
    pub matched_domain: &'a str,
    pub all_domains: &'a [String],
    pub cert_index: Option<u64>,
    pub not_before: Option<u64>,
    pub not_after: Option<u64>,
    pub program_name: Option<&'a str>,
}

impl Notifier {
    pub fn new(cfg: WebhookConfig) -> Self {
        let client = Client::new();
        Self { client, cfg }
    }

    pub async fn notify_match(
        &self,
        domain: &str,
        data: &CertData,
        program_name: Option<&str>,
    ) -> anyhow::Result<()> {
        let all_domains_slice = data
            .all_domains
            .as_ref()
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        let (not_before, not_after) = data
            .leaf_cert
            .as_ref()
            .map(|leaf| (leaf.not_before, leaf.not_after))
            .unwrap_or((None, None));

        let payload = NotificationPayload {
            matched_domain: domain,
            all_domains: all_domains_slice,
            cert_index: data.cert_index,
            not_before,
            not_after,
            program_name,
        };

        let body = serde_json::to_vec(&payload)?;

        let timeout_secs = self.cfg.timeout_secs.unwrap_or(5);
        let mut req = self
            .client
            .post(&self.cfg.url)
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .body(body.clone())
            .header("Content-Type", "application/json");

        // Optional HMAC signature header
        if let Some(secret) = &self.cfg.secret {
            let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
                .map_err(|e| anyhow::anyhow!("HMAC init error: {:?}", e))?;
            mac.update(&body);
            let sig = mac.finalize().into_bytes();
            let sig_hex = hex::encode(sig);
            req = req.header("X-CTScout-Signature", sig_hex);
        }

        let resp = req.send().await?;
        resp.error_for_status()?; // non-2xx -> error

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LeafCert;
    use wiremock::matchers::{body_json_string, header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_test_cert_data() -> CertData {
        CertData {
            all_domains: Some(vec![
                "example.com".to_string(),
                "www.example.com".to_string(),
            ]),
            cert_index: Some(123456),
            seen_unix: Some(1234567890.0),
            leaf_cert: Some(LeafCert {
                not_before: Some(1600000000),
                not_after: Some(1700000000),
                fingerprint: Some("abcdef123456".to_string()),
                issuer: Some("Test CA".to_string()),
            }),
            is_precert: false,
            ct_log_url: Some("https://ct.example.com/log".to_string()),
        }
    }

    #[tokio::test]
    async fn test_notify_match_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .and(header("Content-Type", "application/json"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let config = WebhookConfig {
            url: mock_server.uri(),
            secret: None,
            timeout_secs: Some(5),
        };

        let notifier = Notifier::new(config);
        let cert_data = create_test_cert_data();

        let result = notifier
            .notify_match("example.com", &cert_data, Some("Test Program"))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_notify_match_with_hmac_signature() {
        let mock_server = MockServer::start().await;
        let secret = "test_secret_key";

        // Verify request with HMAC signature header
        Mock::given(method("POST"))
            .and(path("/"))
            .and(header("Content-Type", "application/json"))
            .and(header_exists("X-CTScout-Signature"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let config = WebhookConfig {
            url: mock_server.uri(),
            secret: Some(secret.to_string()),
            timeout_secs: Some(5),
        };

        let notifier = Notifier::new(config);
        let cert_data = create_test_cert_data();

        let result = notifier
            .notify_match("example.com", &cert_data, None)
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_notify_match_payload_structure() {
        let mock_server = MockServer::start().await;

        // Verify the JSON payload has the expected structure
        let expected_json = serde_json::json!({
            "matched_domain": "example.com",
            "all_domains": ["example.com", "www.example.com"],
            "cert_index": 123456,
            "not_before": 1600000000,
            "not_after": 1700000000,
            "program_name": "IBM"
        });

        Mock::given(method("POST"))
            .and(body_json_string(expected_json.to_string()))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let config = WebhookConfig {
            url: mock_server.uri(),
            secret: None,
            timeout_secs: Some(5),
        };

        let notifier = Notifier::new(config);
        let cert_data = create_test_cert_data();

        let result = notifier
            .notify_match("example.com", &cert_data, Some("IBM"))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_notify_match_server_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let config = WebhookConfig {
            url: mock_server.uri(),
            secret: None,
            timeout_secs: Some(5),
        };

        let notifier = Notifier::new(config);
        let cert_data = create_test_cert_data();

        let result = notifier
            .notify_match("example.com", &cert_data, None)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_notify_match_timeout() {
        let mock_server = MockServer::start().await;

        // Delay response longer than timeout
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(
                10,
            )))
            .mount(&mock_server)
            .await;

        let config = WebhookConfig {
            url: mock_server.uri(),
            secret: None,
            timeout_secs: Some(1), // 1 second timeout
        };

        let notifier = Notifier::new(config);
        let cert_data = create_test_cert_data();

        let result = notifier
            .notify_match("example.com", &cert_data, None)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_notify_match_with_minimal_cert_data() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let config = WebhookConfig {
            url: mock_server.uri(),
            secret: None,
            timeout_secs: Some(5),
        };

        let notifier = Notifier::new(config);

        // Minimal cert data with no leaf cert
        let cert_data = CertData {
            all_domains: Some(vec!["minimal.com".to_string()]),
            cert_index: None,
            seen_unix: None,
            leaf_cert: None,
            is_precert: false,
            ct_log_url: None,
        };

        let result = notifier.notify_match("minimal.com", &cert_data, None).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_hmac_signature_verification() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let secret = "my_secret";
        let payload = NotificationPayload {
            matched_domain: "test.com",
            all_domains: &["test.com".to_string()],
            cert_index: Some(100),
            not_before: Some(1600000000),
            not_after: Some(1700000000),
            program_name: None,
        };

        let body = serde_json::to_vec(&payload).unwrap();

        // Generate HMAC signature the same way the notifier does
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(&body);
        let expected_sig = hex::encode(mac.finalize().into_bytes());

        // Verify it's a valid hex string
        assert_eq!(expected_sig.len(), 64); // SHA256 produces 32 bytes = 64 hex chars
        assert!(expected_sig.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
