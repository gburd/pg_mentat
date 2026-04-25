#!/usr/bin/env bash
set -euo pipefail

# Test mentatd with EDN requests simulating Datomic client
# This script tests protocol compatibility without requiring actual Datomic JAR

MENTATD_URL="${MENTATD_URL:-http://localhost:8080}"
TEST_DB="test_db_$(date +%s)"

echo "Testing mentatd protocol at $MENTATD_URL"
echo

# Color codes for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

test_count=0
pass_count=0
fail_count=0

test_request() {
    local name="$1"
    local request="$2"
    local expected="$3"

    test_count=$((test_count + 1))
    echo "Test $test_count: $name"

    response=$(curl -s -X POST "$MENTATD_URL/" \
        -H "Content-Type: application/edn" \
        -d "$request")

    if echo "$response" | grep -q "$expected"; then
        echo -e "${GREEN}✓ PASS${NC}"
        pass_count=$((pass_count + 1))
    else
        echo -e "${RED}✗ FAIL${NC}"
        echo "  Request:  $request"
        echo "  Expected: $expected"
        echo "  Got:      $response"
        fail_count=$((fail_count + 1))
    fi
    echo
}

# Test 1: Health check
test_request "Health check" \
    '{:op :health}' \
    ':result'

# Test 2: List databases
test_request "List databases" \
    '{:op :list-dbs}' \
    ':result'

# Test 3: Connect to database
test_request "Connect to database" \
    "{:op :connect :args {:db-name \"postgres\"}}" \
    ':connection-id'

# Test 4: Invalid operation
test_request "Invalid operation error" \
    '{:op :invalid-op}' \
    ':error'

# Test 5: Missing required field
test_request "Missing op field error" \
    '{:foo :bar}' \
    'Missing required field'

# Test 6: Query with args
test_request "Query operation" \
    '{:op :q :args {:query "[:find ?e :where [?e :name \"Alice\"]]" :args []}}' \
    ':result'

# Test 7: Query with limit and offset
test_request "Query with limit and offset" \
    '{:op :q :args {:query "[:find ?e]" :args [] :limit 10 :offset 5}}' \
    ':result'

# Test 8: Transact operation
# After Task #5, transaction reports use Datomic-compatible format with
# :db-before, :db-after, :tx-data, :tempids (not the old :tx-id format).
test_request "Transact operation" \
    '{:op :transact :args {:connection-id "test-conn-123" :tx-data "[{:db/id -1 :name \"Bob\"}]"}}' \
    ':result'

# Test 9: Db operation with UUID
test_request "Db operation" \
    '{:op :db :args {:connection-id "550e8400-e29b-41d4-a716-446655440000"}}' \
    ':connection-id'

# Test 10: Invalid UUID
test_request "Invalid UUID error" \
    '{:op :db :args {:connection-id "not-a-uuid"}}' \
    ':error'

# Test 11: Alternate namespace (datomic.catalog)
test_request "Datomic catalog namespace" \
    '{:op :datomic.catalog/list-dbs}' \
    ':result'

# Test 12: Create database
test_request "Create database" \
    "{:op :create-db :args {:db-name \"$TEST_DB\"}}" \
    ':result'

# Test 13: Delete database
test_request "Delete database" \
    "{:op :delete-db :args {:db-name \"$TEST_DB\"}}" \
    ':result'

# Test 14: Invalid database name
test_request "Invalid database name error" \
    '{:op :create-db :args {:db-name "invalid-name-with-dashes"}}' \
    ':error'

# Test 15: Connect to nonexistent database
test_request "Connect to nonexistent database" \
    '{:op :connect :args {:db-name "nonexistent_db_xyz"}}' \
    ':error'

# Test 16: Range query with numeric predicate (BYTEA bug regression)
test_request "Numeric range query (> age 30)" \
    '{:op :q :args {:query "[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] [(> ?age 30)]]" :args []}}' \
    ':result'

# Test 17: Range query with less-than predicate
test_request "Numeric range query (< age 10)" \
    '{:op :q :args {:query "[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] [(< ?age 10)]]" :args []}}' \
    ':result'

# Test 18: Text comparison predicate
test_request "Text comparison query (> name Bob)" \
    '{:op :q :args {:query "[:find ?name :where [?e :person/name ?name] [(> ?name \"Bob\")]]" :args []}}' \
    ':result'

# Summary
echo "================================"
echo "Test Summary"
echo "================================"
echo "Total:  $test_count"
echo -e "Passed: ${GREEN}$pass_count${NC}"
echo -e "Failed: ${RED}$fail_count${NC}"
echo

if [ "$fail_count" -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed.${NC}"
    exit 1
fi
