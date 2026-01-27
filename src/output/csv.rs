// src/output/csv.rs
//! CSV output handler

use crate::output::OutputHandler;
use crate::types::MatchResult;
use async_trait::async_trait;
use std::io::{self, Write};
use std::sync::Mutex;

/// CSV output handler
pub struct CsvOutput {
    writer: Mutex<Box<dyn Write + Send>>,
    header_written: Mutex<bool>,
}

impl CsvOutput {
    /// Create a new CsvOutput that writes to stdout
    pub fn new() -> Self {
        Self {
            writer: Mutex::new(Box::new(io::stdout())),
            header_written: Mutex::new(false),
        }
    }

    /// Create a new CsvOutput that writes to a file
    pub fn to_file(file: std::fs::File) -> Self {
        Self {
            writer: Mutex::new(Box::new(file)),
            header_written: Mutex::new(false),
        }
    }

    /// Write CSV header if not already written
    fn ensure_header(&self, writer: &mut dyn Write) -> anyhow::Result<()> {
        let mut header_written = self.header_written.lock().unwrap();
        if !*header_written {
            writeln!(
                writer,
                "timestamp,matched_domain,all_domains,cert_index,not_before,not_after,fingerprint,program_name"
            )?;
            *header_written = true;
        }
        Ok(())
    }

    /// Escape a field for CSV (wrap in quotes if contains comma/quote/newline)
    fn escape_field(field: &str) -> String {
        if field.contains(',') || field.contains('"') || field.contains('\n') {
            format!("\"{}\"", field.replace('"', "\"\""))
        } else {
            field.to_string()
        }
    }

    /// Format optional field
    fn format_optional<T: std::fmt::Display>(opt: &Option<T>) -> String {
        opt.as_ref().map(|v| v.to_string()).unwrap_or_default()
    }
}

impl Default for CsvOutput {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OutputHandler for CsvOutput {
    async fn emit_match(&self, result: &MatchResult) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().unwrap();

        // Ensure header is written
        self.ensure_header(&mut *writer)?;

        // Format all_domains as comma-separated (within quotes)
        let all_domains = result.all_domains.join(";"); // Use semicolon to avoid CSV confusion

        // Write CSV row
        writeln!(
            writer,
            "{},{},{},{},{},{},{},{}",
            result.timestamp,
            Self::escape_field(&result.matched_domain),
            Self::escape_field(&all_domains),
            Self::format_optional(&result.cert_index),
            Self::format_optional(&result.not_before),
            Self::format_optional(&result.not_after),
            result.fingerprint.as_ref().map(|s| Self::escape_field(s)).unwrap_or_default(),
            result.program_name.as_ref().map(|s| Self::escape_field(s)).unwrap_or_default(),
        )?;

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
    async fn test_csv_output() {
        let handler = CsvOutput::new();
        let cert_data = CertData {
            all_domains: Some(vec!["test.com".to_string(), "www.test.com".to_string()]),
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
    async fn test_csv_escape_field() {
        assert_eq!(CsvOutput::escape_field("simple"), "simple");
        assert_eq!(CsvOutput::escape_field("with,comma"), "\"with,comma\"");
        assert_eq!(CsvOutput::escape_field("with\"quote"), "\"with\"\"quote\"");
    }
}
