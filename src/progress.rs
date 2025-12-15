// src/progress.rs
//! Progress indicator using indicatif

use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// Progress indicator wrapper
#[derive(Clone)]
pub struct ProgressIndicator {
    spinner: Option<ProgressBar>,
    enabled: bool,
}

impl ProgressIndicator {
    /// Create a new progress indicator
    pub fn new(enabled: bool) -> Self {
        if !enabled {
            return Self {
                spinner: None,
                enabled: false,
            };
        }

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .expect("Invalid template")
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        spinner.enable_steady_tick(Duration::from_millis(100));

        Self {
            spinner: Some(spinner),
            enabled: true,
        }
    }

    /// Set the status message
    pub fn set_message(&self, msg: impl Into<String>) {
        if let Some(ref spinner) = self.spinner {
            spinner.set_message(msg.into());
        }
    }

    /// Temporarily suspend the spinner to print other output
    pub fn suspend<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        if let Some(ref spinner) = self.spinner {
            spinner.suspend(f)
        } else {
            f()
        }
    }

    /// Finish and clear the progress indicator
    pub fn finish(&self) {
        if let Some(ref spinner) = self.spinner {
            spinner.finish_and_clear();
        }
    }

    /// Check if progress indicator is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Drop for ProgressIndicator {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_indicator_disabled() {
        let progress = ProgressIndicator::new(false);
        assert!(!progress.is_enabled());

        // Should not panic
        progress.set_message("test");
        progress.suspend(|| {});
        progress.finish();
    }

    #[test]
    fn test_progress_indicator_enabled() {
        let progress = ProgressIndicator::new(true);
        assert!(progress.is_enabled());

        progress.set_message("Testing");
        progress.suspend(|| {
            println!("Suspended output");
        });
    }
}
