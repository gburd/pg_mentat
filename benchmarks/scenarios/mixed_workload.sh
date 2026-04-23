#!/usr/bin/env bash
#
# Mixed Workload Scenario
#
# Simulates a realistic production workload with 80% read queries
# and 20% write transactions against mentatd. Validates that
# writes don't degrade read performance excessively.
#
# Usage:
#   ./scenarios/mixed_workload.sh --host 127.0.0.1 --port 8080 --duration 60
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

READ_QUERY='{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}'

# Split workers 80/20
read_workers=$(( (WORKERS * 80 + 99) / 100 ))
write_workers=$((WORKERS - read_workers))
if [[ $write_workers -lt 1 ]]; then
    write_workers=1
    read_workers=$((WORKERS - 1))
fi

echo "# Mixed Workload: $read_workers read workers, $write_workers write workers for ${DURATION}s" >&2

run_read_worker() {
    local wid="$1"
    local end_time=$(($(date +%s) + DURATION))

    while [[ $(date +%s) -lt $end_time ]]; do
        local result
        result=$(curl -s -o /dev/null -w '%{http_code} %{time_total} %{size_download}' \
            -X POST "${BASE_URL}/" \
            -H 'Content-Type: application/edn' \
            -d "$READ_QUERY")

        local status_code time_total size
        read -r status_code time_total size <<< "$result"
        local latency_ms
        latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")

        echo "read_${wid} ${status_code} ${latency_ms} ${size}"
    done
}

run_write_worker() {
    local wid="$1"
    local end_time=$(($(date +%s) + DURATION))
    local counter=0

    while [[ $(date +%s) -lt $end_time ]]; do
        counter=$((counter + 1))
        local tx_body="{:op :transact :args {:connection-id \"bench\" :tx-data [{:person/name \"MixedTest_w${wid}_${counter}\" :person/age $((20 + counter % 60))}]}}"

        local result
        result=$(curl -s -o /dev/null -w '%{http_code} %{time_total} %{size_download}' \
            -X POST "${BASE_URL}/" \
            -H 'Content-Type: application/edn' \
            -d "$tx_body")

        local status_code time_total size
        read -r status_code time_total size <<< "$result"
        local latency_ms
        latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")

        echo "write_${wid} ${status_code} ${latency_ms} ${size}"

        # Writes are typically slower; moderate the rate
        sleep 0.05 2>/dev/null || true
    done
}

pids=()

for i in $(seq 1 "$read_workers"); do
    run_read_worker "$i" >> "$OUTPUT" &
    pids+=($!)
done

for i in $(seq 1 "$write_workers"); do
    run_write_worker "$i" >> "$OUTPUT" &
    pids+=($!)
done

for pid in "${pids[@]}"; do
    wait "$pid" 2>/dev/null || true
done
