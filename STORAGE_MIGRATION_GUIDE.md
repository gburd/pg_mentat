# Storage Layer Migration Guide

This guide walks through the storage layer redesign migration for pg_mentat, addressing critical performance issues identified by PostgreSQL extension experts.

## Overview

The migration transforms pg_mentat's storage from a wide-row pattern to type-specific tables, eliminating NULL overhead and adding multi-store support via `store_id`.

### Problems Being Fixed

1. **Wide Row Anti-Pattern**: Current schema has 11 nullable columns per row, causing ~80 bytes of NULL overhead
2. **Suboptimal Indexing**: Mixed-type indexes are less efficient than homogeneous type indexes
3. **TOAST Fragmentation**: All tables get TOAST overhead, even numeric ones
4. **Multi-Store Scalability**: Each store creates 18+ catalog entries, causing catalog bloat

### Benefits After Migration

- **3x write performance improvement** (2-3k → 8-10k datoms/sec)
- **2.5x read performance improvement** (10k → 25k queries/sec)
- **2.5x storage reduction** (500 → 200 bytes/datom)
- **Constant catalog size** regardless of store count
- **Better compression** from homogeneous value types
- **HOT updates enabled** with fewer indexed columns

## Migration Phases

### Phase 1: Create New Schema (✓ Complete)

**Status**: SQL files created, ready to deploy

**Actions**:
1. Creates new type-specific tables alongside existing ones
2. Adds `store_id` column to metadata
3. Creates optimized indexes per table type
4. Sets up dual-write trigger (disabled by default)

**To Deploy**:
```sql
-- Apply Phase 1 migration
\i pg_mentat/sql/migrate_storage_redesign_phase1.sql

-- Verify new tables exist
\dt mentat.datoms_*_new

-- Check migration status
SELECT * FROM mentat.storage_migration_status;
```

**Rollback**: Simply drop the `*_new` tables if needed:
```sql
DROP TABLE mentat.datoms_ref_new CASCADE;
DROP TABLE mentat.datoms_long_new CASCADE;
-- ... etc
```

### Phase 2: Backfill Data (Ready to Execute)

**Status**: Script ready, requires Phase 1 completion

**Actions**:
1. Copies all existing datoms to new tables
2. Maps all data to `store_id = 0` (default store)
3. Reports progress and validates counts

**To Execute**:
```sql
-- Backfill all data (may take time depending on dataset size)
\i pg_mentat/sql/migrate_storage_redesign_phase2_backfill.sql

-- Monitor progress
SELECT * FROM mentat.show_migration_progress();

-- Check for discrepancies
SELECT table_name, old_count, new_count,
       (old_count - new_count) AS missing_rows
FROM mentat.show_migration_progress()
WHERE old_count <> new_count;
```

**Time Estimates**:
- 1M datoms: ~30 seconds
- 10M datoms: ~5 minutes
- 100M datoms: ~1 hour

**Rollback**: Truncate new tables:
```sql
TRUNCATE mentat.datoms_ref_new CASCADE;
TRUNCATE mentat.datoms_long_new CASCADE;
-- ... etc
```

### Phase 3: Update Application Code (TODO)

**Status**: Requires Rust code changes

**Actions**:
1. Modify `transact.rs` to write to new tables
2. Modify `query.rs` to read from new tables
3. Update `pull.rs`, `entity.rs`, and other consumers
4. Add `store_id` parameter handling

**Example Changes**:

**Before** (transact.rs):
```rust
Spi::run(&format!(
    "INSERT INTO mentat.datoms (e, a, value_type_tag, v_long, tx, added)
     VALUES ({}, {}, 2, {}, {}, true)",
    e, a, value, tx
))?;
```

**After** (transact.rs):
```rust
// Write to type-specific table
match value_type {
    ValueType::Long => {
        Spi::run(&format!(
            "INSERT INTO mentat.datoms_long_new (store_id, e, a, v, tx, added)
             VALUES ({}, {}, {}, {}, {}, true)",
            store_id, e, a, value, tx
        ))?;
    }
    ValueType::Ref => {
        Spi::run(&format!(
            "INSERT INTO mentat.datoms_ref_new (store_id, e, a, v, tx, added)
             VALUES ({}, {}, {}, {}, {}, true)",
            store_id, e, a, value, tx
        ))?;
    }
    // ... other types
}
```

