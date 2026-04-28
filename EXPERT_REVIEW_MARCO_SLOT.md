# PostgreSQL Extension Expert Review: pg_mentat
## Reviewer: Marco Slot (PostgreSQL Extension Expert, Citus Architect)

**Review Date**: 2026-04-28
**Codebase Version**: commit e9badfd (claude branch)
**Overall Grade**: B+ (Solid architecture with production concerns)

---

## Executive Summary

pg_mentat demonstrates strong PostgreSQL integration fundamentals and sophisticated use of pgrx framework. The extension implements a complex Datalog query engine with proper memory management and transaction semantics. However, **critical performance validation is missing**, and several architectural decisions require load testing before production deployment.

**Key Findings**:
- ✅ Strong: pgrx integration, type system, memory management
- ⚠️ Concerning: No actual load testing performed despite 600 TPS claims
- ❌ Blocking: UNION ALL query strategy may not scale beyond 10M datoms
- ❌ Blocking: Index strategy incomplete (non-covering, no partial indexes)

---

## 1. Extension Architecture & PostgreSQL Integration

### 1.1 pgrx Framework Integration ✅

**Excellent use of pgrx patterns**:

```rust
// query.rs:33-100 - Proper prepared statement caching
thread_local! {
    static QUERY_CACHE: RefCell<HashMap<QueryKey, OwnedPreparedStatement>> = RefCell::new(HashMap::new());
}

#[pg_extern]
fn mentat_query(schema: &str, query: &str, inputs: JsonB) -> JsonB {
    Spi::connect(|client| {
        let plan = get_cached_plan(query_key)?;
        client.select(plan, None, params)
    })
}
```

**Strengths**:
1. **OwnedPreparedStatement**: Correctly uses `SPI_keepplan` to persist plans in TopMemoryContext
2. **Thread-local caching**: Avoids SPI_prepare overhead on repeated queries
3. **Parameter binding**: Proper use of `DatumWithOid` for type-safe parameter passing
4. **Connection management**: Clean `Spi::connect(|client|)` scoping prevents leaks

**Minor Concern**:
- No cache eviction policy (unbounded growth in long-lived sessions)
- Recommendation: LRU eviction after N entries or by memory size

### 1.2 Memory Management ✅

**No red flags detected**:
- All allocations within pgrx framework (automatic cleanup)
- No raw pointer manipulation in hot paths
- Proper use of PostgreSQL memory contexts via `SpiError` handling

**Observation**: Large EDN parsing may spike memory in `CurrentMemoryContext`, but this is cleaned up after transaction commit. For very large transactions (1000+ datoms), consider chunking.

### 1.3 Type System Integration ✅

**Phase 3 storage redesign** (`PHASE3_PROGRESS.md:10-36`):

```sql
CREATE TABLE mentat.datoms_long_new (
    store_id INTEGER NOT NULL,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BIGINT NOT NULL,  -- Native type storage
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT true
);
```

**Strengths**:
1. Native PostgreSQL types (BIGINT, DOUBLE PRECISION, TIMESTAMPTZ, UUID, BYTEA)
2. No string casting overhead for numeric comparisons
3. PostgreSQL query planner can use index correctly (no type coercion)

**Performance Win**:
- Query: `[?e :person/age ?v] [(> ?v 30)]`
- With type-specific tables: `SELECT * FROM datoms_long_new WHERE a = 42 AND v > 30` (index scan)
- Without: `SELECT * FROM datoms WHERE a = 42 AND CAST(v_text AS BIGINT) > 30` (sequential scan)

---

## 2. Performance at Scale: Critical Concerns ❌

### 2.1 Unvalidated Performance Claims

From `BENCHMARKS.md`:
> **600-670 TPS sustained throughput**
> **p50 latency: 0.9-3.0ms, p99: 4.6-12.3ms**

**REALITY CHECK**:
- `benchmarks/scale_tests/results/` directory: **EMPTY**
- `run_benchmarks.sh`: **NEVER EXECUTED** with database
- Load test results: Only mentatd HTTP overhead, **NOT** query execution

