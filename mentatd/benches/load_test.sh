#!/usr/bin/env bash
#
# mentatd load test script
#
# Tests mentatd HTTP endpoint under various load patterns using curl.
# For heavier load testing, use a dedicated tool like k6, wrk, or Apache Bench.
#
# Usage:
#   ./load_test.sh [HOST] [PORT]
#
# Prerequisites:
#   - mentatd running with PostgreSQL backend
#   - curl installed
#   - (optional) wrk or ab for high-concurrency tests
#
# Examples:
#   ./load_test.sh                    # defaults to localhost:8484
#   ./load_test.sh 192.168.1.10 8484  # custom host/port

set -euo pipefail

HOST="${1:-localhost}"
PORT="${2:-8484}"
BASE_URL="http://${HOST}:${PORT}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Results directory
RESULTS_DIR="${RESULTS_DIR:-/tmp/mentatd_bench_$(date +%Y%m%d_%H%M%S)}"
mkdir -p "$RESULTS_DIR"

echo -e "${BLUE}=== mentatd Load Test ===${NC}"
echo "Target: ${BASE_URL}"
echo "Results: ${RESULTS_DIR}"
echo ""

# ---------------------------------------------------------------------------
# Helper functions
# ---------------------------------------------------------------------------

measure_request() {
    local label="$1"
    local method="$2"
    local path="$3"
    local body="${4:-}"
    local accept="${5:-application/edn}"

    local url="${BASE_URL}${path}"
    local start_ns end_ns elapsed_ms http_code body_size

    start_ns=$(date +%s%N)

    if [ "$method" = "GET" ]; then
        http_code=$(curl -s -o /dev/null -w '%{http_code}' \
            -H "Accept: ${accept}" \
            "$url" 2>/dev/null)
    else
        http_code=$(curl -s -o /dev/null -w '%{http_code}' \
            -X POST \
            -H "Content-Type: application/edn" \
            -H "Accept: ${accept}" \
            -d "$body" \
            "$url" 2>/dev/null)
    fi

    end_ns=$(date +%s%N)
    elapsed_ms=$(( (end_ns - start_ns) / 1000000 ))

    echo "${label},${elapsed_ms},${http_code}"
}

run_scenario() {
    local name="$1"
    local iterations="$2"
    local method="$3"
    local path="$4"
    local body="${5:-}"
    local accept="${6:-application/edn}"

    echo -e "${YELLOW}--- ${name} (${iterations} iterations) ---${NC}"

    local csv_file="${RESULTS_DIR}/${name// /_}.csv"
    echo "label,latency_ms,status" > "$csv_file"

    local total_ms=0
    local min_ms=999999
    local max_ms=0
    local errors=0

    for i in $(seq 1 "$iterations"); do
        local result
        result=$(measure_request "$name" "$method" "$path" "$body" "$accept")
        echo "$result" >> "$csv_file"

        local ms http_code
        ms=$(echo "$result" | cut -d, -f2)
        http_code=$(echo "$result" | cut -d, -f3)

        total_ms=$((total_ms + ms))
        if [ "$ms" -lt "$min_ms" ]; then min_ms=$ms; fi
        if [ "$ms" -gt "$max_ms" ]; then max_ms=$ms; fi
        if [ "$http_code" != "200" ]; then errors=$((errors + 1)); fi
    done

    local avg_ms=$((total_ms / iterations))

    # Calculate p50, p95, p99 from sorted latencies
    local sorted_latencies
    sorted_latencies=$(tail -n +2 "$csv_file" | cut -d, -f2 | sort -n)
    local p50_idx=$(( (iterations * 50 + 99) / 100 ))
    local p95_idx=$(( (iterations * 95 + 99) / 100 ))
    local p99_idx=$(( (iterations * 99 + 99) / 100 ))
    local p50 p95 p99
    p50=$(echo "$sorted_latencies" | sed -n "${p50_idx}p")
    p95=$(echo "$sorted_latencies" | sed -n "${p95_idx}p")
    p99=$(echo "$sorted_latencies" | sed -n "${p99_idx}p")

    local tps=0
    if [ "$total_ms" -gt 0 ]; then
        tps=$(( iterations * 1000 / total_ms ))
    fi

    if [ "$errors" -eq 0 ]; then
        echo -e "  ${GREEN}OK${NC}  avg=${avg_ms}ms  min=${min_ms}ms  max=${max_ms}ms  p50=${p50}ms  p95=${p95}ms  p99=${p99}ms  tps=${tps}  errors=${errors}"
    else
        echo -e "  ${RED}ERR${NC} avg=${avg_ms}ms  min=${min_ms}ms  max=${max_ms}ms  p50=${p50}ms  p95=${p95}ms  p99=${p99}ms  tps=${tps}  errors=${errors}/${iterations}"
    fi

    # Append summary to results
    echo "${name},${avg_ms},${min_ms},${max_ms},${p50},${p95},${p99},${tps},${errors},${iterations}" >> "${RESULTS_DIR}/summary.csv"
}

# ---------------------------------------------------------------------------
# Pre-flight check
# ---------------------------------------------------------------------------

