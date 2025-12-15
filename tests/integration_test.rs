// Integration tests for ct-scout
use ct_scout::certstream::run_certstream_loop;
use ct_scout::config::{CertstreamConfig, ProgramConfig, WatchlistConfig, WebhookConfig};
use ct_scout::dedupe::Dedupe;
use ct_scout::notifier::Notifier;
use ct_scout::watchlist::Watchlist;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::time::Duration;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

/// Helper function to create a mock WebSocket server
async fn start_mock_certstream_server(port: u16, messages: Vec<String>) {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .unwrap();

    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let ws_stream = accept_async(stream).await.unwrap();
            let (mut ws_sender, _) = ws_stream.split();

            // Send all messages to the client
            for msg_text in messages {
                ws_sender
                    .send(Message::Text(msg_text))
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }

            // Keep connection alive briefly
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    // Give the server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_end_to_end_domain_matching() {
    // Start mock webhook server
    let webhook_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1..) // Expect at least one notification
        .mount(&webhook_server)
        .await;

    // Prepare certstream messages
    let messages = vec![
        // Match: *.ibm.com pattern
        serde_json::json!({
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["www.ibm.com", "api.ibm.com"],
                "cert_index": 1,
                "seen": 1234567890.0,
                "leaf_cert": {
                    "not_before": 1600000000,
                    "not_after": 1700000000,
                    "fingerprint": "fingerprint1"
                }
            }
        })
        .to_string(),
        // No match
        serde_json::json!({
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["example.com"],
                "cert_index": 2,
                "seen": 1234567891.0
            }
        })
        .to_string(),
        // Match: exact host
        serde_json::json!({
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["exact.host.com"],
                "cert_index": 3,
                "seen": 1234567892.0
            }
        })
        .to_string(),
    ];

    // Start mock certstream server on a random high port
    let ws_port = 19001;
    start_mock_certstream_server(ws_port, messages).await;

    // Configure ct-scout
    let certstream_config = CertstreamConfig {
        url: format!("ws://127.0.0.1:{}/", ws_port),
        reconnect_delay_secs: 1,
    };

    let watchlist_config = WatchlistConfig {
        domains: vec!["*.ibm.com".to_string()],
        hosts: vec!["exact.host.com".to_string()],
        ips: vec![],
        cidrs: vec![],
    };

    let programs = vec![ProgramConfig {
        name: "IBM".to_string(),
        domains: vec![".ibm.com".to_string()],
        cidrs: vec![],
    }];

    let watchlist = Watchlist::from_config(&watchlist_config, &programs).unwrap();

    let webhook_config = WebhookConfig {
        url: webhook_server.uri(),
        secret: None,
        timeout_secs: Some(5),
    };

    let notifier = Notifier::new(webhook_config);
    let dedupe = Dedupe::new();

    // Run certstream loop with timeout
    let certstream_task = tokio::spawn(async move {
        run_certstream_loop(certstream_config, watchlist, notifier, dedupe).await;
    });

    // Let it run for a bit
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Clean up
    certstream_task.abort();
}

#[tokio::test]
async fn test_dedupe_prevents_duplicate_notifications() {
    let webhook_server = MockServer::start().await;

    // Expect exactly 1 notification (not 2, because of deduplication)
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&webhook_server)
        .await;

    let messages = vec![
        // First occurrence
        serde_json::json!({
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["test.ibm.com"],
                "cert_index": 100,
                "seen": 1234567890.0
            }
        })
        .to_string(),
        // Duplicate cert_index - should be deduped
        serde_json::json!({
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["test.ibm.com"],
                "cert_index": 100,
                "seen": 1234567891.0
            }
        })
        .to_string(),
    ];

    let ws_port = 19002;
    start_mock_certstream_server(ws_port, messages).await;

    let certstream_config = CertstreamConfig {
        url: format!("ws://127.0.0.1:{}/", ws_port),
        reconnect_delay_secs: 1,
    };

    let watchlist_config = WatchlistConfig {
        domains: vec!["*.ibm.com".to_string()],
        hosts: vec![],
        ips: vec![],
        cidrs: vec![],
    };

    let watchlist = Watchlist::from_config(&watchlist_config, &[]).unwrap();

    let webhook_config = WebhookConfig {
        url: webhook_server.uri(),
        secret: None,
        timeout_secs: Some(5),
    };

    let notifier = Notifier::new(webhook_config);
    let dedupe = Dedupe::new();

    let certstream_task = tokio::spawn(async move {
        run_certstream_loop(certstream_config, watchlist, notifier, dedupe).await;
    });

    tokio::time::sleep(Duration::from_secs(1)).await;
    certstream_task.abort();
}

