#!/usr/bin/env bash
# =============================================================================
# pg_mentat Correctness Test Runner
# =============================================================================
#
# Runs all correctness test SQL files against a PostgreSQL instance with
# pg_mentat installed. Reports pass/fail counts and captures output.
#
# Usage:
#   ./run_correctness_tests.sh [OPTIONS]
#
# Options:
#   -h, --host HOST    PostgreSQL host (default: localhost)
#   -p, --port PORT    PostgreSQL port (default: 5432)
#   -D, --dbname DB    Database name (default: pg_mentat_test)
#   -o, --output DIR   Output directory for logs (default: results/)
#   -v, --verbose      Show full test output
#   --keep-db          Don't drop/recreate the test database
#   --help             Show help
#
# Exit codes:
#   0 - All tests passed
#   1 - Some tests failed
#   2 - Setup error (database, extension, etc.)
#
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Defaults
PGHOST="${PGHOST:-localhost}"
PGPORT="${PGPORT:-5432}"
PGDATABASE="${PGDATABASE:-pg_mentat_test}"
OUTPUT_DIR="${SCRIPT_DIR}/results"
VERBOSE=false
KEEP_DB=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--host)    PGHOST="$2"; shift 2 ;;
        -p|--port)    PGPORT="$2"; shift 2 ;;
        -D|--dbname)  PGDATABASE="$2"; shift 2 ;;
        -o|--output)  OUTPUT_DIR="$2"; shift 2 ;;
        -v|--verbose) VERBOSE=true; shift ;;
        --keep-db)    KEEP_DB=true; shift ;;
        --help)
            head -25 "$0" | tail -20
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULT_DIR="${OUTPUT_DIR}/${TIMESTAMP}"
mkdir -p "${RESULT_DIR}"

export PGHOST PGPORT PGDATABASE

# Colors for terminal output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

TOTAL=0
PASSED=0
FAILED=0
ERRORS=0

echo "============================================================="
echo "  pg_mentat Correctness Test Suite"
echo "============================================================="
echo "  Database:  ${PGDATABASE} @ ${PGHOST}:${PGPORT}"
echo "  Output:    ${RESULT_DIR}"
echo "  Verbose:   ${VERBOSE}"
echo "============================================================="
echo ""

# ---- Database setup ----

if [ "${KEEP_DB}" = false ]; then
    echo "Setting up test database..."

    # Drop and recreate
    dropdb --if-exists "${PGDATABASE}" 2>/dev/null || true
    createdb "${PGDATABASE}" 2>/dev/null || {
        echo -e "${RED}ERROR: Cannot create database ${PGDATABASE}${NC}"
        exit 2
    }

    # Install extension
    psql -c "CREATE EXTENSION IF NOT EXISTS pg_mentat CASCADE;" 2>/dev/null || {
        echo -e "${RED}ERROR: Cannot install pg_mentat extension${NC}"
        exit 2
    }
    echo "  Database ready."
    echo ""
fi

# ---- Run each test file ----

run_test() {
    local test_file="$1"
    local test_name
    test_name=$(basename "${test_file}" .sql)
    local log_file="${RESULT_DIR}/${test_name}.log"

    TOTAL=$((TOTAL + 1))
    printf "  %-40s " "${test_name}..."

    # Run the test file, capture stdout+stderr
    if psql -v ON_ERROR_STOP=1 -f "${test_file}" > "${log_file}" 2>&1; then
        # Check for PASS notices
        local pass_count
        pass_count=$(grep -c "PASS:" "${log_file}" 2>/dev/null || echo "0")

        # Check for FAIL or assertion errors
        local fail_count
        fail_count=$(grep -ci "FAIL\|assertion\|ERROR" "${log_file}" 2>/dev/null || echo "0")

        if [ "${fail_count}" -gt 0 ] && grep -qi "ERROR\|failed assertion" "${log_file}"; then
            echo -e "${RED}FAIL${NC} (${pass_count} passed, errors found)"
            FAILED=$((FAILED + 1))
        else
            echo -e "${GREEN}PASS${NC} (${pass_count} checks)"
            PASSED=$((PASSED + 1))
        fi
    else
        echo -e "${RED}ERROR${NC} (psql exit code $?)"
        ERRORS=$((ERRORS + 1))
    fi

    if [ "${VERBOSE}" = true ]; then
        echo "    --- Output ---"
        grep -E "PASS:|FAIL:|NOTICE:|ERROR:" "${log_file}" | sed 's/^/    /'
        echo "    --- End ---"
    fi
}

echo "Running correctness tests:"
echo ""

# Find and run all .sql test files
for test_file in "${SCRIPT_DIR}"/*.sql; do
    [ -f "${test_file}" ] || continue
    run_test "${test_file}"
done

# ---- Summary ----

echo ""
echo "============================================================="
echo "  Results Summary"
echo "============================================================="
echo -e "  Total:   ${TOTAL}"
echo -e "  Passed:  ${GREEN}${PASSED}${NC}"
echo -e "  Failed:  ${RED}${FAILED}${NC}"
echo -e "  Errors:  ${RED}${ERRORS}${NC}"
echo "  Logs:    ${RESULT_DIR}/"
echo "============================================================="

# Save summary
cat > "${RESULT_DIR}/summary.txt" << EOF
pg_mentat Correctness Test Results
Date: $(date -Iseconds)
Database: ${PGDATABASE} @ ${PGHOST}:${PGPORT}

Total:  ${TOTAL}
Passed: ${PASSED}
Failed: ${FAILED}
Errors: ${ERRORS}
EOF

# Exit with appropriate code
if [ "${FAILED}" -gt 0 ] || [ "${ERRORS}" -gt 0 ]; then
    exit 1
fi
exit 0