echo -e "${BLUE}Checking server health...${NC}"
health_response=$(curl -s -o /dev/null -w '%{http_code}' "${BASE_URL}/health" 2>/dev/null || true)
if [ "$health_response" != "200" ]; then
    echo -e "${RED}ERROR: Server not responding at ${BASE_URL}/health (got HTTP ${health_response})${NC}"
    echo "Make sure mentatd is running: cargo run -p mentatd"
    exit 1
fi
echo -e "${GREEN}Server is healthy${NC}"
echo ""

# Initialize summary CSV
echo "scenario,avg_ms,min_ms,max_ms,p50_ms,p95_ms,p99_ms,tps,errors,iterations" > "${RESULTS_DIR}/summary.csv"

# ---------------------------------------------------------------------------
# Scenario 1: Health check baseline
# ---------------------------------------------------------------------------

run_scenario "health_check" 100 GET "/health"

# ---------------------------------------------------------------------------
# Scenario 2: Health operation via POST
# ---------------------------------------------------------------------------

run_scenario "health_op" 100 POST "/" '{:op :health}'

# ---------------------------------------------------------------------------
# Scenario 3: List databases
# ---------------------------------------------------------------------------

run_scenario "list_dbs" 50 POST "/" '{:op :list-dbs}'

# ---------------------------------------------------------------------------
# Scenario 4: Connect operation
# ---------------------------------------------------------------------------

run_scenario "connect" 50 POST "/" '{:op :connect :args {:db-name "postgres"}}'

# ---------------------------------------------------------------------------
# Scenario 5: Simple query
# ---------------------------------------------------------------------------

run_scenario "query_simple" 50 POST "/" \
    '{:op :q :args {:query "[:find ?e :where [?e :name]]" :args []}}'

# ---------------------------------------------------------------------------
# Scenario 6: Query with arguments
# ---------------------------------------------------------------------------

run_scenario "query_with_args" 50 POST "/" \
    '{:op :q :args {:query "[:find ?e :in $ ?name :where [?e :name ?name]]" :args ["Alice"]}}'

# ---------------------------------------------------------------------------
# Scenario 7: Transit+JSON format
# ---------------------------------------------------------------------------

run_scenario "health_transit_json" 50 POST "/" \
    '{:op :health}' \
    'application/transit+json'

# ---------------------------------------------------------------------------
# Scenario 8: Transit+MessagePack format
# ---------------------------------------------------------------------------

run_scenario "health_transit_msgpack" 50 POST "/" \
    '{:op :health}' \
    'application/transit+msgpack'

# ---------------------------------------------------------------------------
# Scenario 9: Format comparison - same query, all formats
# ---------------------------------------------------------------------------

echo ""
echo -e "${BLUE}=== Format Comparison ===${NC}"

run_scenario "query_edn" 50 POST "/" \
    '{:op :q :args {:query "[:find ?e :where [?e :name]]" :args []}}' \
    'application/edn'

run_scenario "query_transit_json" 50 POST "/" \
    '{:op :q :args {:query "[:find ?e :where [?e :name]]" :args []}}' \
    'application/transit+json'

run_scenario "query_transit_msgpack" 50 POST "/" \
    '{:op :q :args {:query "[:find ?e :where [?e :name]]" :args []}}' \
    'application/transit+msgpack'

# ---------------------------------------------------------------------------
# Scenario 10: Error handling
# ---------------------------------------------------------------------------

run_scenario "error_invalid_op" 50 POST "/" '{:op :nonexistent}'
run_scenario "error_bad_edn" 50 POST "/" 'not valid edn'

# ---------------------------------------------------------------------------
# Scenario 11: Sustained load (steady state)
# ---------------------------------------------------------------------------

echo ""
echo -e "${BLUE}=== Sustained Load (200 requests) ===${NC}"

run_scenario "sustained_health" 200 POST "/" '{:op :health}'
run_scenario "sustained_query" 200 POST "/" \
    '{:op :q :args {:query "[:find ?e :where [?e :name]]" :args []}}'

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

echo ""
echo -e "${BLUE}=== Summary ===${NC}"
echo ""
column -t -s',' "${RESULTS_DIR}/summary.csv" 2>/dev/null || cat "${RESULTS_DIR}/summary.csv"
echo ""
echo -e "Full results saved to: ${GREEN}${RESULTS_DIR}${NC}"

# ---------------------------------------------------------------------------
# wrk integration (if available)
# ---------------------------------------------------------------------------

if command -v wrk &>/dev/null; then
    echo ""
    echo -e "${BLUE}=== wrk High-Concurrency Test ===${NC}"
    echo "Running wrk for 10s with 4 threads, 50 connections..."

    # Create a wrk lua script for POST requests
    cat > "${RESULTS_DIR}/wrk_health.lua" << 'LUAEOF'
wrk.method = "POST"
wrk.body = '{:op :health}'
wrk.headers["Content-Type"] = "application/edn"
wrk.headers["Accept"] = "application/edn"
LUAEOF

    wrk -t4 -c50 -d10s -s "${RESULTS_DIR}/wrk_health.lua" "${BASE_URL}/" | tee "${RESULTS_DIR}/wrk_output.txt"
else
    echo ""
    echo -e "${YELLOW}Note: Install 'wrk' for high-concurrency benchmarks${NC}"
    echo "  On NixOS: nix-env -iA nixpkgs.wrk"
    echo "  On Ubuntu: apt install wrk"
fi

echo ""
echo -e "${GREEN}Load test complete.${NC}"
