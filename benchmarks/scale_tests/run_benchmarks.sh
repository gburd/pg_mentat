#!/usr/bin/env bash
# =============================================================================
# pg_mentat Scale Test Runner
# =============================================================================
#
# Orchestrates the full benchmark suite:
#   1. Creates a fresh test database
#   2. Generates test data at the specified scale
#   3. Runs query benchmarks
#   4. Runs transaction benchmarks
#   5. Optionally runs concurrent load tests
#   6. Collects results
#
# Usage:
#   ./run_benchmarks.sh [OPTIONS]
#
# Options:
#   -s, --scale N        Number of entities to generate (default: 10000)
#   -c, --concurrent     Also run concurrent benchmarks
#   --connections N       Parallel connections for concurrent tests (default: 10)
#   --skip-generate      Skip data generation (reuse existing data)
#   -h, --host HOST      PostgreSQL host (default: localhost)
#   -p, --port PORT      PostgreSQL port (default: 5432)
#   -D, --dbname DB      Database name (default: pg_mentat_bench)
#   -o, --output DIR     Output directory (default: results/)
#   --help               Show this help message
#
# Examples:
#   # Quick smoke test (1K entities)
#   ./run_benchmarks.sh -s 1000
#
#   # Full benchmark with 100K entities and concurrent tests
#   ./run_benchmarks.sh -s 100000 -c --connections 20
#
#   # Stress test with 1M entities
#   ./run_benchmarks.sh -s 1000000 -c --connections 50
#
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Defaults
SCALE=10000
RUN_CONCURRENT=false
CONNECTIONS=10
SKIP_GENERATE=false
PGHOST="${PGHOST:-localhost}"
PGPORT="${PGPORT:-5432}"
PGDATABASE="${PGDATABASE:-pg_mentat_bench}"
OUTPUT_DIR="${SCRIPT_DIR}/results"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -s|--scale)         SCALE="$2"; shift 2 ;;
        -c|--concurrent)    RUN_CONCURRENT=true; shift ;;
        --connections)      CONNECTIONS="$2"; shift 2 ;;
        --skip-generate)    SKIP_GENERATE=true; shift ;;
        -h|--host)          PGHOST="$2"; shift 2 ;;
        -p|--port)          PGPORT="$2"; shift 2 ;;
        -D|--dbname)        PGDATABASE="$2"; shift 2 ;;
        -o|--output)        OUTPUT_DIR="$2"; shift 2 ;;
        --help)
            head -40 "$0" | tail -35
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

RESULT_DIR="${OUTPUT_DIR}/${TIMESTAMP}_scale${SCALE}"
mkdir -p "${RESULT_DIR}"

export PGHOST PGPORT PGDATABASE

echo "============================================================="
echo "  pg_mentat Scale Test Suite"
echo "============================================================="
echo "  Scale:       ${SCALE} entities (~$((SCALE * 7)) datoms)"
echo "  Database:    ${PGDATABASE} @ ${PGHOST}:${PGPORT}"
echo "  Concurrent:  ${RUN_CONCURRENT}"
echo "  Output:      ${RESULT_DIR}"
echo "============================================================="
echo ""

# Record environment
cat > "${RESULT_DIR}/environment.txt" << EOF
Date: $(date -Iseconds)
Scale: ${SCALE}
Database: ${PGDATABASE} @ ${PGHOST}:${PGPORT}
PostgreSQL Version: $(psql -t -c "SELECT version();" 2>/dev/null || echo "unknown")
pg_mentat Version: $(psql -t -c "SELECT extversion FROM pg_extension WHERE extname = 'pg_mentat';" 2>/dev/null || echo "unknown")
OS: $(uname -sr)
CPU: $(nproc 2>/dev/null || echo "unknown") cores
Memory: $(free -h 2>/dev/null | awk '/^Mem:/{print $2}' || echo "unknown")
EOF

# ---- Step 1: Setup database ----

if [ "${SKIP_GENERATE}" = false ]; then
    echo "=== Step 1: Setting up database ==="

    # Create database if it doesn't exist
    if ! psql -lqt | cut -d \| -f 1 | grep -qw "${PGDATABASE}"; then
        echo "  Creating database ${PGDATABASE}..."
        createdb "${PGDATABASE}"
    fi

    # Ensure extension is installed
    psql -c "CREATE EXTENSION IF NOT EXISTS pg_mentat CASCADE;" 2>/dev/null || true

    echo "  Database ready."
    echo ""

    # ---- Step 2: Generate data ----

    echo "=== Step 2: Generating test data (${SCALE} entities) ==="
    psql -v scale="${SCALE}" -f "${SCRIPT_DIR}/generate_data.sql" \
        2>&1 | tee "${RESULT_DIR}/generate_data.log"
    echo ""
