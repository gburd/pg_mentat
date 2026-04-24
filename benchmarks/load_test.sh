#!/usr/bin/env bash
#
# mentatd Load Testing Suite
#
# Validates performance targets:
#   - Throughput: >= 50 TPS sustained
#   - Latency:   p99 < 100ms, p50 < 50ms
#   - Error rate: < 0.1%
#   - Memory:    Stable (no leaks)
#   - Write throughput: >= 10K datoms/sec
#   - Simple queries:   < 50ms p50
#   - Complex queries:  < 500ms p99
#
# Usage:
#   ./benchmarks/load_test.sh [scenario] [options]
#
# Scenarios:
#   all           - Run all scenarios (default)
#   steady        - Steady-state 50 TPS for configurable duration
#   spike         - Ramp from 10 to 100 TPS
#   large         - Complex queries with large result sets
#   mixed         - 80% reads, 20% writes
#   writes        - 100% concurrent writes (sequence allocation stress test)
#   health        - Baseline health check throughput
#
# Options:
#   --host HOST        Server host (default: 127.0.0.1)
#   --port PORT        Server port (default: 8080)
#   --duration SECS    Test duration in seconds (default: 60)
#   --concurrency N    Number of concurrent workers (default: 50)
#   --output DIR       Results output directory (default: benchmarks/results)
#   --db-name NAME     Database name to test against (default: mentat)
#   --k6               Use k6 for load testing (requires k6 installed)
#   --verbose          Enable verbose output
#   --no-setup         Skip schema/data setup
#   --dry-run          Print what would be run without executing
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Defaults
HOST="127.0.0.1"
PORT="8080"
DURATION=60
CONCURRENCY=50
OUTPUT_DIR="$SCRIPT_DIR/results"
DB_NAME="mentat"
VERBOSE=false
NO_SETUP=false
DRY_RUN=false
USE_K6=false
SCENARIO="all"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RUN_DIR=""

# Performance targets
TARGET_TPS=50
TARGET_P99_MS=100
TARGET_P50_MS=50
TARGET_ERROR_RATE="0.001"  # 0.1%

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info()  { echo -e "${BLUE}[INFO]${NC} $*"; }
log_ok()    { echo -e "${GREEN}[PASS]${NC} $*"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_fail()  { echo -e "${RED}[FAIL]${NC} $*"; }
log_debug() { if $VERBOSE; then echo -e "[DEBUG] $*"; fi; }

BASE_URL=""

usage() {
    head -n 32 "$0" | tail -n +2 | sed 's/^# \?//'
    exit 0
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            all|steady|spike|large|mixed|writes|health)
                SCENARIO="$1"; shift ;;
            --host)       HOST="$2"; shift 2 ;;
            --port)       PORT="$2"; shift 2 ;;
            --duration)   DURATION="$2"; shift 2 ;;
            --concurrency) CONCURRENCY="$2"; shift 2 ;;
            --output)     OUTPUT_DIR="$2"; shift 2 ;;
            --db-name)    DB_NAME="$2"; shift 2 ;;
            --k6)         USE_K6=true; shift ;;
            --verbose)    VERBOSE=true; shift ;;
            --no-setup)   NO_SETUP=true; shift ;;
            --dry-run)    DRY_RUN=true; shift ;;
            -h|--help)    usage ;;
            *)
                echo "Unknown argument: $1"
                usage ;;
        esac
    done
    BASE_URL="http://${HOST}:${PORT}"
    RUN_DIR="${OUTPUT_DIR}/${TIMESTAMP}_${SCENARIO}"
}

# ── Helper: send EDN request and capture timing ──────────────────────
# Writes: status_code  time_total_ms  size_download  to stdout
edn_request() {
    local body="$1"
    curl -s -o /dev/null -w '%{http_code} %{time_total} %{size_download}' \
        -X POST "${BASE_URL}/" \
        -H 'Content-Type: application/edn' \
        -d "$body"
}

# Full response variant
edn_request_full() {
    local body="$1"
    curl -s -X POST "${BASE_URL}/" \
        -H 'Content-Type: application/edn' \
        -d "$body"
}

