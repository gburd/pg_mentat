#!/usr/bin/env python3
"""
Mock mentatd server for load testing.
Simulates Datomic-compatible EDN API responses with realistic latencies.
"""

import json
import time
import random
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn
from threading import Lock


class ThreadingHTTPServer(ThreadingMixIn, HTTPServer):
    """Handle requests in separate threads for concurrent load testing."""
    daemon_threads = True
import argparse

# Global metrics
metrics = {
    'requests': 0,
    'errors': 0,
    'total_time': 0,
    'lock': Lock()
}

class MockMentatdHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        """Handle GET requests (health checks)"""
        if self.path == '/health':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"status":"healthy","version":"0.1.0"}')
        else:
            self.send_error(404)

    def do_POST(self):
        """Handle POST requests (EDN operations)"""
        content_length = int(self.headers['Content-Length'])
        body = self.rfile.read(content_length).decode('utf-8')

        with metrics['lock']:
            metrics['requests'] += 1

        start_time = time.time()

        # Parse the EDN-like request (simplified)
        if ':op' in body:
            if ':health' in body:
                response = self._health_response()
            elif ':q' in body or ':query' in body:
                response = self._query_response()
            elif ':transact' in body:
                response = self._transact_response()
            elif ':as-of' in body or ':since' in body or ':history' in body:
                response = self._temporal_query_response()
            elif ':db-snapshot' in body:
                response = self._db_snapshot_response()
            else:
                response = self._error_response("Unknown operation")
        else:
            response = self._error_response("Invalid request")

        # Simulate realistic processing time
        processing_time = random.gauss(30, 10)  # Mean 30ms, stddev 10ms
        processing_time = max(5, min(100, processing_time))  # Clamp to 5-100ms
        time.sleep(processing_time / 1000.0)

        elapsed = time.time() - start_time
        with metrics['lock']:
            metrics['total_time'] += elapsed

        # Send response
        self.send_response(200)
        self.send_header('Content-Type', 'application/edn')
        self.send_header('Content-Length', str(len(response)))
        self.end_headers()
        self.wfile.write(response.encode('utf-8'))

    def _health_response(self):
        """Generate health check response"""
        return '{:status :healthy}'

    def _query_response(self):
        """Generate query response with realistic data"""
        # Simulate returning 10-100 results
        num_results = random.randint(10, 100)
        results = []
        for i in range(num_results):
            results.append(f'[{17592186045418 + i} "Person_{i}" {20 + (i % 60)} "person{i}@example.com"]')

        return '[[' + ' '.join(results) + ']]'

    def _transact_response(self):
        """Generate transaction response"""
        tx_id = random.randint(1000000, 9999999)
        return f'{{:db-before {{:db/id "datomic.db.Db@1234"}} :db-after {{:db/id "datomic.db.Db@5678"}} :tx-data [[{tx_id} :db/txInstant #inst "2024-04-24T10:00:00.000-00:00" {tx_id} 0]] :tempids {{}}}}'

    def _temporal_query_response(self):
        """Generate temporal query response"""
        return self._query_response()  # Simplified: same as regular query

    def _db_snapshot_response(self):
        """Generate database snapshot response"""
        return f'{{:db-id "snapshot-{random.randint(1000, 9999)}" :basis-t {random.randint(1000000, 9999999)}}}'

    def _error_response(self, message):
        """Generate error response"""
        with metrics['lock']:
            metrics['errors'] += 1
        return f'{{:error {{:cognitect.anomalies/category :cognitect.anomalies/incorrect :cognitect.anomalies/message "{message}"}}}}'

    def log_message(self, format, *args):
        """Suppress default logging"""
        pass

def run_server(host='127.0.0.1', port=8080):
    """Run the mock server"""
    server_address = (host, port)
    httpd = ThreadingHTTPServer(server_address, MockMentatdHandler)

    print(f"Mock mentatd server running on http://{host}:{port}")
    print("Press Ctrl+C to stop")
    print("-" * 50)

    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\n" + "-" * 50)
        print("Server statistics:")
        with metrics['lock']:
            print(f"  Total requests: {metrics['requests']}")
            print(f"  Errors: {metrics['errors']}")
            if metrics['requests'] > 0:
                avg_time = (metrics['total_time'] / metrics['requests']) * 1000
                print(f"  Average response time: {avg_time:.2f}ms")
                print(f"  Throughput: {metrics['requests'] / metrics['total_time']:.2f} req/s")
        httpd.shutdown()

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Mock mentatd server for load testing')
    parser.add_argument('--host', default='127.0.0.1', help='Host to bind to')
    parser.add_argument('--port', type=int, default=8080, help='Port to bind to')

    args = parser.parse_args()
    run_server(args.host, args.port)