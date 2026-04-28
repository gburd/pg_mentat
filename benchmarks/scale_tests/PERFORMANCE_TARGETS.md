# pg_mentat Performance Targets

## Overview

This document defines expected performance targets for pg_mentat at various
data scales. These targets are derived from:

1. PostgreSQL's known performance characteristics for B-tree index lookups
2. The schema-aware optimization removing the 9-way UNION ALL overhead
3. Comparison with Datomic's documented performance claims
4. Architecture analysis of the type-specific table storage model

## Performance Targets by Scale

### Query Latency (single-threaded, warm cache)

| Operation | 10K datoms | 100K datoms | 1M datoms | 10M datoms | 100M datoms |
|-----------|-----------|-------------|-----------|------------|-------------|
| Point entity lookup (by unique identity) | < 1ms | < 2ms | < 5ms | < 10ms | < 20ms |
| Schema-aware attribute scan (count) | < 5ms | < 10ms | < 50ms | < 200ms | < 1000ms |
| UNION ALL view attribute scan (count) | < 20ms | < 50ms | < 200ms | < 1000ms | < 5000ms |
| Range scan on indexed numeric attribute | < 5ms | < 15ms | < 50ms | < 200ms | < 800ms |
| Multi-attribute join (3 attributes) | < 10ms | < 30ms | < 100ms | < 500ms | < 2000ms |
| Reference traversal (1-hop) | < 5ms | < 15ms | < 50ms | < 200ms | < 800ms |
| Full-text search (GIN index) | < 10ms | < 20ms | < 50ms | < 100ms | < 300ms |
| Aggregate (avg/min/max) | < 5ms | < 15ms | < 50ms | < 200ms | < 800ms |

### Schema-Aware Optimization Expected Speedup

The schema-aware query optimization replaces the 9-way UNION ALL subquery with
a direct scan on the single type-specific table. Expected speedup factors:

| Workload | Expected Speedup |
|----------|-----------------|
| Point lookups | 3-5x |
| Attribute scans (single type) | 5-9x |
| Range scans on typed columns | 5-9x (native type comparison vs text cast) |
| Multi-attribute joins | 3-5x per join leg |
| Aggregate on typed column | 5-9x (native type aggregation) |

**Rationale**: UNION ALL across 9 tables means PostgreSQL must plan and execute
9 index scans even when data only exists in one table. The schema-aware path
eliminates 8 of these scans entirely.

### Transaction Throughput

| Operation | Target | Notes |
|-----------|--------|-------|
| Single-entity transact (4 datoms) | > 200 tx/sec | Overhead: parse EDN + allocate entity ID + insert 4 rows |
| Batch 10 entities (50 datoms) | > 100 tx/sec | 5000+ datoms/sec |
| Batch 100 entities (500 datoms) | > 30 tx/sec | 15000+ datoms/sec |
| Batch 500 entities (2500 datoms) | > 10 tx/sec | 25000+ datoms/sec |
| Upsert via unique identity | > 150 tx/sec | Extra lookup + conditional insert/update |
| Retraction (single attribute) | > 300 tx/sec | Mark added=false, no row deletion |

### Concurrent Performance

| Metric | 10 connections | 25 connections | 50 connections |
|--------|---------------|----------------|----------------|
| Read throughput (Datalog) | > 500 qps | > 1000 qps | > 1500 qps |
| Read throughput (SQL views) | > 800 qps | > 1500 qps | > 2000 qps |
| Write throughput | > 100 tps | > 200 tps | > 300 tps |
| Mixed (70/30 r/w) | > 400 ops/sec | > 800 ops/sec | > 1200 ops/sec |
| p99 read latency | < 50ms | < 100ms | < 200ms |
| p99 write latency | < 100ms | < 200ms | < 500ms |

## Resource Scaling

### Storage Growth

| Datom Count | Expected Size (data) | Expected Size (indexes) | Total |
|-------------|---------------------|------------------------|-------|
| 10K | ~5 MB | ~10 MB | ~15 MB |
| 100K | ~50 MB | ~80 MB | ~130 MB |
| 1M | ~500 MB | ~700 MB | ~1.2 GB |
| 10M | ~5 GB | ~7 GB | ~12 GB |
| 100M | ~50 GB | ~70 GB | ~120 GB |

