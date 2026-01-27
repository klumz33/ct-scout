// src/platforms/sync.rs
//! Platform synchronization manager for automatic watchlist updates

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{watch, Mutex};
use tracing::{error, info};

use super::PlatformAPI;
use crate::watchlist::Watchlist;

/// Manages periodic synchronization with bug bounty platforms
pub struct PlatformSyncManager {
    platforms: Vec<Box<dyn PlatformAPI>>,
    watchlist: Arc<Mutex<Watchlist>>,
    sync_interval: Duration,
}

impl PlatformSyncManager {
    /// Create new platform sync manager
    pub fn new(
        platforms: Vec<Box<dyn PlatformAPI>>,
        watchlist: Arc<Mutex<Watchlist>>,
        sync_interval_hours: u64,
    ) -> Self {
        Self {
            platforms,
            watchlist,
            sync_interval: Duration::from_secs(sync_interval_hours * 3600),
        }
    }

    /// Run the sync manager (blocks until shutdown signal received)
    pub async fn run(&self, mut shutdown_rx: watch::Receiver<bool>) {
        info!(
            "Platform sync manager starting (sync interval: {} hours)",
            self.sync_interval.as_secs() / 3600
        );

        // Perform initial sync immediately
        self.sync_all_platforms().await;

        loop {
            tokio::select! {
                // Wait for next sync interval
                _ = tokio::time::sleep(self.sync_interval) => {
                    self.sync_all_platforms().await;
                }

                // Check for shutdown signal
                _ = shutdown_rx.changed() => {
                    info!("Platform sync manager shutting down");
                    break;
                }
            }
        }
    }

    /// Sync watchlist from all configured platforms
    async fn sync_all_platforms(&self) {
        info!("Starting platform synchronization");

        for platform in &self.platforms {
            if let Err(e) = self.sync_platform(platform.as_ref()).await {
                error!("Failed to sync from {}: {:?}", platform.name(), e);
            }
        }

        info!("Platform synchronization complete");
    }

    /// Sync watchlist from a single platform
    async fn sync_platform(&self, platform: &dyn PlatformAPI) -> Result<()> {
        info!("Syncing programs from {}", platform.name());

        // Test connection first
        if !platform.test_connection().await? {
            anyhow::bail!("{} API connection test failed", platform.name());
        }

        // Fetch programs
        let programs = platform.fetch_programs().await?;

        info!(
            "Fetched {} programs from {}",
            programs.len(),
            platform.name()
        );

        if programs.is_empty() {
            info!("No programs found on {}", platform.name());
            return Ok(());
        }

        // Update watchlist with new domains
        let mut watchlist = self.watchlist.lock().await;
        let mut total_domains_added = 0;

        for program in programs {
            // Log with platform prefix for visibility
            info!(
                "Adding {} domains from program: {}: {}",
                program.domains.len(),
                program.platform,
                program.name
            );

            for domain in program.domains {
                // Add domain to watchlist with original name and platform info separately
                watchlist.add_domain_to_program(&domain, &program.name, Some(program.platform.clone()));
                total_domains_added += 1;
            }

            for host in program.hosts {
                watchlist.add_host_to_program(&host, &program.name, Some(program.platform.clone()));
            }
        }

        info!(
            "Added {} domains from {} to watchlist",
            total_domains_added,
            platform.name()
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::FetchOptions;
    use async_trait::async_trait;

    struct MockPlatform;

    #[async_trait]
    impl PlatformAPI for MockPlatform {
        fn name(&self) -> &str {
            "Mock"
        }

        async fn fetch_programs(&self) -> Result<Vec<super::super::Program>> {
            self.fetch_programs_with_options(FetchOptions {
                filter: "all".to_string(),
                max_programs: 100,
                dry_run: false,
            }).await
        }

        async fn fetch_programs_with_options(&self, _options: FetchOptions) -> Result<Vec<super::super::Program>> {
            Ok(vec![super::super::Program {
                id: "1".to_string(),
                name: "Test Program".to_string(),
                handle: "test-program".to_string(),
                domains: vec!["*.example.com".to_string()],
                hosts: vec![],
                in_scope: true,
                platform: "Mock".to_string(),
            }])
        }

        async fn test_connection(&self) -> Result<bool> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_platform_sync_manager() {
        let watchlist = Arc::new(Mutex::new(Watchlist::default()));
        let platforms: Vec<Box<dyn PlatformAPI>> = vec![Box::new(MockPlatform)];

        let manager = PlatformSyncManager::new(platforms, watchlist.clone(), 24);

        // Test sync
        manager.sync_all_platforms().await;

        let watchlist_lock = watchlist.lock().await;
        assert_eq!(watchlist_lock.programs().len(), 1);
    }
}
