// src/watcher.rs
//! Configuration file watcher using notify

use crate::config::Config;
use notify::{Event, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::mpsc as tokio_mpsc;

/// Configuration file watcher
pub struct ConfigWatcher {
    path: PathBuf,
}

impl ConfigWatcher {
    /// Create a new config watcher
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Start watching the config file for changes
    ///
    /// Returns a receiver that will get new Config instances when the file changes
    pub fn watch(&self) -> anyhow::Result<tokio_mpsc::Receiver<Config>> {
        let (tx, rx) = tokio_mpsc::channel(10);
        let path = self.path.clone();

        // Spawn blocking task for file watching
        tokio::task::spawn_blocking(move || {
            if let Err(e) = Self::watch_blocking(path, tx) {
                tracing::error!("Config watcher error: {}", e);
            }
        });

        Ok(rx)
    }

    /// Blocking file watch implementation
    fn watch_blocking(path: PathBuf, tx: tokio_mpsc::Sender<Config>) -> anyhow::Result<()> {
        let (notify_tx, notify_rx) = mpsc::channel();

        let mut watcher = notify::recommended_watcher(notify_tx)?;
        watcher.watch(&path, RecursiveMode::NonRecursive)?;

        tracing::info!("Watching config file: {:?}", path);

        // Debounce to avoid multiple reloads for a single file change
        let mut last_reload = std::time::Instant::now();

        while let Ok(event) = notify_rx.recv() {
            if let Ok(event) = event {
                if Self::should_reload(&event) {
                    let now = std::time::Instant::now();

                    // Debounce: only reload if at least 1 second has passed
                    if now.duration_since(last_reload) < Duration::from_secs(1) {
                        continue;
                    }

                    last_reload = now;

                    match Config::from_file(&path) {
                        Ok(config) => {
                            tracing::info!("Config reloaded from {:?}", path);
                            if tx.blocking_send(config).is_err() {
                                tracing::warn!("Config receiver dropped, stopping watcher");
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to reload config: {}. Keeping previous config.", e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if event should trigger a reload
    fn should_reload(event: &Event) -> bool {
        use notify::EventKind;

        matches!(
            event.kind,
            EventKind::Modify(_) | EventKind::Create(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_config_watcher_creation() {
        let temp_file = NamedTempFile::new().unwrap();
        let watcher = ConfigWatcher::new(temp_file.path().to_path_buf());

        // Just verify we can create a watcher
        assert_eq!(watcher.path, temp_file.path());
    }

    // Note: Full integration test of file watching is complex due to
    // file system event timing. The blocking implementation is tested
    // manually and in integration tests.
}
