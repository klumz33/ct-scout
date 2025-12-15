// src/state.rs
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// State manager for tracking last-seen index per CT log
/// Persists state to TOML file for resume capability across restarts
pub struct StateManager {
    state_file_path: PathBuf,
    state: Arc<Mutex<HashMap<String, u64>>>,
    save_counter: Arc<Mutex<u64>>,  // Track entries processed since last save
}

impl StateManager {
    /// Create new state manager and load existing state from file
    pub async fn new(state_file: PathBuf) -> Result<Self> {
        let mut state = HashMap::new();

        // Load existing state if file exists
        if state_file.exists() {
            info!("Loading state from {:?}", state_file);

            let contents = fs::read_to_string(&state_file)
                .await
                .context("Failed to read state file")?;

            let loaded_state: HashMap<String, u64> = toml::from_str(&contents)
                .context("Failed to parse state file")?;

            info!("Loaded state for {} CT logs", loaded_state.len());
            state = loaded_state;
        } else {
            info!(
                "State file {:?} does not exist, starting fresh",
                state_file
            );
        }

        Ok(Self {
            state_file_path: state_file,
            state: Arc::new(Mutex::new(state)),
            save_counter: Arc::new(Mutex::new(0)),
        })
    }

    /// Get last-seen index for a CT log
    pub async fn get_last_index(&self, log_url: &str) -> Option<u64> {
        let state = self.state.lock().await;
        state.get(log_url).copied()
    }

    /// Update last-seen index for a CT log
    /// Auto-saves every 100 entries to balance durability and I/O overhead
    pub async fn update_index(&self, log_url: &str, index: u64) {
        {
            let mut state = self.state.lock().await;
            state.insert(log_url.to_string(), index);
        }

        // Increment counter and save periodically
        let mut counter = self.save_counter.lock().await;
        *counter += 1;

        if *counter >= 100 {
            *counter = 0;
            drop(counter);  // Release lock before async save

            if let Err(e) = self.save().await {
                warn!("Failed to auto-save state: {}", e);
            }
        }
    }

    /// Manually save state to file
    pub async fn save(&self) -> Result<()> {
        let state = self.state.lock().await;

        debug!("Saving state for {} CT logs to {:?}", state.len(), self.state_file_path);

        let toml_string = toml::to_string(&*state)
            .context("Failed to serialize state to TOML")?;

        // Write to temporary file first, then rename for atomicity
        let temp_path = self.state_file_path.with_extension("tmp");

        fs::write(&temp_path, toml_string)
            .await
            .context("Failed to write state to temporary file")?;

        fs::rename(&temp_path, &self.state_file_path)
            .await
            .context("Failed to rename temporary state file")?;

        debug!("State saved successfully");

        Ok(())
    }

    /// Get all tracked log URLs
    pub async fn get_tracked_logs(&self) -> Vec<String> {
        let state = self.state.lock().await;
        state.keys().cloned().collect()
    }

    /// Get total number of tracked logs
    pub async fn count(&self) -> usize {
        let state = self.state.lock().await;
        state.len()
    }
}

impl Clone for StateManager {
    fn clone(&self) -> Self {
        Self {
            state_file_path: self.state_file_path.clone(),
            state: Arc::clone(&self.state),
            save_counter: Arc::clone(&self.save_counter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_state_manager_basic() {
        let temp_file = NamedTempFile::new().unwrap();
        let state_path = temp_file.path().to_path_buf();

        let manager = StateManager::new(state_path.clone()).await.unwrap();

        // Initially no state
        assert_eq!(manager.get_last_index("https://example.com/log").await, None);

        // Update index
        manager.update_index("https://example.com/log", 100).await;
        assert_eq!(
            manager.get_last_index("https://example.com/log").await,
            Some(100)
        );

        // Save
        manager.save().await.unwrap();

        // Load in new manager
        let manager2 = StateManager::new(state_path).await.unwrap();
        assert_eq!(
            manager2.get_last_index("https://example.com/log").await,
            Some(100)
        );
    }

    #[tokio::test]
    async fn test_state_manager_auto_save() {
        let temp_file = NamedTempFile::new().unwrap();
        let state_path = temp_file.path().to_path_buf();

        let manager = StateManager::new(state_path.clone()).await.unwrap();

        // Update 100 times (should trigger auto-save)
        for i in 0..100 {
            manager
                .update_index("https://example.com/log", i)
                .await;
        }

        // Give it a moment to auto-save
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Load in new manager - should have the state
        let manager2 = StateManager::new(state_path).await.unwrap();
        assert!(manager2.get_last_index("https://example.com/log").await.is_some());
    }
}