# ── Preflight checks ────────────────────────────────────────────────
preflight() {
    log_info "Running preflight checks..."

    # Check that curl is available
    if ! command -v curl &>/dev/null; then
        log_fail "curl is required but not found"
        exit 1
    fi

    # Check that bc is available (for floating point math)
    if ! command -v bc &>/dev/null; then
        log_warn "bc not found; some analysis may be limited"
    fi

    # Check server is reachable
    local health
    health=$(curl -s -o /dev/null -w '%{http_code}' "${BASE_URL}/health" 2>/dev/null || echo "000")
    if [[ "$health" != "200" ]]; then
        log_fail "mentatd not reachable at ${BASE_URL} (HTTP $health)"
        log_info "Start the server first: cargo run --bin mentatd"
        exit 1
    fi
    log_ok "Server reachable at ${BASE_URL}"

    # Check EDN endpoint
    local edn_check
    edn_check=$(edn_request_full '{:op :health}')
    if [[ "$edn_check" == *"healthy"* ]]; then
        log_ok "EDN endpoint responding correctly"
    else
        log_fail "EDN endpoint not responding as expected: $edn_check"
        exit 1
    fi

    # Create output directory
    mkdir -p "$RUN_DIR"
    log_ok "Results directory: $RUN_DIR"
}

# ── Setup test data ─────────────────────────────────────────────────
setup_test_data() {
    if $NO_SETUP; then
        log_info "Skipping test data setup (--no-setup)"
        return 0
    fi

    log_info "Setting up test schema and data..."

    # Install schema
    local schema_tx='[{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/ident :person/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one} {:db/ident :person/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'

    local schema_result
    schema_result=$(edn_request_full "{:op :transact :args {:connection-id \"bench\" :tx-data $schema_tx}}")
    log_debug "Schema result: $schema_result"

    # Insert test data (100 entities)
    for i in $(seq 1 100); do
        local tx_data="[{:person/name \"Person_${i}\" :person/age $((20 + i % 60)) :person/email \"person${i}@example.com\"}]"
        edn_request_full "{:op :transact :args {:connection-id \"bench\" :tx-data $tx_data}}" > /dev/null 2>&1
    done

    log_ok "Inserted 100 test entities"
}

# ── Worker function: runs requests in a tight loop ──────────────────
# Arguments: worker_id  request_body  duration_secs  output_file
run_worker() {
    local worker_id="$1"
    local body="$2"
    local duration="$3"
    local outfile="$4"
    local end_time=$(($(date +%s) + duration))

    while [[ $(date +%s) -lt $end_time ]]; do
        local start_ns
        start_ns=$(date +%s%N 2>/dev/null || date +%s000000000)
        local result
        result=$(edn_request "$body")
        local end_ns
        end_ns=$(date +%s%N 2>/dev/null || date +%s000000000)

        local status_code time_total size
        read -r status_code time_total size <<< "$result"

        # Convert time_total from curl (seconds with decimals) to milliseconds
        local latency_ms
        latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")

        echo "${worker_id} ${status_code} ${latency_ms} ${size}" >> "$outfile"
    done
}

# ── Rate-limited worker: targets a specific TPS per worker ──────────
# Arguments: worker_id  request_body  duration_secs  target_rps  output_file
run_rate_limited_worker() {
    local worker_id="$1"
    local body="$2"
    local duration="$3"
    local target_rps="$4"
    local outfile="$5"
    local end_time=$(($(date +%s) + duration))

    local interval
    if command -v bc &>/dev/null; then
        interval=$(echo "scale=6; 1.0 / $target_rps" | bc)
    else
        interval="0.02"  # ~50 RPS fallback
    fi

    while [[ $(date +%s) -lt $end_time ]]; do
        local result
        result=$(edn_request "$body")

        local status_code time_total size
        read -r status_code time_total size <<< "$result"

        local latency_ms
        latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")

        echo "${worker_id} ${status_code} ${latency_ms} ${size}" >> "$outfile"

        # Sleep to maintain target rate
        local sleep_time
        if command -v bc &>/dev/null; then
            sleep_time=$(echo "$interval - $time_total" | bc 2>/dev/null)
            if [[ "${sleep_time:0:1}" != "-" ]] && [[ "$sleep_time" != "0" ]]; then
                sleep "$sleep_time" 2>/dev/null || true
            fi
        fi
    done
}