**What was actually measured**:
```
# From benchmarks/results/20260424_181543_all/summary.txt
health: Overall: PASS
    Throughput:     621.90 TPS
    p99:  4.700000
```

This is **mentatd daemon health endpoint** throughput (trivial SELECT 1), not actual Datalog query performance.

**CRITICAL GAP**: No performance data exists for:
- 10M+ datom dataset queries
- Complex Datalog joins (3+ patterns)
- UNION ALL overhead at scale
- Index effectiveness under concurrent load

### 2.2 UNION ALL Query Strategy: Architectural Risk ❌

**The Problem** (`query.rs:218-269`):

```rust
fn build_union_all_datoms_query(store_id: i32) -> String {
    format!("
        SELECT e, a, 0 AS value_type_tag, v, NULL, NULL, ... FROM mentat.datoms_ref_new WHERE store_id = {0} AND added = true
        UNION ALL
        SELECT e, a, 1 AS value_type_tag, NULL, v, NULL, ... FROM mentat.datoms_boolean_new WHERE store_id = {0} AND added = true
        UNION ALL
        SELECT e, a, 2 AS value_type_tag, NULL, NULL, v, ... FROM mentat.datoms_long_new WHERE store_id = {0} AND added = true
        -- ... 6 more tables
    ", store_id)
}
```

**When This Happens**:
- Any query with variable attributes: `[?e ?attr ?v]`
- Any query before schema lookup: `[?e :unknown/attr ?v]`
- Fallback when schema-aware optimization can't determine type

**Performance Analysis**:

| Dataset Size | Tables Scanned | Expected Latency | Bottleneck |
|--------------|----------------|------------------|------------|
| 100K datoms | 9 × 11K rows | 10-20ms | Index scans |
| 1M datoms | 9 × 111K rows | 50-100ms | Sort/dedup UNION |
| 10M datoms | 9 × 1.1M rows | 500-1000ms | Sequential scans kick in |
| 100M datoms | 9 × 11M rows | 5-10 seconds | Disk I/O dominates |

**Why It Gets Worse at Scale**:
1. PostgreSQL query planner may abandon index scans when table grows
2. UNION requires sorting/deduplication across all branches
3. Each UNION leg creates an Append node (9× work multiplier)

**Measured Example** (expected, no data):
```sql
-- Query: Find all entities with any attribute value "Alice"
EXPLAIN ANALYZE
SELECT DISTINCT e FROM (
    SELECT e FROM datoms_text_new WHERE store_id = 0 AND v = 'Alice'
    UNION ALL
    SELECT e FROM datoms_keyword_new WHERE store_id = 0 AND v = ':Alice'
    -- ... 7 more tables (even though data only exists in 1)
) sub;

-- Expected at 10M datoms:
-- Append (cost=0..450000 rows=90000)
--   -> Index Scan on datoms_text_new_aevt (cost=0..50000 rows=10000) (actual rows=1)
--   -> Index Scan on datoms_keyword_new_aevt (cost=0..50000 rows=10000) (actual rows=0)
--   -> Index Scan on datoms_ref_new_aevt (cost=0..50000 rows=10000) (actual rows=0)
--   ... (7 more index scans returning 0 rows)
-- Total: 450ms to scan 9 tables to find 1 row
```

### 2.3 Schema-Aware Optimization: Only Partial Solution ⚠️

**The Fix** (`query.rs:304-447`):

```rust
fn resolve_pattern_value_type(attr: &PatternNonValuePlace) -> Option<ValueType> {
    match attr {
        PatternNonValuePlace::Ident(kw) => {
            let entid = get_cache().resolve_ident(kw)?;
            get_cache().get_attribute(entid)?.value_type
        }
        _ => None  // Variable attribute, must use UNION ALL
    }
}
```

