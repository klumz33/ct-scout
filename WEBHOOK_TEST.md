# Testing Webhook Notifications

## Quick Test

I've created a test script that starts a local webhook receiver and runs ct-scout:

```bash
./test_webhook.sh
```

This will:
1. Start a Python webhook receiver on http://localhost:8888/webhook
2. Create a test config that watches for *.google.com, *.amazon.com, *.microsoft.com
3. Run ct-scout with --stats and --verbose
4. Display any webhook notifications received

**Expected Output:**

```
🎉 WEBHOOK RECEIVED at 14:23:45
============================================================
  Matched Domain: www.google.com
  All Domains: www.google.com, google.com
  Cert Index: 12345678
  Program: Test Program
  HMAC Signature: a1b2c3d4e5f6...
============================================================
```

Press Ctrl+C when you see a webhook notification to verify it's working!

---

## Manual Test with Your Own Webhook

### Option 1: Using webhook.site

1. Go to https://webhook.site
2. Copy your unique URL (e.g., https://webhook.site/abcd-1234)
3. Run ct-scout:

```bash
./target/release/ct-scout --webhook https://webhook.site/abcd-1234 --stats
```

4. Watch the webhook.site page for incoming requests

### Option 2: Using RequestBin

1. Go to https://requestbin.com
2. Create a new bin
3. Copy the endpoint URL
4. Run ct-scout:

```bash
./target/release/ct-scout --webhook https://requestbin.com/r/your-bin --stats
```

### Option 3: Local Server (netcat)

Start a simple server:
```bash
nc -l 8888
```

In another terminal:
```bash
./target/release/ct-scout --webhook http://localhost:8888 --stats
```

---

## Testing with Custom Config

Create a config file `test-config.toml`:

```toml
[certstream]
url = "ws://127.0.0.1:4000/full-stream"
reconnect_delay_secs = 5

[webhook]
url = "https://webhook.site/YOUR-UNIQUE-ID"
secret = "test_secret_123"  # Optional HMAC secret
timeout_secs = 5

[logging]
level = "info"

[watchlist]
# Use common domains to get hits quickly
domains = ["*.google.com", "*.cloudflare.com", "*.amazon.com"]
hosts = []
ips = []
cidrs = []

[[programs]]
name = "Big Tech"
domains = [".google.com", ".amazon.com"]
cidrs = []
```

Run:
```bash
./target/release/ct-scout --config test-config.toml --stats
```

---

## Webhook Payload Format

ct-scout sends JSON POST requests:

```json
{
  "matched_domain": "www.example.com",
  "all_domains": ["www.example.com", "example.com"],
  "cert_index": 12345678,
  "not_before": 1640000000,
  "not_after": 1671536000,
  "program_name": "Example Program",
  "timestamp": 1640000100,
  "fingerprint": "a1:b2:c3:..."
}
```

### With HMAC Signature

If you configure a `secret`, ct-scout adds an `X-CTScout-Signature` header:

```
X-CTScout-Signature: a1b2c3d4e5f6789...
```

This is a HMAC-SHA256 hash of the request body, hex-encoded.

---

## Troubleshooting

### "Connection refused" error

Make sure your webhook endpoint is accessible. If testing locally:
- Check the port is not blocked by firewall
- Verify the server is running (`netstat -ln | grep 8888`)

### No notifications received

1. Check your watchlist patterns match common domains
2. Verify certstream is connected (look for "Connected" message)
3. Increase logging: `--verbose`
4. Try without webhook first: `--no-webhook` to verify matches are found

### Stats not showing

Make sure you use the `--stats` flag:
```bash
./target/release/ct-scout --stats
```

You should see output like:
```
[⠙] 1,234 processed | 56 matches | 123.4 msg/min | uptime: 1h 23m 45s
```

---

## Current Implementation Status

✅ **Working Now:**
- JSON/CSV/Human/Silent output formats
- Webhook notifications with HMAC signatures
- Live stats display (--stats flag)
- Root domain filtering (-r flag)
- Progress spinner with status updates

📋 **Deferred (Phase C):**
- Config file watching (--watch-config flag)
- Signal handling for graceful shutdown
- Final stats summary on exit

📋 **Deferred (Phase D):**
- Integration test updates
- Comprehensive README
- Example scripts directory
