#!/usr/bin/env python3
"""
Simple mock upstream server for testing Schema Gateway
Echoes back requests with headers and body information
"""

from http.server import HTTPServer, BaseHTTPRequestHandler
import json
import sys

PORT = 3001

class MockUpstreamHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        """Handle GET requests"""
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        
        response = {
            "method": "GET",
            "path": self.path,
            "gateway_validated": self.headers.get('X-Schema-Validated'),
            "gateway_error": self.headers.get('X-Gateway-Error'),
            "message": "GET request received"
        }
        
        self.wfile.write(json.dumps(response, indent=2).encode())
    
    def do_POST(self):
        """Handle POST requests"""
        content_length = int(self.headers.get('Content-Length', 0))
        body = self.rfile.read(content_length).decode('utf-8') if content_length > 0 else None
        
        self.send_response(201)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        
        try:
            body_json = json.loads(body) if body else None
        except json.JSONDecodeError:
            body_json = body
        
        response = {
            "method": "POST",
            "path": self.path,
            "gateway_validated": self.headers.get('X-Schema-Validated'),
            "gateway_error": self.headers.get('X-Gateway-Error'),
            "received_body": body_json,
            "message": "POST request received and processed",
            "created_id": 12345
        }
        
        self.wfile.write(json.dumps(response, indent=2).encode())
    
    def log_message(self, format, *args):
        """Custom logging"""
        print(f"[UPSTREAM] {format % args}")

def run_server(port=PORT):
    server = HTTPServer(('localhost', port), MockUpstreamHandler)
    print(f"""
╔════════════════════════════════════════════════════════════════╗
║           Mock Upstream Server Started                         ║
╚════════════════════════════════════════════════════════════════╝

Listening on: http://localhost:{port}
Ready to receive requests from Schema Gateway

Press Ctrl+C to stop
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
""")
    
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n\n[UPSTREAM] Shutting down...")
        server.shutdown()

if __name__ == '__main__':
    run_server()

