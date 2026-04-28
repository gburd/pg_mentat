#!/usr/bin/env bash
# =============================================================================
# pg_mentat Comprehensive Test Runner
# =============================================================================
#
# Runs all test suites in order:
#   1. SQL integration tests (pg_mentat/test/sql/*.sql)
#   2. Correctness tests (tests/correctness/*.sql)
#   3. Performance benchmarks (benchmarks/scale_tests/) [optional]
#
# Designed for CI/CD integration. Returns exit code 0 only if all
# mandatory tests pass.
#
# Usage:
#   ./run_all_tests.sh [OPTIONS]
#
# Options:
#   -h, --host HOST      PostgreSQL host (default: localhost)
#   -p, --port PORT      PostgreSQL port (default: 5432)
#   -D, --dbname DB      Database name (default: pg_mentat_test)
#   --with-benchmarks    Also run performance benchmarks
#   --benchmark-scale N  Scale for benchmarks (default: 1000)
#   --help               Show help
#
# Exit codes:
#   0 - All tests passed
#   1 - Test failures
#   2 - Setup error
#
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Defaults
PGHOST="${PGHOST:-localhost}"
PGPORT="${PGPORT:-5432}"
PGDATABASE="${PGDATABASE:-pg_mentat_test}"
WITH_BENCHMARKS=false
BENCHMARK_SCALE=1000

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--host)          PGHOST="$2"; shift 2 ;;
        -p|--port)          PGPORT="$2"; shift 2 ;;
        -D|--dbname)        PGDATABASE="$2"; shift 2 ;;
        --with-benchmarks)  WITH_BENCHMARKS=true; shift ;;
        --benchmark-scale)  BENCHMARK_SCALE="$2"; shift 2 ;;
        --help)
            head -30 "$0" | tail -25
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

export PGHOST PGPORT PGDATABASE

TOTAL_SUITES=0
PASSED_SUITES=0
FAILED_SUITES=0

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

echo "============================================================="
echo -e "${BOLD}  pg_mentat Comprehensive Test Suite${NC}"
echo "============================================================="
echo "  Database:    ${PGDATABASE} @ ${PGHOST}:${PGPORT}"
echo "  Benchmarks:  ${WITH_BENCHMARKS}"
echo "============================================================="
echo ""

# ---- Setup ----

echo "=== Setting up test database ==="
dropdb --if-exists "${PGDATABASE}" 2>/dev/null || true
createdb "${PGDATABASE}" 2>/dev/null || {
    echo -e "${RED}ERROR: Cannot create database ${PGDATABASE}${NC}"
    exit 2
}
psql -c "CREATE EXTENSION IF NOT EXISTS pg_mentat CASCADE;" 2>/dev/null || {
    echo -e "${RED}ERROR: Cannot install pg_mentat extension${NC}"
    exit 2
}
echo "  Database ready."
echo ""

# ---- Suite 1: SQL integration tests ----

echo "============================================================="
echo "  Suite 1: SQL Integration Tests"
echo "============================================================="

