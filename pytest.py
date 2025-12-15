import http.server
import json
import socketserver
from datetime import datetime


class WebhookHandler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers["Content-Length"])
        body = self.rfile.read(content_length)
        data = json.loads(body)
        timestamp = datetime.now().strftime("%H:%M:%S")

        print("\n" + "=" * 60)
        print(f"🎉 WEBHOOK at {timestamp}")
        print("=" * 60)
        print(f"  Domain: {data.get('matched_domain', 'N/A')}")
        print(f"  Program: {data.get('program_name', 'N/A')}")
        print("=" * 60 + "\n")

        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"OK")

    def log_message(self, format, *args):
        pass


with socketserver.TCPServer(("", 8888), WebhookHandler) as httpd:
    print("📡 Listening on http://localhost:8888\n")
    httpd.serve_forever()
