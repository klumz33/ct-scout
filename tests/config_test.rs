// Test configuration loading
use ct_scout::config::Config;
use std::path::Path;

#[test]
fn test_load_test_config() {
    let config_path = Path::new("tests/test_config.toml");
    let config = Config::from_file(config_path).expect("Failed to load test config");

    // Verify certstream config
    assert_eq!(config.certstream.url, "ws://127.0.0.1:4000/full-stream");
    assert_eq!(config.certstream.reconnect_delay_secs, 5);

    // Verify webhook config
    assert_eq!(config.webhook.url, "https://example.com/webhook");
    assert_eq!(config.webhook.secret, Some("test_secret_key".to_string()));
    assert_eq!(config.webhook.timeout_secs, Some(10));

    // Verify logging config
    assert_eq!(config.logging.level, "info");

    // Verify watchlist config
    assert_eq!(config.watchlist.domains.len(), 3);
    assert!(config.watchlist.domains.contains(&"*.ibm.com".to_string()));
    assert!(config.watchlist.domains.contains(&".hilton.com".to_string()));

    assert_eq!(config.watchlist.hosts.len(), 2);
    assert!(config.watchlist.hosts.contains(&"exact.example.com".to_string()));

    assert_eq!(config.watchlist.ips.len(), 2);
    assert_eq!(config.watchlist.cidrs.len(), 2);

    // Verify programs
    assert_eq!(config.programs.len(), 3);

    let ibm_program = config.programs.iter().find(|p| p.name == "IBM").unwrap();
    assert_eq!(ibm_program.domains.len(), 1);
    assert_eq!(ibm_program.cidrs.len(), 0);

    let hilton_program = config.programs.iter().find(|p| p.name == "Hilton").unwrap();
    assert_eq!(hilton_program.domains.len(), 2);
    assert_eq!(hilton_program.cidrs.len(), 2);
}

#[test]
fn test_watchlist_from_config() {
    let config_path = Path::new("tests/test_config.toml");
    let config = Config::from_file(config_path).unwrap();

    let watchlist = ct_scout::watchlist::Watchlist::from_config(
        &config.watchlist,
        &config.programs,
    )
    .expect("Failed to create watchlist");

    // Test domain matching
    assert!(watchlist.matches_domain("www.ibm.com"));
    assert!(watchlist.matches_domain("hilton.com"));
    assert!(watchlist.matches_domain("api.hilton.com"));
    assert!(watchlist.matches_domain("exact.example.com"));

    // Test exact host that shouldn't match subdomains
    assert!(!watchlist.matches_domain("sub.exact.example.com"));

    // Test wildcard that shouldn't match base domain
    assert!(!watchlist.matches_domain("ibm.com")); // *.ibm.com doesn't match ibm.com itself

    // Test IP matching
    let ip1: std::net::IpAddr = "192.168.1.100".parse().unwrap();
    assert!(watchlist.matches_ip(&ip1));

    let ip_in_cidr: std::net::IpAddr = "172.16.5.10".parse().unwrap();
    assert!(watchlist.matches_ip(&ip_in_cidr));

    let ip_not_in_list: std::net::IpAddr = "8.8.8.8".parse().unwrap();
    assert!(!watchlist.matches_ip(&ip_not_in_list));

    // Test program assignment
    let program = watchlist.program_for_domain("www.ibm.com");
    assert!(program.is_some());
    assert_eq!(program.unwrap().name, "IBM");

    let program = watchlist.program_for_domain("hotels.hilton.com");
    assert!(program.is_some());
    assert_eq!(program.unwrap().name, "Hilton");

    // Test IP program assignment
    let hilton_ip: std::net::IpAddr = "192.251.125.50".parse().unwrap();
    let program = watchlist.program_for_ip(&hilton_ip);
    assert!(program.is_some());
    assert_eq!(program.unwrap().name, "Hilton");
}