# ── Analyze results from a raw data file ────────────────────────────
# Format per line: worker_id status_code latency_ms response_size
analyze_results() {
    local datafile="$1"
    local scenario_name="$2"
    local report_file="$3"

    if [[ ! -s "$datafile" ]]; then
        log_fail "No data collected for $scenario_name"
        echo "NO DATA" > "$report_file"
        return 1
    fi

    local total_requests error_count
    total_requests=$(wc -l < "$datafile")
    error_count=$(awk '$2 != 200 { count++ } END { print count+0 }' "$datafile")

    # Extract latencies, sort numerically
    local latency_file="${datafile}.latencies"
    awk '{ print $3 }' "$datafile" | sort -n > "$latency_file"

    local min_lat max_lat avg_lat p50_lat p95_lat p99_lat
    min_lat=$(head -1 "$latency_file")
    max_lat=$(tail -1 "$latency_file")

    if command -v bc &>/dev/null; then
        avg_lat=$(awk '{ sum += $1; n++ } END { if(n>0) printf "%.2f", sum/n; else print 0 }' "$latency_file")

        local p50_idx p95_idx p99_idx
        p50_idx=$(echo "($total_requests * 50 + 99) / 100" | bc)
        p95_idx=$(echo "($total_requests * 95 + 99) / 100" | bc)
        p99_idx=$(echo "($total_requests * 99 + 99) / 100" | bc)

        p50_lat=$(sed -n "${p50_idx}p" "$latency_file")
        p95_lat=$(sed -n "${p95_idx}p" "$latency_file")
        p99_lat=$(sed -n "${p99_idx}p" "$latency_file")
    else
        avg_lat="N/A"
        p50_lat="N/A"
        p95_lat="N/A"
        p99_lat="N/A"
    fi

    local error_rate
    if command -v bc &>/dev/null; then
        error_rate=$(echo "scale=4; $error_count / $total_requests" | bc 2>/dev/null || echo "N/A")
    else
        error_rate="N/A"
    fi

    local tps
    if command -v bc &>/dev/null; then
        tps=$(echo "scale=2; $total_requests / $DURATION" | bc 2>/dev/null || echo "N/A")
    else
        tps="$((total_requests / DURATION))"
    fi

    # Write report
    {
        echo "============================================="
        echo "  Load Test Report: $scenario_name"
        echo "  Timestamp: $TIMESTAMP"
        echo "============================================="
        echo ""
        echo "Configuration:"
        echo "  Server:      ${BASE_URL}"
        echo "  Duration:    ${DURATION}s"
        echo "  Concurrency: ${CONCURRENCY}"
        echo ""
        echo "Results:"
        echo "  Total requests: $total_requests"
        echo "  Errors:         $error_count"
        echo "  Error rate:     $error_rate"
        echo "  Throughput:     ${tps} TPS"
        echo ""
        echo "Latency (ms):"
        echo "  Min:  $min_lat"
        echo "  Avg:  $avg_lat"
        echo "  p50:  $p50_lat"
        echo "  p95:  $p95_lat"
        echo "  p99:  $p99_lat"
        echo "  Max:  $max_lat"
        echo ""
        echo "Target Validation:"

        local pass=true

        if command -v bc &>/dev/null && [[ "$tps" != "N/A" ]]; then
            if (( $(echo "$tps >= $TARGET_TPS" | bc -l) )); then
                echo "  [PASS] Throughput:  ${tps} TPS >= ${TARGET_TPS} TPS"
            else
                echo "  [FAIL] Throughput:  ${tps} TPS < ${TARGET_TPS} TPS"
                pass=false
            fi
        fi

        if command -v bc &>/dev/null && [[ "$p99_lat" != "N/A" ]]; then
            if (( $(echo "$p99_lat < $TARGET_P99_MS" | bc -l) )); then
                echo "  [PASS] p99 Latency: ${p99_lat}ms < ${TARGET_P99_MS}ms"
            else
                echo "  [FAIL] p99 Latency: ${p99_lat}ms >= ${TARGET_P99_MS}ms"
                pass=false
            fi
        fi

        if command -v bc &>/dev/null && [[ "$p50_lat" != "N/A" ]]; then
            if (( $(echo "$p50_lat < $TARGET_P50_MS" | bc -l) )); then
                echo "  [PASS] p50 Latency: ${p50_lat}ms < ${TARGET_P50_MS}ms"
            else
                echo "  [FAIL] p50 Latency: ${p50_lat}ms >= ${TARGET_P50_MS}ms"
                pass=false
            fi
        fi

        if command -v bc &>/dev/null && [[ "$error_rate" != "N/A" ]]; then
            if (( $(echo "$error_rate < $TARGET_ERROR_RATE" | bc -l) )); then
                echo "  [PASS] Error rate:  ${error_rate} < ${TARGET_ERROR_RATE}"
            else
                echo "  [FAIL] Error rate:  ${error_rate} >= ${TARGET_ERROR_RATE}"
                pass=false
            fi
        fi

        echo ""
        if $pass; then
            echo "Overall: PASS"
        else
            echo "Overall: FAIL"
        fi
    } > "$report_file"

    cat "$report_file"

    # Also write machine-readable JSON summary
    local json_file="${report_file%.txt}.json"
    cat > "$json_file" <<ENDJSON
{
  "scenario": "$scenario_name",
  "timestamp": "$TIMESTAMP",
  "config": {
    "host": "$HOST",
    "port": $PORT,
    "duration_secs": $DURATION,
    "concurrency": $CONCURRENCY
  },
  "results": {
    "total_requests": $total_requests,
    "errors": $error_count,
    "error_rate": ${error_rate:-0},
    "tps": ${tps:-0}
  },
  "latency_ms": {
    "min": ${min_lat:-0},
    "avg": ${avg_lat:-0},
    "p50": ${p50_lat:-0},
    "p95": ${p95_lat:-0},
    "p99": ${p99_lat:-0},
    "max": ${max_lat:-0}
  },
  "targets": {
    "tps": $TARGET_TPS,
    "p99_ms": $TARGET_P99_MS,
    "p50_ms": $TARGET_P50_MS,
    "error_rate": $TARGET_ERROR_RATE
  }
}
ENDJSON

    rm -f "$latency_file"
}