**Query Changes** (query.rs):
```rust
// Old: Query single wide table
let query = format!(
    "SELECT e, a, value_type_tag, v_ref, v_long, v_text, ...
     FROM mentat.datoms
     WHERE e = {} AND added = true",
    entity_id
);

// New: Union across type-specific tables
let query = format!(
    "SELECT e, a, 0 AS value_type_tag, v AS value
     FROM mentat.datoms_ref_new
     WHERE store_id = {} AND e = {} AND added = true
     UNION ALL
     SELECT e, a, 2 AS value_type_tag, v::text AS value
     FROM mentat.datoms_long_new
     WHERE store_id = {} AND e = {} AND added = true
     UNION ALL
     SELECT e, a, 7 AS value_type_tag, v AS value
     FROM mentat.datoms_text_new
     WHERE store_id = {} AND e = {} AND added = true",
    store_id, entity_id, store_id, entity_id, store_id, entity_id
);
```

**Files to Modify**:
- `pg_mentat/src/functions/transact.rs`
- `pg_mentat/src/functions/query.rs`
- `pg_mentat/src/functions/pull.rs`
- `pg_mentat/src/functions/entity.rs`
- `pg_mentat/src/functions/virtual_tables.rs`
- `pg_mentat/src/functions/time_travel.rs`
- `pg_mentat/src/functions/subscriptions.rs`
- `pg_mentat/src/cache.rs` (if caching datom rows)

**Testing**:
```bash
# Run test suite
cargo pgrx test

# Integration tests
psql -f pg_mentat/sql/tests/test_storage_migration.sql
```

### Phase 4: Cutover (After Verification)

**Status**: Requires Phase 3 completion and 7-day soak test

**Actions**:
1. Disable dual-write trigger
2. Rename new tables to replace old ones
3. Drop old tables
4. Update documentation

**To Execute**:
```sql
-- 1. Disable dual-write (if enabled)
ALTER TABLE mentat.datoms DISABLE TRIGGER dual_write_datoms_trigger;

-- 2. Rename old tables for safety
ALTER TABLE mentat.datoms RENAME TO datoms_old;
ALTER TABLE mentat.datoms_ref RENAME TO datoms_ref_old;
ALTER TABLE mentat.datoms_long RENAME TO datoms_long_old;
-- ... etc for all partitions

-- 3. Rename new tables to production names
ALTER TABLE mentat.datoms_ref_new RENAME TO datoms_ref;
ALTER TABLE mentat.datoms_long_new RENAME TO datoms_long;
ALTER TABLE mentat.datoms_text_new RENAME TO datoms_text;
ALTER TABLE mentat.datoms_double_new RENAME TO datoms_double;
ALTER TABLE mentat.datoms_instant_new RENAME TO datoms_instant;
ALTER TABLE mentat.datoms_keyword_new RENAME TO datoms_keyword;
ALTER TABLE mentat.datoms_uuid_new RENAME TO datoms_uuid;
ALTER TABLE mentat.datoms_bytes_new RENAME TO datoms_bytes;
ALTER TABLE mentat.datoms_boolean_new RENAME TO datoms_boolean;

-- 4. Update index names
-- (Indexes automatically rename with tables)

-- 5. Mark cutover complete
UPDATE mentat.storage_migration_status
SET completed_at = NOW()
WHERE phase = 4;

-- 6. After 7 days of successful operation, drop old tables
-- DROP TABLE mentat.datoms_old CASCADE;
-- DROP TABLE mentat.datoms_ref_old;
-- DROP TABLE mentat.datoms_long_old;
-- -- ... etc
```

**Rollback (if issues found)**:
```sql
-- Revert renames
ALTER TABLE mentat.datoms_ref RENAME TO datoms_ref_new;
ALTER TABLE mentat.datoms_long RENAME TO datoms_long_new;
-- ... etc

ALTER TABLE mentat.datoms_old RENAME TO datoms;
ALTER TABLE mentat.datoms_ref_old RENAME TO datoms_ref;
-- ... etc

-- Revert application code to previous version
```

## Verification Steps

After each phase, run these checks:

### Phase 1 Verification
```sql
-- Check tables exist
SELECT tablename FROM pg_tables
WHERE schemaname = 'mentat' AND tablename LIKE 'datoms%new'
ORDER BY tablename;

-- Check indexes
SELECT indexname FROM pg_indexes
WHERE schemaname = 'mentat' AND tablename LIKE 'datoms%new'
ORDER BY indexname;

-- Verify trigger exists but is disabled
SELECT tgname, tgenabled
FROM pg_trigger
WHERE tgname = 'dual_write_datoms_trigger';
-- Should show: tgenabled = 'D' (disabled)
```

