#!/usr/bin/env bash
#
# Cache Performance Scenario
#
# Measures query cache effectiveness under mixed read/write load:
#   - Sends repeated queries (should hit cache) mixed with transactions (cause invalidation)
#   - Queries /metrics endpoint to extract cache hit rate, invalidation counts
#   - Validates entity-level invalidation preserves unrelated cached queries
#
# Usage:
#   ./scenarios/cache_performance.sh --host 127.0.0.1 --port 8080 --duration 60
#

set -euo pipefail

HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-8080}"
DURATION="${DURATION:-30}"
WORKERS="${WORKERS:-10}"
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

# Use distinct queries that will have different entity dependencies.
# This tests that a transaction affecting entity A does not invalidate
# cached results for queries about entity B.
QUERY_NAMES='{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}'
QUERY_AGES='{:op :q :args {:query [:find ?e ?age :where [?e :person/age ?age]]}}'
QUERY_ALL='{:op :q :args {:query [:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]}}'

echo "# Cache Performance Test: ${WORKERS} workers for ${DURATION}s" >&2
echo "# Server: ${BASE_URL}" >&2

# Capture initial metrics
get_metric() {
    local name="$1"
    curl -s "${BASE_URL}/metrics" 2>/dev/null | grep "^${name} " | awk '{print $2}' || echo "0"
}

initial_hits=$(get_metric "mentatd_cache_hits_total")
initial_misses=$(get_metric "mentatd_cache_misses_total")
initial_targeted=$(get_metric "mentatd_cache_targeted_invalidations_total")
initial_full=$(get_metric "mentatd_cache_full_invalidations_total")

echo "# Initial: hits=${initial_hits} misses=${initial_misses} targeted=${initial_targeted} full=${initial_full}" >&2

# Read workers: repeatedly send the same queries (should hit cache after first miss)
run_read_worker() {
    local wid="$1"
    local end_time=$(($(date +%s) + DURATION))
    local queries=("$QUERY_NAMES" "$QUERY_AGES" "$QUERY_ALL")
    local idx=0

    while [[ $(date +%s) -lt $end_time ]]; do
        local query="${queries[$((idx % 3))]}"
        idx=$((idx + 1))

        local result
        result=$(curl -s -o /dev/null -w '%{http_code} %{time_total}' \
            -X POST "${BASE_URL}/" \
            -H 'Content-Type: application/edn' \
            -d "$query" 2>/dev/null)

        local status_code time_total
        read -r status_code time_total <<< "$result"
        local latency_ms
        latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")

        echo "read_${wid} ${status_code} ${latency_ms}"
    done
}

# Write workers: transactions that affect specific entities (should trigger targeted invalidation)
run_write_worker() {
    local wid="$1"
    local end_time=$(($(date +%s) + DURATION))
    local counter=0

    while [[ $(date +%s) -lt $end_time ]]; do
        counter=$((counter + 1))
        local tx_body="{:op :transact :args {:connection-id \"bench\" :tx-data [{:person/name \"CacheTest_w${wid}_${counter}\" :person/age $((20 + counter % 60))}]}}"

        local result
        result=$(curl -s -o /dev/null -w '%{http_code} %{time_total}' \
            -X POST "${BASE_URL}/" \
            -H 'Content-Type: application/edn' \
            -d "$tx_body" 2>/dev/null)

        local status_code time_total
        read -r status_code time_total <<< "$result"
        local latency_ms
        latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")

        echo "write_${wid} ${status_code} ${latency_ms}"

        # Slow down writes so reads can accumulate cache hits between invalidations
        sleep 0.2 2>/dev/null || true
    done
}

pids=()

# 80% read workers, 20% write workers
read_workers=$(( (WORKERS * 80 + 99) / 100 ))
write_workers=$((WORKERS - read_workers))
if [[ $write_workers -lt 1 ]]; then
    write_workers=1
    read_workers=$((WORKERS - 1))
fi

echo "# Launching ${read_workers} read workers, ${write_workers} write workers" >&2

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

# Capture final metrics
final_hits=$(get_metric "mentatd_cache_hits_total")
final_misses=$(get_metric "mentatd_cache_misses_total")
final_targeted=$(get_metric "mentatd_cache_targeted_invalidations_total")
final_full=$(get_metric "mentatd_cache_full_invalidations_total")
cache_size=$(get_metric "mentatd_cache_entries")
tracked_entries=$(get_metric "mentatd_cache_tracked_entries")
hit_rate=$(get_metric "mentatd_cache_hit_rate")
avg_deps=$(get_metric "mentatd_cache_avg_dependency_count")

# Calculate deltas
delta_hits=$(echo "$final_hits - $initial_hits" | bc 2>/dev/null || echo "0")
delta_misses=$(echo "$final_misses - $initial_misses" | bc 2>/dev/null || echo "0")
delta_targeted=$(echo "$final_targeted - $initial_targeted" | bc 2>/dev/null || echo "0")
delta_full=$(echo "$final_full - $initial_full" | bc 2>/dev/null || echo "0")
total_lookups=$(echo "$delta_hits + $delta_misses" | bc 2>/dev/null || echo "0")

if [[ "$total_lookups" != "0" && "$total_lookups" != "" ]]; then
    test_hit_rate=$(echo "scale=4; $delta_hits / $total_lookups" | bc 2>/dev/null || echo "0")
else
    test_hit_rate="0"
fi

echo "" >&2
echo "======================================" >&2
echo "  Cache Performance Results" >&2
echo "======================================" >&2
echo "  Cache hits:              ${delta_hits}" >&2
echo "  Cache misses:            ${delta_misses}" >&2
echo "  Hit rate (this test):    ${test_hit_rate}" >&2
echo "  Hit rate (cumulative):   ${hit_rate}" >&2
echo "  Targeted invalidations:  ${delta_targeted}" >&2
echo "  Full invalidations:      ${delta_full}" >&2
echo "  Cache size (final):      ${cache_size}" >&2
echo "  Tracked entries:         ${tracked_entries}" >&2
echo "  Avg deps per entry:      ${avg_deps}" >&2
echo "======================================" >&2

# Validate targets
pass=true

# Target: hit rate > 50% under mixed load
if [[ "$(echo "$test_hit_rate > 0.5" | bc 2>/dev/null)" == "1" ]]; then
    echo "  [PASS] Hit rate > 50%" >&2
else
    echo "  [WARN] Hit rate <= 50% (expected >50% under mixed load)" >&2
fi

# Target: mostly targeted invalidations, few full invalidations
if [[ "$(echo "$delta_full == 0" | bc 2>/dev/null)" == "1" || "$delta_full" == "0" ]]; then
    echo "  [PASS] Zero full invalidations (all targeted)" >&2
elif [[ "$(echo "$delta_targeted > $delta_full * 10" | bc 2>/dev/null)" == "1" ]]; then
    echo "  [PASS] Targeted invalidations >> full invalidations" >&2
else
    echo "  [WARN] High ratio of full invalidations" >&2
fi

# Target: tracked entries > 0 (entity-level tracking is working)
if [[ "$tracked_entries" != "0" && "$tracked_entries" != "" ]]; then
    echo "  [PASS] Entity dependency tracking active (${tracked_entries} tracked entries)" >&2
else
    echo "  [WARN] No tracked entries (entity dependency tracking may not be working)" >&2
fi

echo "======================================" >&2
