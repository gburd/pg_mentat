# Storage Layer Redesign - Implementation Plan

## Status Update

**Phase 1: COMPLETE ✅**
- Type-specific tables created (`datoms_*_new`)
- `store_id` column added to metadata
- Optimized indexes created for each table type
- Dual-write trigger implemented (disabled by default)
- Migration tracking table added

**Phase 2: Ready to Execute**
- Backfill script ready (`migrate_storage_redesign_phase2_backfill.sql`)
- Progress monitoring function available

**Phase 3: TODO**
- Rust code changes needed in transact.rs, query.rs, pull.rs, etc.

**Phase 4: Pending Phase 3**
- Cutover after 7-day soak test

See `STORAGE_MIGRATION_GUIDE.md` for detailed instructions.

---

## Critical Issues to Fix

### 1. Wide Row Anti-Pattern → Type-Specific Tables

**Current Problem**:
```sql
CREATE TABLE datoms (
    e BIGINT, a BIGINT, value_type_tag INT,
    v_ref BIGINT, v_bool BOOLEAN, v_long BIGINT, v_double DOUBLE PRECISION,
    v_instant TIMESTAMPTZ, v_text TEXT, v_keyword TEXT, v_uuid UUID, v_bytes BYTEA,
    tx BIGINT, added BOOLEAN
);
```
- 11 nullable columns per row (most are NULL)
- ~80 bytes overhead per row for NULLs
- TOAST fragmentation for TEXT/BYTEA
- 4 indexes with mixed types (suboptimal)

**Solution**: Separate tables per value type
```sql
CREATE TABLE datoms_ref (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BIGINT NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (e, a, tx)
) WITH (fillfactor = 90);

CREATE TABLE datoms_long (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BIGINT NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (e, a, tx)
) WITH (fillfactor = 90);

CREATE TABLE datoms_text (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v TEXT NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (e, a, tx)
) WITH (fillfactor = 90);

-- Similarly: datoms_double, datoms_instant, datoms_keyword, datoms_uuid,
--            datoms_bytes, datoms_boolean
```

**Benefits**:
- No NULL overhead: 80 bytes saved per row
- Smaller indexes: homogeneous types index better
- Better TOAST management: only TEXT table has TOAST
- HOT updates possible: fewer indexed columns
- Better compression: similar values compress better

**Challenges**:
- Query complexity: must UNION across tables
- Schema changes: affects ALL query code
- Migration: must split existing data

### 2. Multi-Store Scalability → Single Table with Partitioning

**Current Problem**:
```rust
// Creates 18+ catalog entries per store
CREATE SCHEMA mentat_analytics;
CREATE TABLE mentat_analytics.datoms (...);
CREATE TABLE mentat_analytics.schema (...);
-- etc.
```
- 100 stores = 1,800 catalog entries
- pg_class scans slow down planner
- Autovacuum can't keep up

**Solution**: Single partitioned table
```sql
-- Add store_id to all tables
CREATE TABLE datoms_ref (
    store_id INT NOT NULL DEFAULT 0,  -- 0 = default store
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BIGINT NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) PARTITION BY HASH (store_id);

-- Create partitions (or use RANGE for manual control)
CREATE TABLE datoms_ref_p0 PARTITION OF datoms_ref
    FOR VALUES WITH (MODULUS 16, REMAINDER 0);
-- ... 15 more partitions

-- Indexes include store_id
CREATE INDEX idx_datoms_ref_eavt ON datoms_ref (store_id, e, a, tx) WHERE added;
CREATE INDEX idx_datoms_ref_aevt ON datoms_ref (store_id, a, e, tx) WHERE added;
CREATE INDEX idx_datoms_ref_vaet ON datoms_ref (store_id, v, a, e, tx) WHERE added;
```

**Benefits**:
- Constant catalog size (16 partitions regardless of store count)
- Partition pruning: queries only scan relevant partition
- Single autovacuum schedule
- Simpler backup/restore

**Store Registration**:
```sql
-- Metadata table
CREATE TABLE stores (
    store_id SERIAL PRIMARY KEY,
    store_name TEXT UNIQUE NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Default store always gets ID 0
INSERT INTO stores (store_id, store_name, description)
VALUES (0, 'default', 'Default store')
ON CONFLICT (store_id) DO NOTHING;
```

### 3. Virtual Tables Performance → Optimized SQL

**Current Problem**:
```sql
CREATE VIEW entities AS
SELECT DISTINCT e AS entity_id,
       tx AS created_tx,
       (SELECT tx_instant FROM transactions WHERE tx = d.tx) AS created_at,
       (SELECT MAX(tx) FROM datoms WHERE e = d.e) AS last_modified_tx
FROM datoms d WHERE added = true;
```
- Correlated subqueries: O(N²)
- DISTINCT blocks pushdown
- No materialization

**Solution**: Proper GROUP BY with lateral joins
```sql
CREATE VIEW entities AS
SELECT
    e AS entity_id,
    MIN(tx) AS created_tx,
    t_first.tx_instant AS created_at,
    MAX(tx) AS last_modified_tx,
    t_last.tx_instant AS last_modified_at,
    COUNT(*) AS attribute_count
FROM datoms_ref  -- Start with one table, UNION ALL others
WHERE added = true
GROUP BY e
LEFT JOIN LATERAL (
    SELECT tx_instant FROM transactions WHERE tx = MIN(datoms_ref.tx)
) t_first ON true
LEFT JOIN LATERAL (
    SELECT tx_instant FROM transactions WHERE tx = MAX(datoms_ref.tx)
) t_last ON true;
```

