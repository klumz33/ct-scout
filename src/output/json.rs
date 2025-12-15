// src/output/json.rs
//! JSON Lines (JSONL) output handler

use crate::output::OutputHandler;
use crate::types::MatchResult;
use async_trait::async_trait;
use std::io::{self, Write};
use std::sync::Mutex;

/// JSON Lines output handler
///
/// Outputs one JSON object per line (JSONL/NDJSON format)
pub struct JsonOutput {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl JsonOutput {
    /// Create a new JsonOutput that writes to stdout
    pub fn new() -> Self {
        Self {
            writer: Mutex::new(Box::new(io::stdout())),
        }
    }

    /// Create a new JsonOutput that writes to a file
    pub fn to_file(file: std::fs::File) -> Self {
        Self {
            writer: Mutex::new(Box::new(file)),
        }
    }
}

impl Default for JsonOutput {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OutputHandler for JsonOutput {
    async fn emit_match(&self, result: &MatchResult) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().unwrap();

        // Serialize to JSON
        let json = serde_json::to_string(result)?;

        // Write JSON line
        writeln!(writer, "{}", json)?;
        writer.flush()?;

        Ok(())
    }

    async fn flush(&self) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CertData;

    #[tokio::test]
    async fn test_json_output_format() {
        let handler = JsonOutput::new();
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

        assert!(handler.emit_match(&result).await.is_ok());

        // Verify it's valid JSON by serializing
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test.com"));
        assert!(json.contains("Test Program"));
    }

    #[tokio::test]
    async fn test_json_flush() {
        let handler = JsonOutput::new();
        assert!(handler.flush().await.is_ok());
    }
}
