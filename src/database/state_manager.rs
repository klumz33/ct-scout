// src/database/state_manager.rs
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use super::DatabaseBackend;

/// Database-backed state manager for CT log tracking
/// Drop-in replacement for TOML-based StateManager
pub struct DbStateManager {
    db: Arc<dyn DatabaseBackend>,
    save_counter: Arc<Mutex<u64>>,
}

impl DbStateManager {
    /// Create new database-backed state manager
    pub fn new(db: Arc<dyn DatabaseBackend>) -> Self {
        Self {
            db,
            save_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Get last-seen index for a CT log
    pub async fn get_last_index(&self, log_url: &str) -> Option<u64> {
        match self.db.get_log_state(log_url).await {
            Ok(index) => index,
            Err(e) => {
                warn!("Failed to get log state for {}: {}", log_url, e);
                None
            }
        }
    }

    /// Update last-seen index for a CT log
    /// Auto-saves every 100 entries (though DB writes are immediate)
    pub async fn update_index(&self, log_url: &str, index: u64) {
        // Increment counter for compatibility with TOML version
        // (DB backend already writes immediately, but we keep this for logging)
        let mut counter = self.save_counter.lock().await;
        *counter += 1;

        let should_log = *counter % 100 == 0;
        drop(counter);

        if let Err(e) = self.db.update_log_state(log_url, index).await {
            warn!("Failed to update log state for {}: {}", log_url, e);
        } else if should_log {
            debug!("Updated log state for {} to index {}", log_url, index);
        }
    }

    /// Save state (no-op for DB backend, kept for API compatibility)
    pub async fn save(&self) -> Result<()> {
        debug!("Save called (no-op for DB backend)");
        Ok(())
    }

    /// Get all tracked log URLs
    pub async fn get_tracked_logs(&self) -> Vec<String> {
        match self.db.get_all_log_states().await {
            Ok(states) => states.into_iter().map(|(url, _)| url).collect(),
            Err(e) => {
                warn!("Failed to get tracked logs: {}", e);
                Vec::new()
            }
        }
    }

    /// Get total number of tracked logs
    pub async fn count(&self) -> usize {
        self.get_tracked_logs().await.len()
    }
}

impl Clone for DbStateManager {
    fn clone(&self) -> Self {
        Self {
            db: Arc::clone(&self.db),
            save_counter: Arc::clone(&self.save_counter),
        }
    }
}
