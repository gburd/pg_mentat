# PostgreSQL Test Port Status

## Overview

This document tracks the progress of porting Mentat's 187 test functions (~11,200 lines) from SQLite to PostgreSQL.

**Repository:** mentat (mozilla/mentat fork)
**Target:** pg_mentat extension with pgrx
**Test Framework:** pgrx-tests
**Date Started:** 2026-03-05

## Test Infrastructure

### Created Files

1. **`tests/test_common.rs`** - Common test utilities
   - `setup_test_db()` - Initialize PostgreSQL schema
   - `bootstrap_schema()` - Add core Datomic schema attributes
   - `cleanup_test_db()` - Clean up between tests
   - Helper functions: `query()`, `transact()`, `entity()`, `schema()`

2. **`tests/test_query.rs`** - Core query tests (11 tests)
   - ✅ test_pg_rel - Relational queries
   - ✅ test_pg_failing_scalar - Scalar with no results
   - ✅ test_pg_scalar - Scalar queries
   - ✅ test_pg_tuple - Tuple queries
   - ✅ test_pg_coll - Collection queries
   - ✅ test_pg_query_with_inputs - Parameterized queries
   - ✅ test_pg_multi_clause - Multi-clause joins
   - ✅ test_pg_query_not - Negation
   - ✅ test_pg_query_or - OR clauses
   - ✅ test_pg_query_order - Ordering
   - ✅ test_pg_query_limit - Limits

3. **`tests/test_fulltext.rs`** - Full-text search tests (7 tests)
   - ✅ test_pg_fulltext_basic - Basic FTS
   - ✅ test_pg_fulltext_multi_term - Multiple search terms
   - ✅ test_pg_fulltext_non_fts_attribute - Non-FTS attribute handling
   - ✅ test_pg_fulltext_scoring - Relevance scoring
   - ✅ test_pg_fulltext_special_chars - Special character handling
   - ✅ test_pg_fulltext_phrase - Phrase search
   - ✅ test_pg_fulltext_empty_query - Empty query handling

4. **`tests/test_rules.rs`** - Rules and recursive queries (8 tests)
   - ✅ test_pg_simple_rule - Basic rule invocation
   - ✅ test_pg_recursive_rule - Recursive queries (ancestor example)
   - ✅ test_pg_rule_multi_clause - Rules with multiple clauses
   - ✅ test_pg_rule_with_predicates - Built-in predicates
   - ✅ test_pg_rule_negation - Negation in rules
   - ✅ test_pg_rule_aggregation - Aggregates in rules
   - ✅ test_pg_rule_or - OR clauses in rules
   - ✅ test_pg_rule_bind - Bind clauses

5. **`tests/test_timetravel.rs`** - Temporal queries (8 tests)
   - ✅ test_pg_as_of - as-of queries
   - ✅ test_pg_since - since queries
   - ✅ test_pg_history - history queries
   - ✅ test_pg_as_of_future_entity - Entity before creation
   - ✅ test_pg_history_retraction - Retractions in history
   - ✅ test_pg_as_of_complex - Complex as-of queries
   - ✅ test_pg_tx_metadata - Transaction metadata

## Progress Summary

| Category | Original Tests | Ported | Status | Notes |
|----------|---------------|--------|--------|-------|
| Core Queries | 24 | 11 | 🟡 In Progress | Basic patterns covered |
| Fulltext Search | 34+ | 7 | 🟡 In Progress | SQLite FTS4 → PG tsvector |
| Rules/Recursive | ~20 | 8 | 🟡 In Progress | Covers key patterns |
| Time-Travel | ~10 | 8 | ✅ Complete | as-of, since, history |
| Cache | 6 | 0 | ⏳ Pending | |
| Entity Builder | 3 | 0 | ⏳ Pending | |
| Vocabulary | 4 | 0 | ⏳ Pending | |
| Pull API | 1 | 0 | ⏳ Pending | |
| Aggregates | ~10 | 0 | ⏳ Pending | count, sum, min, max, avg |
| Transactions | ~20 | 0 | ⏳ Pending | |
| **Total** | **187** | **34** | **18%** | |

## Key Differences: SQLite vs PostgreSQL