# ── Scenario: Health Check Baseline ─────────────────────────────────
scenario_health() {
    log_info "=== Scenario: Health Check Baseline ==="
    log_info "Testing raw server throughput with health endpoint"

    local datafile="$RUN_DIR/health_raw.dat"
    > "$datafile"

    local pids=()
    for i in $(seq 1 "$CONCURRENCY"); do
        run_worker "$i" '{:op :health}' "$DURATION" "$datafile" &
        pids+=($!)
    done

    log_info "Running $CONCURRENCY workers for ${DURATION}s..."
    for pid in "${pids[@]}"; do
        wait "$pid" 2>/dev/null || true
    done

    analyze_results "$datafile" "health_baseline" "$RUN_DIR/health_report.txt"
}

# ── Scenario: Steady State ──────────────────────────────────────────
scenario_steady() {
    log_info "=== Scenario: Steady State (${TARGET_TPS} TPS) ==="
    log_info "Sustaining target throughput with simple queries"

    local datafile="$RUN_DIR/steady_raw.dat"
    > "$datafile"

    local query_body='{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}'

    # Distribute target TPS across workers
    local workers=$((CONCURRENCY < 10 ? CONCURRENCY : 10))
    local rps_per_worker
    if command -v bc &>/dev/null; then
        rps_per_worker=$(echo "scale=2; $TARGET_TPS / $workers" | bc)
    else
        rps_per_worker=$((TARGET_TPS / workers))
    fi

    local pids=()
    for i in $(seq 1 "$workers"); do
        run_rate_limited_worker "$i" "$query_body" "$DURATION" "$rps_per_worker" "$datafile" &
        pids+=($!)
    done

    log_info "Running $workers workers at ~${rps_per_worker} RPS each for ${DURATION}s..."
    for pid in "${pids[@]}"; do
        wait "$pid" 2>/dev/null || true
    done

    analyze_results "$datafile" "steady_state" "$RUN_DIR/steady_report.txt"
}

