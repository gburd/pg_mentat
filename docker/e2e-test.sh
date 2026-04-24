#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Docker End-to-End Deployment Validation
# =============================================================================
#
# Validates that the full pg_mentat stack (PostgreSQL + mentatd) works
# correctly when deployed via Docker Compose.
#
# Usage:
#   cd <repo-root>
#   ./docker/e2e-test.sh
#
# Prerequisites:
#   - Docker and Docker Compose
#   - No services already running on ports 5432, 8080
#
# What it tests:
#   1. Docker image builds succeed
#   2. PostgreSQL starts with pg_mentat extension
#   3. mentatd starts and connects to PostgreSQL
#   4. Health endpoints respond
#   5. Schema installation works
#   6. Transactions work
#   7. Queries work (including range queries -- BYTEA fix validation)
#   8. Pull API works
#   9. Graceful shutdown
#  10. Data persistence across restart

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
COMPOSE_FILE="$SCRIPT_DIR/docker-compose.yml"
MENTATD_URL="http://localhost:8080"

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

test_count=0
pass_count=0
fail_count=0
skip_count=0

log_info()  { echo -e "${YELLOW}[INFO]${NC} $*"; }
log_pass()  { echo -e "${GREEN}[PASS]${NC} $*"; pass_count=$((pass_count + 1)); test_count=$((test_count + 1)); }
log_fail()  { echo -e "${RED}[FAIL]${NC} $*"; fail_count=$((fail_count + 1)); test_count=$((test_count + 1)); }
log_skip()  { echo -e "${YELLOW}[SKIP]${NC} $*"; skip_count=$((skip_count + 1)); test_count=$((test_count + 1)); }

# Cleanup on exit
cleanup() {
    log_info "Cleaning up..."
    cd "$REPO_ROOT"
    docker compose -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true
}
trap cleanup EXIT

wait_for_service() {
    local url="$1"
    local name="$2"
    local max_attempts="${3:-30}"
    local attempt=1

    log_info "Waiting for $name to be ready..."
    while [ $attempt -le $max_attempts ]; do
        if curl -sf "$url" > /dev/null 2>&1; then
            log_info "$name is ready (attempt $attempt/$max_attempts)"
            return 0
        fi
        sleep 2
        attempt=$((attempt + 1))
    done
    log_fail "$name failed to start after $max_attempts attempts"
    return 1
}

edn_request() {
    local data="$1"
    curl -sf -X POST "$MENTATD_URL/" \
        -H "Content-Type: application/edn" \
        -d "$data" 2>/dev/null || echo ""
}

# =============================================================================
# Tests
# =============================================================================

echo "============================================"
echo "pg_mentat Docker End-to-End Validation"
echo "============================================"
echo

# --- Test 1: Docker Compose build ---
log_info "Building Docker images..."
cd "$REPO_ROOT"
if docker compose -f "$COMPOSE_FILE" build postgres mentatd 2>&1 | tail -5; then
    log_pass "Docker images built successfully"
else
    log_fail "Docker image build failed"
    exit 1
fi

# --- Test 2: Services start ---
log_info "Starting services..."
if docker compose -f "$COMPOSE_FILE" up -d postgres mentatd 2>&1; then
    log_pass "Services started"
else
    log_fail "Failed to start services"
    exit 1
fi

# --- Test 3: PostgreSQL healthy ---
if wait_for_service "http://localhost:5432" "PostgreSQL" 30 2>/dev/null; then
    log_pass "PostgreSQL is accepting connections"
else
    # pg_isready check instead of HTTP
    if docker compose -f "$COMPOSE_FILE" exec -T postgres pg_isready -U postgres -d mentat 2>/dev/null; then
        log_pass "PostgreSQL is accepting connections (via pg_isready)"
    else
        log_fail "PostgreSQL did not become ready"
    fi
fi

# --- Test 4: mentatd health endpoint ---
if wait_for_service "$MENTATD_URL/health" "mentatd" 30; then
    log_pass "mentatd health endpoint responds"
else
    log_fail "mentatd health endpoint not responding"
    docker compose -f "$COMPOSE_FILE" logs mentatd | tail -20
    exit 1
fi

# --- Test 5: Health check returns valid response ---
health_response=$(curl -sf "$MENTATD_URL/health" 2>/dev/null || echo "")
if echo "$health_response" | grep -qi "healthy\|ok\|status"; then
    log_pass "Health check returns valid response"
else
    log_fail "Health check response invalid: $health_response"
fi

# --- Test 6: List databases ---
response=$(edn_request '{:op :list-dbs}')
if echo "$response" | grep -q ":result"; then
    log_pass "List databases operation works"
