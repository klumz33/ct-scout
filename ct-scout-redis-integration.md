# ct-scout Redis Pub/Sub Integration

This adds direct Redis pub/sub support to ct-scout, eliminating the webhook overhead.

## Why Direct Redis?

```
BEFORE (webhook):
  ct-scout → HTTP POST → webhook receiver → Redis → workers

AFTER (direct):
  ct-scout → Redis → workers
```

Benefits:
- Lower latency (~50ms vs ~200ms)
- No additional service to maintain
- Built-in retry/reconnection
- Works with Upstash Redis (serverless)

---

## Option 1: Add to ct-scout Rust Code

### 1. Add dependencies to `Cargo.toml`

```toml
[dependencies]
# ... existing deps ...
redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }
```

### 2. Create `src/redis_publisher.rs`

```rust
//! Redis Pub/Sub publisher for ct-scout matches
//! 
//! Publishes certificate matches directly to Redis channels,
//! enabling real-time integration with automation pipelines.

use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Redis publisher configuration
#[derive(Debug, Clone)]
pub struct RedisConfig {
    /// Redis URL (supports Upstash format: rediss://...)
    pub url: String,
    /// Optional auth token (for Upstash)
    pub token: Option<String>,
    /// Channel name for CT events
    pub channel: String,
    /// Also push to a list for persistence (optional)
    pub queue_name: Option<String>,
    /// Maximum queue size (older items evicted)
    pub max_queue_size: Option<i64>,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://localhost:6379".to_string(),
            token: None,
            channel: "bb:ct_events".to_string(),
            queue_name: Some("bb:ct_events_queue".to_string()),
            max_queue_size: Some(10000),
        }
    }
}

/// Message published to Redis
#[derive(Debug, Clone, Serialize)]
pub struct CTEventMessage {
    /// Event type (always "ct_match")
    pub event_type: String,
    /// Unix timestamp
    pub timestamp: i64,
    /// Primary matched domain
    pub matched_domain: String,
    /// All domains in the certificate (SANs + CN)
    pub all_domains: Vec<String>,
    /// Certificate index in the CT log
    pub cert_index: u64,
    /// Certificate validity start (Unix timestamp)
    pub not_before: i64,
    /// Certificate validity end (Unix timestamp)  
    pub not_after: i64,
    /// SHA-256 fingerprint of the certificate
    pub fingerprint: String,
    /// Bug bounty program name (if configured)
    pub program_name: Option<String>,
    /// CT log URL where this was found
    pub ct_log: String,
    /// Issuer common name
    pub issuer: Option<String>,
    /// Is this a precertificate?
    pub is_precert: bool,
}

/// Redis publisher with automatic reconnection
pub struct RedisPublisher {
    config: RedisConfig,
    connection: Arc<RwLock<Option<ConnectionManager>>>,
    connected: Arc<RwLock<bool>>,
}

impl RedisPublisher {
    /// Create a new Redis publisher
    pub fn new(config: RedisConfig) -> Self {
        Self {
            config,
            connection: Arc::new(RwLock::new(None)),
            connected: Arc::new(RwLock::new(false)),
        }
    }

    /// Connect to Redis (with Upstash support)
    pub async fn connect(&self) -> Result<(), redis::RedisError> {
        let url = if let Some(ref token) = self.config.token {
            // Upstash format: rediss://default:TOKEN@host:port
            if self.config.url.contains("@") {
                self.config.url.clone()
            } else {
                // Insert token into URL
                self.config.url.replace("rediss://", &format!("rediss://default:{}@", token))
            }
        } else {
            self.config.url.clone()
        };

        info!("Connecting to Redis...");
        
        let client = redis::Client::open(url)?;
        let manager = ConnectionManager::new(client).await?;
        
        // Test connection
        let mut conn = manager.clone();
        redis::cmd("PING").query_async::<String>(&mut conn).await?;
        
        *self.connection.write().await = Some(manager);
        *self.connected.write().await = true;
        
        info!("Redis connected successfully");
        Ok(())
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Publish a CT match event
    pub async fn publish(&self, event: CTEventMessage) -> Result<(), redis::RedisError> {
        let conn_guard = self.connection.read().await;
        let conn = match conn_guard.as_ref() {
            Some(c) => c.clone(),
            None => {
                error!("Redis not connected");
                return Err(redis::RedisError::from((
                    redis::ErrorKind::IoError,
                    "Not connected",
                )));
            }
        };
        drop(conn_guard);

        let payload = serde_json::to_string(&event)
            .map_err(|e| redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Serialization failed",
                e.to_string(),
            )))?;

        let mut conn = conn;

        // Publish to channel (for real-time subscribers)
        let subscribers: i64 = conn.publish(&self.config.channel, &payload).await?;
        debug!(
            "Published to channel {} ({} subscribers)",
            self.config.channel, subscribers
        );

        // Also push to queue for persistence (if configured)
        if let Some(ref queue_name) = self.config.queue_name {
            conn.lpush::<_, _, ()>(queue_name, &payload).await?;
            
            // Trim queue to max size
            if let Some(max_size) = self.config.max_queue_size {
                conn.ltrim::<_, ()>(queue_name, 0, max_size - 1).await?;
            }
            
            debug!("Pushed to queue {}", queue_name);
        }

        Ok(())
    }

    /// Publish with automatic retry
    pub async fn publish_with_retry(&self, event: CTEventMessage, max_retries: u32) -> bool {
        for attempt in 0..max_retries {
            match self.publish(event.clone()).await {
                Ok(_) => return true,
                Err(e) => {
                    warn!(
                        "Redis publish failed (attempt {}/{}): {}",
                        attempt + 1,
                        max_retries,
                        e
                    );
                    
                    // Try to reconnect
                    if let Err(reconnect_err) = self.connect().await {
                        error!("Redis reconnection failed: {}", reconnect_err);
                    }
                    
                    tokio::time::sleep(tokio::time::Duration::from_millis(
                        100 * 2_u64.pow(attempt),
                    )).await;
                }
            }
        }
        
        error!("Redis publish failed after {} retries", max_retries);
        false
    }
}

/// Builder for CTEventMessage from ct-scout's internal types
impl CTEventMessage {
    pub fn from_match(
        matched_domain: String,
        all_domains: Vec<String>,
        cert_index: u64,
        not_before: i64,
        not_after: i64,
        fingerprint: String,
        program_name: Option<String>,
        ct_log: String,
        issuer: Option<String>,
        is_precert: bool,
    ) -> Self {
        Self {
            event_type: "ct_match".to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            matched_domain,
            all_domains,
            cert_index,
            not_before,
            not_after,
            fingerprint,
            program_name,
            ct_log,
            issuer,
            is_precert,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_serialization() {
        let event = CTEventMessage::from_match(
            "test.example.com".to_string(),
            vec!["test.example.com".to_string(), "www.test.example.com".to_string()],
            12345,
            1704067200,
            1735689600,
            "abc123def456".to_string(),
            Some("Example Program".to_string()),
            "https://ct.googleapis.com/logs/us1/argon2024/".to_string(),
            Some("Let's Encrypt".to_string()),
            false,
        );

        let json = serde_json::to_string_pretty(&event).unwrap();
        println!("{}", json);
        
        assert!(json.contains("ct_match"));
        assert!(json.contains("test.example.com"));
    }
}
```

