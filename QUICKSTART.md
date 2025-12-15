# CT-Scout Quick Start

## ✅ What's Working Now

**Phase A + B Complete!** All core features are implemented and working:

- ✅ Multiple output formats (Human, JSON, CSV, Silent)
- ✅ Webhook notifications with HMAC signatures
- ✅ **Live stats display** (updates every 10 seconds)
- ✅ Progress indicator with spinner
- ✅ Root domain filtering
- ✅ All CLI flags working

## 🧪 Test Webhook Notifications

### **Option 1: Manual Test (Recommended)**

Open **two terminal windows**:

**Terminal 1** - Start webhook receiver:
```bash
cd /home/msuda/Documents/BBH/Tools/ct-scout

python3 -c "
import http.server
import socketserver
import json
from datetime import datetime

class WebhookHandler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers['Content-Length'])
        body = self.rfile.read(content_length)
        data = json.loads(body)
        timestamp = datetime.now().strftime('%H:%M:%S')

        print('\n' + '='*60)
        print(f'🎉 WEBHOOK RECEIVED at {timestamp}')
        print('='*60)
        print(f\"  Matched Domain: {data.get('matched_domain', 'N/A')}\")
        print(f\"  All Domains: {', '.join(data.get('all_domains', []))}\")
        print(f\"  Cert Index: {data.get('cert_index', 'N/A')}\")
        print(f\"  Program: {data.get('program_name', 'N/A')}\")
        print('='*60 + '\n')

        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'OK')

    def log_message(self, format, *args):
        pass

PORT = 8888
print('📡 Webhook receiver listening on http://localhost:8888')
print('Waiting for notifications...\n')

with socketserver.TCPServer(('', PORT), WebhookHandler) as httpd:
    httpd.serve_forever()
"
```

**Terminal 2** - Run ct-scout:
```bash
cd /home/msuda/Documents/BBH/Tools/ct-scout

# Create test config
cat > /tmp/test.toml <<EOF
[certstream]
url = "ws://127.0.0.1:4000/full-stream"
reconnect_delay_secs = 5

[webhook]
url = "http://localhost:8888"
timeout_secs = 5

[logging]
level = "info"

[watchlist]
domains = ["*.google.com", "*.amazon.com", "*.cloudflare.com"]
hosts = []
ips = []
cidrs = []

[[programs]]
name = "Big Tech"
domains = [".google.com"]
cidrs = []
EOF

# Run ct-scout with stats
./target/release/ct-scout --config /tmp/test.toml --stats
```

**What you should see:**

Terminal 1 (webhook receiver) will show:
```
============================================================
🎉 WEBHOOK RECEIVED at 18:23:45
============================================================
  Matched Domain: www.google.com
  All Domains: www.google.com, google.com
  Cert Index: 12345678
  Program: Big Tech
============================================================
```

Terminal 2 (ct-scout) will show:
```
[⠙] 1,234 processed | 12 matches | 123.4 msg/min | uptime: 1m 23s
[2025-12-10T18:23:45Z] [+] www.google.com
    Program: Big Tech
    All domains: www.google.com, google.com
```

**Press Ctrl+C in both terminals when done!**

---

### **Option 2: Using webhook.site**

Even simpler - no local server needed:

1. Go to https://webhook.site in your browser
2. Copy your unique URL (e.g., `https://webhook.site/abcd-1234`)
3. Run ct-scout:

```bash
./target/release/ct-scout \
  --webhook https://webhook.site/YOUR-UNIQUE-ID \
  --stats
```

4. Watch the webhook.site page for incoming requests!

---

## 🎯 Test Stats Display

The --stats flag now works with live updates:

```bash
./target/release/ct-scout --stats
```

You'll see:
```
[⠙] 1,234 processed | 56 matches | 123.4 msg/min | uptime: 1h 23m 45s
```

The stats update every 10 seconds (configurable with `--stats-interval 5`).

When you press Ctrl+C, final stats are shown:
```
📊 Final Statistics:
  Total processed: 1234
  Matches found: 56
  Rate: 123.4 msg/min
  Uptime: 1h 23m 45s
```

---

## 📋 All Available Commands

```bash
# Default: human-readable output
./target/release/ct-scout

# JSON pipeline mode
./target/release/ct-scout --json

# CSV export
./target/release/ct-scout --csv -o matches.csv

# With stats tracking
./target/release/ct-scout --stats

# Filter to specific domains
echo "ibm.com" > roots.txt
./target/release/ct-scout --root-domains roots.txt --stats

# Silent mode (webhooks only)
./target/release/ct-scout --silent --stats

# Disable webhooks
./target/release/ct-scout --no-webhook

# Multiple outputs: JSON + webhooks
./target/release/ct-scout --json --stats

# See all options
./target/release/ct-scout --help
```

---

## 🏗️ What's Deferred (Phase C & D)

Still need to complete (~1 hour of work):

**Phase C** - Polish features:
- ✅ ~~Stats display~~ (DONE!)
- 📋 Config file watching (`--watch-config`)
- 📋 Signal handling (graceful Ctrl+C shutdown)

**Phase D** - Testing & docs:
- 📋 Update integration tests for new API
- 📋 Full README with examples
- 📋 Example scripts directory

---

## 🐛 Troubleshooting

**"Connection refused" for webhook:**
- Make sure webhook receiver is running first
- Check port 8888 is not in use: `lsof -i :8888`
- Try different port and update config

**No matches found:**
- Check your watchlist patterns
- Use common domains like `*.google.com` for testing
- Increase logging: `--verbose`

**Stats not showing:**
- Make sure you use `--stats` flag
- Stats update every 10 seconds by default
- Try `--stats-interval 5` for faster updates

---

**Ready to test?** Open two terminals and follow Option 1 above!
