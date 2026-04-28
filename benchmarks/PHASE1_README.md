# Phase 1: Performance Validation & Load Testing

**Timeline**: 2 weeks
**Goal**: Validate performance claims with actual measurements at scale

## Overview

This phase implements comprehensive performance benchmarks to validate:
1. Query latency claims (<50ms for simple patterns)
2. Transaction throughput claims (>600 TPS)
3. UNION ALL overhead at scale
4. Scalability from 1M to 10M to 100M datoms

## Critical Motivation

The expert reviews identified that **no actual load testing has been performed** despite claims of "600 TPS" performance. The `benchmarks/scale_tests/results/` directory is empty, indicating benchmark results are unvalidated.

**This phase addresses the #1 blocker**: Prove the architecture performs as claimed.

## Quick Start

### Prerequisites

- PostgreSQL 13+ with pg_mentat extension installed
- At least 8GB RAM for 10M datom dataset
- At least 50GB disk space

### Run All Benchmarks (1M dataset)

```bash
cd /home/gburd/ws/pg_mentat

# 1. Load the benchmark SQL functions
psql -d your_database -f benchmarks/datasets/create_test_data.sql

# 2. Run all benchmarks
./benchmarks/BENCHMARK_RUNNER.sh

# Results will be in: benchmarks/results/SUMMARY_<timestamp>.md
```

### Run Individual Benchmarks

```bash
# Create 1M datom dataset (~3-5 minutes)
psql -d your_database -c "SELECT * FROM create_benchmark_data_1m();"

# Run query performance benchmarks
psql -d your_database -f benchmarks/query_performance/benchmark_queries.sql

# Run transaction throughput benchmarks
psql -d your_database -f benchmarks/transaction_throughput/benchmark_transactions.sql
```

## Benchmark Components

### 1. Dataset Creation (`datasets/create_test_data.sql`)

Creates realistic test datasets with 100k-1M entities containing:
- **10 attributes per entity**: name, age, email, dept, salary, active, score, joined, bio, tags
- **Value type diversity**: strings, longs, doubles, booleans, instants
- **Cardinality-many**: tags attribute (2 per entity)
- **Unique identity**: email attribute
- **Full-text search**: bio attribute

**Datasets**:
- **1M datoms**: 100k entities × 10 attributes + 200k tags (~2-5 min to create)
- **10M datoms**: 1M entities × 10 attributes + 2M tags (~20-40 min to create)
- **100M datoms**: 10M entities × 10 attributes + 20M tags (~3-6 hours to create)

### 2. Query Performance Benchmarks (`query_performance/benchmark_queries.sql`)

Tests 12 different query patterns:

| Benchmark | Query Type | Expected (1M) | Tests |
|-----------|------------|---------------|-------|
| 1 | Simple Pattern | <50ms | Basic pattern matching |
| 2 | Join (2 patterns) | <100ms | Implicit joins on shared variables |
| 3 | Join with Predicate | <150ms | Predicates in base query |
| 4 | Complex Join (3+ patterns) | <200ms | Multi-pattern joins |
| 5 | OR-join | <250ms | OR clauses with patterns |
| 6 | OR-join with Predicates | <300ms | **NEWLY IMPLEMENTED FEATURE** |
| 7 | Aggregate | <500ms | count, avg aggregations |
| 8 | NOT clause | <400ms | Negation queries |
| 9 | Full-text Search | <600ms | BM25 scoring |
| 10 | Cardinality-many | <200ms | Many-valued attributes |
| 11 | Rule with Predicate | <300ms | **PREDICATES IN RULE BODIES** |
| 12 | Recursive Rule | TBD | (Skipped for now) |

### 3. Transaction Throughput Benchmarks (`transaction_throughput/benchmark_transactions.sql`)

Tests 5 transaction operation types:

| Benchmark | Operation Type | Expected | Tests |
|-----------|---------------|----------|-------|
| 1 | Single Transaction | >600 TPS | Individual tx commits |
| 2 | Batch Transaction | >5000 datoms/sec | Large batches |
| 3 | CAS Operations | >500 ops/sec | Compare-and-swap |
| 4 | Upsert Operations | >400 ops/sec | Unique identity upsert |
| 5 | Retractions | >300 ops/sec | Entity retractions |

### 4. UNION ALL Analysis

Compares UNION ALL query strategy (current) vs single-table queries (optimized):

- **Current**: Query all 9 type-specific tables with UNION ALL
- **Optimized**: Query single table when type is known from schema
- **Expected**: UNION ALL overhead <2x vs single-table

**Critical Question**: Does UNION ALL strategy scale to 10M+ datoms?

## Expected Outcomes

### Success Criteria (1M datoms)

- ✅ Query latency: <50ms for simple patterns
- ✅ Query latency: <300ms for complex patterns
- ✅ Transaction throughput: >600 TPS single-threaded
- ✅ UNION ALL overhead: <2x vs single-table queries
- ✅ Memory usage: <2GB for query execution

### Success Criteria (10M datoms)

- ✅ Query latency: <200ms for simple patterns
- ✅ Query latency: <1000ms for complex patterns
- ✅ Transaction throughput: >600 TPS (should be constant)
- ✅ UNION ALL overhead: <2x (should not degrade)
- ✅ Memory usage: <4GB for query execution

### Failure Scenarios

