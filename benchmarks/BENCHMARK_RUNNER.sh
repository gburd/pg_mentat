#!/bin/bash
# Master Benchmark Runner Script
# Executes all Phase 1 performance validation benchmarks

set -e

# Configuration
DB_HOST="${DB_HOST:-localhost}"
DB_PORT="${DB_PORT:-5432}"
DB_NAME="${DB_NAME:-postgres}"
DB_USER="${DB_USER:-$USER}"
RESULTS_DIR="./benchmarks/results"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "========================================"
echo "  pg_mentat Performance Benchmarks"
echo "  Phase 1: Performance Validation"
echo "========================================"
echo ""
echo "Database: $DB_HOST:$DB_PORT/$DB_NAME"
echo "Results: $RESULTS_DIR"
echo "Timestamp: $TIMESTAMP"
echo ""

# Create results directory
mkdir -p "$RESULTS_DIR"

# ============================================================================
# Step 1: Create 1M Datom Dataset
# ============================================================================

echo -e "${YELLOW}Step 1: Creating 1M datom dataset...${NC}"
echo "This will take approximately 2-5 minutes..."

psql -h "$DB_HOST" -p "$DB_PORT" -d "$DB_NAME" -U "$DB_USER" <<EOF
\o $RESULTS_DIR/dataset_1m_creation_${TIMESTAMP}.txt
\timing on
SELECT * FROM create_benchmark_data_1m();
\o
EOF

if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓ 1M datom dataset created${NC}"
else
    echo -e "${RED}✗ Failed to create 1M dataset${NC}"
    exit 1
fi

# ============================================================================
# Step 2: Run Query Performance Benchmarks (1M dataset)
# ============================================================================

echo ""
echo -e "${YELLOW}Step 2: Running query performance benchmarks (1M dataset)...${NC}"
echo "This will take approximately 5-10 minutes..."

psql -h "$DB_HOST" -p "$DB_PORT" -d "$DB_NAME" -U "$DB_USER" \
    -f benchmarks/query_performance/benchmark_queries.sql \
    > "$RESULTS_DIR/query_performance_1m_${TIMESTAMP}.txt" 2>&1

if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓ Query benchmarks completed (1M)${NC}"
else
    echo -e "${RED}✗ Query benchmarks failed (1M)${NC}"
    exit 1
fi

# ============================================================================
# Step 3: Run Transaction Throughput Benchmarks
# ============================================================================

echo ""
echo -e "${YELLOW}Step 3: Running transaction throughput benchmarks...${NC}"
echo "This will take approximately 3-5 minutes..."

psql -h "$DB_HOST" -p "$DB_PORT" -d "$DB_NAME" -U "$DB_USER" \
    -f benchmarks/transaction_throughput/benchmark_transactions.sql \
    > "$RESULTS_DIR/transaction_throughput_${TIMESTAMP}.txt" 2>&1

if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓ Transaction throughput benchmarks completed${NC}"
else
    echo -e "${RED}✗ Transaction throughput benchmarks failed${NC}"
    exit 1
fi

# ============================================================================
# Step 4: Run UNION ALL Analysis
# ============================================================================

echo ""
echo -e "${YELLOW}Step 4: Running UNION ALL performance analysis...${NC}"

psql -h "$DB_HOST" -p "$DB_PORT" -d "$DB_NAME" -U "$DB_USER" <<EOF > "$RESULTS_DIR/union_all_analysis_${TIMESTAMP}.txt"
\timing on

-- Test 1: UNION ALL across all 9 tables (current strategy)
\echo '=== Test 1: UNION ALL across 9 tables ==='
EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT COUNT(*) FROM (
    SELECT e, a, v::text FROM mentat.datoms_ref_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text FROM mentat.datoms_long_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text FROM mentat.datoms_text_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text FROM mentat.datoms_double_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text FROM mentat.datoms_boolean_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text FROM mentat.datoms_instant_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text FROM mentat.datoms_keyword_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text FROM mentat.datoms_uuid_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text FROM mentat.datoms_bytes_new WHERE store_id = 0 AND added = true
) u;

