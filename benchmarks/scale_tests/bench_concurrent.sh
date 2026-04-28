#!/usr/bin/env bash
# =============================================================================
# Concurrent Load Benchmark for pg_mentat
# =============================================================================
#
# Uses psql sessions in parallel to simulate concurrent access patterns.
# Measures throughput and latency under contention.
#
# Usage:
#   ./bench_concurrent.sh [OPTIONS]
#
# Options:
#   -c, --connections N    Number of parallel connections (default: 10)
#   -d, --duration SECS    Duration per test in seconds (default: 30)
#   -h, --host HOST        PostgreSQL host (default: localhost)
#   -p, --port PORT        PostgreSQL port (default: 5432)
#   -D, --dbname DB        Database name (default: pg_mentat_bench)
#   -o, --output DIR       Output directory (default: results/)
#
# Prerequisites:
#   - PostgreSQL running with pg_mentat extension
#   - generate_data.sql already executed
#   - pgbench available (ships with PostgreSQL)
#
# =============================================================================

set -euo pipefail

# Defaults
CONNECTIONS=10
DURATION=30
PGHOST="${PGHOST:-localhost}"
PGPORT="${PGPORT:-5432}"
PGDATABASE="${PGDATABASE:-pg_mentat_bench}"
OUTPUT_DIR="results"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -c|--connections) CONNECTIONS="$2"; shift 2 ;;
        -d|--duration)    DURATION="$2"; shift 2 ;;
        -h|--host)        PGHOST="$2"; shift 2 ;;
        -p|--port)        PGPORT="$2"; shift 2 ;;
        -D|--dbname)      PGDATABASE="$2"; shift 2 ;;
        -o|--output)      OUTPUT_DIR="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

RESULT_DIR="${OUTPUT_DIR}/${TIMESTAMP}_concurrent"
mkdir -p "${RESULT_DIR}"

export PGHOST PGPORT PGDATABASE

echo "============================================================="
echo "  pg_mentat Concurrent Load Benchmark"
echo "============================================================="
echo "  Connections: ${CONNECTIONS}"
echo "  Duration:    ${DURATION}s per test"
echo "  Database:    ${PGDATABASE} @ ${PGHOST}:${PGPORT}"
echo "  Output:      ${RESULT_DIR}"
echo "============================================================="
echo ""

# Create temporary pgbench scripts
SCRIPT_DIR=$(mktemp -d)
trap "rm -rf ${SCRIPT_DIR}" EXIT

# ---- Test 1: Concurrent reads (Datalog queries) ----

cat > "${SCRIPT_DIR}/read_datalog.sql" << 'PGBENCH_SQL'
-- Concurrent Datalog read: query random age range
\set age_min random(18, 40)
\set age_max :age_min + 10
SELECT mentat_query(
    format('[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] [(> ?age %s)] [(< ?age %s)]]', :age_min, :age_max),
    '{"limit": 20}'
);
PGBENCH_SQL

echo "--- Test 1: Concurrent Datalog reads ---"
pgbench \
    -c "${CONNECTIONS}" \
    -j "${CONNECTIONS}" \
    -T "${DURATION}" \
    -f "${SCRIPT_DIR}/read_datalog.sql" \
    -P 5 \
    --no-vacuum \
    2>&1 | tee "${RESULT_DIR}/read_datalog.txt"
echo ""

# ---- Test 2: Concurrent reads (SQL views) ----

cat > "${SCRIPT_DIR}/read_sql.sql" << 'PGBENCH_SQL'
-- Concurrent SQL view read: query via virtual tables
\set age_min random(18, 40)
SELECT COUNT(*) FROM mentat.numeric_values
WHERE attribute = ':person/age' AND value > :age_min;
PGBENCH_SQL

echo "--- Test 2: Concurrent SQL view reads ---"
pgbench \
    -c "${CONNECTIONS}" \
    -j "${CONNECTIONS}" \
    -T "${DURATION}" \
    -f "${SCRIPT_DIR}/read_sql.sql" \
    -P 5 \
    --no-vacuum \
    2>&1 | tee "${RESULT_DIR}/read_sql.txt"
echo ""

# ---- Test 3: Concurrent writes ----

cat > "${SCRIPT_DIR}/write.sql" << 'PGBENCH_SQL'
-- Concurrent write: insert a new entity
\set id random(1, 999999999)
SELECT mentat_transact(format(
    '[{:db/id "cw_%s" :person/name "ConcWrite %s" :person/email "cw_%s@test.com" :person/age %s}]',
    :id, :id, :id, 18 + (:id % 48)
));
PGBENCH_SQL

echo "--- Test 3: Concurrent writes ---"
pgbench \
    -c "${CONNECTIONS}" \
    -j "${CONNECTIONS}" \
    -T "${DURATION}" \
    -f "${SCRIPT_DIR}/write.sql" \
    -P 5 \
    --no-vacuum \
    2>&1 | tee "${RESULT_DIR}/write.txt"
echo ""

# ---- Test 4: Mixed read-write (70/30) ----

cat > "${SCRIPT_DIR}/mixed_read.sql" << 'PGBENCH_SQL'
-- Mixed workload: read path (weighted 70%)
\set age_min random(18, 50)
SELECT mentat_query(
    format('[:find (count ?e) :where [?e :person/age ?a] [(> ?a %s)]]', :age_min),
    '{}'
);
PGBENCH_SQL

cat > "${SCRIPT_DIR}/mixed_write.sql" << 'PGBENCH_SQL'
-- Mixed workload: write path (weighted 30%)
\set id random(1, 999999999)
SELECT mentat_transact(format(
    '[{:db/id "mx_%s" :person/name "MixWrite %s" :person/email "mx_%s@test.com" :person/age %s}]',
    :id, :id, :id, 20 + (:id % 40)
));
PGBENCH_SQL

echo "--- Test 4: Mixed read-write (70/30) ---"
pgbench \
    -c "${CONNECTIONS}" \
    -j "${CONNECTIONS}" \
    -T "${DURATION}" \
    -f "${SCRIPT_DIR}/mixed_read.sql"@7 \
    -f "${SCRIPT_DIR}/mixed_write.sql"@3 \
    -P 5 \
    --no-vacuum \
    2>&1 | tee "${RESULT_DIR}/mixed.txt"
echo ""

# ---- Test 5: Scaling test (vary connections) ----

echo "--- Test 5: Connection scaling ---"
for conns in 1 5 10 25 50; do
    if [ "${conns}" -gt "${CONNECTIONS}" ]; then
        echo "  Skipping ${conns} connections (exceeds --connections ${CONNECTIONS})"
        continue
    fi
    echo "  Connections: ${conns}"
    pgbench \
        -c "${conns}" \
        -j "${conns}" \
        -T 10 \
        -f "${SCRIPT_DIR}/read_datalog.sql" \
        --no-vacuum \
        2>&1 | grep -E "^(number|latency|tps)" | sed 's/^/    /'
done | tee "${RESULT_DIR}/scaling.txt"
echo ""

# ---- Summary ----

echo "============================================================="
echo "  Concurrent benchmark complete."
echo "  Results saved to: ${RESULT_DIR}/"
echo ""
echo "  Files:"
ls -la "${RESULT_DIR}/" | grep -v "^total" | sed 's/^/    /'
echo "============================================================="
