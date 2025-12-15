// src/watchlist.rs
use crate::config::{ProgramConfig, WatchlistConfig};
use ipnet::IpNet;
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct Program {
    pub name: String,
    pub domains: Vec<String>, // suffixes like ".hilton.com"
    pub hosts: Vec<String>,   // exact hostnames
    pub ips: Vec<IpAddr>,     // specific IP addresses
    pub cidrs: Vec<IpNet>,    // IP ranges
}

#[derive(Debug, Clone)]
pub struct Watchlist {
    pub global_domains: Vec<String>, // suffixes, e.g. ".world.org"
    pub global_hosts: Vec<String>,   // exact names
    pub global_ips: Vec<IpAddr>,
    pub global_cidrs: Vec<IpNet>,
    pub programs: Vec<Program>,
}

impl Watchlist {
    pub fn from_config(wl: &WatchlistConfig, progs: &[ProgramConfig]) -> anyhow::Result<Self> {
        let global_ips = wl
            .ips
            .iter()
            .map(|s| s.parse())
            .collect::<Result<Vec<_>, _>>()?;

        let global_cidrs = wl
            .cidrs
            .iter()
            .map(|s| s.parse())
            .collect::<Result<Vec<_>, _>>()?;

        let programs = progs
            .iter()
            .map(|p| {
                let ips = p
                    .ips
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<Vec<_>, _>>()?;
                let cidrs = p
                    .cidrs
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Program {
                    name: p.name.clone(),
                    domains: p.domains.clone(),
                    hosts: p.hosts.clone(),
                    ips,
                    cidrs,
                })
            })
            .collect::<Result<Vec<_>, anyhow::Error>>()?;