If benchmarks don't meet targets, proceed to:
1. **Phase 2 (Index Optimization)**: Add partial indexes, VAET indexes, covering indexes
2. **Schema-Aware Optimization**: Implement single-table query generation when type is known
3. **Query Planner Hints**: Add PostgreSQL optimizer hints for complex queries

## Benchmark Results Location

All results are saved to `benchmarks/results/` with timestamps:

- `dataset_1m_creation_<timestamp>.txt` - Dataset creation logs
- `query_performance_1m_<timestamp>.txt` - Query benchmark results with EXPLAIN ANALYZE
- `transaction_throughput_<timestamp>.txt` - Transaction benchmark results
- `union_all_analysis_<timestamp>.txt` - UNION ALL overhead analysis
- `system_stats_<timestamp>.txt` - Table sizes, datom counts, index statistics
- `SUMMARY_<timestamp>.md` - Executive summary with pass/fail status

## Interpreting Results

### Query Performance

Look for in `query_performance_1m_<timestamp>.txt`:

```
Execution Time: 45.123 ms  ← Should be <50ms for simple patterns
Buffers: shared hit=123 read=45  ← Lower is better
Planning Time: 2.456 ms  ← Should be <5ms
```

**Red flags**:
- Execution time >2x expected
- Sequential scans instead of index scans
- High buffer reads (indicates cache misses)

### Transaction Throughput

Look for in `transaction_throughput_<timestamp>.txt`:

```
 transactions_completed | duration_ms |  tps   | avg_tx_ms
------------------------+-------------+--------+-----------
                   1000 |    1543.21  | 648.25 |    1.543
```

**Target**: `tps` > 600

**Red flags**:
- TPS < 600
- High variability (run multiple times)
- Decreasing TPS over time (indicates index bloat)

### UNION ALL Overhead

Look for in `union_all_analysis_<timestamp>.txt`:

```
        metric         | ratio
-----------------------+-------
 UNION ALL overhead    |  1.73
```

**Target**: ratio < 2.0

**Red flags**:
- Ratio > 3.0 (indicates significant overhead)
- Ratio increasing with dataset size (scalability issue)

## Next Steps After Phase 1

### If benchmarks PASS all targets:

Proceed to **Phase 3 (Client Libraries)**:
- Build Clojure peer library
- Build Python native client
- Skip Phase 2 (optimization not needed)

### If benchmarks FAIL some targets:

Proceed to **Phase 2 (Index Optimization)**:
- Add partial indexes (`WHERE added = true`)
- Add VAET indexes for value lookups
- Add covering indexes for common queries
- Re-run benchmarks to measure improvement

### If UNION ALL overhead is high (>3x):

Implement **Schema-Aware Query Optimization**:
- Generate single-table queries when attribute type is known
- Use UNION ALL only for variable attributes (e.g., `[?e ?attr ?v]`)
- Estimated improvement: 5-10x for typed queries

## Running 10M Datom Benchmarks

After validating 1M performance, scale to 10M:

```bash
# Create 10M dataset (~30-60 minutes)
psql -d your_database -c "SELECT * FROM create_benchmark_data_10m();"

# Run benchmarks
./benchmarks/BENCHMARK_RUNNER.sh > benchmarks/results/run_10m_$(date +%Y%m%d_%H%M%S).log

# Compare results
diff benchmarks/results/SUMMARY_*_1m_*.md benchmarks/results/SUMMARY_*_10m_*.md
```

**Expected**: Linear or sub-linear scaling (10x data → <10x latency)

## Troubleshooting

### "Out of memory" errors

Increase PostgreSQL memory settings in `postgresql.conf`:

```
shared_buffers = 4GB        # 25% of RAM
work_mem = 256MB            # Increase for large sorts/aggregates
maintenance_work_mem = 1GB  # For index creation
```

### Slow dataset creation

Batch size can be adjusted in SQL functions:

```sql
-- Change batch_size from 1000 to 10000 for faster creation
batch_size int := 10000;
```

### Connection timeouts

Increase statement timeout:

```sql
SET statement_timeout = '10min';
```

## Continuous Benchmarking

For regression testing, run benchmarks weekly:

```bash
# Automated benchmark runner (add to cron)
#!/bin/bash
cd /home/gburd/ws/pg_mentat
./benchmarks/BENCHMARK_RUNNER.sh > benchmarks/results/weekly_$(date +%Y%m%d).log

# Alert if TPS drops below 600
TPS=$(grep -A1 "Single Transaction" benchmarks/results/weekly_$(date +%Y%m%d).log | tail -1 | awk '{print $5}')
if (( $(echo "$TPS < 600" | bc -l) )); then
    echo "ALERT: TPS dropped to $TPS (expected >600)" | mail -s "pg_mentat performance regression" admin@example.com
fi
```

## References

- Original plan: `REVISED_PRODUCTION_PLAN.md` (Phase 1)
- Expert review: `EXPERT_REVIEW_MARCO_SLOT.md` (performance concerns)
- Architecture: `STORAGE_REDESIGN_PLAN.md` (type-specific tables)

## Timeline

| Task | Duration | Deliverable |
|------|----------|-------------|
| Create benchmark infrastructure | 2 days | ✅ **COMPLETE** |
| Run 1M datom benchmarks | 1 day | In progress |
| Run 10M datom benchmarks | 1 day | Pending |
| Analyze results | 2 days | Pending |
| Write BENCHMARKS_RESULTS.md | 2 days | Pending |
| **Total** | **2 weeks** | |

**Current Status**: Benchmark infrastructure complete, ready to run
