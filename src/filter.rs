// src/filter.rs
//! Root domain filtering for output

use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Filter that checks if domains belong to specified root domains
#[derive(Clone)]
pub struct RootDomainFilter {
    roots: HashSet<String>,
}

impl RootDomainFilter {
    /// Create a filter from a file containing root domains (one per line)
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let roots = content
            .lines()
            .map(|l| l.trim().to_lowercase())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();

        Ok(Self { roots })
    }

    /// Create a filter from a list of root domains
    pub fn from_list(domains: Vec<String>) -> Self {
        let roots = domains
            .into_iter()
            .map(|d| d.to_lowercase())
            .collect();

        Self { roots }
    }

    /// Check if a domain should be emitted based on root domain filter
    ///
    /// Returns true if the domain matches any root domain (exact or subdomain)
    pub fn should_emit(&self, domain: &str) -> bool {
        let domain_lower = domain.to_lowercase();

        for root in &self.roots {
            // Exact match
            if domain_lower == *root {
                return true;
            }

            // Subdomain match
            if domain_lower.ends_with(&format!(".{}", root)) {
                return true;
            }
        }

        false
    }

    /// Get the number of root domains in the filter
    pub fn count(&self) -> usize {
        self.roots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_from_list() {
        let filter = RootDomainFilter::from_list(vec![
            "example.com".to_string(),
            "test.org".to_string(),
        ]);

        assert_eq!(filter.count(), 2);
        assert!(filter.should_emit("example.com"));
        assert!(filter.should_emit("www.example.com"));
        assert!(filter.should_emit("test.org"));
        assert!(!filter.should_emit("other.com"));
    }

    #[test]
    fn test_from_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "example.com").unwrap();
        writeln!(temp_file, "# comment line").unwrap();
        writeln!(temp_file, "").unwrap();
        writeln!(temp_file, "test.org").unwrap();
        temp_file.flush().unwrap();

        let filter = RootDomainFilter::from_file(temp_file.path()).unwrap();

        assert_eq!(filter.count(), 2);
        assert!(filter.should_emit("example.com"));
        assert!(filter.should_emit("test.org"));
    }

    #[test]
    fn test_exact_match() {
        let filter = RootDomainFilter::from_list(vec!["example.com".to_string()]);

        assert!(filter.should_emit("example.com"));
        assert!(filter.should_emit("EXAMPLE.COM")); // Case insensitive
    }

    #[test]
    fn test_subdomain_match() {
        let filter = RootDomainFilter::from_list(vec!["example.com".to_string()]);

        assert!(filter.should_emit("www.example.com"));
        assert!(filter.should_emit("api.example.com"));
        assert!(filter.should_emit("deep.sub.example.com"));
    }

    #[test]
    fn test_no_match() {
        let filter = RootDomainFilter::from_list(vec!["example.com".to_string()]);

        assert!(!filter.should_emit("example.org"));
        assert!(!filter.should_emit("notexample.com"));
        assert!(!filter.should_emit("examplecom"));
    }

    #[test]
    fn test_case_insensitive() {
        let filter = RootDomainFilter::from_list(vec!["Example.COM".to_string()]);

        assert!(filter.should_emit("example.com"));
        assert!(filter.should_emit("WWW.EXAMPLE.COM"));
        assert!(filter.should_emit("Api.Example.Com"));
    }
}
