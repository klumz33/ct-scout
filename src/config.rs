// src/config.rs

use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct CtLogConfig {
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
    #[serde(default = "default_log_list_url")]
    pub log_list_url: String,
    #[serde(default)]
    pub custom_logs: Option<Vec<String>>,  // Replaces Google list (backward compat)
    #[serde(default)]
    pub additional_logs: Option<Vec<String>>,  // Merges with Google list
    #[serde(default = "default_state_file")]
    pub state_file: String,
    #[serde(default = "default_max_concurrent_logs")]
    pub max_concurrent_logs: usize,
    #[serde(default = "default_parse_precerts")]
    pub parse_precerts: bool,
    #[serde(default = "default_include_readonly_logs")]
    pub include_readonly_logs: bool,
    #[serde(default = "default_include_all_logs")]
    pub include_all_logs: bool,
    #[serde(default = "default_include_pending")]
    pub include_pending: bool,  // Include pending logs (like gungnir)
}

fn default_poll_interval() -> u64 { 10 }
fn default_batch_size() -> u64 { 256 }
fn default_log_list_url() -> String {
    "https://www.gstatic.com/ct/log_list/v3/all_logs_list.json".to_string()
}
fn default_state_file() -> String { "ct-scout-state.toml".to_string() }
fn default_max_concurrent_logs() -> usize { 100 }
fn default_parse_precerts() -> bool { true }
fn default_include_readonly_logs() -> bool { false }
fn default_include_all_logs() -> bool { false }
fn default_include_pending() -> bool { false }

#[derive(Debug, Deserialize, Clone)]
pub struct WebhookConfig {
    pub url: String,
    pub secret: Option<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct WatchlistConfig {
    pub domains: Vec<String>,
    pub hosts: Vec<String>,
    pub ips: Vec<String>,
    pub cidrs: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProgramConfig {
    pub name: String,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub ips: Vec<String>,
    #[serde(default)]
    pub cidrs: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub enabled: bool,
    #[serde(default = "default_database_url")]
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

fn default_database_url() -> String {
    "postgresql://localhost/ctscout".to_string()
}

fn default_max_connections() -> u32 {
    20
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: default_database_url(),
            max_connections: default_max_connections(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct PlatformsConfig {
    #[serde(default)]
    pub hackerone: Option<HackerOneConfig>,
    #[serde(default)]
    pub intigriti: Option<IntigritiConfig>,
    #[serde(default = "default_sync_interval_hours")]
    pub sync_interval_hours: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HackerOneConfig {
    pub enabled: bool,
    pub username: String,
    pub api_token: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IntigritiConfig {
    pub enabled: bool,
    pub api_token: String,
}

fn default_sync_interval_hours() -> u64 { 6 }

impl Default for PlatformsConfig {
    fn default() -> Self {
        Self {
            hackerone: None,
            intigriti: None,
            sync_interval_hours: default_sync_interval_hours(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub ct_logs: CtLogConfig,
    #[serde(default)]
    pub webhook: Option<WebhookConfig>,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub platforms: PlatformsConfig,
    pub logging: LoggingConfig,
    pub watchlist: WatchlistConfig,
    #[serde(default)]
    pub programs: Vec<ProgramConfig>,
}

impl Default for CtLogConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: default_poll_interval(),
            batch_size: default_batch_size(),
            log_list_url: default_log_list_url(),
            custom_logs: None,
            additional_logs: None,
            state_file: default_state_file(),
            max_concurrent_logs: default_max_concurrent_logs(),
            parse_precerts: default_parse_precerts(),
            include_readonly_logs: default_include_readonly_logs(),
            include_all_logs: default_include_all_logs(),
            include_pending: default_include_pending(),
        }
    }
}

impl Config {
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&contents)?;
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_from_valid_toml() {
        let toml_content = r#"
[ct_logs]
poll_interval_secs = 10
batch_size = 512
state_file = "test-state.toml"

[webhook]
url = "https://example.com/webhook"
secret = "test_secret"
timeout_secs = 5

[logging]
level = "debug"

[watchlist]
domains = ["*.example.com", ".test.com"]
hosts = ["exact.host.com"]
ips = ["192.168.1.1"]
cidrs = ["10.0.0.0/8"]

[[programs]]
name = "Test Program"
domains = [".example.org"]
cidrs = ["172.16.0.0/12"]
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();

        assert_eq!(config.ct_logs.poll_interval_secs, 10);
        assert_eq!(config.ct_logs.batch_size, 512);
        assert_eq!(config.ct_logs.state_file, "test-state.toml");
        assert!(config.webhook.is_some());
        let webhook = config.webhook.as_ref().unwrap();
        assert_eq!(webhook.url, "https://example.com/webhook");
        assert_eq!(webhook.secret, Some("test_secret".to_string()));
        assert_eq!(webhook.timeout_secs, Some(5));
        assert_eq!(config.logging.level, "debug");
        assert_eq!(config.watchlist.domains.len(), 2);
        assert_eq!(config.watchlist.hosts.len(), 1);
        assert_eq!(config.watchlist.ips.len(), 1);
        assert_eq!(config.watchlist.cidrs.len(), 1);
        assert_eq!(config.programs.len(), 1);
        assert_eq!(config.programs[0].name, "Test Program");
    }

    #[test]
    fn test_config_minimal_toml() {
        let toml_content = r#"
[webhook]
url = "https://example.com"

[logging]
level = "info"

[watchlist]
domains = []
hosts = []
ips = []
cidrs = []
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = Config::from_file(temp_file.path()).unwrap();

        // ct_logs should use defaults
        assert_eq!(config.ct_logs.poll_interval_secs, 10);
        assert_eq!(config.ct_logs.batch_size, 256);

        assert!(config.webhook.is_some());
        let webhook = config.webhook.as_ref().unwrap();
        assert_eq!(webhook.secret, None);
        assert_eq!(webhook.timeout_secs, None);
        assert_eq!(config.programs.len(), 0);
    }

    #[test]
    fn test_config_invalid_toml() {
        let toml_content = "invalid toml content {{{";

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let result = Config::from_file(temp_file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_config_missing_required_fields() {
        let toml_content = r#"
[watchlist]
domains = []
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let result = Config::from_file(temp_file.path());
        assert!(result.is_err());  // Missing logging section
    }

    #[test]
    fn test_config_nonexistent_file() {
        let result = Config::from_file(Path::new("/nonexistent/path/config.toml"));
        assert!(result.is_err());
    }
}