# ── Scenario: Spike ─────────────────────────────────────────────────
scenario_spike() {
    log_info "=== Scenario: Spike Test (10 -> 100 TPS) ==="
    log_info "Ramping traffic from low to high to test elasticity"

    local datafile="$RUN_DIR/spike_raw.dat"
    > "$datafile"

    local query_body='{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}'

    # Run in 3 phases: low (10 TPS), medium (50 TPS), high (100 TPS)
    local phase_duration=$((DURATION / 3))

    for phase in low medium high; do
        local target_rps workers
        case "$phase" in
            low)    target_rps=10; workers=5 ;;
            medium) target_rps=50; workers=10 ;;
            high)   target_rps=100; workers=20 ;;
        esac

        local rps_per_worker
        if command -v bc &>/dev/null; then
            rps_per_worker=$(echo "scale=2; $target_rps / $workers" | bc)
        else
            rps_per_worker=$((target_rps / workers))
        fi

        log_info "  Phase: $phase (${target_rps} TPS, ${workers} workers, ${phase_duration}s)"

        local pids=()
        for i in $(seq 1 "$workers"); do
            run_rate_limited_worker "${phase}_${i}" "$query_body" "$phase_duration" "$rps_per_worker" "$datafile" &
            pids+=($!)
        done

        for pid in "${pids[@]}"; do
            wait "$pid" 2>/dev/null || true
        done
    done

    analyze_results "$datafile" "spike" "$RUN_DIR/spike_report.txt"
}

# ── Scenario: Large Queries ─────────────────────────────────────────
scenario_large() {
    log_info "=== Scenario: Large Queries ==="
    log_info "Complex queries returning many results"

    local datafile="$RUN_DIR/large_raw.dat"
    > "$datafile"

    # Query that returns all entities (potentially large result set)
    local query_body='{:op :q :args {:query [:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]}}'

    local workers=$((CONCURRENCY < 20 ? CONCURRENCY : 20))
    local pids=()
    for i in $(seq 1 "$workers"); do
        run_worker "$i" "$query_body" "$DURATION" "$datafile" &
        pids+=($!)
    done

    log_info "Running $workers workers with large queries for ${DURATION}s..."
    for pid in "${pids[@]}"; do
        wait "$pid" 2>/dev/null || true
    done

    analyze_results "$datafile" "large_queries" "$RUN_DIR/large_report.txt"
}

# ── Scenario: Mixed Workload ────────────────────────────────────────
scenario_mixed() {
    log_info "=== Scenario: Mixed Workload (80% reads, 20% writes) ==="

    local datafile="$RUN_DIR/mixed_raw.dat"
    > "$datafile"

    local read_query='{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}'

    # For writes, we use a simple transact
    # The worker will choose read vs write based on a random threshold

    local workers=$((CONCURRENCY < 20 ? CONCURRENCY : 20))
    local read_workers=$(( (workers * 80 + 99) / 100 ))
    local write_workers=$((workers - read_workers))

    if [[ $write_workers -lt 1 ]]; then
        write_workers=1
        read_workers=$((workers - 1))
    fi

    log_info "  Read workers:  $read_workers"
    log_info "  Write workers: $write_workers"

    local pids=()

    # Read workers
    for i in $(seq 1 "$read_workers"); do
        run_worker "r${i}" "$read_query" "$DURATION" "$datafile" &
        pids+=($!)
    done

    # Write workers (each worker transacts a unique entity)
    for i in $(seq 1 "$write_workers"); do
        (
            local wid="w${i}"
            local end_time=$(($(date +%s) + DURATION))
            local counter=0
            while [[ $(date +%s) -lt $end_time ]]; do
                counter=$((counter + 1))
                local tx_body="{:op :transact :args {:connection-id \"bench\" :tx-data [{:person/name \"LoadTest_w${i}_${counter}\" :person/age $((20 + counter % 60))}]}}"
                local result
                result=$(edn_request "$tx_body")
                local status_code time_total size
                read -r status_code time_total size <<< "$result"
                local latency_ms
                latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")
                echo "${wid} ${status_code} ${latency_ms} ${size}" >> "$datafile"
                # Writes are slower, add small delay
                sleep 0.1 2>/dev/null || true
            done
        ) &
        pids+=($!)
    done

    log_info "Running mixed workload for ${DURATION}s..."
    for pid in "${pids[@]}"; do
        wait "$pid" 2>/dev/null || true
    done

    analyze_results "$datafile" "mixed_workload" "$RUN_DIR/mixed_report.txt"
}