        Ok(Watchlist {
            global_domains: wl.domains.clone(),
            global_hosts: wl.hosts.clone(),
            global_ips,
            global_cidrs,
            programs,
        })
    }

    pub fn matches_domain(&self, domain: &str) -> bool {
        let host = domain.to_ascii_lowercase();

        // Check exact host match in global watchlist
        if self.global_hosts.iter().any(|h| h.eq_ignore_ascii_case(&host)) {
            return true;
        }

        // Check wildcard/suffix patterns in global_domains
        if self.global_domains.iter().any(|pattern| {
            Self::matches_pattern(&host, pattern)
        }) {
            return true;
        }

        // Check program-specific hosts and domains
        for program in &self.programs {
            // Check exact host match
            if program.hosts.iter().any(|h| h.eq_ignore_ascii_case(&host)) {
                return true;
            }

            // Check domain patterns
            if program.domains.iter().any(|pattern| {
                Self::matches_pattern(&host, pattern)
            }) {
                return true;
            }
        }

        false
    }

    /// Match a hostname against a pattern (wildcard or suffix)
    /// Patterns can be:
    /// - "*.example.com" - wildcard, matches "foo.example.com" but NOT "example.com"
    /// - ".example.com" - suffix, matches "foo.example.com" AND "example.com"
    /// - "example.com" - exact match or suffix match
    fn matches_pattern(host: &str, pattern: &str) -> bool {
        let pattern_lower = pattern.to_ascii_lowercase();

        // Wildcard pattern: "*.example.com"
        if let Some(suffix) = pattern_lower.strip_prefix("*.") {
            // Must have at least one subdomain
            return host.ends_with(&format!(".{}", suffix));
        }

        // Suffix pattern: ".example.com"
        if let Some(suffix) = pattern_lower.strip_prefix('.') {
            // Matches both "example.com" and "foo.example.com"
            return host == suffix || host.ends_with(&format!(".{}", suffix));
        }

        // Plain pattern: "example.com" - treat as suffix match
        host == pattern_lower || host.ends_with(&format!(".{}", pattern_lower))
    }

    pub fn program_for_domain(&self, domain: &str) -> Option<&Program> {
        let host = domain.to_ascii_lowercase();
        for program in &self.programs {
            // Check exact host match first
            if program.hosts.iter().any(|h| h.eq_ignore_ascii_case(&host)) {
                return Some(program);
            }

            // Check domain patterns
            for pattern in &program.domains {
                if Self::matches_pattern(&host, pattern) {
                    return Some(program);
                }
            }
        }
        None
    }

    /// Check if an IP address matches any in the global watchlist or programs
    pub fn matches_ip(&self, ip: &IpAddr) -> bool {
        // Check exact IP match in global watchlist
        if self.global_ips.contains(ip) {
            return true;
        }

        // Check CIDR ranges in global watchlist
        if self.global_cidrs.iter().any(|cidr| cidr.contains(ip)) {
            return true;
        }

        // Check program-specific IPs and CIDRs
        for program in &self.programs {
            // Check exact IP match
            if program.ips.contains(ip) {
                return true;
            }

            // Check CIDR ranges
            if program.cidrs.iter().any(|cidr| cidr.contains(ip)) {
                return true;
            }
        }

        false
    }

    /// Find which program (if any) an IP belongs to based on exact IP or CIDR ranges
    pub fn program_for_ip(&self, ip: &IpAddr) -> Option<&Program> {
        for program in &self.programs {
            // Check exact IP match
            if program.ips.contains(ip) {
                return Some(program);
            }

            // Check CIDR ranges
            if program.cidrs.iter().any(|cidr| cidr.contains(ip)) {
                return Some(program);
            }
        }
        None
    }

    /// Add a domain to a program, creating the program if it doesn't exist
    pub fn add_domain_to_program(&mut self, domain: &str, program_name: &str) {
        if let Some(program) = self.programs.iter_mut().find(|p| p.name == program_name) {
            if !program.domains.contains(&domain.to_string()) {
                program.domains.push(domain.to_string());
            }
        } else {
            self.programs.push(Program {
                name: program_name.to_string(),
                domains: vec![domain.to_string()],
                hosts: Vec::new(),
                ips: Vec::new(),
                cidrs: Vec::new(),
            });
        }
    }

    /// Add a host to a program, creating the program if it doesn't exist
    pub fn add_host_to_program(&mut self, host: &str, program_name: &str) {
        if let Some(program) = self.programs.iter_mut().find(|p| p.name == program_name) {
            if !program.hosts.contains(&host.to_string()) {
                program.hosts.push(host.to_string());
            }
        } else {
            self.programs.push(Program {
                name: program_name.to_string(),
                domains: Vec::new(),
                hosts: vec![host.to_string()],
                ips: Vec::new(),
                cidrs: Vec::new(),
            });
        }
    }

    /// Get all programs
    pub fn programs(&self) -> &[Program] {
        &self.programs
    }

    /// Export watchlist to TOML format
    pub fn export_to_toml(&self) -> String {
        let mut output = String::new();

        // Export global watchlist
        output.push_str("[watchlist]\n");
        output.push_str(&format!("domains = {:?}\n", self.global_domains));
        output.push_str(&format!("hosts = {:?}\n", self.global_hosts));

        let global_ips_str: Vec<String> = self.global_ips.iter().map(|ip| ip.to_string()).collect();
        output.push_str(&format!("ips = {:?}\n", global_ips_str));

        let global_cidrs_str: Vec<String> = self.global_cidrs.iter().map(|cidr| cidr.to_string()).collect();
        output.push_str(&format!("cidrs = {:?}\n", global_cidrs_str));

        output.push('\n');

        // Export programs
        for program in &self.programs {
            output.push_str("[[programs]]\n");
            output.push_str(&format!("name = \"{}\"\n", program.name));
            output.push_str(&format!("domains = {:?}\n", program.domains));
            output.push_str(&format!("hosts = {:?}\n", program.hosts));

            let program_ips_str: Vec<String> = program.ips.iter().map(|ip| ip.to_string()).collect();
            output.push_str(&format!("ips = {:?}\n", program_ips_str));

            let program_cidrs_str: Vec<String> = program.cidrs.iter().map(|cidr| cidr.to_string()).collect();
            output.push_str(&format!("cidrs = {:?}\n", program_cidrs_str));

            output.push('\n');
        }

        output
    }
}

