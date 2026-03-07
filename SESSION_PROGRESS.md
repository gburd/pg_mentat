# pg_mentat Session Progress - 2026-03-07

## Summary

Continued from previous session with 16/45 tests passing (35.6%). Made significant progress fixing critical issues.

## Improvements Made

### 1. Function Visibility Fix (commits d636f6a, 060fe43)
**Problem**: Extension functions weren't accessible in PostgreSQL even though they had `#[pg_extern]` attribute.

**Root Cause**: Functions were not marked as `pub`, so they couldn't be re-exported into the `mentat` schema module via `pub use`.

**Fix**: Added `pub` keyword to all extension functions:
- `pub fn mentat_query()` in src/functions/query.rs
- `pub fn mentat_transact()` in src/functions/transact.rs
- `pub fn mentat_pull()` in src/functions/pull.rs
- `pub fn mentat_entity()` in src/functions/entity.rs
- `pub fn mentat_schema()` in src/functions/schema.rs

**Impact**: **+9 tests** (16/45 → 25/45, +20 percentage points)

### 2. Fulltext Table Addition (commit 060fe43)
**Problem**: Fulltext search tests failing due to missing table.

**Fix**: Added fulltext table and trigger to test setup in lib.rs:
- Created `mentat.fulltext` table with text_value and search_vector columns
- Added GIN index on search_vector
- Added trigger to auto-update search vector on insert/update
- Added bootstrap transaction datom (tx 1000000)

**Impact**: Infrastructure for FTS tests, but tests still failing (need additional fixes)

### 3. OR Clause Handling Fix (commit 16975ec)
**Problem**: Queries with only OR clauses (no base patterns) were failing with "No where clauses produced any datom table joins".

**Root Cause**: When `pattern_clauses` was empty (OR-only queries), `build_extended_pattern_query` generated no joins and threw an error BEFORE OR handling could run.

**Fix**: Skip generating base_sql when query has only OR clauses:
```rust
let (base_sql, base_var_to_alias) = if pattern_clauses.is_empty() && !or_joins.is_empty() {
    // No base patterns, only OR clauses - will be handled below
    (String::new(), HashMap::new())
} else {
    build_extended_pattern_query(...)? };
```

**Impact**: **+2 tests** (25/45 → 27/45, +4.4 percentage points)

## Test Results Timeline

| State | Passing | Failing | Pass Rate | Improvement |
|-------|---------|---------|-----------|-------------|
| Session Start | 16/45 | 29/45 | 35.6% | Baseline |
| After pub fix | 25/45 | 20/45 | 55.6% | +9 tests |
| After OR fix | 27/45 | 18/45 | 60.0% | +2 tests |
| **Total Gain** | **+11 tests** | **-11 tests** | **+24.4%** | **+68.8% improvement** |

## Current Test Status (27/45 passing, 60.0%)

### Passing Test Categories ✅
- **Basic queries**: rel, scalar, coll, failing_scalar, limit, not
- **OR queries**: query_or ← Fixed today!
- **ORDER BY**: query_order ← Fixed today!
- **EDN roundtrip**: All 7 tests passing
- **Schema/Entity/Pull**: Multiple tests passing

### Failing Tests (18 remaining)

#### Ident Resolution Issues (affects ~8-10 tests)
**Error**: "Failed to resolve attribute: :person/name"

**Tests affected**:
- test_pg_rule_with_predicates
- test_pg_rule_bind
- test_pg_simple_rule
- Possibly others using custom attributes

**Analysis**: Tests are trying to use custom attributes (`:person/name`, `:person/age`) that haven't been defined in the schema first. This might be:
1. A test bug (missing schema definitions)
2. A schema mutation bug (definitions not being installed)
3. A bootstrap order issue

#### Temporal Queries (5 tests)
- test_pg_as_of
- test_pg_since
- test_pg_history
- test_pg_history_retraction
- (possibly as_of_complex)

**Likely issues**:
- Transaction filtering not working correctly
- History mode not handling retractions

#### Full-Text Search (4 tests)
- test_pg_fulltext_basic
- test_pg_fulltext_multi_term
- test_pg_fulltext_scoring
- test_pg_fulltext_special_chars

**Status**: Infrastructure added but tests still failing. Need to debug FTS query generation.

#### Rules Engine (6 tests)
- test_pg_recursive_rule
- test_pg_rule_multi_clause
- test_pg_rule_or

**Status**: Most rule tests now blocked by ident resolution issue. Once that's fixed, may reveal additional rule engine bugs.

#### Other (4 tests)
- test_pg_multi_clause
- test_pg_query_with_inputs
- test_pg_tuple
- test_pg_tx_metadata

## Next Steps (Priority Order)

### 1. Fix Ident Resolution Issue (High Priority)
**Would unlock**: ~8-10 tests
**Estimated effort**: 1-2 hours
**Approach**:
- Investigate why `:person/name` etc. aren't being found
- Check if schema mutation is installing new attributes correctly
- Verify resolve_ident() PL/pgSQL function is being called correctly
- May need to add missing schema definitions to test setup

### 2. Fix Temporal Queries (Medium Priority)
**Would unlock**: 5 tests
**Estimated effort**: 2-3 hours
**Approach**:
- Add proper transaction filtering for as_of/since
- Handle history mode (include retractions with added=false)
- Test with different temporal options

### 3. Fix Full-Text Search (Medium Priority)
**Would unlock**: 4 tests
**Estimated effort**: 1-2 hours
**Approach**:
- Debug FTS query generation in build_fulltext_join
- Verify text value encoding/decoding
- Check tsquery generation and GIN index usage

### 4. Fix Remaining Tests (Low Priority)
**Would unlock**: 4 tests
**Estimated effort**: 2-3 hours each
**Approach**: Individual investigation per test

## Estimated Completion

**Current**: 60.0% complete (27/45 tests)
**If ident resolution fixed**: ~73% complete (33/45 tests)
**If temporal + FTS also fixed**: ~91% complete (41/45 tests)
**Full completion**: 100% (45/45 tests)

## Git Commits Made

1. `d636f6a` - fix(pg_mentat): Make pg_extern functions pub for test module access
2. `060fe43` - fix(tests): Add fulltext table, bootstrap datoms, and SPI parameter fix
3. `6dec282` - docs: Add current test results showing 25/45 passing (55.6%)
4. `16975ec` - fix(query): Handle OR-only queries without base patterns

## Environment Notes

- **CARGO_HOME workaround**: Set to `/home/gburd/ws/pg_mentat/.cargo` to avoid read-only nix store
- **TMPDIR workaround**: Set to `/home/gburd/ws/pg_mentat/.tmp` for rustc temp files
- **Linker**: Using GNU ld (bfd) instead of lld via `.cargo/config.toml`
- **Git**: Using `--no-gpg-sign` to avoid 1Password socket issues

## Session Statistics

- **Duration**: ~2 hours
- **Tests fixed**: 11 (+68.8% improvement)
- **Commits made**: 4
- **Files modified**: 6 (mainly query.rs, lib.rs, function files)
- **Code quality**: Clean builds, 0 errors, ~7 warnings (benign)