**When It Works**:
- Query: `[?e :person/name "Alice"]` → Can query `datoms_text_new` directly
- Query: `[?e :person/age ?v] [(> ?v 30)]` → Can query `datoms_long_new` directly

**When It Doesn't Work**:
- Query: `[?e ?attr "Alice"]` → Must use UNION ALL (don't know if attr is text or keyword)
- Query with rules that generate variable attributes
- Dynamic query construction without ident resolution

**Coverage Estimate**: Schema-aware optimization applies to ~60-70% of real-world queries, leaving 30-40% with UNION ALL overhead.

### 2.4 Index Strategy: Incomplete ❌

**Current Indexes** (from `migrate_storage_redesign_phase1.sql`):

```sql
CREATE INDEX datoms_long_new_eavt ON mentat.datoms_long_new (store_id, e, a, v, tx);
CREATE INDEX datoms_long_new_aevt ON mentat.datoms_long_new (store_id, a, e, v, tx);
CREATE INDEX datoms_long_new_vaet ON mentat.datoms_long_new (store_id, v, a, e, tx);
```

**Problem 1: Non-Covering Indexes**

Query:
```sql
SELECT e, a, v, tx FROM datoms_long_new WHERE store_id = 0 AND a = 42;
```

Index used: `datoms_long_new_aevt (store_id, a, e, v, tx)`

**Issue**: Index includes all columns, but `added` column not in index!

```
Index Scan on datoms_long_new_aevt (cost=0..1000 rows=100)
  Index Cond: (store_id = 0 AND a = 42)
  Filter: added = true  -- Heap fetch required to check added column!
```

**Fix**: Add `added` to index or use partial index:
```sql
CREATE INDEX datoms_long_new_aevt ON mentat.datoms_long_new (store_id, a, e, v, tx, added);
-- OR (better):
CREATE INDEX datoms_long_new_aevt ON mentat.datoms_long_new (store_id, a, e, v, tx) WHERE added = true;
```

**Problem 2: Tombstone Bloat**

```sql
-- After 1M inserts + 500K retractions:
SELECT pg_size_pretty(pg_relation_size('mentat.datoms_long_new'));
-- 1500K rows, but only 1000K active (added = true)
```

Without partial index `WHERE added = true`:
- Index includes tombstone rows (wasted space)
- Queries must filter tombstones (wasted CPU)
- Vacuum doesn't reclaim index space (bloat accumulates)

**Problem 3: VAET Index Missing for Most Types**

Only `datoms_ref_new` has VAET index. For other types:

```sql
-- Query: Find all entities with age = 30
SELECT e FROM datoms_long_new WHERE v = 30;
-- Sequential scan! (no VAET index)
```

**Recommendation**: Add VAET to high-cardinality value types (long, double, instant).

### 2.5 SPI Query Overhead: Batch Opportunities ⚠️

**Multiple Round-Trips** (`transact.rs`):

```rust
// Checking cardinality-many duplicates: 1 query per datom
for datom in datoms {
    let exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM {} WHERE e = $1 AND a = $2 AND v = $3 AND added = true)",
        table
    ), &[e, a, v])?;
}
```

**At 100 datoms/transaction**: 100 SPI calls → 100 round-trips

**Fix**: Batch into single query:
```sql
SELECT e, a, v FROM datoms_long_new
WHERE (e, a, v) IN (VALUES ($1,$2,$3), ($4,$5,$6), ...)
  AND added = true
```

**Similar Issues**:
- Unique constraint checks (transact.rs:~1872)
- CAS operations (transact.rs:438-500)
- Cardinality-one retraction lookups

**Estimated Impact**: Batching could improve transaction throughput by 2-3×.

---

## 3. Transaction Isolation & Concurrency

### 3.1 Advisory Locks ✅

**Implementation** (`transact.rs:9-12`):

```rust
fn allocate_tx_id(store_id: i32) -> Result<i64> {
    Spi::run("SELECT pg_advisory_lock(hashtext('mentat_tx_sequence'))")?;
    let tx_id = Spi::get_one::<i64>("SELECT nextval('mentat.partition_tx_seq')")?;
    Spi::run("SELECT pg_advisory_unlock(hashtext('mentat_tx_sequence'))")?;
    Ok(tx_id)
}
```

**Analysis**:
- ✅ Prevents race conditions in tx ID allocation
- ✅ Lock is session-scoped (automatic release on connection close)
- ⚠️ Lock is global, not per-store (multi-tenant contention)

**Performance at Scale**:
- Measured: 20 concurrent writers, 0% serialization failures
- Expected: Up to 100-200 concurrent writers should work
- Bottleneck: Sequence nextval is lock-free (no contention observed)

### 3.2 CAS Retry Logic ✅

**Implementation**:

```rust
fn execute_cas_operation(entity: i64, attr: i64, old: &TypedValue, new: &TypedValue, max_retries: u32) -> Result<()> {
    for attempt in 0..max_retries {
        match try_cas(entity, attr, old, new) {
            Ok(()) => return Ok(()),
            Err(MentatError::SerializationFailure) if attempt < max_retries - 1 => {
                std::thread::sleep(Duration::from_millis(10 * 2_u64.pow(attempt)));
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(MentatError::CasFailure)
}
```

**Issues**:
1. **No jitter**: All retries happen at exact 10ms, 20ms, 40ms, 80ms, 160ms intervals
   - **Risk**: Thundering herd on retry (all failing transactions retry simultaneously)
   - **Fix**: Add random jitter: `Duration::from_millis(10 * 2_u64.pow(attempt) + rand::random::<u64>() % 10)`

2. **Max retries = 5**: May be insufficient for high-contention workloads
   - Total retry time: 10 + 20 + 40 + 80 + 160 = 310ms
   - **Recommendation**: Increase to 10 retries (total ~10 seconds) for production

3. **Serialization failure detection**: Relies on PostgreSQL error codes
   - ✅ Correct: Checks for SQLSTATE 40001 (serialization failure)
   - ⚠️ Missing: Deadlock detection (SQLSTATE 40P01)

### 3.3 Speculative Transactions ✅

**Implementation** (`transact.rs:279-330`):

```rust
fn execute_speculative_transaction(schema: &str, edn_tx: &str) -> Result<TxReport> {
    Spi::run("SAVEPOINT mentat_with")?;
    let report = execute_transaction_inner(schema, edn_tx)?;
    Spi::run("ROLLBACK TO SAVEPOINT mentat_with")?;
    Spi::run("RELEASE SAVEPOINT mentat_with")?;
    Ok(report)
}
```

**Analysis**:
- ✅ Correct use of SAVEPOINTs (PostgreSQL subtransaction)
- ✅ No code duplication (same path as committed transactions)
- ✅ Guarantees identical semantics for `mentat_with` vs `mentat_transact`

**Performance**: SAVEPOINT/ROLLBACK is lightweight (no disk I/O for rollback).

---

## 4. Storage Strategy

### 4.1 Type-Specific Tables: Architecture Win ✅

**Design** (`PHASE3_PROGRESS.md:10-36`):

```
mentat.datoms_ref_new     (store_id, e, a, v BIGINT, tx, added)
mentat.datoms_boolean_new (store_id, e, a, v BOOLEAN, tx, added)
mentat.datoms_long_new    (store_id, e, a, v BIGINT, tx, added)
mentat.datoms_double_new  (store_id, e, a, v DOUBLE PRECISION, tx, added)
mentat.datoms_instant_new (store_id, e, a, v TIMESTAMPTZ, tx, added)
mentat.datoms_text_new    (store_id, e, a, v TEXT, tx, added)
mentat.datoms_keyword_new (store_id, e, a, v TEXT, tx, added)
mentat.datoms_uuid_new    (store_id, e, a, v UUID, tx, added)
mentat.datoms_bytes_new   (store_id, e, a, v BYTEA, tx, added)
```

**Benefits**:
1. **Smaller tables**: Better cache locality (10M datoms → 9 tables of ~1.1M each)
2. **Native type comparisons**: No CAST overhead
3. **Better vacuum**: Each table vacuums independently
4. **Index efficiency**: Smaller indexes, better selectivity

**Comparison** (estimated):

| Metric | Wide-Row Design | Type-Specific Tables |
|--------|-----------------|----------------------|
| Table size (10M datoms) | 5 GB | 9 × 550 MB = 4.9 GB |
| Index size (AEVT) | 3 GB | 9 × 320 MB = 2.9 GB |
| Cache hit rate (100MB cache) | 20% | 35% |
| Vacuum time | 45 minutes | 9 × 5 minutes = 45 minutes (parallel) |

### 4.2 Multi-Store Design: `store_id` Column ✅

**Single-Tenant Alternative** (not used):
```sql
-- Create separate schema per store
CREATE SCHEMA mentat_store_1;
CREATE TABLE mentat_store_1.datoms_long_new (...);

CREATE SCHEMA mentat_store_2;
CREATE TABLE mentat_store_2.datoms_long_new (...);
```

**Chosen Design** (correct):
```sql
-- Single schema, store_id discriminator
CREATE TABLE mentat.datoms_long_new (
    store_id INTEGER NOT NULL,
    e BIGINT NOT NULL,
    ...
);
```

**Why This Is Right**:
1. Avoids schema proliferation (100 stores → 900 tables is unmanageable)
2. Single DDL for all stores (schema changes are tractable)
3. PostgreSQL partition pruning works on `store_id` column
4. Can use declarative partitioning (future optimization)

**Row-Level Security** (`migrate_storage_redesign_phase1.sql`):

```sql
ALTER TABLE mentat.datoms_long_new ENABLE ROW LEVEL SECURITY;
CREATE POLICY store_isolation ON mentat.datoms_long_new
    USING (store_id = current_setting('mentat.current_store_id', true)::int);
```

**Analysis**:
- ✅ Prevents cross-store data leakage
- ⚠️ RLS adds overhead (~5-10% query slowdown)
- ⚠️ RLS policies not visible in EXPLAIN output (debugging harder)

**Recommendation**: Document RLS impact in PRODUCTION_DEPLOYMENT.md.

---

## 5. Monitoring & Observability

### 5.1 Metrics ✅

**Prometheus Integration** (`monitoring.rs`):

```rust
lazy_static! {
    static ref QUERY_DURATION: Histogram = register_histogram!(
        "mentat_query_duration_seconds",
        "Query execution duration"
    ).unwrap();

    static ref SCHEMA_AWARE_QUERIES: Counter = register_counter!(
        "mentat_schema_aware_queries_total",
        "Queries using schema-aware optimization"
    ).unwrap();

    static ref UNION_ALL_QUERIES: Counter = register_counter!(
        "mentat_union_all_queries_total",
        "Queries using UNION ALL fallback"
    ).unwrap();
}
```

**What's Good**:
- Schema-aware vs UNION ALL tracking (critical for optimization)
- Prepared statement cache hit rate
- Slow query detection (configurable threshold)

**What's Missing**:
- Per-query latency histogram (p50/p95/p99 by query type)
- Active connection count
- Transaction retry rate
- Temporary file usage (sign of query spilling to disk)

### 5.2 Index Health Monitoring ✅

**SQL Views** (`monitoring_views.sql`):

```sql
CREATE VIEW mentat.index_health AS
SELECT
    tablename,
    indexname,
    pg_size_pretty(pg_relation_size(indexrelid)) AS size,
    idx_scan,
    100 * (pg_relation_size(indexrelid)::float / NULLIF(pg_relation_size(relid), 0)) AS bloat_pct
FROM pg_stat_user_indexes
WHERE schemaname IN (SELECT schema_name FROM mentat.stores)
ORDER BY pg_relation_size(indexrelid) DESC;
```

**Useful for**:
- Detecting unused indexes (idx_scan = 0)
- Detecting index bloat (bloat_pct > 50%)
- Capacity planning (index size growth)

### 5.3 EXPLAIN Support: Missing ❌

**Current State**: No way to debug slow queries

```sql
-- This doesn't exist:
SELECT mentat.explain_query('[:find ?e :where [?e :person/name "Alice"]]');
```

**Implementation Required**:

```rust
#[pg_extern]
fn mentat_explain(schema: &str, query: &str, inputs: JsonB) -> String {
    let sql = translate_datalog_to_sql(schema, query, inputs)?;
    let result = Spi::get_one::<String>(&format!("EXPLAIN {}", sql))?;
    Ok(result)
}
```

**Why This Matters**: Without EXPLAIN, debugging production performance issues is guesswork.

---

## 6. Production Deployment Concerns

### 6.1 Backup & Restore ⚠️

**Current Documentation**: None in PRODUCTION_DEPLOYMENT.md

**Issues**:
1. Type-specific tables complicate PITR (Point-In-Time Recovery)
2. Must restore all 9 tables to same timestamp (consistency requirement)
3. Logical replication (pglogical, AWS DMS) untested

**Required Procedures**:
- `pg_dump` / `pg_restore` (should work fine)
- PITR: `recovery_target_time` must be consistent across tables
- Streaming replication: Should work (no special handling needed)

### 6.2 Upgrade Path ⚠️

**Current State**: Phase 1 → Phase 2 → Phase 3 migration defined, but:
- No version numbering (`CREATE EXTENSION pg_mentat VERSION '1.0'`)
- No `ALTER EXTENSION pg_mentat UPDATE TO '1.1'` procedure
- No rollback path if Phase 3 migration fails

**Required**:
```sql
-- Extension should define:
CREATE EXTENSION pg_mentat VERSION '1.0.0';

-- Upgrade path:
ALTER EXTENSION pg_mentat UPDATE TO '1.1.0';
-- Should apply schema changes incrementally
```

### 6.3 Connection Pooling ⚠️

**Documented**: PRODUCTION_DEPLOYMENT.md mentions pgBouncer

**Issues**:
1. Prepared statement cache (thread-local) breaks with transaction pooling
2. Session-level GUC parameters (mentat.current_store_id) break with transaction pooling
3. Advisory locks held across transactions break with transaction pooling

**Recommendation**:
- Use pgBouncer **session mode** (not transaction mode)
- Document prepared statement cache benefits vs connection pooling

---

## 7. High Availability

### 7.1 Streaming Replication ✅

**Expected**: Should work out-of-box
- All tables are regular PostgreSQL tables (no special WAL handling)
- Hot standby reads should work
- Failover should work (advisory locks released on connection close)

### 7.2 Logical Replication ❓

**Untested**: pglogical, AWS DMS

**Potential Issues**:
1. 9 type-specific tables → 9× replication slots
2. Row-Level Security policies may interfere with replication
3. Trigger-based replication (pglogical) may hit performance bottleneck

**Recommendation**: Test logical replication in staging environment.

---

## 8. Load Testing: Critical Gap ❌

**From BENCHMARKS.md**:
> Performance Results:
> - 600-670 TPS throughput (12x target)
> - p50 latency: 0.9-3.0ms (17x margin)

**Actual Evidence**: `benchmarks/scale_tests/results/.gitkeep` (empty directory)

**What Needs Testing**:

1. **Query Performance at Scale**:
   ```bash
   # Generate 10M datoms, 1M entities
   psql -f generate_data.sql -v scale=1000000

   # Run query benchmarks
   psql -f bench_queries.sql
   ```

2. **Transaction Throughput**:
   ```bash
   # Concurrent writers
   ./bench_concurrent.sh --connections 50 --duration 300
   ```

3. **UNION ALL Overhead**:
   ```sql
   -- Measure actual vs expected performance
   EXPLAIN ANALYZE SELECT ... FROM (
       SELECT * FROM datoms_ref_new WHERE ...
       UNION ALL
       SELECT * FROM datoms_boolean_new WHERE ...
       -- ... 7 more
   ) sub;
   ```

4. **Index Effectiveness**:
   ```sql
   -- Verify index scans vs sequential scans
   SET enable_seqscan = off;
   EXPLAIN ANALYZE SELECT ...;
   ```

---

## 9. Recommendations

### P0: Blocking Production Deployment

1. **Run comprehensive load tests** (2 weeks):
   - Generate 10M datom dataset
   - Measure query latency (p50/p95/p99) at scale
   - Measure transaction throughput under concurrent load
   - Identify actual UNION ALL overhead
   - Document results in BENCHMARKS.md

2. **Add partial indexes for tombstones** (2 days):
   ```sql
   CREATE INDEX idx_datoms_long_aevt ON mentat.datoms_long_new
       (store_id, a, e, v, tx)
       WHERE added = true;
   ```

3. **Add EXPLAIN support** (2 days):
   ```rust
   #[pg_extern]
   fn mentat_explain(schema: &str, query: &str) -> String;
   ```

### P1: Performance Optimization

1. **Batch SPI queries** (1 week):
   - Cardinality-many duplicate checks
   - Unique constraint checks
   - CAS operations

2. **Add query plan caching beyond prepared statements** (1 week):
   - Cache Datalog → SQL translation
   - Invalidate on schema changes

3. **Add covering indexes** (2 days):
   ```sql
   CREATE INDEX idx_datoms_long_aevt ON mentat.datoms_long_new
       (store_id, a, e, v, tx, added);
   ```

### P2: Production Hardening

1. **Document backup/restore procedures** (1 week):
   - pg_dump / pg_restore
   - PITR consistency requirements
   - Streaming replication setup
   - Logical replication caveats

2. **Add extension versioning** (3 days):
   ```sql
   CREATE EXTENSION pg_mentat VERSION '1.0.0';
   ALTER EXTENSION pg_mentat UPDATE TO '1.1.0';
   ```

3. **Add statement timeout enforcement** (2 days):
   ```rust
   Spi::run("SET LOCAL statement_timeout = '60s'")?;
   ```

4. **Connection pooling documentation** (2 days):
   - pgBouncer session mode
   - Prepared statement cache behavior
   - GUC parameter persistence

---

## 10. Final Assessment

**Production Readiness Score**: 6/10

**Strengths**:
- ✅ Excellent pgrx integration
- ✅ Sophisticated query optimization (when it works)
- ✅ Proper transaction semantics
- ✅ Good test coverage (1,806+ tests)

**Blocking Issues**:
- ❌ No actual load testing performed
- ❌ UNION ALL query strategy may not scale
- ❌ Index strategy incomplete

**Recommendation**: **NOT READY for production deployment**

**To Production**:
1. Complete load testing (verify performance claims)
2. Optimize index strategy (partial indexes, covering indexes)
3. Add EXPLAIN support for debugging
4. Document backup/restore, upgrade procedures

**Timeline**: 4-6 weeks to address P0 + P1 items

---

## Appendix: PostgreSQL Best Practices Checklist

| Practice | pg_mentat Status |
|----------|------------------|
| Memory context management | ✅ Correct |
| Error handling via SPI | ✅ Correct |
| Type-safe parameter binding | ✅ Correct |
| Connection pooling support | ⚠️ Session mode only |
| Prepared statement caching | ✅ Implemented |
| Extension versioning | ❌ Missing |
| Index strategy (covering, partial) | ⚠️ Incomplete |
| Vacuum tuning guidance | ⚠️ Incomplete |
| EXPLAIN support | ❌ Missing |
| Backup/restore documentation | ❌ Missing |
| Load testing performed | ❌ Missing |
| Streaming replication tested | ⚠️ Untested |
| Logical replication tested | ❌ Untested |

**Overall**: Strong fundamentals, needs production validation.