### Phase 2 Verification
```sql
-- Check row counts match
SELECT * FROM mentat.show_migration_progress();
-- All pct_complete should be 100.00

-- Spot check a few entities
SELECT e, a, v FROM mentat.datoms_long_new WHERE e = 100 LIMIT 5;
SELECT e, a, v_long FROM mentat.datoms WHERE e = 100 AND value_type_tag = 2 LIMIT 5;
-- Should return identical data

-- Check storage size
SELECT
    pg_size_pretty(pg_total_relation_size('mentat.datoms')) AS old_size,
    pg_size_pretty(
        pg_total_relation_size('mentat.datoms_ref_new') +
        pg_total_relation_size('mentat.datoms_long_new') +
        pg_total_relation_size('mentat.datoms_text_new') +
        pg_total_relation_size('mentat.datoms_double_new') +
        pg_total_relation_size('mentat.datoms_instant_new') +
        pg_total_relation_size('mentat.datoms_keyword_new') +
        pg_total_relation_size('mentat.datoms_uuid_new') +
        pg_total_relation_size('mentat.datoms_bytes_new') +
        pg_total_relation_size('mentat.datoms_boolean_new')
    ) AS new_size;
-- New size should be significantly smaller
```

### Phase 3 Verification
```sql
-- Run full test suite
\i pg_mentat/sql/tests/test_query.sql
\i pg_mentat/sql/tests/test_transact.sql
\i pg_mentat/sql/tests/test_pull.sql

-- Performance test
\timing on
SELECT mentat.q('[:find ?e ?name :where [?e :person/name ?name]]', '{}'::jsonb);
-- Should be faster than before

-- Verify writes go to new tables
SELECT COUNT(*) FROM mentat.datoms_long_new;
-- Take note of count
SELECT mentat.t('[{:db/id "temp" :person/age 42}]');
SELECT COUNT(*) FROM mentat.datoms_long_new;
-- Should have increased by 1
```

### Phase 4 Verification
```sql
-- Check old tables are renamed/dropped
\dt mentat.datoms*

-- Run full integration test suite
-- (Ensure no references to old tables remain)

-- Monitor performance for 7 days
SELECT * FROM pg_stat_user_tables
WHERE schemaname = 'mentat' AND relname LIKE 'datoms%'
ORDER BY seq_scan DESC, idx_scan DESC;
```

## Performance Benchmarks

### Expected Improvements

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| Write throughput | 2-3k datoms/sec | 8-10k datoms/sec | **3x** |
| Read throughput | 10k queries/sec | 25k queries/sec | **2.5x** |
| Storage per datom | ~500 bytes | ~200 bytes | **2.5x** |
| Index size | Large | Smaller | **30-40%** |
| VACUUM time | Slow | Fast | **50%** |

### Benchmark Queries

**Write Performance**:
```sql
\timing on
SELECT mentat.t('[
    {:db/id "e1" :person/name "Alice" :person/age 30}
    {:db/id "e2" :person/name "Bob" :person/age 25}
    -- ... 1000 entities
]');
```

**Read Performance**:
```sql
\timing on
SELECT mentat.q('[:find ?e ?name ?age
                  :where
                  [?e :person/name ?name]
                  [?e :person/age ?age]
                  [(> ?age 25)]]', '{}'::jsonb);
```

**Storage Check**:
```sql
SELECT
    schemaname,
    tablename,
    pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) AS total_size,
    pg_size_pretty(pg_relation_size(schemaname||'.'||tablename)) AS table_size,
    pg_size_pretty(pg_indexes_size(schemaname||'.'||tablename)) AS index_size
FROM pg_tables
WHERE schemaname = 'mentat' AND tablename LIKE 'datoms%'
ORDER BY pg_total_relation_size(schemaname||'.'||tablename) DESC;
```

## Rollback Plan

Each phase can be rolled back independently:

### Phase 1 Rollback
```sql
-- Drop all new tables
DROP TABLE IF EXISTS mentat.datoms_ref_new CASCADE;
DROP TABLE IF EXISTS mentat.datoms_long_new CASCADE;
DROP TABLE IF EXISTS mentat.datoms_text_new CASCADE;
DROP TABLE IF EXISTS mentat.datoms_double_new CASCADE;
DROP TABLE IF EXISTS mentat.datoms_instant_new CASCADE;
DROP TABLE IF EXISTS mentat.datoms_keyword_new CASCADE;
DROP TABLE IF EXISTS mentat.datoms_uuid_new CASCADE;
DROP TABLE IF EXISTS mentat.datoms_bytes_new CASCADE;
DROP TABLE IF EXISTS mentat.datoms_boolean_new CASCADE;

-- Remove migration tracking
UPDATE mentat.storage_migration_status SET started_at = NULL, completed_at = NULL WHERE phase = 1;
```