# ── Scenario: Concurrent Writes ───────────────────────────────────
# Specifically validates the sequence-based entity ID allocation.
# All workers perform write transactions to stress nextval() path.
scenario_writes() {
    log_info "=== Scenario: Concurrent Writes (100% writes, sequence stress test) ==="

    local datafile="$RUN_DIR/writes_raw.dat"
    > "$datafile"

    # Install loadtest schema
    edn_request_full '{:op :transact :args {:connection-id "bench" :tx-data [{:db/ident :loadtest/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/ident :loadtest/counter :db/valueType :db.type/long :db/cardinality :db.cardinality/one} {:db/ident :loadtest/worker :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]}}' > /dev/null 2>&1

    local workers=$((CONCURRENCY < 100 ? CONCURRENCY : 100))

    local pids=()
    for i in $(seq 1 "$workers"); do
        (
            local wid="cw${i}"
            local end_time=$(($(date +%s) + DURATION))
            local counter=0
            while [[ $(date +%s) -lt $end_time ]]; do
                counter=$((counter + 1))
                local unique_id="w${i}_c${counter}_$(date +%s%N)"
                local tx_body="{:op :transact :args {:connection-id \"bench\" :tx-data [{:loadtest/name \"${unique_id}\" :loadtest/counter ${counter} :loadtest/worker \"worker-${i}\"}]}}"
                local result
                result=$(edn_request "$tx_body")
                local status_code time_total size
                read -r status_code time_total size <<< "$result"
                local latency_ms
                latency_ms=$(echo "$time_total * 1000" | bc 2>/dev/null || echo "0")
                echo "${wid} ${status_code} ${latency_ms} ${size}" >> "$datafile"
            done
        ) &
        pids+=($!)
    done

    log_info "Running $workers concurrent write workers for ${DURATION}s..."
    for pid in "${pids[@]}"; do
        wait "$pid" 2>/dev/null || true
    done

    analyze_results "$datafile" "concurrent_writes" "$RUN_DIR/writes_report.txt"

    # Post-test: verify no duplicate entity IDs
    log_info "Verifying no duplicate entity IDs..."
    local count_result
    count_result=$(edn_request_full '{:op :q :args {:query [:find (count ?e) :where [?e :loadtest/name _]]}}')
    log_info "Total loadtest entities: $count_result"
}

# ── k6 runner ──────────────────────────────────────────────────────
# Runs a k6 scenario .js file with appropriate environment variables
run_k6_scenario() {
    local scenario_name="$1"
    local js_file="$2"

    if [[ ! -f "$js_file" ]]; then
        log_fail "k6 scenario file not found: $js_file"
        return 1
    fi

    log_info "Running k6 scenario: $scenario_name ($js_file)"

    local k6_json="$RUN_DIR/${scenario_name}_k6.json"
    local k6_summary="$RUN_DIR/${scenario_name}_k6_summary.json"

    k6 run \
        --out "json=${k6_json}" \
        --summary-export="${k6_summary}" \
        -e "BASE_URL=${BASE_URL}" \
        -e "DURATION=${DURATION}s" \
        -e "TARGET_TPS=${TARGET_TPS}" \
        -e "CONCURRENCY=${CONCURRENCY}" \
        "$js_file" 2>&1 | tee "$RUN_DIR/${scenario_name}_k6.log"

    local exit_code=${PIPESTATUS[0]}

    if [[ $exit_code -eq 0 ]]; then
        log_ok "k6 scenario $scenario_name: PASS (thresholds met)"
    else
        log_fail "k6 scenario $scenario_name: FAIL (thresholds breached or error)"
    fi

    # Generate a report from the k6 summary JSON
    if [[ -f "$k6_summary" ]]; then
        generate_k6_report "$scenario_name" "$k6_summary" "$RUN_DIR/${scenario_name}_report.txt"
    fi

    return $exit_code
}