-- Test 2: Single table query (schema-aware optimization potential)
\echo '\n=== Test 2: Single table query (text) ==='
EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT COUNT(*) FROM mentat.datoms_text_new
WHERE store_id = 0 AND added = true;

-- Test 3: Measure overhead ratio
\echo '\n=== Overhead Comparison ==='
SELECT
    'UNION ALL overhead' AS metric,
    CASE
        WHEN single_table_ms > 0 THEN ROUND((union_all_ms / single_table_ms)::numeric, 2)
        ELSE 0
    END AS ratio
FROM (
    SELECT
        (SELECT EXTRACT(MILLISECONDS FROM query_duration)
         FROM pg_stat_statements
         WHERE query LIKE '%UNION ALL%'
         ORDER BY calls DESC LIMIT 1) AS union_all_ms,
        (SELECT EXTRACT(MILLISECONDS FROM query_duration)
         FROM pg_stat_statements
         WHERE query LIKE '%datoms_text_new%' AND query NOT LIKE '%UNION%'
         ORDER BY calls DESC LIMIT 1) AS single_table_ms
) metrics;
EOF

if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓ UNION ALL analysis completed${NC}"
else
    echo -e "${RED}✗ UNION ALL analysis failed${NC}"
fi

# ============================================================================
# Step 5: Collect System Statistics
# ============================================================================

echo ""
echo -e "${YELLOW}Step 5: Collecting system statistics...${NC}"

psql -h "$DB_HOST" -p "$DB_PORT" -d "$DB_NAME" -U "$DB_USER" <<EOF > "$RESULTS_DIR/system_stats_${TIMESTAMP}.txt"
-- Table sizes
SELECT
    tablename,
    pg_size_pretty(pg_total_relation_size('mentat.' || tablename)) AS total_size,
    pg_size_pretty(pg_relation_size('mentat.' || tablename)) AS table_size,
    pg_size_pretty(pg_total_relation_size('mentat.' || tablename) - pg_relation_size('mentat.' || tablename)) AS index_size,
    ROUND(100 * (pg_total_relation_size('mentat.' || tablename) - pg_relation_size('mentat.' || tablename))::numeric /
          NULLIF(pg_total_relation_size('mentat.' || tablename), 0), 2) AS index_ratio_pct
FROM pg_tables
WHERE schemaname = 'mentat' AND tablename LIKE 'datoms_%_new'
ORDER BY pg_total_relation_size('mentat.' || tablename) DESC;

-- Datom counts by type
SELECT 'ref' AS type, COUNT(*) AS count FROM mentat.datoms_ref_new WHERE added = true
UNION ALL
SELECT 'long', COUNT(*) FROM mentat.datoms_long_new WHERE added = true
UNION ALL
SELECT 'text', COUNT(*) FROM mentat.datoms_text_new WHERE added = true
UNION ALL
SELECT 'double', COUNT(*) FROM mentat.datoms_double_new WHERE added = true
UNION ALL
SELECT 'boolean', COUNT(*) FROM mentat.datoms_boolean_new WHERE added = true
UNION ALL
SELECT 'instant', COUNT(*) FROM mentat.datoms_instant_new WHERE added = true
UNION ALL
SELECT 'keyword', COUNT(*) FROM mentat.datoms_keyword_new WHERE added = true
UNION ALL
SELECT 'uuid', COUNT(*) FROM mentat.datoms_uuid_new WHERE added = true
UNION ALL
SELECT 'bytes', COUNT(*) FROM mentat.datoms_bytes_new WHERE added = true
ORDER BY count DESC;