### 3. Update `src/config.rs` (add Redis section)

Add to your config structure:

```rust
/// Redis configuration (optional)
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RedisConfigToml {
    /// Enable Redis publishing
    #[serde(default)]
    pub enabled: bool,
    
    /// Redis URL (supports rediss:// for TLS/Upstash)
    #[serde(default = "default_redis_url")]
    pub url: String,
    
    /// Auth token (for Upstash)
    pub token: Option<String>,
    
    /// Pub/sub channel name
    #[serde(default = "default_redis_channel")]
    pub channel: String,
    
    /// Queue name for persistence (optional)
    pub queue_name: Option<String>,
    
    /// Max queue size
    pub max_queue_size: Option<i64>,
}

fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

fn default_redis_channel() -> String {
    "bb:ct_events".to_string()
}
```

### 4. Integration in main.rs

In your match handling code (where you currently call the webhook), add:

```rust
// After finding a match...
if let Some(ref redis_publisher) = redis_publisher {
    let event = CTEventMessage::from_match(
        matched_domain.clone(),
        all_domains.clone(),
        cert_index,
        not_before,
        not_after,
        fingerprint.clone(),
        program_name.clone(),
        ct_log_url.clone(),
        issuer.clone(),
        is_precert,
    );
    
    // Fire and forget with retry
    let publisher = redis_publisher.clone();
    tokio::spawn(async move {
        publisher.publish_with_retry(event, 3).await;
    });
}
```