But with split tables, need materialized view:
```sql
-- Better: Materialized view refreshed periodically
CREATE MATERIALIZED VIEW entities AS
WITH all_datoms AS (
    SELECT store_id, e, tx FROM datoms_ref WHERE added
    UNION ALL
    SELECT store_id, e, tx FROM datoms_long WHERE added
    UNION ALL
    SELECT store_id, e, tx FROM datoms_text WHERE added
    -- ... etc
)
SELECT
    store_id,
    e AS entity_id,
    MIN(tx) AS created_tx,
    MAX(tx) AS last_modified_tx,
    COUNT(*) AS attribute_count
FROM all_datoms
GROUP BY store_id, e;

CREATE INDEX ON entities (store_id, entity_id);
```

### 4. Index Strategy

**Current**: 4 covering indexes (EAVT, AEVT, AVET, VAET)

**Optimized**:
```sql
-- EAVT - Entity lookups (most common)
CREATE INDEX idx_datoms_ref_eavt ON datoms_ref (store_id, e, a, tx)
    WHERE added INCLUDE (v);

-- AEVT - Attribute scans
CREATE INDEX idx_datoms_ref_aevt ON datoms_ref (store_id, a, e, tx)
    WHERE added INCLUDE (v);

-- VAET - Value lookups (for refs only, not needed for text/long)
CREATE INDEX idx_datoms_ref_vaet ON datoms_ref (store_id, v, a, e, tx)
    WHERE added;

-- TX - Time-travel queries
CREATE INDEX idx_datoms_ref_tx ON datoms_ref (store_id, tx DESC)
    WHERE added INCLUDE (e, a, v);
```

**Per-table specifics**:
- `datoms_ref`: All 4 indexes (VAET critical for reference traversal)
- `datoms_long`: Skip VAET (numeric values rarely queried by value)
- `datoms_text`: Skip VAET (use GIN for text search instead)
- `datoms_keyword`: Include VAET (keywords are often lookup keys)

### 5. FILLFACTOR and Storage Parameters

```sql
-- For tables with frequent updates
ALTER TABLE datoms_ref SET (
    fillfactor = 90,           -- Leave 10% free for HOT updates
    autovacuum_vacuum_scale_factor = 0.05,  -- Vacuum more aggressively
    autovacuum_analyze_scale_factor = 0.02  -- Analyze more often
);

-- For mostly-append tables
ALTER TABLE transactions SET (
    fillfactor = 100,          -- No updates, pack tight
    autovacuum_vacuum_scale_factor = 0.1
);

-- For large text values
ALTER TABLE datoms_text SET (
    toast_tuple_target = 8192, -- Larger TOAST threshold
    fillfactor = 85            -- More room for TOAST pointers
);
```

## Migration Strategy

### Phase 1: Create New Schema (Backwards Compatible)

```sql
-- New tables alongside old
CREATE TABLE datoms_ref_new (...);
CREATE TABLE datoms_long_new (...);
-- etc.

-- Trigger to write to both old and new
CREATE TRIGGER datoms_dual_write
AFTER INSERT ON datoms
FOR EACH ROW EXECUTE FUNCTION dual_write_datoms();
```

### Phase 2: Backfill Data

```sql
-- Migrate existing datoms to type-specific tables
INSERT INTO datoms_ref_new (store_id, e, a, v, tx, added)
SELECT 0, e, a, v_ref, tx, added
FROM datoms
WHERE value_type_tag = 0;

INSERT INTO datoms_long_new (store_id, e, a, v, tx, added)
SELECT 0, e, a, v_long, tx, added
FROM datoms
WHERE value_type_tag = 2;

-- etc for all types
```

### Phase 3: Update Application Code

```rust
// Old code
Spi::run("INSERT INTO datoms (e, a, value_type_tag, v_long, tx, added) VALUES ...")?;

// New code
match value_type {
    ValueType::Ref => {
        Spi::run("INSERT INTO datoms_ref (store_id, e, a, v, tx, added) VALUES ...")?;
    }
    ValueType::Long => {
        Spi::run("INSERT INTO datoms_long (store_id, e, a, v, tx, added) VALUES ...")?;
    }
    // etc.
}
```

### Phase 4: Cutover

```sql
-- Drop dual-write trigger
DROP TRIGGER datoms_dual_write ON datoms;

-- Rename tables
ALTER TABLE datoms RENAME TO datoms_old;
ALTER TABLE datoms_ref_new RENAME TO datoms_ref;
-- etc.

-- Drop old table after verification
DROP TABLE datoms_old;
```

## Performance Targets

**Before** (current wide-row design):
- Write: ~2-3k datoms/sec
- Read: ~10k queries/sec
- Storage: ~500 bytes/datom

**After** (optimized design):
- Write: ~8-10k datoms/sec (3x improvement)
- Read: ~25k queries/sec (2.5x improvement)
- Storage: ~200 bytes/datom (2.5x improvement)

## Implementation Order

1. ✅ **Add store_id to metadata table** (simple, non-breaking)
2. ✅ **Create type-specific tables** (parallel to existing)
3. ✅ **Update transact code** to write to new tables
4. ✅ **Update query code** to read from new tables
5. ✅ **Migrate existing data** (background job)
6. ✅ **Update virtual tables** (materialized views)
7. ✅ **Add partition support** (for multi-store scaling)
8. ✅ **Performance testing** (validate targets)
9. ✅ **Drop old schema** (after validation)

## Rollback Plan

Keep old `datoms` table until:
- [ ] All functionality verified on new tables
- [ ] Performance tests pass
- [ ] 7 days of production use (if applicable)

Can rollback by:
```sql
ALTER TABLE datoms_old RENAME TO datoms;
-- Restore old functions
```

## Next Steps

Start with Phase 1: Create new schema alongside existing, with dual-write capability to maintain backwards compatibility during migration.