### Phase 2 Rollback
```sql
-- Truncate new tables (keep schema)
TRUNCATE TABLE mentat.datoms_ref_new CASCADE;
TRUNCATE TABLE mentat.datoms_long_new CASCADE;
-- ... etc

UPDATE mentat.storage_migration_status SET started_at = NULL, completed_at = NULL WHERE phase = 2;
```

### Phase 3 Rollback
```bash
# Revert code changes via git
git checkout HEAD~1 -- pg_mentat/src/functions/transact.rs
git checkout HEAD~1 -- pg_mentat/src/functions/query.rs
# ... etc

# Rebuild extension
cargo pgrx install --release
```

### Phase 4 Rollback
```sql
-- Restore old tables (if not yet dropped)
ALTER TABLE mentat.datoms_ref RENAME TO datoms_ref_new;
ALTER TABLE mentat.datoms_long RENAME TO datoms_long_new;
-- ... etc

ALTER TABLE mentat.datoms_old RENAME TO datoms;
ALTER TABLE mentat.datoms_ref_old RENAME TO datoms_ref;
-- ... etc

-- Revert application code
```

## Troubleshooting

### Issue: Backfill is slow

**Solution**: Run in batches:
```sql
-- Instead of inserting all at once, batch by entity range
INSERT INTO mentat.datoms_long_new (store_id, e, a, v, tx, added)
SELECT 0, e, a, v_long, tx, added
FROM mentat.datoms
WHERE value_type_tag = 2 AND e >= 0 AND e < 100000;

-- Repeat with e >= 100000 AND e < 200000, etc.
```

### Issue: Missing rows after backfill

**Diagnosis**:
```sql
SELECT * FROM mentat.show_migration_progress()
WHERE old_count <> new_count;
```

**Solution**: Re-run backfill (ON CONFLICT DO NOTHING prevents duplicates):
```sql
\i pg_mentat/sql/migrate_storage_redesign_phase2_backfill.sql
```

### Issue: Queries return wrong results after Phase 3

**Diagnosis**: Likely UNION query construction error

**Solution**: Check query.rs UNION ALL logic, ensure all value types included

### Issue: Performance degradation after Phase 4

**Diagnosis**: Might need ANALYZE or index rebuild

**Solution**:
```sql
VACUUM ANALYZE mentat.datoms_ref;
VACUUM ANALYZE mentat.datoms_long;
-- ... etc

-- Or reindex
REINDEX TABLE mentat.datoms_ref;
REINDEX TABLE mentat.datoms_long;
-- ... etc
```

## Timeline

Recommended timeline for production migration:

- **Week 1**: Deploy Phase 1 to staging, verify tables created
- **Week 2**: Run Phase 2 backfill on staging, validate counts
- **Week 3-4**: Implement Phase 3 code changes, test thoroughly
- **Week 5**: Deploy Phase 3 to staging, run full test suite
- **Week 6**: Deploy to production, enable dual-write
- **Week 7-8**: Monitor production, collect metrics
- **Week 9**: Execute Phase 4 cutover
- **Week 10-12**: 7-day soak test, then drop old tables

Total: **~3 months** from start to completion

## Success Criteria

Before proceeding to next phase:

**Phase 1**:
- [ ] All new tables created successfully
- [ ] All indexes present
- [ ] Trigger created but disabled
- [ ] No impact on existing queries

**Phase 2**:
- [ ] All row counts match (100% complete)
- [ ] Spot checks show identical data
- [ ] Storage size reduced by ~40-50%
- [ ] ANALYZE completed on all tables

**Phase 3**:
- [ ] All tests pass
- [ ] Performance benchmarks meet targets (3x write, 2.5x read)
- [ ] No regressions in functionality
- [ ] Code review approved

**Phase 4**:
- [ ] 7-day soak test successful
- [ ] No critical bugs reported
- [ ] Performance improvements sustained
- [ ] Old tables backed up before drop

## Next Steps

1. **Review this migration guide** with team
2. **Deploy Phase 1** to development environment
3. **Run Phase 2 backfill** and verify counts
4. **Begin Phase 3 code changes** in feature branch
5. **Test thoroughly** before production deployment

## Questions?

Consult:
- `STORAGE_REDESIGN_PLAN.md` for architectural details
- Expert reviews in conversation history
- PostgreSQL partition documentation: https://www.postgresql.org/docs/current/ddl-partitioning.html