### 1. Full-Text Search
- **SQLite:** FTS4 with MATCH operator
  ```sql
  SELECT * FROM docs WHERE docs MATCH 'search term';
  ```
- **PostgreSQL:** tsvector/tsquery
  ```sql
  SELECT * FROM docs WHERE to_tsvector('english', content) @@ to_tsquery('english', 'search & term');
  ```

### 2. Connection Pattern
- **SQLite:** `new_connection("")` creates in-memory database
- **PostgreSQL:** Uses pgrx SPI within test transactions

### 3. Type Mapping
- SQLite BLOB → PostgreSQL bytea
- SQLite TEXT → PostgreSQL text
- SQLite INTEGER → PostgreSQL bigint
- SQLite REAL → PostgreSQL double precision

### 4. Schema Differences
- PostgreSQL requires explicit schema (created via `setup_test_db()`)
- SQLite bootstrap simpler, fewer type constraints
- PostgreSQL enforces more type safety

## Running Tests

### Prerequisites
```bash
cargo install cargo-pgrx
cargo pgrx init
```

### Run All Tests
```bash
cd pg_mentat
cargo pgrx test
```

### Run Specific Test Category
```bash
cargo pgrx test test_query
cargo pgrx test test_fulltext
cargo pgrx test test_rules
cargo pgrx test test_timetravel
```

### Run Single Test
```bash
cargo pgrx test test_pg_scalar
```

## Next Steps

### Phase 2: Additional Core Tests (Priority: High)
- [ ] Port remaining query.rs tests (13 remaining)
  - Aggregate queries
  - Complex joins
  - Subqueries
  - Error handling

### Phase 3: Specialized Tests (Priority: Medium)
- [ ] Port cache.rs tests (6 tests)
- [ ] Port entity_builder.rs tests (3 tests)
- [ ] Port vocabulary.rs tests (4 tests)
- [ ] Port pull.rs test (1 test)

### Phase 4: Transaction Tests (Priority: High)
- [ ] Basic transact operations
- [ ] Transaction rollback
- [ ] Concurrent transactions
- [ ] Constraint violations
- [ ] Unique value enforcement

### Phase 5: Integration Tests (Priority: High)
- [ ] End-to-end: Datomic client → mentatd → pg_mentat
- [ ] Performance benchmarks vs SQLite
- [ ] Concurrent client stress tests
- [ ] Large dataset tests

### Phase 6: Documentation
- [ ] Test coverage report
- [ ] Known limitations vs SQLite
- [ ] Performance comparison
- [ ] Migration guide

## Known Issues

### Not Yet Implemented
1. **WASM Integration** - Tests blocked until Task #12 complete
2. **Query Planner Hooks** - Advanced optimization tests pending Task #17
3. **Some Aggregate Functions** - Need verification of all operators

### SQLite-Specific Features
1. **EXPLAIN QUERY PLAN** - Different format in PostgreSQL
2. **FTS4 Tokenizers** - Must map to PostgreSQL equivalents
3. **In-memory Performance** - PostgreSQL overhead vs SQLite

## Test Execution Status

### Last Run: Not yet executed
**Reason:** Tests created but not run against PostgreSQL instance

### Next Action
1. Set up PostgreSQL test instance
2. Run `cargo pgrx test` to validate
3. Fix any failures
4. Document results

## Performance Expectations

### SQLite Baseline (from existing tests)
- Rel query: ~100-500µs
- Scalar query: ~50-200µs
- FTS query: ~1-5ms

### PostgreSQL Targets
- Rel query: <1ms (acceptable overhead)
- Scalar query: <500µs
- FTS query: <10ms

*Benchmarks pending actual test runs*

## Test Coverage Goals

- **Minimum Acceptable:** 70% of original tests passing
- **Target:** 85% of original tests passing
- **Stretch:** 95% of original tests passing

Some tests may not be portable due to SQLite-specific features. Alternative PostgreSQL tests should be created where direct ports aren't feasible.

## Contributors

- test-migrator (agent) - Initial port implementation
- team-lead - Task coordination

## References

- Original tests: `/tests/*.rs`, `/*/tests/*.rs`
- pgrx docs: https://github.com/pgcentralfoundation/pgrx
- PostgreSQL FTS: https://www.postgresql.org/docs/current/textsearch.html
