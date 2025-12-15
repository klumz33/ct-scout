// src/database/mod.rs
use anyhow::Result;
use async_trait::async_trait;

use crate::types::MatchResult;

pub mod postgres;
pub mod state_manager;

pub use postgres::PostgresBackend;
pub use state_manager::DbStateManager;

/// Query parameters for fetching matches from database
#[derive(Debug, Clone)]
pub struct MatchQuery {
    pub domain_pattern: Option<String>,
    pub since: Option<u64>,  // Unix timestamp
    pub until: Option<u64>,  // Unix timestamp
    pub program_name: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Default for MatchQuery {
    fn default() -> Self {
        Self {
            domain_pattern: None,
            since: None,
            until: None,
            program_name: None,
            limit: Some(100),
            offset: None,
        }
    }
}

/// Database backend trait for state and match storage
#[async_trait]
pub trait DatabaseBackend: Send + Sync {
    /// Save a match to the database
    async fn save_match(&self, match_result: &MatchResult) -> Result<()>;

    /// Query historical matches
    async fn get_matches(&self, query: MatchQuery) -> Result<Vec<MatchResult>>;

    /// Update CT log state (last processed index)
    async fn update_log_state(&self, log_url: &str, index: u64) -> Result<()>;

    /// Get last processed index for a CT log
    async fn get_log_state(&self, log_url: &str) -> Result<Option<u64>>;

    /// Get all tracked log URLs with their last indices
    async fn get_all_log_states(&self) -> Result<Vec<(String, u64)>>;

    /// Health check
    async fn ping(&self) -> Result<()>;
}