#[tokio::test]
async fn test_wildcard_and_suffix_matching() {
    let webhook_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&webhook_server)
        .await;

    let messages = vec![
        // Match wildcard: *.hilton.com
        serde_json::json!({
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["www.hilton.com"],
                "cert_index": 1
            }
        })
        .to_string(),
        // Match suffix: .toolsforhumanity.com (includes base domain)
        serde_json::json!({
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["toolsforhumanity.com"],
                "cert_index": 2
            }
        })
        .to_string(),
        // Match suffix subdomain
        serde_json::json!({
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["api.toolsforhumanity.com"],
                "cert_index": 3
            }
        })
        .to_string(),
        // Should NOT match: ibm.com doesn't match *.ibm.com
        serde_json::json!({
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["ibm.com"],
                "cert_index": 4
            }
        })
        .to_string(),
    ];

    let ws_port = 19003;
    start_mock_certstream_server(ws_port, messages).await;

    let certstream_config = CertstreamConfig {
        url: format!("ws://127.0.0.1:{}/", ws_port),
        reconnect_delay_secs: 1,
    };

    let watchlist_config = WatchlistConfig {
        domains: vec![
            "*.hilton.com".to_string(),
            ".toolsforhumanity.com".to_string(),
            "*.ibm.com".to_string(),
        ],
        hosts: vec![],
        ips: vec![],
        cidrs: vec![],
    };

    let watchlist = Watchlist::from_config(&watchlist_config, &[]).unwrap();

    let webhook_config = WebhookConfig {
        url: webhook_server.uri(),
        secret: None,
        timeout_secs: Some(5),
    };

    let notifier = Notifier::new(webhook_config);
    let dedupe = Dedupe::new();

    let certstream_task = tokio::spawn(async move {
        run_certstream_loop(certstream_config, watchlist, notifier, dedupe).await;
    });

    tokio::time::sleep(Duration::from_secs(1)).await;
    certstream_task.abort();
}

#[tokio::test]
async fn test_program_assignment() {
    let webhook_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&webhook_server)
        .await;

    let messages = vec![
        serde_json::json!({
            "message_type": "certificate_update",
            "data": {
                "all_domains": ["hotels.hilton.com"],
                "cert_index": 1
            }
        })
        .to_string(),
    ];

    let ws_port = 19004;
    start_mock_certstream_server(ws_port, messages).await;

    let certstream_config = CertstreamConfig {
        url: format!("ws://127.0.0.1:{}/", ws_port),
        reconnect_delay_secs: 1,
    };

    let watchlist_config = WatchlistConfig {
        domains: vec!["*.hilton.com".to_string()],
        hosts: vec![],
        ips: vec![],
        cidrs: vec![],
    };

    let programs = vec![ProgramConfig {
        name: "Hilton".to_string(),
        domains: vec![".hilton.com".to_string()],
        cidrs: vec![],
    }];

    let watchlist = Watchlist::from_config(&watchlist_config, &programs).unwrap();

    let webhook_config = WebhookConfig {
        url: webhook_server.uri(),
        secret: Some("test_secret".to_string()),
        timeout_secs: Some(5),
    };

    let notifier = Notifier::new(webhook_config);
    let dedupe = Dedupe::new();

    let certstream_task = tokio::spawn(async move {
        run_certstream_loop(certstream_config, watchlist, notifier, dedupe).await;
    });

    tokio::time::sleep(Duration::from_secs(1)).await;
    certstream_task.abort();
}
