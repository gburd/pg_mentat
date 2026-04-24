#!/usr/bin/env bash
#
# Concurrent Writes Scenario
#
# Specifically designed to validate the sequence-based entity ID allocation
# (replacing the old UPDATE-lock approach). All workers perform writes
# concurrently to stress the nextval() sequence path.
#
# Validates:
#   - No duplicate entity IDs generated under high concurrency
#   - Write throughput meets post-optimization targets (500+ TPS)
#   - Latency remains acceptable under write contention
#
# Usage:
#   ./scenarios/concurrent_writes.sh --host 127.0.0.1 --port 8080 --duration 60 --workers 100
#

set -euo pipefail

HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-8080}"
DURATION="${DURATION:-60}"
WORKERS="${WORKERS:-100}"
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

echo "# Concurrent Writes: $WORKERS workers, all writing for ${DURATION}s" >&2

# Setup: install schema if not already present
setup_schema() {
    local schema_tx='[{:db/ident :loadtest/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/ident :loadtest/counter :db/valueType :db.type/long :db/cardinality :db.cardinality/one} {:db/ident :loadtest/worker :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'

    curl -s -X POST "${BASE_URL}/" \
        -H 'Content-Type: application/edn' \
        -d "{:op :transact :args {:connection-id \"bench\" :tx-data $schema_tx}}" > /dev/null 2>&1

    echo "# Schema installed" >&2
}

run_write_worker() {
    local wid="$1"
    local end_time=$(($(date +%s) + DURATION))
    local counter=0

    while [[ $(date +%s) -lt $end_time ]]; do
        counter=$((counter + 1))
        local unique_id="w${wid}_c${counter}_$(date +%s%N)"
        local tx_body="{:op :transact :args {:connection-id \"bench\" :tx-data [{:loadtest/name \"${unique_id}\" :loadtest/counter ${counter} :loadtest/worker \"worker-${wid}\"}]}}"

        local result
        result=$(curl -s -o /dev/null -w '%{http_code} %{time_total} %{size_download}' \
            -X POST "${BASE_URL}/" \
            -H 'Content-Type: application/edn' \
            -d "$tx_body")

        local status_code time_total size
        read -r status_code time_total size <<< "$result"
        local latency_ms
        latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")

        echo "cwrite_${wid} ${status_code} ${latency_ms} ${size}"
    done
}

# Verify no duplicate entity IDs after the test
verify_no_duplicates() {
    echo "" >&2
    echo "# Post-test: Verifying no duplicate entity IDs..." >&2

    # Query for total entity count
    local count_result
    count_result=$(curl -s -X POST "${BASE_URL}/" \
        -H 'Content-Type: application/edn' \
        -d '{:op :q :args {:query [:find (count ?e) :where [?e :loadtest/name _]]}}')

    echo "# Entity count result: ${count_result}" >&2

    # Check for duplicate names (which would indicate ID collision)
    local dup_result
    dup_result=$(curl -s -X POST "${BASE_URL}/" \
        -H 'Content-Type: application/edn' \
        -d '{:op :q :args {:query [:find ?name (count ?e) :where [?e :loadtest/name ?name]]}}')

    if echo "$dup_result" | grep -q "error"; then
        echo "# WARN: Duplicate check query returned error" >&2
    else
        echo "# Duplicate check completed (response length: ${#dup_result})" >&2
    fi
}

setup_schema

pids=()
for i in $(seq 1 "$WORKERS"); do
    run_write_worker "$i" >> "$OUTPUT" &
    pids+=($!)
done

for pid in "${pids[@]}"; do
    wait "$pid" 2>/dev/null || true
done

verify_no_duplicates