SQL_TEST_DIR="${PROJECT_ROOT}/pg_mentat/test/sql"
if [ -d "${SQL_TEST_DIR}" ]; then
    sql_total=0
    sql_passed=0
    sql_failed=0

    for test_file in "${SQL_TEST_DIR}"/*.sql; do
        [ -f "${test_file}" ] || continue
        test_name=$(basename "${test_file}" .sql)
        sql_total=$((sql_total + 1))
        printf "  %-40s " "${test_name}..."

        if psql -v ON_ERROR_STOP=1 -f "${test_file}" > /dev/null 2>&1; then
            echo -e "${GREEN}PASS${NC}"
            sql_passed=$((sql_passed + 1))
        else
            echo -e "${RED}FAIL${NC}"
            sql_failed=$((sql_failed + 1))
        fi
    done

    TOTAL_SUITES=$((TOTAL_SUITES + 1))
    echo ""
    echo "  SQL Integration: ${sql_passed}/${sql_total} passed"
    if [ "${sql_failed}" -eq 0 ]; then
        echo -e "  ${GREEN}Suite PASSED${NC}"
        PASSED_SUITES=$((PASSED_SUITES + 1))
    else
        echo -e "  ${RED}Suite FAILED (${sql_failed} failures)${NC}"
        FAILED_SUITES=$((FAILED_SUITES + 1))
    fi
else
    echo "  (no SQL test directory found, skipping)"
fi
echo ""

# ---- Suite 2: Correctness tests ----

echo "============================================================="
echo "  Suite 2: Correctness Tests"
echo "============================================================="

CORRECTNESS_DIR="${SCRIPT_DIR}/correctness"
if [ -d "${CORRECTNESS_DIR}" ] && ls "${CORRECTNESS_DIR}"/*.sql 1>/dev/null 2>&1; then
    # Recreate database for clean state
    dropdb --if-exists "${PGDATABASE}" 2>/dev/null || true
    createdb "${PGDATABASE}" 2>/dev/null
    psql -c "CREATE EXTENSION IF NOT EXISTS pg_mentat CASCADE;" 2>/dev/null

    correctness_total=0
    correctness_passed=0
    correctness_failed=0

    for test_file in "${CORRECTNESS_DIR}"/*.sql; do
        [ -f "${test_file}" ] || continue
        test_name=$(basename "${test_file}" .sql)
        correctness_total=$((correctness_total + 1))
        printf "  %-40s " "${test_name}..."

        if psql -v ON_ERROR_STOP=1 -f "${test_file}" > /dev/null 2>&1; then
            echo -e "${GREEN}PASS${NC}"
            correctness_passed=$((correctness_passed + 1))
        else
            echo -e "${RED}FAIL${NC}"
            correctness_failed=$((correctness_failed + 1))
        fi
    done

    TOTAL_SUITES=$((TOTAL_SUITES + 1))
    echo ""
    echo "  Correctness: ${correctness_passed}/${correctness_total} passed"
    if [ "${correctness_failed}" -eq 0 ]; then
        echo -e "  ${GREEN}Suite PASSED${NC}"
        PASSED_SUITES=$((PASSED_SUITES + 1))
    else
        echo -e "  ${RED}Suite FAILED (${correctness_failed} failures)${NC}"
        FAILED_SUITES=$((FAILED_SUITES + 1))
    fi
else
    echo "  (no correctness test files found, skipping)"
fi
echo ""

# ---- Suite 3: Performance benchmarks (optional) ----

if [ "${WITH_BENCHMARKS}" = true ]; then
    echo "============================================================="
    echo "  Suite 3: Performance Benchmarks (scale=${BENCHMARK_SCALE})"
    echo "============================================================="

    BENCH_RUNNER="${PROJECT_ROOT}/benchmarks/scale_tests/run_benchmarks.sh"
    if [ -x "${BENCH_RUNNER}" ]; then
        TOTAL_SUITES=$((TOTAL_SUITES + 1))

        if "${BENCH_RUNNER}" \
            -s "${BENCHMARK_SCALE}" \
            -D "${PGDATABASE}" \
            -h "${PGHOST}" \
            -p "${PGPORT}" 2>&1; then
            echo -e "  ${GREEN}Benchmarks completed${NC}"
            PASSED_SUITES=$((PASSED_SUITES + 1))
        else
            echo -e "  ${RED}Benchmarks failed${NC}"
            FAILED_SUITES=$((FAILED_SUITES + 1))
        fi
    else
        echo "  (benchmark runner not found at ${BENCH_RUNNER}, skipping)"
    fi
    echo ""
fi

# ---- Final Summary ----

echo "============================================================="
echo -e "${BOLD}  Final Summary${NC}"
echo "============================================================="
echo -e "  Test Suites:  ${TOTAL_SUITES}"
echo -e "  Passed:       ${GREEN}${PASSED_SUITES}${NC}"
echo -e "  Failed:       ${RED}${FAILED_SUITES}${NC}"
echo "============================================================="

if [ "${FAILED_SUITES}" -gt 0 ]; then
    echo -e "${RED}OVERALL: FAILED${NC}"
    exit 1
fi

echo -e "${GREEN}OVERALL: PASSED${NC}"
exit 0
