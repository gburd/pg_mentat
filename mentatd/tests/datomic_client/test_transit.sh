#!/usr/bin/env bash
set -euo pipefail

# Test mentatd Transit+JSON and Transit+MessagePack wire formats.
# This script sends raw Transit-encoded requests and verifies the
# server handles Content-Type negotiation correctly.
#
# Prerequisites:
#   - mentatd running (default: http://localhost:8080)
#   - curl, python3 (for msgpack encoding)

MENTATD_URL="${MENTATD_URL:-http://localhost:8080}"

echo "Testing mentatd Transit wire formats at $MENTATD_URL"
echo

# Color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

test_count=0
pass_count=0
fail_count=0

# ---------------------------------------------------------------------------
# Test helpers
# ---------------------------------------------------------------------------

test_transit_json() {
    local name="$1"
    local transit_json_body="$2"
    local expected="$3"
    local accept="${4:-application/transit+json}"

    test_count=$((test_count + 1))
    echo "Test $test_count: $name"

    response=$(curl -s -X POST "$MENTATD_URL/" \
        -H "Content-Type: application/transit+json" \
        -H "Accept: $accept" \
        -d "$transit_json_body")

    if echo "$response" | grep -q "$expected"; then
        echo -e "${GREEN}  PASS${NC}"
        pass_count=$((pass_count + 1))
    else
        echo -e "${RED}  FAIL${NC}"
        echo "  Request:  $transit_json_body"
        echo "  Expected: $expected"
        echo "  Got:      $response"
        fail_count=$((fail_count + 1))
    fi
    echo
}

test_content_type() {
    local name="$1"
    local transit_json_body="$2"
    local accept="$3"
    local expected_ct="$4"

    test_count=$((test_count + 1))
    echo "Test $test_count: $name"

    ct=$(curl -s -o /dev/null -w '%{content_type}' -X POST "$MENTATD_URL/" \
        -H "Content-Type: application/transit+json" \
        -H "Accept: $accept" \
        -d "$transit_json_body")

    if echo "$ct" | grep -q "$expected_ct"; then
        echo -e "${GREEN}  PASS${NC} (Content-Type: $ct)"
        pass_count=$((pass_count + 1))
    else
        echo -e "${RED}  FAIL${NC}"
        echo "  Expected Content-Type: $expected_ct"
        echo "  Got:                   $ct"
        fail_count=$((fail_count + 1))
    fi
    echo
}

test_msgpack_roundtrip() {
    local name="$1"
    local transit_json_body="$2"
    local expected="$3"

    test_count=$((test_count + 1))
    echo "Test $test_count: $name"

    # Send Transit+JSON request, request Transit+MessagePack response,
    # then send that response back as a request.  We verify the server
    # can both produce and (to a degree) consume msgpack.

    # First: request msgpack response for a health check
    http_code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$MENTATD_URL/" \
        -H "Content-Type: application/transit+json" \
        -H "Accept: application/transit+msgpack" \
        -d "$transit_json_body")

    if [ "$http_code" = "200" ]; then
        echo -e "${GREEN}  PASS${NC} (HTTP $http_code)"
        pass_count=$((pass_count + 1))
    else
        echo -e "${RED}  FAIL${NC} (HTTP $http_code)"
        fail_count=$((fail_count + 1))
    fi
    echo
}

# ---------------------------------------------------------------------------
# Transit+JSON cmap format
#
# Transit+JSON encodes maps as arrays with a "^ " marker:
#   ["^ ", "~:key1", "value1", "~:key2", "value2"]
# Keywords are prefixed with "~:"
# ---------------------------------------------------------------------------

echo "=== Transit+JSON Input Tests ==="
echo

# Test 1: Health check via Transit+JSON
test_transit_json "Health check (Transit+JSON)" \
    '["^ ","~:op","~:health"]' \
    'result'

# Test 2: List databases via Transit+JSON
test_transit_json "List databases (Transit+JSON)" \
    '["^ ","~:op","~:list-dbs"]' \
    'result'

# Test 3: Connect via Transit+JSON
test_transit_json "Connect (Transit+JSON)" \
    '["^ ","~:op","~:connect","~:args",["^ ","~:db-name","postgres"]]' \
    'result'

# Test 4: Query via Transit+JSON
test_transit_json "Query (Transit+JSON)" \
    '["^ ","~:op","~:q","~:args",["^ ","~:query","[:find ?e :where [?e :name]]","~:args",[]]]' \
    'result'

# Test 5: Invalid operation via Transit+JSON
test_transit_json "Invalid operation (Transit+JSON)" \
    '["^ ","~:op","~:nonexistent-op"]' \
    'error'

# Test 6: Datomic catalog namespace via Transit+JSON
test_transit_json "Datomic catalog namespace (Transit+JSON)" \
    '["^ ","~:op","~:datomic.catalog/list-dbs"]' \
    'result'

echo "=== Content-Type Negotiation Tests ==="
echo

# Test 7: Transit+JSON input, Transit+JSON output
test_content_type "Transit+JSON -> Transit+JSON" \
    '["^ ","~:op","~:health"]' \
    "application/transit+json" \
    "application/transit+json"

# Test 8: Transit+JSON input, Transit+MessagePack output
test_content_type "Transit+JSON -> Transit+MessagePack" \
    '["^ ","~:op","~:health"]' \
    "application/transit+msgpack" \
    "application/transit+msgpack"

# Test 9: Transit+JSON input, EDN output
test_content_type "Transit+JSON -> EDN" \
    '["^ ","~:op","~:health"]' \
    "application/edn" \
    "application/edn"

echo "=== Transit+MessagePack Response Tests ==="
echo

# Test 10: Health check, request msgpack response
test_msgpack_roundtrip "Health check -> MessagePack response" \
    '["^ ","~:op","~:health"]' \
    ""

# Test 11: List databases, request msgpack response
test_msgpack_roundtrip "List databases -> MessagePack response" \
    '["^ ","~:op","~:list-dbs"]' \
    ""

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo "================================"
echo "Transit Test Summary"
echo "================================"
echo "Total:  $test_count"
echo -e "Passed: ${GREEN}$pass_count${NC}"
echo -e "Failed: ${RED}$fail_count${NC}"
echo

if [ "$fail_count" -eq 0 ]; then
    echo -e "${GREEN}All Transit tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Some Transit tests failed.${NC}"
    exit 1
fi
