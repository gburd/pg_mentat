#!/usr/bin/env bash
# Quick test to verify mock server functionality

set -euo pipefail

echo "Starting mock server on port 8083..."
python3 /home/gburd/ws/pg_mentat/benchmarks/mock_server.py --port 8083 &
MOCK_PID=$!

# Give server time to start
sleep 2

echo "Testing health endpoint..."
curl -s http://localhost:8083/health | jq .

echo "Testing query endpoint..."
curl -s -X POST http://localhost:8083/api/query \
  -H "Content-Type: application/edn" \
  -d '{:query [:find ?e :where [?e :db/ident :test]]}' | head -c 100

echo -e "\n\nMock server functional. Stopping..."
kill $MOCK_PID 2>/dev/null || true

echo "Test complete."