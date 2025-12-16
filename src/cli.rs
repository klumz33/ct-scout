// src/cli.rs
use clap::Parser;

/// CT-Scout: Certificate Transparency Log Monitor
///
/// Monitor Certificate Transparency logs for domains matching your watchlist.
/// Supports multiple output formats and notification methods.
#[derive(Parser, Debug, Clone)]
#[command(name = "ct-scout")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    // ===== Input & Configuration =====
    /// Path to TOML config file
    #[arg(short = 'c', long = "config", default_value = "config.toml")]
    pub config: String,

    /// Watch config file for changes and reload
    #[arg(short = 'w', long = "watch-config")]
    pub watch_config: bool,

    /// File containing root domains to filter output (one per line)
    #[arg(short = 'r', long = "root-domains")]
    pub root_domains: Option<String>,

    // ===== Output Format =====
    /// Output matches in JSONL format to stdout
    #[arg(short = 'j', long = "json")]
    pub json: bool,

    /// Output matches in CSV format to stdout
    #[arg(long = "csv")]
    pub csv: bool,

    /// Suppress all stdout output (webhook only mode)
    #[arg(short = 's', long = "silent")]
    pub silent: bool,

    // ===== Output Destination =====
    /// Write output to file instead of stdout
    #[arg(short = 'o', long = "output")]
    pub output: Option<String>,

    /// Override webhook URL from config
    #[arg(long = "webhook")]
    pub webhook_url: Option<String>,

    /// Override webhook secret from config
    #[arg(long = "webhook-secret")]
    pub webhook_secret: Option<String>,

    /// Disable webhook notifications even if configured
    #[arg(long = "no-webhook")]
    pub no_webhook: bool,

    // ===== Filtering & Matching =====
    /// Disable certificate deduplication
    #[arg(long = "no-dedupe")]
    pub no_dedupe: bool,

    // ===== Performance =====
    /// Override certstream reconnect delay in seconds
    #[arg(long = "reconnect-delay")]
    pub reconnect_delay: Option<u64>,

    /// Override webhook timeout in seconds
    #[arg(long = "webhook-timeout")]
    pub webhook_timeout: Option<u64>,

    // ===== Display & Statistics =====
    /// Display statistics (msgs/min, total processed, matches found)
    #[arg(long = "stats")]
    pub stats: bool,

    /// Stats update interval in seconds
    #[arg(long = "stats-interval", default_value = "10")]
    pub stats_interval: u64,

    /// Disable progress indicator
    #[arg(long = "no-progress")]
    pub no_progress: bool,

    // ===== Logging =====
    /// Verbose logging (set log level to debug)
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Quiet logging (set log level to warn)
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    // ===== Utility Commands =====
    /// Export current scope (config + platforms) to TOML format and exit
    #[arg(long = "export-scope")]
    pub export_scope: bool,

    /// Dry-run platform sync: show what programs would be synced without actually syncing
    #[arg(long = "dry-run-sync")]
    pub dry_run_sync: bool,
}

impl Cli {
    /// Validate flag combinations and return errors for invalid usage
    pub fn validate(&self) -> anyhow::Result<()> {
        // Cannot specify multiple output formats
        let format_count = [self.json, self.csv, self.silent]
            .iter()
            .filter(|&&x| x)
            .count();

        if format_count > 1 {
            anyhow::bail!(
                "Cannot specify multiple output formats. \
                Choose one of: --json, --csv, or --silent"
            );
        }

        // Silent mode requires some output (webhook)
        if self.silent && self.no_webhook {
            anyhow::bail!(
                "Cannot use --silent with --no-webhook: no output would be generated.\n\
                Either enable webhooks or use a different output format."
            );
        }

        // Stats interval must be reasonable
        if self.stats && self.stats_interval == 0 {
            anyhow::bail!("--stats-interval must be greater than 0");
        }

        // Verbose and quiet are mutually exclusive
        if self.verbose && self.quiet {
            anyhow::bail!("Cannot specify both --verbose and --quiet");
        }

        Ok(())
    }

