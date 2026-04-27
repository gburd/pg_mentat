# Function Naming Cleanup Plan

## Problem
Current function names are redundant with "mentat_" prefix when already in "mentat" schema.

## Proposed Changes

### Query Functions
Current → New:
- `mentat.mentat_q_store(store, query, inputs)` → `mentat.q(query, inputs, store:=NULL)`
- `mentat.mentat_q_full(store, query, inputs, as_of)` → merge into `mentat.q()` with optional params
- `mentat.mentat_q_default(query, inputs)` → remove (use `mentat.q()` directly)

**Consolidated signature:**
```sql
mentat.q(
  query TEXT,
  inputs JSONB DEFAULT '{}'::jsonb,
  store TEXT DEFAULT NULL,        -- NULL means 'default' store
  as_of_tx BIGINT DEFAULT NULL    -- NULL means current state
) RETURNS Edn
```

### Store Management
- `mentat.mentat_create_store()` → `mentat.create_store()`
- `mentat.mentat_drop_store()` → `mentat.drop_store()`
- `mentat.mentat_list_stores()` → `mentat.list_stores()`
- `mentat.mentat_rename_store()` → `mentat.rename_store()`

### Materialized Views
- `mentat.mentat_materialize()` → `mentat.materialize()`
- `mentat.mentat_refresh()` → `mentat.refresh()`
- `mentat.mentat_drop_matview()` → `mentat.drop_matview()`
- `mentat.mentat_list_matviews()` → `mentat.list_matviews()`

**Why a function instead of SQL syntax?**

SQL syntax would be:
```sql
CREATE MATERIALIZED VIEW mentat_users.active_engineers AS
  SELECT * FROM mentat.q_view('users',
    '[:find ?name ?email ...', '{}'::jsonb);
```

Custom function `mentat.materialize()` provides:
1. **Auto-refresh triggers** - `refresh_policy: 'on_write'` automatically creates triggers
2. **Datalog integration** - Direct Datalog query input, automatic column inference
3. **Metadata tracking** - Tracks which queries power which views
4. **Store-scoped** - Validates store exists, creates view in correct schema
5. **Lifecycle management** - Cleanup of triggers when dropping views

User can still use SQL syntax if desired, but custom function is more convenient.

### Time-Travel
- `mentat.mentat_diff()` → `mentat.diff()`
- `mentat.mentat_diff_default()` → remove (use `mentat.diff()` with default store)
- `mentat.mentat_log()` → `mentat.log()`
- `mentat.mentat_log_default()` → remove

### Subscriptions
- `mentat.mentat_subscribe()` → `mentat.subscribe()`
- `mentat.mentat_unsubscribe()` → `mentat.unsubscribe()`
- `mentat.mentat_list_subscriptions()` → `mentat.list_subscriptions()`

### Recursive Queries
- `mentat.mentat_recursive()` → `mentat.recursive()`
- `mentat.mentat_drop_recursive()` → `mentat.drop_recursive()`
- `mentat.mentat_list_recursive()` → `mentat.list_recursive()`

### Transact
- `mentat.mentat_transact_full(store, tx)` → `mentat.t(tx, store:=NULL)`
- Already have `mentat.t()` for default store, extend it

### Pull/Entity
- `mentat.mentat_pull_in_store()` → `mentat.pull(pattern, eid, store:=NULL)`
- `mentat.mentat_entity_in_store()` → `mentat.entity(eid, store:=NULL)`
- `mentat.mentat_schema_in_store()` → `mentat.schema(store:=NULL)`

## Implementation Strategy

1. Keep old names as deprecated aliases for backwards compatibility
2. Update all internal calls to use new names
3. Update documentation
4. Update tests
5. Add deprecation warnings to old functions

## Example After Cleanup

```sql
-- Query with store
SELECT mentat.q('[:find ?name ...]', store := 'users');

-- Query with time-travel
SELECT mentat.q('[:find ?name ...]', as_of_tx := 268435500);

-- Query with both
SELECT mentat.q('[:find ?name ...]',
                store := 'users',
                as_of_tx := 268435500);

-- Materialized view
SELECT mentat.materialize('users', 'active_engineers',
  '[:find ?name ?email ...]',
  refresh_policy := 'on_write');

-- Subscribe
SELECT mentat.subscribe('users', 'user_changes',
  '[:find ?name ...]');

-- Time-travel diff
SELECT mentat.diff('users', 100, 200, '[:find ?name ...]');
```

## Files to Modify

1. `src/functions/query.rs` - Consolidate q functions
2. `src/functions/transact.rs` - Update t() function
3. `src/functions/pull.rs` - Update pull functions
4. `src/functions/entity.rs` - Update entity function
5. `src/functions/schema.rs` - Update schema function
6. `src/functions/store_management.rs` - Rename all functions
7. `src/functions/materialized_views.rs` - Rename all functions
8. `src/functions/time_travel.rs` - Rename and consolidate
9. `src/functions/subscriptions.rs` - Rename all functions
10. `src/functions/recursive_queries.rs` - Rename all functions
11. All test files - Update function calls
12. Documentation - Update examples