### Memory Requirements

| Datom Count | Recommended shared_buffers | Recommended work_mem |
|-------------|---------------------------|---------------------|
| < 100K | 256 MB | 16 MB |
| 100K - 1M | 1 GB | 32 MB |
| 1M - 10M | 4 GB | 64 MB |
| 10M - 100M | 16 GB | 128 MB |
| > 100M | 32+ GB | 256 MB |

## Comparison with Datomic

| Feature | Datomic Cloud (i3 instance) | pg_mentat Target |
|---------|---------------------------|-----------------|
| Point entity lookup | < 1ms (from cache) | < 5ms (PostgreSQL B-tree) |
| Attribute scan (indexed) | < 10ms | < 50ms at 1M |
| Transaction throughput | ~1000 tx/sec (single writer) | > 200 tx/sec (PostgreSQL single-writer) |
| Datoms/sec (batch) | ~50K datoms/sec | > 25K datoms/sec |
| Max data size | Unlimited (S3 backed) | Depends on disk (100M+ datoms proven) |
| Concurrent reads | Near-linear scaling | Near-linear to ~50 connections |

**Note**: pg_mentat trades some write throughput for strong consistency
(PostgreSQL ACID) and simpler deployment (no DynamoDB/S3/ion dependency).

## Bottleneck Analysis

### Known Bottlenecks (Architecture-Based)

1. **UNION ALL query fan-out**: Queries through `mentat.facts` view scan all 9
   type-specific tables. Schema-aware optimization eliminates this for Datalog
   queries. SQL view users pay this cost.

2. **Entity ID allocation**: Sequence-based allocation (no global lock), but
   each `mentat_transact()` must allocate IDs before inserting. Batch transactions
   amortize this cost.

3. **Schema lookup per query**: Resolving attribute idents to entids requires a
   JOIN with the schema table. Prepared statement cache mitigates repeated lookups.

4. **EDN parsing overhead**: Every `mentat_transact()` and `mentat_query()` must
   parse EDN text. This is pure CPU work proportional to input size.

5. **Store ID resolution**: Multi-store queries resolve store names to IDs via
   the `mentat.stores` table. Could be cached per-backend.

### Optimization Opportunities

1. **Prepared statement cache** (implemented): Reuses SPI plans across identical
   query shapes within the same backend.

2. **Schema-aware query routing** (implemented): Routes to single type-specific
   table when attribute type is known at plan time.

3. **GIN indexes for full-text** (implemented): `to_tsvector('english', v)` on
   `datoms_text_new` enables sub-linear full-text search.

4. **Partial indexes**: Could add `WHERE added = true` partial indexes to
   eliminate tombstone rows from index scans.

5. **BRIN indexes**: For time-ordered data (tx column), BRIN indexes provide
   very compact index structures at large scale.

## How to Run Benchmarks

```bash
# Quick smoke test
./run_benchmarks.sh -s 1000

# Standard benchmark (10K entities, ~70K datoms)
./run_benchmarks.sh -s 10000

# Performance baseline (100K entities, ~700K datoms)
./run_benchmarks.sh -s 100000 -c --connections 20

# Stress test (1M entities, ~7M datoms)
./run_benchmarks.sh -s 1000000 -c --connections 50
```

## Interpreting Results

### Green (Target Met)
- Latency within target range for the data scale
- Throughput meets or exceeds target
- Linear or sub-linear scaling with data size

### Yellow (Acceptable, Monitor)
- Latency 1.5-2x target
- Throughput 50-100% of target
- Super-linear but manageable scaling

### Red (Requires Investigation)
- Latency > 2x target
- Throughput < 50% of target
- Exponential scaling indicating algorithmic issues

## Updating Targets

These targets should be updated after:
1. Initial benchmark results establish actual baselines
2. Major architecture changes (new index strategies, caching layers)
3. PostgreSQL version upgrades that affect query planning
4. Hardware changes in the reference test environment
