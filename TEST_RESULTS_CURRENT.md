# pg_mentat Test Results - Current Status

**Date**: 2026-03-07
**Session**: Post-pub fix + fulltext table additions

## Test Pass Rate

**Current**: 25/45 passing (55.6%)
**Previous**: 16/45 passing (35.6%)
**Improvement**: +9 tests (+20 percentage points)

## Summary

Major fix: Added `pub` keyword to all `#[pg_extern]` functions so they can be properly re-exported into the `mentat` schema module. Without `pub`, the functions weren't accessible even though they had the `#[pg_extern]` attribute.

### Fixed Functions
- `pub fn mentat_query()` in src/functions/query.rs
- `pub fn mentat_transact()` in src/functions/transact.rs
- `pub fn mentat_pull()` in src/functions/pull.rs
- `pub fn mentat_entity()` in src/functions/entity.rs
- `pub fn mentat_schema()` in src/functions/schema.rs

### Additional Fixes (commit 060fe43)
- Added fulltext table to test setup
- Added bootstrap transaction datom (tx 1000000)
- Fixed SPI parameter handling

## Test Results

### Passing Tests (25/45)

#### Basic Query Tests ✅
- test_pg_rel - Relational queries
- test_pg_scalar - Scalar results
- test_pg_coll - Collection results
- test_pg_failing_scalar - Scalar with no results
- test_pg_query_limit - LIMIT clause
- test_pg_query_not - NOT clause

#### Entity/Pull Tests ✅
- (several entity and pull tests passing)

#### Schema Tests ✅
- (schema initialization tests passing)

#### EDN Roundtrip Tests ✅ (all 7)
- test_basic_query
- test_edn_roundtrip_boolean
- test_edn_roundtrip_integer
- test_edn_roundtrip_keyword
- test_edn_roundtrip_map
- test_edn_roundtrip_string
- test_edn_roundtrip_vector

### Failing Tests (20/45)

#### OR Clause Issues (1 test)
- `test_pg_query_or` - ERROR: "No where clauses produced any datom table joins"

#### Temporal Query Issues (5 tests)
- test_pg_as_of
- test_pg_since
- test_pg_history
- test_pg_history_retraction
- test_pg_as_of_complex (if exists)

#### Full-Text Search Issues (4 tests)
- test_pg_fulltext_basic
- test_pg_fulltext_multi_term
- test_pg_fulltext_scoring
- test_pg_fulltext_special_chars

#### Rule-Based Query Issues (6 tests)
- test_pg_simple_rule
- test_pg_recursive_rule
- test_pg_rule_bind
- test_pg_rule_multi_clause
- test_pg_rule_or
- test_pg_rule_with_predicates

#### Other Failures (4 tests)
- test_pg_multi_clause
- test_pg_query_order
- test_pg_query_with_inputs
- test_pg_tuple
- test_pg_tx_metadata

## Root Cause Analysis

### Primary Issue: OR Clause Handling
The error "No where clauses produced any datom table joins" indicates that OR clauses in queries are not generating the required SQL JOINs to the datoms table. This is a critical issue affecting:
- Direct OR queries
- Rule queries (which often use OR internally)

### Temporal Query Issues
Likely issues with:
- Transaction filtering (as_of, since parameters)
- History mode queries (need to handle added=false datoms)

### Full-Text Search Issues
Despite adding the fulltext table, the FTS queries are still failing. Possible issues:
- Text value insertion/lookup
- tsquery generation
- FTS join generation

### Rules Issues
All rule-based tests are failing, suggesting the rules engine implementation needs work:
- Rule CTE generation
- Rule invocation handling
- Rule variable binding

## Next Steps (Priority Order)

### High Priority
1. **Fix OR clause handling** (would unlock ~7 tests)
   - Debug build_or_clause in query.rs
   - Ensure OR clauses generate proper datom joins
   - May need UNION or EXISTS subqueries

2. **Fix temporal queries** (would unlock 5 tests)
   - Add proper tx filtering for as_of/since
   - Handle history mode (include retractions)

### Medium Priority
3. **Fix full-text search** (would unlock 4 tests)
   - Debug FTS table integration
   - Check text value encoding/decoding
   - Verify tsquery generation

4. **Fix rules engine** (would unlock 6 tests)
   - Debug rule CTE generation
   - Fix rule invocation handling
   - Ensure proper variable binding

### Low Priority
5. **Fix remaining individual tests** (4 tests)
   - Investigate tuple, multi_clause, order, with_inputs failures

## Completion Estimate

**Current**: ~55% complete
**If OR + Temporal fixed**: ~67% complete (30/45)
**If OR + Temporal + FTS fixed**: ~76% complete (34/45)
**If all core features fixed**: ~91% complete (41/45)

## Recent Commits

- `060fe43` - fix(tests): Add fulltext table, bootstrap datoms, and SPI parameter fix
- `d636f6a` - fix(pg_mentat): Make pg_extern functions pub for test module access
- `16b939c` - Fix partitions table schema and allocate_entid function
- `69b6fd6` - fix(query): Replace Box::leak with static CTE column name array
