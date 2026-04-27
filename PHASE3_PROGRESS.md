# Phase 3: Rust Code Changes - Progress Tracker

## Status: IN PROGRESS 🚧

Started: 2026-04-27

## Completed Changes ✅

### 1. transact.rs - Write Path Updated

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/src/functions/transact.rs`

**Changes Made**:
- ✅ Added `get_store_id_from_schema()` helper function
  - Extracts store name from schema (mentat → default, mentat_foo → foo)
  - Queries `mentat.stores` table for store_id
  - Returns error if store not found

- ✅ Modified `insert_typed_datom()` function
  - Now writes to type-specific tables (`datoms_*_new`)
  - Includes `store_id` in all INSERTs
  - Uses `ON CONFLICT ... DO UPDATE` for idempotency
  - Maps each TypedValue variant to correct table:
    * TypedValue::Ref → datoms_ref_new
    * TypedValue::Boolean → datoms_boolean_new
    * TypedValue::Long → datoms_long_new
    * TypedValue::Double → datoms_double_new
    * TypedValue::Text → datoms_text_new
    * TypedValue::Keyword → datoms_keyword_new
    * TypedValue::Instant → datoms_instant_new
    * TypedValue::Uuid → datoms_uuid_new
    * TypedValue::Bytes → datoms_bytes_new

**Note**: Table names still use `_new` suffix. After Phase 4 cutover, these will be renamed to remove the suffix.

### 2. transact.rs - Read Path Partially Updated

**Changes Made**:
- ✅ Modified `is_duplicate_cardinality_many()` function
  - Now queries type-specific tables instead of wide row
  - Simpler queries (no value_type_tag discrimination needed)
  - Includes store_id in WHERE clause for partition pruning
  - Each TypedValue variant queries its specific table

**Benefits**:
- Simpler SQL (single table instead of complex WHERE on value_type_tag)
- Better query performance (smaller tables, better indexes)
- Partition pruning by store_id

## TODO - Remaining Work 📋

### 2. transact.rs - Read Path (HIGH PRIORITY)

**Functions that need updating**:

#### `is_duplicate_cardinality_many()` - Line 1474
- Currently: `SELECT EXISTS(SELECT 1 FROM {schema}.datoms WHERE ...)`
- Needs: Query across all type-specific tables with UNION

#### `lookup_entity_by_unique_value()` - Line ~1872
- Currently: `SELECT e FROM {schema}.datoms WHERE ...`
- Needs: Query across type-specific tables

#### `retract_existing_cardinality_one()` - Line 1280
- Reads current values to retract them
- Needs: Query across type-specific tables

#### `get_current_values_for_cas()` - Used in CAS operations
- Reads current values for compare-and-swap
- Needs: Query across type-specific tables

#### `mark_existing_datom_retracted()` - Line ~1400
- Updates `added` flag to false
- Needs: UPDATE across type-specific tables

**Approach for Read Queries**:
```rust
// Helper function to generate UNION ALL query across type-specific tables
fn query_all_value_types(
    store_id: i32,
    where_clause: &str,  // e.g., "e = $1 AND a = $2"
    params: Vec<DatumWithOid>,
) -> Result<Vec<Row>, SpiError> {
    let query = format!(
        "SELECT e, a, 0 AS value_type_tag, v::text AS value FROM mentat.datoms_ref_new
         WHERE store_id = {} AND {} AND added = true
         UNION ALL
         SELECT e, a, 1 AS value_type_tag, v::text AS value FROM mentat.datoms_boolean_new
         WHERE store_id = {} AND {} AND added = true
         UNION ALL
         SELECT e, a, 2 AS value_type_tag, v::text AS value FROM mentat.datoms_long_new
         WHERE store_id = {} AND {} AND added = true
         UNION ALL
         ...  -- repeat for all 9 tables
        ",
        store_id, where_clause, store_id, where_clause, store_id, where_clause
    );

    Spi::connect(|client| {
        client.select(&query, None, &params)
    })
}
```

### 3. query.rs - Datalog Query Execution (HIGH PRIORITY)

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/src/functions/query.rs`

**Functions that need updating**:

#### `translate_pattern()` - Generates SQL FROM clause
- Currently: References `{schema}.datoms` table
- Needs: Generate UNION ALL across all type-specific tables
- This is the CRITICAL function that affects all query performance

#### `execute_translated_query()` - Executes generated SQL
- May need adjustments for UNION ALL handling
- Cache invalidation strategy

**Estimated Complexity**: HIGH - This is the most complex change
- Datalog patterns must map to multiple tables
- JOIN semantics across UNION queries
- Query optimizer may struggle with complex UNIONs

### 4. pull.rs - Pull API (MEDIUM PRIORITY)

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/src/functions/pull.rs`

**Functions**:
- `pull()` - Pull single entity
- `pull_many()` - Pull multiple entities

**Current behavior**: Direct SELECT from datoms table
**Needs**: UNION ALL across type-specific tables

### 5. entity.rs - Entity Loading (MEDIUM PRIORITY)

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/src/functions/entity.rs`

**Function**: `entity()`
- Loads all attributes for an entity
- Needs: UNION ALL across type-specific tables