impl Default for Watchlist {
    fn default() -> Self {
        Self {
            global_domains: Vec::new(),
            global_hosts: Vec::new(),
            global_ips: Vec::new(),
            global_cidrs: Vec::new(),
            programs: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProgramConfig, WatchlistConfig};

    fn create_test_watchlist() -> Watchlist {
        let watchlist_config = WatchlistConfig {
            domains: vec![
                "*.ibm.com".to_string(),
                ".hilton.com".to_string(),
                "example.com".to_string(),
            ],
            hosts: vec![
                "exact.host.com".to_string(),
                "api.service.io".to_string(),
            ],
            ips: vec![
                "192.168.1.1".to_string(),
                "10.0.0.5".to_string(),
            ],
            cidrs: vec![
                "172.16.0.0/12".to_string(),
                "203.79.37.0/29".to_string(),
            ],
        };

        let programs = vec![
            ProgramConfig {
                name: "IBM".to_string(),
                domains: vec![".ibm.com".to_string()],
                cidrs: vec![],
            },
            ProgramConfig {
                name: "Hilton".to_string(),
                domains: vec![".hilton.com".to_string(), ".hilton.io".to_string()],
                cidrs: vec!["192.251.125.0/24".to_string()],
            },
        ];

        Watchlist::from_config(&watchlist_config, &programs).unwrap()
    }

    #[test]
    fn test_wildcard_pattern_matching() {
        let watchlist = create_test_watchlist();

        // "*.ibm.com" should match subdomains but NOT the domain itself
        assert!(watchlist.matches_domain("foo.ibm.com"));
        assert!(watchlist.matches_domain("bar.baz.ibm.com"));
        assert!(watchlist.matches_domain("www.ibm.com"));
        assert!(!watchlist.matches_domain("ibm.com"));
    }

    #[test]
    fn test_suffix_pattern_matching() {
        let watchlist = create_test_watchlist();

        // ".hilton.com" should match both the domain and subdomains
        assert!(watchlist.matches_domain("hilton.com"));
        assert!(watchlist.matches_domain("www.hilton.com"));
        assert!(watchlist.matches_domain("api.hotels.hilton.com"));
    }

    #[test]
    fn test_plain_domain_matching() {
        let watchlist = create_test_watchlist();

        // "example.com" should match itself and subdomains
        assert!(watchlist.matches_domain("example.com"));
        assert!(watchlist.matches_domain("www.example.com"));
        assert!(watchlist.matches_domain("api.example.com"));
    }

    #[test]
    fn test_exact_host_matching() {
        let watchlist = create_test_watchlist();

        // Exact hosts should match only exact strings
        assert!(watchlist.matches_domain("exact.host.com"));
        assert!(watchlist.matches_domain("api.service.io"));

        // Should not match subdomains
        assert!(!watchlist.matches_domain("subdomain.exact.host.com"));
        assert!(!watchlist.matches_domain("foo.api.service.io"));
    }

    #[test]
    fn test_case_insensitive_matching() {
        let watchlist = create_test_watchlist();

        assert!(watchlist.matches_domain("FOO.IBM.COM"));
        assert!(watchlist.matches_domain("Www.Hilton.Com"));
        assert!(watchlist.matches_domain("EXACT.HOST.COM"));
    }

    #[test]
    fn test_no_match() {
        let watchlist = create_test_watchlist();

        assert!(!watchlist.matches_domain("notinlist.com"));
        assert!(!watchlist.matches_domain("fake-ibm.com"));
        assert!(!watchlist.matches_domain("ibmfake.com"));
    }

    #[test]
    fn test_program_for_domain() {
        let watchlist = create_test_watchlist();

        let program = watchlist.program_for_domain("www.ibm.com");
        assert!(program.is_some());
        assert_eq!(program.unwrap().name, "IBM");

        let program = watchlist.program_for_domain("hotels.hilton.com");
        assert!(program.is_some());
        assert_eq!(program.unwrap().name, "Hilton");

        let program = watchlist.program_for_domain("subdomain.hilton.io");
        assert!(program.is_some());
        assert_eq!(program.unwrap().name, "Hilton");

        let program = watchlist.program_for_domain("notinanyprogram.com");
        assert!(program.is_none());
    }

    #[test]
    fn test_ip_exact_match() {
        let watchlist = create_test_watchlist();

        let ip1: IpAddr = "192.168.1.1".parse().unwrap();
        let ip2: IpAddr = "10.0.0.5".parse().unwrap();
        let ip3: IpAddr = "8.8.8.8".parse().unwrap();

        assert!(watchlist.matches_ip(&ip1));
        assert!(watchlist.matches_ip(&ip2));
        assert!(!watchlist.matches_ip(&ip3));
    }

    #[test]
    fn test_cidr_matching() {
        let watchlist = create_test_watchlist();

        // 172.16.0.0/12 includes 172.16.0.0 - 172.31.255.255
        let ip_in_range: IpAddr = "172.16.0.1".parse().unwrap();
        let ip_in_range2: IpAddr = "172.31.255.254".parse().unwrap();
        let ip_out_range: IpAddr = "172.32.0.1".parse().unwrap();

        assert!(watchlist.matches_ip(&ip_in_range));
        assert!(watchlist.matches_ip(&ip_in_range2));
        assert!(!watchlist.matches_ip(&ip_out_range));

        // 203.79.37.0/29 includes 203.79.37.0 - 203.79.37.7
        let ip_in_small: IpAddr = "203.79.37.5".parse().unwrap();
        let ip_out_small: IpAddr = "203.79.37.10".parse().unwrap();

        assert!(watchlist.matches_ip(&ip_in_small));
        assert!(!watchlist.matches_ip(&ip_out_small));
    }

    #[test]
    fn test_program_for_ip() {
        let watchlist = create_test_watchlist();

        // 192.251.125.0/24 is in Hilton program
        let ip_hilton: IpAddr = "192.251.125.100".parse().unwrap();
        let program = watchlist.program_for_ip(&ip_hilton);
        assert!(program.is_some());
        assert_eq!(program.unwrap().name, "Hilton");

        // IP not in any program
        let ip_none: IpAddr = "8.8.8.8".parse().unwrap();
        assert!(watchlist.program_for_ip(&ip_none).is_none());
    }

    #[test]
    fn test_invalid_cidr_parsing() {
        let watchlist_config = WatchlistConfig {
            domains: vec![],
            hosts: vec![],
            ips: vec![],
            cidrs: vec!["invalid_cidr".to_string()],
        };

        let result = Watchlist::from_config(&watchlist_config, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_ip_parsing() {
        let watchlist_config = WatchlistConfig {
            domains: vec![],
            hosts: vec![],
            ips: vec!["not.an.ip".to_string()],
            cidrs: vec![],
        };

        let result = Watchlist::from_config(&watchlist_config, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_watchlist() {
        let watchlist_config = WatchlistConfig::default();
        let watchlist = Watchlist::from_config(&watchlist_config, &[]).unwrap();

        assert!(!watchlist.matches_domain("anything.com"));
        assert!(!watchlist.matches_ip(&"1.2.3.4".parse().unwrap()));
        assert!(watchlist.program_for_domain("anything.com").is_none());
    }

    #[test]
    fn test_add_domain_to_program() {
        let mut watchlist = Watchlist::default();
        watchlist.add_domain_to_program("*.example.com", "Test Program");

        assert!(watchlist.matches_domain("sub.example.com"));
        assert!(watchlist.program_for_domain("sub.example.com").is_some());
    }
}
