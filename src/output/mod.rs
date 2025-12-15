// src/output/mod.rs
//! Output handling abstraction for ct-scout
//!
//! This module provides a flexible output system that supports multiple
//! output formats and destinations simultaneously.

use crate::types::MatchResult;
use async_trait::async_trait;
use std::sync::Arc;

pub mod csv;
pub mod human;
pub mod json;
pub mod silent;
pub mod webhook;

/// Trait for output handlers that process matched certificates
#[async_trait]
pub trait OutputHandler: Send + Sync {
    /// Emit a matched certificate result
    async fn emit_match(&self, result: &MatchResult) -> anyhow::Result<()>;

    /// Flush any buffered output
    async fn flush(&self) -> anyhow::Result<()>;
}

/// Manager that dispatches output to multiple handlers
pub struct OutputManager {
    handlers: Vec<Arc<dyn OutputHandler>>,
}

impl OutputManager {
    /// Create a new OutputManager
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Add an output handler
    pub fn add_handler(&mut self, handler: Arc<dyn OutputHandler>) {
        self.handlers.push(handler);
    }

    /// Emit a match to all handlers
    ///
    /// Errors from individual handlers are logged but don't stop processing.
    /// This ensures webhook failures don't prevent stdout output, etc.
    pub async fn emit(&self, result: &MatchResult) -> anyhow::Result<()> {
        let mut last_error = None;

        for handler in &self.handlers {
            if let Err(e) = handler.emit_match(result).await {
                tracing::warn!("Output handler error: {}", e);
                last_error = Some(e);
            }
        }

        // Return error only if ALL handlers failed
        if let Some(err) = last_error {
            if self.handlers.len() == 1 {
                return Err(err);
            }
        }

        Ok(())
    }

    /// Flush all handlers
    pub async fn flush(&self) -> anyhow::Result<()> {
        for handler in &self.handlers {
            handler.flush().await?;
        }
        Ok(())
    }
}

impl Default for OutputManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CertData;

    #[tokio::test]
    async fn test_output_manager_no_handlers() {
        let manager = OutputManager::new();
        let result = create_test_result();

        // Should succeed with no handlers
        assert!(manager.emit(&result).await.is_ok());
    }

    #[tokio::test]
    async fn test_output_manager_with_handlers() {
        let mut manager = OutputManager::new();
        manager.add_handler(Arc::new(silent::SilentOutput));

        let result = create_test_result();
        assert!(manager.emit(&result).await.is_ok());
    }

    fn create_test_result() -> MatchResult {
        let cert_data = CertData {
            all_domains: Some(vec!["test.com".to_string()]),
            cert_index: Some(123),
            seen_unix: Some(1234567890.0),
            leaf_cert: None,
        };

        MatchResult::from_cert_data(
            "test.com".to_string(),
            &cert_data,
            Some("Test Program".to_string()),
        )
    }
}