---

## Option 2: Sidecar Approach (No Code Changes)

If you want to avoid modifying ct-scout, use the existing webhook and a tiny bridge:

```rust
// redis-bridge.rs - Receives webhook, publishes to Redis
// Compile: cargo build --release

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize)]
struct CTEvent {
    timestamp: i64,
    matched_domain: String,
    all_domains: Vec<String>,
    cert_index: u64,
    not_before: i64,
    not_after: i64,
    fingerprint: String,
    program_name: Option<String>,
}

struct AppState {
    redis: redis::aio::ConnectionManager,
    channel: String,
}

async fn handle_webhook(
    State(state): State<Arc<AppState>>,
    Json(event): Json<CTEvent>,
) -> StatusCode {
    let payload = serde_json::to_string(&event).unwrap();
    let mut conn = state.redis.clone();
    
    match conn.publish::<_, _, i64>(&state.channel, &payload).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[tokio::main]
async fn main() {
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL required");
    let client = redis::Client::open(redis_url).unwrap();
    let conn = redis::aio::ConnectionManager::new(client).await.unwrap();
    
    let state = Arc::new(AppState {
        redis: conn,
        channel: "bb:ct_events".to_string(),
    });

    let app = Router::new()
        .route("/webhook/ct", post(handle_webhook))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:9000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

---

## Configuration Examples

### config.toml with Redis (Direct Integration)

```toml
[logging]
level = "info"

[redis]
enabled = true
url = "rediss://default:YOUR_UPSTASH_TOKEN@your-redis.upstash.io:6379"
channel = "bb:ct_events"
queue_name = "bb:ct_events_queue"
max_queue_size = 10000

[database]
enabled = true
url = "postgresql://user:pass@your-neon.neon.tech/ctscout?sslmode=require"

[platforms.hackerone]
enabled = true
username = "your-h1-username"
api_token = "your-h1-token"

[ct_logs]
include_all_logs = true
max_concurrent_logs = 100
parse_precerts = true
poll_interval_secs = 10

[output]
format = "silent"  # We're publishing to Redis, no need for stdout
```

### Environment Variables Alternative

```bash
export CT_SCOUT_REDIS_URL="rediss://default:token@host:6379"
export CT_SCOUT_REDIS_CHANNEL="bb:ct_events"
export CT_SCOUT_DATABASE_URL="postgresql://..."
```

---

## Testing the Integration

### 1. Subscribe to the channel (from another terminal):

```bash
# Using redis-cli
redis-cli -u $REDIS_URL SUBSCRIBE bb:ct_events

# Or with Upstash
redis-cli --tls -u $REDIS_URL SUBSCRIBE bb:ct_events
```

### 2. Run ct-scout:

```bash
./ct-scout --config config.toml
```

### 3. You should see messages like:

```json
{
  "event_type": "ct_match",
  "timestamp": 1704067200,
  "matched_domain": "new.target.com",
  "all_domains": ["new.target.com", "*.new.target.com"],
  "cert_index": 98765432,
  "not_before": 1704067200,
  "not_after": 1711929600,
  "fingerprint": "sha256:abcdef...",
  "program_name": "Target Bug Bounty",
  "ct_log": "https://ct.googleapis.com/logs/us1/argon2024/",
  "issuer": "Let's Encrypt",
  "is_precert": false
}
```

---

## Next Steps

After implementing Redis pub/sub in ct-scout:

1. The recon worker listens directly to Redis (no webhook receiver needed)
2. Events trigger recon pipeline immediately
3. Results flow to PostgreSQL

I recommend **Option 1** (direct integration) since you own the code and it's cleaner.

Want me to help with the actual PR/code changes to ct-scout?
