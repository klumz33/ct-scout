// src/types.rs
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Deserialize)]
pub struct CertStreamMessage {
    pub message_type: Option<String>,
    pub data: Option<CertData>,
}

#[derive(Debug, Deserialize)]
pub struct CertData {
    pub all_domains: Option<Vec<String>>,

    #[serde(rename = "cert_index")]
    pub cert_index: Option<u64>,

    #[serde(rename = "seen")]
    pub seen_unix: Option<f64>,

    #[serde(rename = "leaf_cert")]
    pub leaf_cert: Option<LeafCert>,

    #[serde(default)]
    pub is_precert: bool,

    #[serde(rename = "ct_log")]
    pub ct_log_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LeafCert {
    #[serde(rename = "not_before")]
    pub not_before: Option<u64>,
    #[serde(rename = "not_after")]
    pub not_after: Option<u64>,
    pub fingerprint: Option<String>,
    pub issuer: Option<String>,
}

/// Represents a matched certificate for output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    /// Timestamp when the match was found (Unix timestamp)
    pub timestamp: u64,

    /// The specific domain that matched the watchlist
    pub matched_domain: String,

    /// All domains in the certificate
    pub all_domains: Vec<String>,

    /// Certificate index from the CT log
    pub cert_index: Option<u64>,

    /// Certificate validity start time (Unix timestamp)
    pub not_before: Option<u64>,

    /// Certificate validity end time (Unix timestamp)
    pub not_after: Option<u64>,

    /// Certificate fingerprint
    pub fingerprint: Option<String>,

    /// Bug bounty program name (if matched)
    pub program_name: Option<String>,

    /// Platform the program belongs to (if matched)
    pub platform: Option<String>,

    /// Unix timestamp when the cert was seen
    pub seen_unix: Option<f64>,

    /// Certificate issuer
    pub issuer: Option<String>,

    /// Whether this is a precertificate
    pub is_precert: bool,

    /// CT log URL where this cert was found
    pub ct_log_url: Option<String>,
}

impl MatchResult {
    /// Create a new MatchResult from CertData
    pub fn from_cert_data(
        matched_domain: String,
        data: &CertData,
        program_name: Option<String>,
        platform: Option<String>,
    ) -> Self {
        let (not_before, not_after, fingerprint, issuer) = data
            .leaf_cert
            .as_ref()
            .map(|leaf| (leaf.not_before, leaf.not_after, leaf.fingerprint.clone(), leaf.issuer.clone()))
            .unwrap_or((None, None, None, None));

        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            matched_domain,
            all_domains: data.all_domains.clone().unwrap_or_default(),
            cert_index: data.cert_index,
            not_before,
            not_after,
            fingerprint,
            program_name,
            platform,
            seen_unix: data.seen_unix,
            issuer,
            is_precert: data.is_precert,
            ct_log_url: data.ct_log_url.clone(),
        }
    }
}

impl fmt::Display for MatchResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[+] Match: {}", self.matched_domain)?;
        if let Some(ref program) = self.program_name {
            write!(f, " (Program: {})", program)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_full_certstream_message() {
        let json = r#"{
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["example.com", "www.example.com"],
                "cert_index": 123456789,
                "seen": 1234567890.123,
                "leaf_cert": {
                    "not_before": 1600000000,
                    "not_after": 1700000000,
                    "fingerprint": "AA:BB:CC:DD:EE:FF"
                }
            }
        }"#;

        let msg: CertStreamMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.message_type, Some("certificate_update".to_string()));
        assert!(msg.data.is_some());

        let data = msg.data.unwrap();
        assert_eq!(data.all_domains.as_ref().unwrap().len(), 2);
        assert_eq!(data.all_domains.as_ref().unwrap()[0], "example.com");
        assert_eq!(data.cert_index, Some(123456789));
        assert_eq!(data.seen_unix, Some(1234567890.123));

        let leaf = data.leaf_cert.unwrap();
        assert_eq!(leaf.not_before, Some(1600000000));
        assert_eq!(leaf.not_after, Some(1700000000));
        assert_eq!(leaf.fingerprint, Some("AA:BB:CC:DD:EE:FF".to_string()));
    }

    #[test]
    fn test_deserialize_minimal_certstream_message() {
        let json = r#"{
            "message_type": "heartbeat"
        }"#;

        let msg: CertStreamMessage = serde_json::from_str(json).unwrap();

        assert_eq!(msg.message_type, Some("heartbeat".to_string()));
        assert!(msg.data.is_none());
    }

    #[test]
    fn test_deserialize_cert_data_with_missing_fields() {
        let json = r#"{
            "all_domains": ["test.com"]
        }"#;

        let data: CertData = serde_json::from_str(json).unwrap();

        assert_eq!(data.all_domains.as_ref().unwrap().len(), 1);
        assert_eq!(data.cert_index, None);
        assert_eq!(data.seen_unix, None);
        assert!(data.leaf_cert.is_none());
    }

    #[test]
    fn test_deserialize_leaf_cert_with_missing_fingerprint() {
        let json = r#"{
            "not_before": 1600000000,
            "not_after": 1700000000
        }"#;

        let leaf: LeafCert = serde_json::from_str(json).unwrap();

        assert_eq!(leaf.not_before, Some(1600000000));
        assert_eq!(leaf.not_after, Some(1700000000));
        assert_eq!(leaf.fingerprint, None);
    }

    #[test]
    fn test_deserialize_cert_data_empty_domains() {
        let json = r#"{
            "all_domains": [],
            "cert_index": 42
        }"#;

        let data: CertData = serde_json::from_str(json).unwrap();

        assert!(data.all_domains.is_some());
        assert_eq!(data.all_domains.unwrap().len(), 0);
        assert_eq!(data.cert_index, Some(42));
    }

    #[test]
    fn test_deserialize_certstream_message_empty_object() {
        let json = r#"{}"#;

        let msg: CertStreamMessage = serde_json::from_str(json).unwrap();

        assert!(msg.message_type.is_none());
        assert!(msg.data.is_none());
    }

    #[test]
    fn test_deserialize_real_certstream_example() {
        // This is based on actual certstream output format
        let json = r#"{
            "message_type": "certificate_update",
            "data": {
                "update_type": "X509LogEntry",
                "leaf_cert": {
                    "subject": {
                        "CN": "example.com"
                    },
                    "extensions": {
                        "subjectAltName": "DNS:example.com, DNS:www.example.com"
                    },
                    "not_before": 1609459200,
                    "not_after": 1617235200,
                    "fingerprint": "01:23:45:67:89:AB:CD:EF"
                },
                "cert_index": 987654321,
                "seen": 1609459300.5,
                "all_domains": ["example.com", "www.example.com"]
            }
        }"#;

        let msg: CertStreamMessage = serde_json::from_str(json).unwrap();

        assert!(msg.message_type.is_some());
        assert!(msg.data.is_some());

        let data = msg.data.unwrap();
        assert!(data.all_domains.is_some());
        assert_eq!(data.all_domains.unwrap().len(), 2);
    }

    #[test]
    fn test_deserialize_invalid_json() {
        let json = r#"{ invalid json }"#;
        let result: Result<CertStreamMessage, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_debug_trait() {
        let msg = CertStreamMessage {
            message_type: Some("test".to_string()),
            data: None,
        };

        let debug_str = format!("{:?}", msg);
        assert!(debug_str.contains("CertStreamMessage"));
    }
}
