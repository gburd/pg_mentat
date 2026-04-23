#!/usr/bin/env bash
#
# Steady State Scenario
#
# Maintains a constant 50 TPS query load against mentatd for
# a configurable duration. Validates that the server can sustain
# the target throughput without degradation.
#
# Usage: source this file or run standalone:
#   ./scenarios/steady_state.sh --host 127.0.0.1 --port 8080 --duration 60
#

set -euo pipefail

HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-8080}"
DURATION="${DURATION:-60}"
TARGET_TPS="${TARGET_TPS:-50}"
WORKERS="${WORKERS:-10}"
OUTPUT="${OUTPUT:-/dev/stdout}"
BASE_URL="http://${HOST}:${PORT}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host)     HOST="$2"; shift 2 ;;
        --port)     PORT="$2"; shift 2 ;;
        --duration) DURATION="$2"; shift 2 ;;
        --tps)      TARGET_TPS="$2"; shift 2 ;;
        --workers)  WORKERS="$2"; shift 2 ;;
        --output)   OUTPUT="$2"; shift 2 ;;
        *) shift ;;
    esac
done

BASE_URL="http://${HOST}:${PORT}"

QUERY_BODY='{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}'

rps_per_worker=$(echo "scale=2; $TARGET_TPS / $WORKERS" | bc 2>/dev/null || echo "5")
interval=$(echo "scale=6; 1.0 / $rps_per_worker" | bc 2>/dev/null || echo "0.2")

echo "# Steady State: ${TARGET_TPS} TPS with ${WORKERS} workers for ${DURATION}s"
echo "# RPS per worker: $rps_per_worker"

run_worker() {
    local wid="$1"
    local end_time=$(($(date +%s) + DURATION))

    while [[ $(date +%s) -lt $end_time ]]; do
        local result
        result=$(curl -s -o /dev/null -w '%{http_code} %{time_total} %{size_download}' \
            -X POST "${BASE_URL}/" \
            -H 'Content-Type: application/edn' \
            -d "$QUERY_BODY")

        local status_code time_total size
        read -r status_code time_total size <<< "$result"
        local latency_ms
        latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")

        echo "steady_${wid} ${status_code} ${latency_ms} ${size}"

        local sleep_time
        sleep_time=$(echo "$interval - $time_total" | bc 2>/dev/null || echo "0")
        if [[ "${sleep_time:0:1}" != "-" ]] && [[ "$sleep_time" != "0" ]]; then
            sleep "$sleep_time" 2>/dev/null || true
        fi
    done
}

pids=()
for i in $(seq 1 "$WORKERS"); do
    run_worker "$i" >> "$OUTPUT" &
    pids+=($!)
done

for pid in "${pids[@]}"; do
    wait "$pid" 2>/dev/null || true
done
