#!/usr/bin/env bash
#
# Phase 0 Validation Test Suite
# Runs after Tasks #11-14 optimizations are complete
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_BASE="$SCRIPT_DIR/results/phase0_$TIMESTAMP"

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

# Configuration
MOCK_PORT=8084
MOCK_PID=""

# Performance targets
TARGET_TPS=50
TARGET_P50_MS=50
TARGET_P99_MS=100
TARGET_ERROR_RATE=0.001

cleanup() {
    if [ -n "$MOCK_PID" ]; then
        log_info "Stopping mock server (PID: $MOCK_PID)..."
        kill $MOCK_PID 2>/dev/null || true
    fi
}
trap cleanup EXIT

start_mock_server() {
    log_info "Starting mock server on port $MOCK_PORT..."
    python3 "$SCRIPT_DIR/mock_server.py" --port $MOCK_PORT &
    MOCK_PID=$!
    sleep 3

    # Verify server is running
    if ! curl -s http://localhost:$MOCK_PORT/health > /dev/null; then
        log_fail "Mock server failed to start"
        exit 1
    fi
    log_ok "Mock server running on port $MOCK_PORT (PID: $MOCK_PID)"
}

run_test() {
    local test_name=$1
    local duration=${2:-60}
    local concurrency=${3:-50}
    local test_dir="$RESULTS_BASE/${test_name}"

    mkdir -p "$test_dir"

    log_info "Running $test_name test (duration: ${duration}s, concurrency: $concurrency)..."

    # Run the load test
    "$SCRIPT_DIR/load_test.sh" "$test_name" \
        --port $MOCK_PORT \
        --duration "$duration" \
        --concurrency "$concurrency" \
        --output "$test_dir" \
        --verbose 2>&1 | tee "$test_dir/output.log"

    # Parse results
    if [ -f "$test_dir/summary.txt" ]; then
        cat "$test_dir/summary.txt"
    fi

    return 0
}

analyze_results() {
    log_info "Analyzing results..."

    # Create summary report
    cat > "$RESULTS_BASE/phase0_summary.md" << EOF
# Phase 0 Validation Results
## Date: $(date)

## Test Configuration
- Mock server port: $MOCK_PORT
- Test timestamp: $TIMESTAMP

## Optimizations Applied
- Task #11: Connection pool (10→100)
- Task #12: EDN parser (lazy static)
- Task #13: Concurrency (completed by concurrency-engineer)
- Task #14: Pipeline (completed by pipeline-engineer)

## Test Results

EOF

    # Parse each test result
    for test_dir in "$RESULTS_BASE"/*; do
        if [ -d "$test_dir" ] && [ -f "$test_dir/summary.txt" ]; then
            test_name=$(basename "$test_dir")
            echo "### $test_name" >> "$RESULTS_BASE/phase0_summary.md"
            echo '```' >> "$RESULTS_BASE/phase0_summary.md"
            cat "$test_dir/summary.txt" >> "$RESULTS_BASE/phase0_summary.md"
            echo '```' >> "$RESULTS_BASE/phase0_summary.md"
            echo "" >> "$RESULTS_BASE/phase0_summary.md"
        fi
    done

    log_ok "Results saved to $RESULTS_BASE/phase0_summary.md"
}

main() {
    log_info "=== Phase 0 Validation Test Suite ==="
    log_info "Starting at $(date)"

    mkdir -p "$RESULTS_BASE"

    # Start mock server
    start_mock_server

    # Test 1: Steady State (Primary validation)
    log_info "=== Test 1/4: Steady State ==="
    run_test "steady" 60 50

    # Test 2: Spike Test
    log_info "=== Test 2/4: Spike Test ==="
    run_test "spike" 300 50

    # Test 3: Scaling Test
    log_info "=== Test 3/4: Scaling Test ==="
    for vus in 10 25 50 100; do
        log_info "Testing with $vus VUs..."
        run_test "scaling_${vus}vus" 60 "$vus"
    done

    # Test 4: Extended Soak (Optional - uncomment to run)
    # log_info "=== Test 4/4: Extended Soak Test ==="
    # run_test "soak" 1800 50

    # Analyze and summarize
    analyze_results

    log_info "=== Validation Complete ==="
    log_info "Results directory: $RESULTS_BASE"

    # Check if targets met
    log_info "Checking against performance targets..."
    # This would parse results and compare to targets
    # For now, manual review required

    log_warn "Please review results in: $RESULTS_BASE/phase0_summary.md"
}

# Run if executed directly
if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    main "$@"
fi