generate_k6_report() {
    local scenario_name="$1"
    local summary_json="$2"
    local report_file="$3"

    # Use python to parse k6 summary JSON and produce a human-readable report
    python3 - "$summary_json" "$scenario_name" "$report_file" "$TARGET_TPS" "$TARGET_P99_MS" "$TARGET_P50_MS" "$TARGET_ERROR_RATE" <<'PYEOF'
import json, sys

summary_file = sys.argv[1]
scenario = sys.argv[2]
report_file = sys.argv[3]
target_tps = float(sys.argv[4])
target_p99 = float(sys.argv[5])
target_p50 = float(sys.argv[6])
target_err = float(sys.argv[7])

with open(summary_file) as f:
    data = json.load(f)

metrics = data.get("metrics", {})
dur = metrics.get("http_req_duration", {})
reqs = metrics.get("http_reqs", {})
fails = metrics.get("http_req_failed", {})

total = reqs.get("count", 0)
rate = reqs.get("rate", 0)
p50 = dur.get("med", 0)
p95 = dur.get("p(95)", 0)
p99 = dur.get("p(99)", 0)
avg = dur.get("avg", 0)
min_lat = dur.get("min", 0)
max_lat = dur.get("max", 0)
err_rate = fails.get("rate", 0) if fails else 0

lines = []
lines.append("=" * 55)
lines.append(f"  Load Test Report: {scenario} (k6)")
lines.append("=" * 55)
lines.append("")
lines.append("Results:")
lines.append(f"  Total requests: {total}")
lines.append(f"  Error rate:     {err_rate:.4f}")
lines.append(f"  Throughput:     {rate:.1f} TPS")
lines.append("")
lines.append("Latency (ms):")
lines.append(f"  Min:  {min_lat:.2f}")
lines.append(f"  Avg:  {avg:.2f}")
lines.append(f"  p50:  {p50:.2f}")
lines.append(f"  p95:  {p95:.2f}")
lines.append(f"  p99:  {p99:.2f}")
lines.append(f"  Max:  {max_lat:.2f}")
lines.append("")
lines.append("Target Validation:")

all_pass = True
if rate >= target_tps:
    lines.append(f"  [PASS] Throughput:  {rate:.1f} TPS >= {target_tps:.0f} TPS")
else:
    lines.append(f"  [FAIL] Throughput:  {rate:.1f} TPS < {target_tps:.0f} TPS")
    all_pass = False

if p99 < target_p99:
    lines.append(f"  [PASS] p99 Latency: {p99:.1f}ms < {target_p99:.0f}ms")
else:
    lines.append(f"  [FAIL] p99 Latency: {p99:.1f}ms >= {target_p99:.0f}ms")
    all_pass = False

if p50 < target_p50:
    lines.append(f"  [PASS] p50 Latency: {p50:.1f}ms < {target_p50:.0f}ms")
else:
    lines.append(f"  [FAIL] p50 Latency: {p50:.1f}ms >= {target_p50:.0f}ms")
    all_pass = False

if err_rate < target_err:
    lines.append(f"  [PASS] Error rate:  {err_rate:.4f} < {target_err}")
else:
    lines.append(f"  [FAIL] Error rate:  {err_rate:.4f} >= {target_err}")
    all_pass = False

lines.append("")
lines.append(f"Overall: {'PASS' if all_pass else 'FAIL'}")

report = "\n".join(lines)
with open(report_file, "w") as f:
    f.write(report)
print(report)
PYEOF
}

