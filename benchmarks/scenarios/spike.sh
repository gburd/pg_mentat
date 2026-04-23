#!/usr/bin/env bash
#
# Spike Scenario
#
# Ramps traffic from 10 TPS to 100 TPS in three phases to test
# how mentatd handles sudden load increases and whether it
# recovers gracefully.
#
# Phases:
#   1. Low    - 10 TPS  for 1/3 of duration
#   2. Medium - 50 TPS  for 1/3 of duration
#   3. High   - 100 TPS for 1/3 of duration
#
# Usage:
#   ./scenarios/spike.sh --host 127.0.0.1 --port 8080 --duration 90
#

set -euo pipefail

HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-8080}"
DURATION="${DURATION:-90}"
OUTPUT="${OUTPUT:-/dev/stdout}"
BASE_URL="http://${HOST}:${PORT}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host)     HOST="$2"; shift 2 ;;
        --port)     PORT="$2"; shift 2 ;;
        --duration) DURATION="$2"; shift 2 ;;
        --output)   OUTPUT="$2"; shift 2 ;;
        *) shift ;;
    esac
done

BASE_URL="http://${HOST}:${PORT}"

QUERY_BODY='{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}'

phase_duration=$((DURATION / 3))

run_phase_workers() {
    local phase_name="$1"
    local target_rps="$2"
    local workers="$3"
    local dur="$4"

    local rps_per_worker
    rps_per_worker=$(echo "scale=2; $target_rps / $workers" | bc 2>/dev/null || echo "5")
    local interval
    interval=$(echo "scale=6; 1.0 / $rps_per_worker" | bc 2>/dev/null || echo "0.2")

    echo "# Phase: $phase_name ($target_rps TPS, $workers workers, ${dur}s)" >&2

    local pids=()
    for i in $(seq 1 "$workers"); do
        (
            local end_time=$(($(date +%s) + dur))
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
                echo "spike_${phase_name}_${i} ${status_code} ${latency_ms} ${size}"

                local sleep_time
                sleep_time=$(echo "$interval - $time_total" | bc 2>/dev/null || echo "0")
                if [[ "${sleep_time:0:1}" != "-" ]] && [[ "$sleep_time" != "0" ]]; then
                    sleep "$sleep_time" 2>/dev/null || true
                fi
            done
        ) >> "$OUTPUT" &
        pids+=($!)
    done

    for pid in "${pids[@]}"; do
        wait "$pid" 2>/dev/null || true
    done
}

# Phase 1: Low load
run_phase_workers "low" 10 5 "$phase_duration"

# Phase 2: Medium load
run_phase_workers "medium" 50 10 "$phase_duration"

# Phase 3: High load / spike
run_phase_workers "high" 100 20 "$phase_duration"
