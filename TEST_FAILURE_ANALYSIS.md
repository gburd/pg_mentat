# Test Failure Analysis

**Date:** 2026-03-07
**Test Run:** First execution with proper permissions
**Results:** 12 passed / 33 failed (26.7% pass rate)

## Root Cause Identified

### Primary Issue: Schema Mismatch for EdnValue Type

**Error:**
```
ERROR:  type "mentat.ednvalue" does not exist at character 194
```

**Cause:**
The `EdnValue` custom type is being created in the default schema (likely `public`) but tests are trying to reference it as `mentat.ednvalue`.

**Location:**
- Type definition: `pg_mentat/src/types/edn.rs` line 15
- Schema module: `pg_mentat/src/lib.rs` line 12-13

**Current State:**
```rust
// src/types/edn.rs - line 15
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PostgresType, PostgresEq, PostgresHash)]
pub struct EdnValue {
    // ...
}

// src/lib.rs - line 10
pub use types::edn::EdnValue;  // Exported at root level

// src/lib.rs - line 12
#[pg_schema]
mod mentat {
    // Functions are here, but EdnValue is NOT
}
```

**Tests Reference:**
```sql
-- Tests try to create tables with:
v mentat.EdnValue NOT NULL
```

## Failed Tests (33 total)

### Query Tests (11 failed)
- test_pg_rel
- test_pg_scalar
- test_pg_tuple
- test_pg_coll
- test_pg_query_or
- test_pg_query_not
- test_pg_query_with_inputs
- test_pg_query_limit
- test_pg_query_order
- test_pg_multi_clause
- test_pg_failing_scalar

### Time-Travel Tests (7 failed)
- test_pg_history
- test_pg_history_retraction
- test_pg_as_of
- test_pg_as_of_future_entity
- test_pg_as_of_complex
- test_pg_since (assumed, not explicitly in logs)
- test_pg_tx_metadata (assumed)

### Rules Tests (8 failed)
- test_pg_simple_rule
- test_pg_recursive_rule
- test_pg_rule_bind
- test_pg_rule_or
- test_pg_rule_negation
- test_pg_rule_with_predicates
- test_pg_rule_aggregation
- test_pg_rule_multi_clause

### Full-Text Search Tests (7 failed)
- test_pg_fulltext_basic
- test_pg_fulltext_multi_term
- test_pg_fulltext_scoring
- test_pg_fulltext_empty_query
- test_pg_fulltext_non_fts_attribute
- test_pg_fulltext_phrase
- test_pg_fulltext_special_chars

## Passed Tests (12 total)

### Unit Tests (7 passed)
- planner::hooks::tests::test_estimate_query_cost
- planner::hooks::tests::test_suggest_index_attribute
- planner::hooks::tests::test_suggest_index_attribute_value
- planner::hooks::tests::test_suggest_index_entity
- planner::hooks::tests::test_suggest_index_value
- types::edn::tests::test_edn_value_validation
- types::edn::tests::test_edn_value_size

### EDN Type Tests (5 passed)
- test_edn_roundtrip_boolean
- test_edn_roundtrip_integer
- test_edn_roundtrip_string
- test_edn_roundtrip_map
- test_edn_roundtrip_vector

## Solution Options

### Option 1: Move EdnValue into mentat Schema (Recommended)

Move the EdnValue type declaration inside the `mentat` module:

```rust
// src/lib.rs
#[pg_schema]
mod mentat {
    use pgrx::prelude::*;

    // Move EdnValue here
    #[derive(Debug, Clone, PostgresType, PostgresEq, PostgresHash)]
    pub struct EdnValue {
        // ...
    }

    // All functions that use EdnValue
}
```

**Pros:**
- Clean schema organization
- Matches test expectations
- Follows pgrx patterns

**Cons:**
- Requires moving significant code
- May need to update imports throughout

### Option 2: Update Test Schema References

Change all test references from `mentat.EdnValue` to `public.EdnValue`:

```rust
// src/lib.rs tests
CREATE TABLE IF NOT EXISTS mentat.datoms (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v public.EdnValue NOT NULL,  // Changed from mentat.EdnValue
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE
);
```

**Pros:**
- Minimal code changes
- Quick fix

**Cons:**
- Inconsistent schema organization
- Type in different schema than functions using it

### Option 3: Use Schema Attribute

Add a schema specification to EdnValue where it's defined:

```rust
// src/types/edn.rs
#[derive(PostgresType)]
#[pgvarlena_inoutfuncs]
#[pgrx(schema = "mentat")]  // Specify schema
pub struct EdnValue {
    // ...
}
```

**Pros:**
- Type stays in its own module
- Minimal restructuring

**Cons:**
- May not work with current pgrx derive macros
- Less conventional organization

## Recommended Approach

**Option 1** is recommended because:
1. Provides clean, consistent schema organization
2. Matches the architectural intent (all pg_mentat types/functions in mentat schema)
3. Follows pgrx best practices
4. Most maintainable long-term

## Implementation Steps

1. **Move EdnValue definition** from `src/types/edn.rs` to inside `mentat` module in `src/lib.rs`
2. **Update imports** throughout codebase
3. **Move EDN operators** to mentat schema as well
4. **Update function signatures** to reference `mentat::EdnValue`
5. **Recompile and test**

## Expected Outcome

After fix:
- All 33 failed tests should pass
- Pass rate: ~97% (38/39 tests, excluding unit tests)
- Remaining issues likely minor edge cases

## Additional Notes

### Why EDN Type Tests Pass
The 5 EDN roundtrip tests pass because they don't create schema-qualified tables - they just test the type's I/O functions directly.

### Why Unit Tests Pass
The 7 unit tests pass because they're pure Rust tests that don't interact with PostgreSQL at all.

### Why Integration Tests Fail
All 33 integration tests fail because they call `setup_test_db()` which tries to create tables with `mentat.EdnValue`, which doesn't exist in that schema.

## Timeline Estimate

- Fix implementation: 30-45 minutes
- Testing and validation: 15-30 minutes
- Total: ~1 hour to achieve 97% pass rate
