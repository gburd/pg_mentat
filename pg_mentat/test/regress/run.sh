#!/usr/bin/env bash
# pg_mentat regression test runner
#
# Usage:
#   run.sh [--generate]
#
# --generate: Accept current output as expected baselines.
# Without --generate: Diff against expected/, fail on mismatch.
#
# Environment variables:
#   PGHOST     (default: /tmp)
#   PGPORT     (default: 28816, the pgrx-managed instance)
#   PGDATABASE (default: postgres)
#   PGUSER     (default: current user)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SQL_DIR="$SCRIPT_DIR/sql"
EXPECTED_DIR="$SCRIPT_DIR/expected"
RESULTS_DIR="$SCRIPT_DIR/results"

GENERATE=false
if [[ "${1:-}" == "--generate" ]]; then
    GENERATE=true
fi

# Defaults tuned to pgrx-managed local instance
export PGHOST="${PGHOST:-/tmp}"
export PGPORT="${PGPORT:-28816}"
export PGDATABASE="${PGDATABASE:-postgres}"
export PGUSER="${PGUSER:-$(whoami)}"

REGRESS_DB="pg_mentat_regress"

cleanup() {
    psql -X -q -c "DROP DATABASE IF EXISTS ${REGRESS_DB};" 2>/dev/null || true
}

# Normalize output: strip NOTICE lines, collapse multiple blank lines,
# trim trailing whitespace.
normalize() {
    sed -E \
        -e '/^(NOTICE|psql|SET|CREATE|DROP|DO):/d' \
        -e 's/[[:space:]]+$//' \
        -e 's/"_t":"inst","v":[0-9]+/"_t":"inst","v":TIMESTAMP/g' \
    | cat -s
}

echo "=== pg_mentat regression tests ==="
echo "Host: ${PGHOST}:${PGPORT} Database: ${PGDATABASE} (regress db: ${REGRESS_DB})"

# Create a fresh database for isolation
cleanup
psql -X -q -c "CREATE DATABASE ${REGRESS_DB};"

trap cleanup EXIT

mkdir -p "$RESULTS_DIR"

PASS=0
FAIL=0
ERRORS=""

# Run each SQL file in sorted order
for sql_file in "$SQL_DIR"/*.sql; do
    test_name="$(basename "$sql_file" .sql)"
    result_file="$RESULTS_DIR/${test_name}.out"
    expected_file="$EXPECTED_DIR/${test_name}.out"

    printf "  %-30s" "$test_name"

    # Run the test. psql settings ensure deterministic output.
    # Prepend search_path to each file since sessions are independent.
    if ! { echo "SET search_path TO mentat, public;"; cat "$sql_file"; } | \
        psql -X -q -d "$REGRESS_DB" \
        --set=ON_ERROR_STOP=on \
        -P tuples_only \
        -P format=unaligned \
        2>&1 | normalize > "$result_file"; then
        # Test script errored out
        echo "ERROR"
        ERRORS="${ERRORS} ${test_name}"
        FAIL=$((FAIL + 1))
        continue
    fi

    if $GENERATE; then
        cp "$result_file" "$expected_file"
        echo "GENERATED"
        PASS=$((PASS + 1))
    else
        if [[ ! -f "$expected_file" ]]; then
            echo "MISSING EXPECTED (run with --generate first)"
            FAIL=$((FAIL + 1))
            ERRORS="${ERRORS} ${test_name}"
        elif diff -u "$expected_file" "$result_file" > /dev/null 2>&1; then
            echo "ok"
            PASS=$((PASS + 1))
        else
            echo "FAILED"
            echo "--- diff for ${test_name} ---"
            diff -u "$expected_file" "$result_file" || true
            echo "---"
            FAIL=$((FAIL + 1))
            ERRORS="${ERRORS} ${test_name}"
        fi
    fi
done

echo ""
echo "Results: ${PASS} passed, ${FAIL} failed"
if [[ -n "$ERRORS" ]]; then
    echo "Failed tests:${ERRORS}"
    exit 1
fi
exit 0
