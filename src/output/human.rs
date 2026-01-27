// src/output/human.rs
//! Human-readable colored terminal output

use crate::output::OutputHandler;
use crate::types::MatchResult;
use async_trait::async_trait;
use colored::Colorize;
use std::io::{self, Write};
use std::sync::Mutex;

/// Human-readable output handler with colored terminal output
pub struct HumanOutput {
    writer: Mutex<Box<dyn Write + Send>>,
    use_colors: bool,
}

impl HumanOutput {
    /// Create a new HumanOutput that writes to stdout
    pub fn new() -> Self {
        Self {
            writer: Mutex::new(Box::new(io::stdout())),
            use_colors: is_terminal::is_terminal(std::io::stdout()),
        }
    }

    /// Create a new HumanOutput that writes to a file
    pub fn to_file(file: std::fs::File) -> Self {
        Self {
            writer: Mutex::new(Box::new(file)),
            use_colors: false, // No colors when writing to file
        }
    }

    /// Format a timestamp as human-readable string
    fn format_timestamp(ts: u64) -> String {
        use chrono::DateTime;

        // Convert Unix timestamp to DateTime
        if let Some(datetime) = DateTime::from_timestamp(ts as i64, 0) {
            // Format as YYYY-MM-DD HH:MM:SS UTC
            datetime.format("%Y-%m-%d %H:%M:%S").to_string()
        } else {
            // Fallback if timestamp is invalid
            format!("{}", ts)
        }
    }
}

impl Default for HumanOutput {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OutputHandler for HumanOutput {
    async fn emit_match(&self, result: &MatchResult) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().unwrap();

        let timestamp = Self::format_timestamp(result.timestamp);

        if self.use_colors {
            write!(
                writer,
                "{} {} {}\n",
                format!("[{}]", timestamp).dimmed(),
                "[+]".green().bold(),
                result.matched_domain.cyan().bold()
            )?;

            if let Some(ref program) = result.program_name {
                let program_display = if let Some(ref platform) = result.platform {
                    format!("{} ({})", program, platform)
                } else {
                    program.clone()
                };

                writeln!(
                    writer,
                    "    {} {}",
                    "Program:".dimmed(),
                    program_display.yellow()
                )?;
            }

            if result.all_domains.len() > 1 {
                writeln!(
                    writer,
                    "    {} {}",
                    "All domains:".dimmed(),
                    result.all_domains.join(", ")
                )?;
            }
        } else {
            writeln!(writer, "[{}] [+] {}", timestamp, result.matched_domain)?;

            if let Some(ref program) = result.program_name {
                let program_display = if let Some(ref platform) = result.platform {
                    format!("{} ({})", program, platform)
                } else {
                    program.clone()
                };

                writeln!(writer, "    Program: {}", program_display)?;
            }

            if result.all_domains.len() > 1 {
                writeln!(writer, "    All domains: {}", result.all_domains.join(", "))?;
            }
        }

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
    async fn test_human_output() {
        let handler = HumanOutput::new();
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
        assert!(handler.flush().await.is_ok());
    }
}