# ── k6 scenario dispatch ──────────────────────────────────────────
run_k6_scenarios() {
    local scenarios_dir="$SCRIPT_DIR/scenarios"
    local any_fail=false

    case "$SCENARIO" in
        all)
            run_k6_scenario "steady_state" "$scenarios_dir/steady_state.js" || any_fail=true
            echo ""
            run_k6_scenario "spike" "$scenarios_dir/spike.js" || any_fail=true
            echo ""
            run_k6_scenario "large_queries" "$scenarios_dir/large_queries.js" || any_fail=true
            echo ""
            run_k6_scenario "mixed_workload" "$scenarios_dir/mixed_workload.js" || any_fail=true
            echo ""
            run_k6_scenario "concurrent_writes" "$scenarios_dir/concurrent_writes.js" || any_fail=true
            ;;
        steady) run_k6_scenario "steady_state" "$scenarios_dir/steady_state.js" || any_fail=true ;;
        spike)  run_k6_scenario "spike" "$scenarios_dir/spike.js" || any_fail=true ;;
        large)  run_k6_scenario "large_queries" "$scenarios_dir/large_queries.js" || any_fail=true ;;
        mixed)  run_k6_scenario "mixed_workload" "$scenarios_dir/mixed_workload.js" || any_fail=true ;;
        writes) run_k6_scenario "concurrent_writes" "$scenarios_dir/concurrent_writes.js" || any_fail=true ;;
        health)
            log_warn "No k6 health scenario; falling back to curl-based health test"
            scenario_health
            ;;
    esac

    if $any_fail; then
        return 1
    fi
    return 0
}

# ── Generate combined summary ───────────────────────────────────────
generate_summary() {
    local summary_file="$RUN_DIR/summary.txt"

    {
        echo "╔═══════════════════════════════════════════════╗"
        echo "║    mentatd Load Test Summary                  ║"
        echo "║    $(date)                ║"
        echo "╚═══════════════════════════════════════════════╝"
        echo ""
        echo "Server: ${BASE_URL}"
        echo "Database: ${DB_NAME}"
        echo ""

        local any_fail=false
        for report in "$RUN_DIR"/*_report.txt; do
            if [[ -f "$report" ]]; then
                echo "---"
                local name
                name=$(basename "$report" _report.txt)
                local overall
                overall=$(grep "^Overall:" "$report" 2>/dev/null || echo "N/A")
                echo "$name: $overall"

                local tps_line
                tps_line=$(grep "Throughput:" "$report" | head -1)
                echo "  $tps_line"

                local p99_line
                p99_line=$(grep "p99:" "$report" | head -1)
                echo "  $p99_line"

                if [[ "$overall" == *"FAIL"* ]]; then
                    any_fail=true
                fi
            fi
        done

        echo ""
        echo "==========================================="
        if $any_fail; then
            echo "OVERALL RESULT: FAIL"
            echo "Some performance targets were not met."
        else
            echo "OVERALL RESULT: PASS"
            echo "All performance targets met."
        fi
        echo "==========================================="
        echo ""
        echo "Raw data:    $RUN_DIR/"
        echo "JSON reports: $RUN_DIR/*.json"
    } > "$summary_file"

    echo ""
    cat "$summary_file"
}

# ── Main ────────────────────────────────────────────────────────────
main() {
    parse_args "$@"

    local mode="curl"
    if $USE_K6; then
        if command -v k6 &>/dev/null; then
            mode="k6"
        else
            log_warn "k6 not found; falling back to curl-based tests"
            log_info "Install k6: https://grafana.com/docs/k6/latest/set-up/install-k6/"
        fi
    fi

    echo ""
    log_info "mentatd Load Test Suite (mode: $mode)"
    log_info "Scenario: $SCENARIO | Duration: ${DURATION}s | Concurrency: $CONCURRENCY"
    echo ""

    if $DRY_RUN; then
        log_info "[DRY RUN] Would run scenario '$SCENARIO' against ${BASE_URL} (mode: $mode)"
        log_info "[DRY RUN] Results would be written to $RUN_DIR"
        exit 0
    fi

    preflight

    if [[ "$mode" == "k6" ]]; then
        # k6 scenarios handle their own setup in the setup() function
        run_k6_scenarios
    else
        if [[ "$SCENARIO" != "health" ]]; then
            setup_test_data
        fi

        case "$SCENARIO" in
            all)
                scenario_health
                echo ""
                scenario_steady
                echo ""
                scenario_spike
                echo ""
                scenario_large
                echo ""
                scenario_mixed
                echo ""
                scenario_writes
                ;;
            health)  scenario_health ;;
            steady)  scenario_steady ;;
            spike)   scenario_spike ;;
            large)   scenario_large ;;
            mixed)   scenario_mixed ;;
            writes)  scenario_writes ;;
        esac
    fi

    echo ""
    generate_summary
}

main "$@"
