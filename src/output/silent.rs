// src/output/silent.rs
//! Silent output handler - produces no output

use crate::output::OutputHandler;
use crate::types::MatchResult;
use async_trait::async_trait;

/// Silent output handler that produces no output
///
/// Used when --silent flag is set (webhook-only mode)
pub struct SilentOutput;

#[async_trait]
impl OutputHandler for SilentOutput {
    async fn emit_match(&self, _result: &MatchResult) -> anyhow::Result<()> {
        // Intentionally do nothing
        Ok(())
    }

    async fn flush(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CertData;

    #[tokio::test]
    async fn test_silent_output() {
        let handler = SilentOutput;
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

        // Should always succeed
        assert!(handler.emit_match(&result).await.is_ok());
        assert!(handler.flush().await.is_ok());
    }
}
