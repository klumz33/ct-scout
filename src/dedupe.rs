// src/dedupe.rs
use crate::types::CertData;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct Dedupe {
    inner: Arc<Mutex<HashSet<String>>>,
}

impl Dedupe {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Returns true if this entry has not been seen before (and records it)
    pub async fn should_emit(&self, data: &CertData) -> bool {
        // Use cert_index if available, else fingerprint, else no dedupe
        let key = if let Some(idx) = data.cert_index {
            format!("idx:{}", idx)
        } else if let Some(leaf) = &data.leaf_cert {
            if let Some(fp) = &leaf.fingerprint {
                format!("fp:{}", fp)
            } else {
                return true;
            }
        } else {
            return true;
        };

        let mut guard = self.inner.lock().await;
        if guard.contains(&key) {
            false
        } else {
            guard.insert(key);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LeafCert;

    #[tokio::test]
    async fn test_dedupe_by_cert_index() {
        let dedupe = Dedupe::new();

        let cert_data1 = CertData {
            all_domains: Some(vec!["example.com".to_string()]),
            cert_index: Some(12345),
            seen_unix: Some(1234567890.0),
            leaf_cert: None,
            is_precert: false,
            ct_log_url: None,
        };

        let cert_data2 = CertData {
            all_domains: Some(vec!["different.com".to_string()]),
            cert_index: Some(12345), // Same index
            seen_unix: Some(1234567891.0),
            leaf_cert: None,
            is_precert: false,
            ct_log_url: None,
        };

        let cert_data3 = CertData {
            all_domains: Some(vec!["another.com".to_string()]),
            cert_index: Some(67890), // Different index
            seen_unix: Some(1234567892.0),
            leaf_cert: None,
            is_precert: false,
            ct_log_url: None,
        };

        // First cert should be emitted
        assert!(dedupe.should_emit(&cert_data1).await);

        // Second cert with same index should NOT be emitted
        assert!(!dedupe.should_emit(&cert_data2).await);

        // Third cert with different index should be emitted
        assert!(dedupe.should_emit(&cert_data3).await);
    }

    #[tokio::test]
    async fn test_dedupe_by_fingerprint() {
        let dedupe = Dedupe::new();

        let cert_data1 = CertData {
            all_domains: Some(vec!["example.com".to_string()]),
            cert_index: None,
            seen_unix: Some(1234567890.0),
            leaf_cert: Some(LeafCert {
                not_before: Some(1600000000),
                not_after: Some(1700000000),
                fingerprint: Some("abc123def456".to_string()),
                issuer: None,
            }),
            is_precert: false,
            ct_log_url: None,
        };

        let cert_data2 = CertData {
            all_domains: Some(vec!["different.com".to_string()]),
            cert_index: None,
            seen_unix: Some(1234567891.0),
            leaf_cert: Some(LeafCert {
                not_before: Some(1600000000),
                not_after: Some(1700000000),
                fingerprint: Some("abc123def456".to_string()), // Same fingerprint
                issuer: None,
            }),
            is_precert: false,
            ct_log_url: None,
        };

        let cert_data3 = CertData {
            all_domains: Some(vec!["another.com".to_string()]),
            cert_index: None,
            seen_unix: Some(1234567892.0),
            leaf_cert: Some(LeafCert {
                not_before: Some(1600000000),
                not_after: Some(1700000000),
                fingerprint: Some("xyz789ghi012".to_string()), // Different fingerprint
                issuer: None,
            }),
            is_precert: false,
            ct_log_url: None,
        };

        assert!(dedupe.should_emit(&cert_data1).await);
        assert!(!dedupe.should_emit(&cert_data2).await);
        assert!(dedupe.should_emit(&cert_data3).await);
    }

    #[tokio::test]
    async fn test_dedupe_prefers_cert_index_over_fingerprint() {
        let dedupe = Dedupe::new();

        let cert_data1 = CertData {
            all_domains: Some(vec!["example.com".to_string()]),
            cert_index: Some(100),
            seen_unix: Some(1234567890.0),
            leaf_cert: Some(LeafCert {
                not_before: Some(1600000000),
                not_after: Some(1700000000),
                fingerprint: Some("fingerprint1".to_string()),
                issuer: None,
            }),
            is_precert: false,
            ct_log_url: None,
        };

        let cert_data2 = CertData {
            all_domains: Some(vec!["different.com".to_string()]),
            cert_index: Some(100), // Same cert_index
            seen_unix: Some(1234567891.0),
            leaf_cert: Some(LeafCert {
                not_before: Some(1600000000),
                not_after: Some(1700000000),
                fingerprint: Some("fingerprint2".to_string()), // Different fingerprint
                issuer: None,
            }),
            is_precert: false,
            ct_log_url: None,
        };

        assert!(dedupe.should_emit(&cert_data1).await);
        // Should dedupe by cert_index even though fingerprint differs
        assert!(!dedupe.should_emit(&cert_data2).await);
    }

    #[tokio::test]
    async fn test_dedupe_no_identifiers() {
        let dedupe = Dedupe::new();

        let cert_data1 = CertData {
            all_domains: Some(vec!["example.com".to_string()]),
            cert_index: None,
            seen_unix: Some(1234567890.0),
            leaf_cert: None,
            is_precert: false,
            ct_log_url: None,
        };

        let cert_data2 = CertData {
            all_domains: Some(vec!["different.com".to_string()]),
            cert_index: None,
            seen_unix: Some(1234567891.0),
            leaf_cert: None,
            is_precert: false,
            ct_log_url: None,
        };

        // Both should be emitted since there's no way to dedupe
        assert!(dedupe.should_emit(&cert_data1).await);
        assert!(dedupe.should_emit(&cert_data2).await);
    }

    #[tokio::test]
    async fn test_dedupe_no_fingerprint_in_leaf() {
        let dedupe = Dedupe::new();

        let cert_data1 = CertData {
            all_domains: Some(vec!["example.com".to_string()]),
            cert_index: None,
            seen_unix: Some(1234567890.0),
            leaf_cert: Some(LeafCert {
                not_before: Some(1600000000),
                not_after: Some(1700000000),
                fingerprint: None, // No fingerprint
                issuer: None,
            }),
            is_precert: false,
            ct_log_url: None,
        };

        let cert_data2 = CertData {
            all_domains: Some(vec!["different.com".to_string()]),
            cert_index: None,
            seen_unix: Some(1234567891.0),
            leaf_cert: Some(LeafCert {
                not_before: Some(1600000000),
                not_after: Some(1700000000),
                fingerprint: None,
                issuer: None,
            }),
            is_precert: false,
            ct_log_url: None,
        };

        // Both should be emitted since there's no fingerprint
        assert!(dedupe.should_emit(&cert_data1).await);
        assert!(dedupe.should_emit(&cert_data2).await);
    }

    #[tokio::test]
    async fn test_dedupe_clone_shares_state() {
        let dedupe1 = Dedupe::new();
        let dedupe2 = dedupe1.clone();

        let cert_data = CertData {
            all_domains: Some(vec!["example.com".to_string()]),
            cert_index: Some(999),
            seen_unix: Some(1234567890.0),
            leaf_cert: None,
            is_precert: false,
            ct_log_url: None,
        };

        // Emit through first instance
        assert!(dedupe1.should_emit(&cert_data).await);

        // Should be deduped through cloned instance (shared state)
        assert!(!dedupe2.should_emit(&cert_data).await);
    }
}
