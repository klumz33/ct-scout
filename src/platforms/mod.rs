// src/platforms/mod.rs
//! Bug bounty platform API integrations for automatic watchlist synchronization

use anyhow::Result;
use async_trait::async_trait;

pub mod hackerone;
pub mod intigriti;
pub mod sync;

pub use hackerone::HackerOneAPI;
pub use intigriti::IntigritiAPI;
pub use sync::PlatformSyncManager;

/// Represents a bug bounty program with its scope
#[derive(Debug, Clone)]
pub struct Program {
    /// Platform-specific program ID
    pub id: String,

    /// Display name of the program
    pub name: String,

    /// Platform handle (e.g., "company-name")
    pub handle: String,

    /// List of in-scope domains
    pub domains: Vec<String>,

    /// List of in-scope hosts
    pub hosts: Vec<String>,

    /// Whether this program is currently in scope
    pub in_scope: bool,
}

/// Platform API trait for fetching program data
#[async_trait]
pub trait PlatformAPI: Send + Sync {
    /// Get the platform name (e.g., "HackerOne", "Intigriti")
    fn name(&self) -> &str;

    /// Fetch all enrolled programs with their scopes
    async fn fetch_programs(&self) -> Result<Vec<Program>>;

    /// Check if API credentials are valid
    async fn test_connection(&self) -> Result<bool>;
}

/// Extract domain from URL or pattern
/// Examples:
/// - "https://example.com" -> "example.com"
/// - "*.example.com" -> "*.example.com"
/// - "example.com" -> "example.com"
pub fn extract_domain(url_or_pattern: &str) -> String {
    let trimmed = url_or_pattern.trim();

    // If starts with wildcard, keep as-is
    if trimmed.starts_with("*.") {
        return trimmed.to_string();
    }

    // If URL, parse and extract host
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        if let Ok(url) = url::Url::parse(trimmed) {
            if let Some(host) = url.host_str() {
                return host.to_string();
            }
        }
    }

    // Otherwise assume it's a domain
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://example.com"), "example.com");
        assert_eq!(extract_domain("http://www.example.com/path"), "www.example.com");
        assert_eq!(extract_domain("*.example.com"), "*.example.com");
        assert_eq!(extract_domain("example.com"), "example.com");
        assert_eq!(extract_domain("  example.com  "), "example.com");
    }
}