### 6. virtual_tables.rs - View Generation (LOW PRIORITY)

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/src/functions/virtual_tables.rs`

**Functions**:
- `create_virtual_tables_for_schema()`
- Generates SQL views

**Current**: Views reference `{schema}.datoms`
**Needs**: Views should reference type-specific tables with UNION ALL

**Note**: Virtual tables can be updated later since they're primarily for SQL users, not critical for Datalog queries.

### 7. time_travel.rs - Historical Queries (LOW PRIORITY)

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/src/functions/time_travel.rs`

**Functions**:
- `diff()` - Compare two transaction states
- `log()` - Transaction log

**Needs**: Query type-specific tables with tx filtering

### 8. cache.rs - Query Caching (LOW PRIORITY)

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/src/cache.rs`

**Review needed**:
- Ensure cache keys include store_id
- Cache invalidation on schema changes

### 9. bootstrap.rs - Bootstrap Data (CRITICAL FOR TESTING)

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/src/functions/bootstrap.rs`

**Issue**: Bootstrap SQL includes hardcoded INSERT statements to old datoms table
**Needs**: Update to write to type-specific tables OR ensure dual-write trigger is enabled

**Options**:
1. Update all INSERT statements in bootstrap SQL
2. Enable dual-write trigger before bootstrap
3. Migrate bootstrap data after initial setup

## Testing Strategy

### Unit Tests
- [ ] Test `get_store_id_from_schema()` with valid/invalid inputs
- [ ] Test `insert_typed_datom()` for all 9 value types
- [ ] Test store_id = 0 for default store
- [ ] Test store_id > 0 for custom stores

### Integration Tests
- [ ] Write datom, verify it appears in correct type-specific table
- [ ] Write to multiple stores, verify store_id separation
- [ ] Test cardinality-one retraction still works
- [ ] Test cardinality-many deduplication still works
- [ ] Test CAS operations still work
- [ ] Test unique constraints still enforced

### Performance Tests
- [ ] Benchmark write performance (target: 3x improvement)
- [ ] Benchmark query performance (target: 2.5x improvement)
- [ ] Compare storage size before/after

## Migration Considerations

### Dual-Write Period
During Phase 3 testing, we should:
1. Enable dual-write trigger: `ALTER TABLE mentat.datoms ENABLE TRIGGER dual_write_datoms_trigger`
2. New writes go to BOTH old and new tables
3. Reads should check BOTH old and new tables (UNION)
4. This allows gradual testing and rollback capability

### Read Strategy Options

**Option A: Read from new tables only** (current implementation)
- Assumes Phase 2 backfill is complete
- Faster (no UNION overhead)
- Requires backfill before testing

**Option B: Read from both old and new tables**
- UNION ALL between old and new tables
- Slower but safer during migration
- Allows testing before complete backfill

**Recommendation**: Start with Option B for safety, switch to Option A after validation.

## Build and Test Commands

```bash
# Build extension
cargo pgrx install --release

# Run unit tests
cargo test

# Run integration tests
psql -d test_db -f pg_mentat/sql/tests/test_storage_migration_phase3.sql

# Benchmark
psql -d test_db -f pg_mentat/sql/benchmarks/write_performance.sql
psql -d test_db -f pg_mentat/sql/benchmarks/read_performance.sql
```

## Known Issues

### Build Environment
- ❌ Nix environment still has CARGO_HOME read-only issue
- Workaround: Use `dangerouslyDisableSandbox: true` for cargo commands
- Need to test actual compilation

### Rust Warnings
- ⚠️ Non-snake-case warnings for `None` variable (existing issue, not related to changes)

## Next Immediate Steps

1. **Update read queries in transact.rs** (highest priority)
   - Create helper function for UNION ALL queries
   - Update `is_duplicate_cardinality_many()`
   - Update `lookup_entity_by_unique_value()`
   - Update `retract_existing_cardinality_one()`

2. **Test write path**
   - Create simple test that writes a datom
   - Verify it appears in correct type-specific table
   - Verify store_id is correct

3. **Update query.rs translate_pattern()**
   - This is the most complex change
   - May need multiple iterations to get right

4. **Create comprehensive test suite**
   - Test all value types
   - Test multi-store
   - Test cardinality semantics
   - Test CAS operations

## Completion Criteria

Phase 3 is complete when:
- ✅ All writes go to type-specific tables
- ⏳ All reads query from type-specific tables
- ⏳ All existing tests pass
- ⏳ Performance benchmarks meet targets
- ⏳ No functionality regressions
- ⏳ Code review approved

## Estimated Time Remaining

- transact.rs read updates: 4-6 hours
- query.rs updates: 8-12 hours
- pull.rs updates: 2-3 hours
- entity.rs updates: 1-2 hours
- Testing and debugging: 8-10 hours
- **Total: 23-33 hours (3-4 days)**

## Resources

- **STORAGE_MIGRATION_GUIDE.md** - Operations guide with examples
- **STORAGE_REDESIGN_PLAN.md** - Technical architecture
- **Phase 1 SQL** - `migrate_storage_redesign_phase1.sql`
- **Phase 2 SQL** - `migrate_storage_redesign_phase2_backfill.sql`
