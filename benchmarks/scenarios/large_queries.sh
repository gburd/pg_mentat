#!/usr/bin/env bash
#
# Large Queries Scenario
#
# Sends complex queries that return many results to test mentatd's
# behavior under heavy result serialization. Tests queries with
# multiple join conditions and large result sets.
#
# Usage:
#   ./scenarios/large_queries.sh --host 127.0.0.1 --port 8080 --duration 60
#

set -euo pipefail

HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-8080}"
DURATION="${DURATION:-60}"
WORKERS="${WORKERS:-20}"
OUTPUT="${OUTPUT:-/dev/stdout}"
BASE_URL="http://${HOST}:${PORT}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host)     HOST="$2"; shift 2 ;;
        --port)     PORT="$2"; shift 2 ;;
        --duration) DURATION="$2"; shift 2 ;;
        --workers)  WORKERS="$2"; shift 2 ;;
        --output)   OUTPUT="$2"; shift 2 ;;
        *) shift ;;
    esac
done

BASE_URL="http://${HOST}:${PORT}"

# Multiple query patterns to exercise different code paths
QUERIES=(
    # All entities with name and age (cross-product potential)
    '{:op :q :args {:query [:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]}}'
    # All entities with all three attributes
    '{:op :q :args {:query [:find ?e ?name ?age ?email :where [?e :person/name ?name] [?e :person/age ?age] [?e :person/email ?email]]}}'
    # Filtered by age range (still returns many rows)
    '{:op :q :args {:query [:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]}}'
    # Simple name lookup (lighter but still queries all)
    '{:op :q :args {:query [:find ?name :where [?e :person/name ?name]]}}'
)

NUM_QUERIES=${#QUERIES[@]}

run_worker() {
    local wid="$1"
    local end_time=$(($(date +%s) + DURATION))
    local counter=0

    while [[ $(date +%s) -lt $end_time ]]; do
        # Cycle through query patterns
        local idx=$((counter % NUM_QUERIES))
        local query_body="${QUERIES[$idx]}"
        counter=$((counter + 1))

        local result
        result=$(curl -s -o /dev/null -w '%{http_code} %{time_total} %{size_download}' \
            -X POST "${BASE_URL}/" \
            -H 'Content-Type: application/edn' \
            -d "$query_body")

        local status_code time_total size
        read -r status_code time_total size <<< "$result"
        local latency_ms
        latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")

        echo "large_${wid} ${status_code} ${latency_ms} ${size}"
    done
}

echo "# Large Queries: $WORKERS workers, cycling through $NUM_QUERIES query patterns for ${DURATION}s" >&2

pids=()
for i in $(seq 1 "$WORKERS"); do
    run_worker "$i" >> "$OUTPUT" &
    pids+=($!)
done

for pid in "${pids[@]}"; do
    wait "$pid" 2>/dev/null || true
done