else
    echo "=== Skipping data generation (--skip-generate) ==="
    echo ""
fi

# ---- Step 3: Query benchmarks ----

echo "=== Step 3: Running query benchmarks ==="
psql -f "${SCRIPT_DIR}/bench_queries.sql" \
    2>&1 | tee "${RESULT_DIR}/bench_queries.log"
echo ""

# ---- Step 4: Transaction benchmarks ----

echo "=== Step 4: Running transaction benchmarks ==="
psql -f "${SCRIPT_DIR}/bench_transactions.sql" \
    2>&1 | tee "${RESULT_DIR}/bench_transactions.log"
echo ""

# ---- Step 5: Concurrent benchmarks (optional) ----

if [ "${RUN_CONCURRENT}" = true ]; then
    echo "=== Step 5: Running concurrent benchmarks ==="
    "${SCRIPT_DIR}/bench_concurrent.sh" \
        -c "${CONNECTIONS}" \
        -D "${PGDATABASE}" \
        -h "${PGHOST}" \
        -p "${PGPORT}" \
        -o "${RESULT_DIR}" \
        2>&1 | tee "${RESULT_DIR}/bench_concurrent.log"
    echo ""
fi

# ---- Step 6: Collect dataset statistics ----

echo "=== Final: Dataset statistics ==="
psql -c "
    SELECT 'datoms_ref_new'     AS table_name, COUNT(*) AS rows, pg_size_pretty(pg_relation_size('mentat.datoms_ref_new'))     AS size FROM mentat.datoms_ref_new     WHERE added = true
    UNION ALL
    SELECT 'datoms_boolean_new', COUNT(*), pg_size_pretty(pg_relation_size('mentat.datoms_boolean_new')) FROM mentat.datoms_boolean_new WHERE added = true
    UNION ALL
    SELECT 'datoms_long_new',    COUNT(*), pg_size_pretty(pg_relation_size('mentat.datoms_long_new'))    FROM mentat.datoms_long_new    WHERE added = true
    UNION ALL
    SELECT 'datoms_double_new',  COUNT(*), pg_size_pretty(pg_relation_size('mentat.datoms_double_new'))  FROM mentat.datoms_double_new  WHERE added = true
    UNION ALL
    SELECT 'datoms_instant_new', COUNT(*), pg_size_pretty(pg_relation_size('mentat.datoms_instant_new')) FROM mentat.datoms_instant_new WHERE added = true
    UNION ALL
    SELECT 'datoms_text_new',    COUNT(*), pg_size_pretty(pg_relation_size('mentat.datoms_text_new'))    FROM mentat.datoms_text_new    WHERE added = true
    UNION ALL
    SELECT 'datoms_keyword_new', COUNT(*), pg_size_pretty(pg_relation_size('mentat.datoms_keyword_new')) FROM mentat.datoms_keyword_new WHERE added = true
    UNION ALL
    SELECT 'datoms_uuid_new',    COUNT(*), pg_size_pretty(pg_relation_size('mentat.datoms_uuid_new'))    FROM mentat.datoms_uuid_new    WHERE added = true
    UNION ALL
    SELECT 'datoms_bytes_new',   COUNT(*), pg_size_pretty(pg_relation_size('mentat.datoms_bytes_new'))   FROM mentat.datoms_bytes_new   WHERE added = true
    ORDER BY 1;
" 2>&1 | tee "${RESULT_DIR}/dataset_stats.txt"

# ---- Summary ----

echo ""
echo "============================================================="
echo "  Benchmark suite complete."
echo ""
echo "  Results directory: ${RESULT_DIR}/"
echo "  Files:"
ls -la "${RESULT_DIR}/" | grep -v "^total\|^d" | sed 's/^/    /'
echo ""
echo "  To compare results across scales, run:"
echo "    diff <(grep 'ms avg' results/YYYYMMDD_scale1000/bench_queries.log) \\"
echo "         <(grep 'ms avg' results/YYYYMMDD_scale100000/bench_queries.log)"
echo "============================================================="