-- Entity count
SELECT COUNT(DISTINCT e) AS entity_count
FROM (
    SELECT e FROM mentat.datoms_text_new WHERE added = true
    UNION ALL
    SELECT e FROM mentat.datoms_long_new WHERE added = true
    UNION ALL
    SELECT e FROM mentat.datoms_double_new WHERE added = true
) entities;
EOF

echo -e "${GREEN}✓ System statistics collected${NC}"

# ============================================================================
# Step 6: Generate Summary Report
# ============================================================================

echo ""
echo -e "${YELLOW}Step 6: Generating summary report...${NC}"

cat > "$RESULTS_DIR/SUMMARY_${TIMESTAMP}.md" <<EOF
# Performance Benchmark Results

**Date**: $(date)
**Database**: $DB_HOST:$DB_PORT/$DB_NAME
**Dataset**: 1M datoms

## Executive Summary

### Query Performance Benchmarks

See detailed results in: \`query_performance_1m_${TIMESTAMP}.txt\`

| Query Type | Expected | Actual | Status |
|------------|----------|--------|--------|
| Simple Pattern | <50ms | TBD | TBD |
| Join (2 patterns) | <100ms | TBD | TBD |
| Join with Predicate | <150ms | TBD | TBD |
| Complex Join (3+ patterns) | <200ms | TBD | TBD |
| OR-join | <250ms | TBD | TBD |
| OR-join with Predicates | <300ms | TBD | TBD |
| Aggregate | <500ms | TBD | TBD |
| NOT clause | <400ms | TBD | TBD |
| Full-text Search | <600ms | TBD | TBD |
| Cardinality-many | <200ms | TBD | TBD |
| Rule with Predicate | <300ms | TBD | TBD |

### Transaction Throughput Benchmarks

See detailed results in: \`transaction_throughput_${TIMESTAMP}.txt\`

| Operation Type | Expected | Actual | Status |
|----------------|----------|--------|--------|
| Single Transaction | >600 TPS | TBD | TBD |
| Batch Transaction | >5000 datoms/sec | TBD | TBD |
| CAS Operations | >500 ops/sec | TBD | TBD |
| Upsert Operations | >400 ops/sec | TBD | TBD |
| Retractions | >300 ops/sec | TBD | TBD |

### UNION ALL Analysis

See detailed results in: \`union_all_analysis_${TIMESTAMP}.txt\`

| Metric | Expected | Actual | Status |
|--------|----------|--------|--------|
| UNION ALL overhead | <2x | TBD | TBD |

## Next Steps

1. Review detailed benchmark results
2. Compare against expected performance targets
3. Identify bottlenecks if targets not met
4. Proceed to Phase 2 (Index Optimization) if needed
5. Run 10M datom benchmarks for scalability testing

## Files Generated

- Dataset creation: \`dataset_1m_creation_${TIMESTAMP}.txt\`
- Query performance: \`query_performance_1m_${TIMESTAMP}.txt\`
- Transaction throughput: \`transaction_throughput_${TIMESTAMP}.txt\`
- UNION ALL analysis: \`union_all_analysis_${TIMESTAMP}.txt\`
- System statistics: \`system_stats_${TIMESTAMP}.txt\`
- This summary: \`SUMMARY_${TIMESTAMP}.md\`
EOF

echo -e "${GREEN}✓ Summary report generated${NC}"

# ============================================================================
# Complete
# ============================================================================

echo ""
echo "========================================"
echo -e "${GREEN}  Benchmarks Complete!${NC}"
echo "========================================"
echo ""
echo "Results saved to: $RESULTS_DIR"
echo "Summary report: $RESULTS_DIR/SUMMARY_${TIMESTAMP}.md"
echo ""
echo "To review results:"
echo "  cat $RESULTS_DIR/SUMMARY_${TIMESTAMP}.md"
echo ""
echo "To run 10M datom benchmarks:"
echo "  psql -c 'SELECT * FROM create_benchmark_data_10m();'"
echo "  ./benchmarks/BENCHMARK_RUNNER.sh"
echo ""