    /// Determine the output format based on flags
    pub fn output_format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else if self.csv {
            OutputFormat::Csv
        } else if self.silent {
            OutputFormat::Silent
        } else {
            OutputFormat::Human
        }
    }

    /// Check if progress indicator should be enabled
    pub fn should_show_progress(&self) -> bool {
        !self.no_progress && !self.json && !self.csv && !self.silent
    }

    /// Determine log level based on verbose/quiet flags
    pub fn log_level(&self) -> &str {
        if self.verbose {
            "debug"
        } else if self.quiet {
            "warn"
        } else {
            "info"
        }
    }
}

/// Output format selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable colored text output (default)
    Human,
    /// JSON Lines format (one JSON object per line)
    Json,
    /// CSV format
    Csv,
    /// No stdout output
    Silent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_path() {
        let cli = Cli::parse_from(&["ct-scout"]);
        assert_eq!(cli.config, "config.toml");
    }

    #[test]
    fn test_custom_config_path() {
        let cli = Cli::parse_from(&["ct-scout", "--config", "custom.toml"]);
        assert_eq!(cli.config, "custom.toml");
    }

    #[test]
    fn test_json_output_format() {
        let cli = Cli::parse_from(&["ct-scout", "--json"]);
        assert_eq!(cli.output_format(), OutputFormat::Json);
    }

    #[test]
    fn test_csv_output_format() {
        let cli = Cli::parse_from(&["ct-scout", "--csv"]);
        assert_eq!(cli.output_format(), OutputFormat::Csv);
    }

    #[test]
    fn test_silent_output_format() {
        let cli = Cli::parse_from(&["ct-scout", "--silent"]);
        assert_eq!(cli.output_format(), OutputFormat::Silent);
    }

    #[test]
    fn test_default_is_human() {
        let cli = Cli::parse_from(&["ct-scout"]);
        assert_eq!(cli.output_format(), OutputFormat::Human);
    }

    #[test]
    fn test_multiple_formats_invalid() {
        let cli = Cli::parse_from(&["ct-scout", "--json", "--csv"]);
        assert!(cli.validate().is_err());
    }

    #[test]
    fn test_silent_without_webhook_invalid() {
        let cli = Cli::parse_from(&["ct-scout", "--silent", "--no-webhook"]);
        assert!(cli.validate().is_err());
    }

    #[test]
    fn test_verbose_and_quiet_invalid() {
        let cli = Cli::parse_from(&["ct-scout", "--verbose", "--quiet"]);
        assert!(cli.validate().is_err());
    }

    #[test]
    fn test_valid_combination() {
        let cli = Cli::parse_from(&["ct-scout", "--json", "--stats", "--no-webhook"]);
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn test_progress_disabled_for_json() {
        let cli = Cli::parse_from(&["ct-scout", "--json"]);
        assert!(!cli.should_show_progress());
    }

    #[test]
    fn test_progress_enabled_by_default() {
        let cli = Cli::parse_from(&["ct-scout"]);
        assert!(cli.should_show_progress());
    }

    #[test]
    fn test_log_level_verbose() {
        let cli = Cli::parse_from(&["ct-scout", "--verbose"]);
        assert_eq!(cli.log_level(), "debug");
    }

    #[test]
    fn test_log_level_quiet() {
        let cli = Cli::parse_from(&["ct-scout", "--quiet"]);
        assert_eq!(cli.log_level(), "warn");
    }

    #[test]
    fn test_log_level_default() {
        let cli = Cli::parse_from(&["ct-scout"]);
        assert_eq!(cli.log_level(), "info");
    }

    #[test]
    fn test_short_flags() {
        let cli = Cli::parse_from(&[
            "ct-scout",
            "-c", "test.toml",
            "-j",
            "-r", "roots.txt",
            "-w",
            "-s",
        ]);
        assert_eq!(cli.config, "test.toml");
        assert!(cli.json);
        assert_eq!(cli.root_domains, Some("roots.txt".to_string()));
        assert!(cli.watch_config);
        assert!(cli.silent);
    }
}
