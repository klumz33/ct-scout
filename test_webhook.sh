#!/bin/bash
# Test webhook notification

echo "🎯 CT-Scout Webhook Test"
echo "========================"
echo ""

# Create a temporary config for testing
cat > /tmp/ct-scout-webhook-test.toml <<EOF
[certstream]
url = "ws://127.0.0.1:4000/full-stream"
reconnect_delay_secs = 5

[webhook]
url = "http://localhost:8888/webhook"
timeout_secs = 5

[logging]
level = "info"

[watchlist]
domains = ["*.google.com", "*.amazon.com", "*.microsoft.com"]
hosts = []
ips = []
cidrs = []

[[programs]]
name = "Test Program"
domains = [".google.com"]
cidrs = []
EOF

echo "✅ Created test config at /tmp/ct-scout-webhook-test.toml"
echo ""
echo "Webhook config:"
echo "  URL: http://localhost:8888/webhook"
echo "  Watchlist: *.google.com, *.amazon.com, *.microsoft.com"
echo ""

# Create webhook receiver Python script
cat > /tmp/webhook_receiver.py <<'PYEOF'
import http.server
import socketserver
import json
import sys
from datetime import datetime

class WebhookHandler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers["Content-Length"])
        body = self.rfile.read(content_length)

        try:
            data = json.loads(body)
            timestamp = datetime.now().strftime("%H:%M:%S")

            separator = "=" * 60
            print(f"\n{separator}")
            print(f"🎉 WEBHOOK RECEIVED at {timestamp}")
            print(separator)
            print(f"  Matched Domain: {data.get('matched_domain', 'N/A')}")
            print(f"  All Domains: {', '.join(data.get('all_domains', []))}")
            print(f"  Cert Index: {data.get('cert_index', 'N/A')}")
            print(f"  Program: {data.get('program_name', 'N/A')}")

            if "X-CTScout-Signature" in self.headers:
                sig = self.headers["X-CTScout-Signature"]
                print(f"  HMAC Signature: {sig[:16]}...")

            print(f"{separator}\n")
            sys.stdout.flush()

        except json.JSONDecodeError as e:
            print(f"❌ Invalid JSON: {e}")

        self.send_response(200)
        self.send_header("Content-type", "text/plain")
        self.end_headers()
        self.wfile.write(b"OK")

    def log_message(self, format, *args):
        pass

PORT = 8888
print("📡 Webhook receiver listening on port 8888...")
print("Waiting for notifications from ct-scout...")
print("")
sys.stdout.flush()

with socketserver.TCPServer(("", PORT), WebhookHandler) as httpd:
    httpd.serve_forever()
PYEOF

# Start webhook receiver in background
python3 /tmp/webhook_receiver.py &
WEBHOOK_PID=$!
echo "📡 Webhook receiver started (PID: $WEBHOOK_PID)"
echo ""

# Give the webhook receiver time to start
sleep 2

echo "🚀 Starting ct-scout with webhook test config..."
echo "   Press Ctrl+C to stop when you see a webhook notification"
echo ""
echo "---"
echo ""

# Run ct-scout with the test config
./target/release/ct-scout --config /tmp/ct-scout-webhook-test.toml --stats --verbose

# Cleanup
echo ""
echo "Cleaning up..."
kill $WEBHOOK_PID 2>/dev/null
rm -f /tmp/ct-scout-webhook-test.toml /tmp/webhook_receiver.py
echo "Done!"