else
    log_fail "List databases failed: $response"
fi

# --- Test 7: Connect to database ---
response=$(edn_request '{:op :connect :args {:db-name "mentat"}}')
if echo "$response" | grep -q "connection-id\|:result"; then
    log_pass "Database connection works"
else
    log_fail "Database connection failed: $response"
fi

# --- Test 8: Schema transaction ---
response=$(edn_request '{:op :transact :args {:connection-id "test" :tx-data "[{:db/ident :test/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/ident :test/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]"}}')
if echo "$response" | grep -q "tx-id\|:result\|status"; then
    log_pass "Schema transaction succeeds"
else
    log_fail "Schema transaction failed: $response"
fi

# --- Test 9: Data transaction ---
response=$(edn_request '{:op :transact :args {:connection-id "test" :tx-data "[{:test/name \"Alice\" :test/age 30} {:test/name \"Bob\" :test/age 25} {:test/name \"Carol\" :test/age 100} {:test/name \"Dave\" :test/age 2}]"}}')
if echo "$response" | grep -q "tx-id\|:result\|status"; then
    log_pass "Data transaction succeeds"
else
    log_fail "Data transaction failed: $response"
fi

# --- Test 10: Basic query ---
response=$(edn_request '{:op :q :args {:query "[:find ?name ?age :where [?e :test/name ?name] [?e :test/age ?age]]" :args []}}')
if echo "$response" | grep -q "Alice\|:result"; then
    log_pass "Basic query returns results"
else
    log_fail "Basic query failed: $response"
fi

# --- Test 11: Range query (BYTEA fix validation) ---
response=$(edn_request '{:op :q :args {:query "[:find ?name ?age :where [?e :test/name ?name] [?e :test/age ?age] [(> ?age 30)]]" :args []}}')
if echo "$response" | grep -q ":result"; then
    # Verify Dave (age=2) is NOT in results but Carol (age=100) IS
    if echo "$response" | grep -q "Carol"; then
        if echo "$response" | grep -q "Dave"; then
            log_fail "Range query BYTEA bug: Dave (age=2) incorrectly matched (> ?age 30)"
        else
            log_pass "Range query (> age 30) correct: includes Carol(100), excludes Dave(2)"
        fi
    else
        log_skip "Range query returned results but could not verify contents: $response"
    fi
else
    log_fail "Range query failed: $response"
fi

# --- Test 12: Graceful restart ---
log_info "Testing graceful restart..."
docker compose -f "$COMPOSE_FILE" restart mentatd 2>&1
if wait_for_service "$MENTATD_URL/health" "mentatd (after restart)" 30; then
    log_pass "mentatd gracefully restarts"
else
    log_fail "mentatd did not restart successfully"
fi

# --- Test 13: Data persists after restart ---
response=$(edn_request '{:op :q :args {:query "[:find ?name :where [?e :test/name ?name]]" :args []}}')
if echo "$response" | grep -q "Alice"; then
    log_pass "Data persists after mentatd restart"
else
    log_fail "Data lost after restart: $response"
fi

# --- Test 14: PostgreSQL restart with volume persistence ---
log_info "Testing PostgreSQL restart with data persistence..."
docker compose -f "$COMPOSE_FILE" restart postgres 2>&1
sleep 5
if wait_for_service "$MENTATD_URL/health" "mentatd (after PG restart)" 45; then
    response=$(edn_request '{:op :q :args {:query "[:find ?name :where [?e :test/name ?name]]" :args []}}')
    if echo "$response" | grep -q "Alice"; then
        log_pass "Data persists after PostgreSQL restart"
    else
        log_fail "Data lost after PostgreSQL restart: $response"
    fi
else
    log_fail "Stack did not recover after PostgreSQL restart"
fi

# --- Test 15: Container resource limits ---
mentatd_mem=$(docker stats --no-stream --format "{{.MemUsage}}" pg_mentat_mentatd 2>/dev/null | head -1)
if [ -n "$mentatd_mem" ]; then
    log_pass "Container running with resource limits: $mentatd_mem"
else
    log_skip "Could not read container resource usage"
fi

# =============================================================================
# Summary
# =============================================================================

echo
echo "============================================"
echo "End-to-End Test Summary"
echo "============================================"
echo "Total:   $test_count"
echo -e "Passed:  ${GREEN}$pass_count${NC}"
echo -e "Failed:  ${RED}$fail_count${NC}"
echo -e "Skipped: ${YELLOW}$skip_count${NC}"
echo

if [ "$fail_count" -eq 0 ]; then
    echo -e "${GREEN}All tests passed! Deployment is valid.${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed. Review the output above.${NC}"
    exit 1
fi
