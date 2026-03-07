# Test Execution Results

**Date**: 2026-03-07
**PostgreSQL**: 16.13
**Command**: `cargo pgrx test pg16` (without `--no-schema`)
**Duration**: 5.38s

## Summary

| Category | Count |
|----------|-------|
| Total tests | 45 |
| Passed | 12 |
| Failed | 33 |
| Ignored | 0 |

## Passed Tests (12)

### Unit tests (non-PostgreSQL, 7 passed)
1. `planner::hooks::tests::test_suggest_index_attribute` - ok
2. `planner::hooks::tests::test_suggest_index_entity` - ok
3. `planner::hooks::tests::test_suggest_index_value` - ok
4. `planner::hooks::tests::test_suggest_index_attribute_value` - ok
5. `planner::hooks::tests::test_estimate_query_cost` - ok
6. `types::edn::tests::test_edn_value_validation` - ok
7. `types::edn::tests::test_edn_value_size` - ok

### pg_test integration tests (5 passed)
8. `tests::pg_test_edn_roundtrip_boolean` - ok
9. `tests::pg_test_edn_roundtrip_string` - ok
10. `tests::pg_test_edn_roundtrip_map` - ok
11. `tests::pg_test_edn_roundtrip_integer` - ok
12. `tests::pg_test_edn_roundtrip_vector` - ok

## Failed Tests (33)

All 33 failures share the **same root cause**: missing SQL functions in the `mentat` schema.

### Error Signature 1: `function mentat.mentat_transact(unknown) does not exist` (23 tests)
1. `tests::pg_test_pg_history` (lib.rs:632)
2. `tests::pg_test_pg_fulltext_special_chars` (lib.rs:1280)
3. `tests::pg_test_pg_fulltext_basic` (lib.rs:1136)
4. `tests::pg_test_pg_as_of_future_entity` (lib.rs:667)
5. `tests::pg_test_pg_history_retraction` (lib.rs:xxx)
6. `tests::pg_test_pg_fulltext_phrase` (lib.rs:1312)
7. `tests::pg_test_pg_fulltext_empty_query` (lib.rs:1348)
8. `tests::pg_test_pg_fulltext_scoring` (lib.rs:xxx)
9. `tests::pg_test_pg_as_of_complex` (lib.rs:739)
10. `tests::pg_test_pg_as_of` (lib.rs:xxx)
11. `tests::pg_test_pg_coll` (lib.rs:332)
12. `tests::pg_test_pg_failing_scalar` (lib.rs:268)
13. `tests::pg_test_pg_scalar` (lib.rs:xxx)
14. `tests::pg_test_pg_tuple` (lib.rs:309)
15. `tests::pg_test_pg_rel` (lib.rs:xxx)
16. `tests::pg_test_pg_since` (lib.rs:xxx)
17. `tests::pg_test_pg_tx_metadata` (lib.rs:xxx)
18. `tests::pg_test_pg_simple_rule` (lib.rs:xxx)
19. `tests::pg_test_pg_recursive_rule` (lib.rs:xxx)
20. `tests::pg_test_pg_rule_bind` (lib.rs:xxx)
21. `tests::pg_test_pg_rule_multi_clause` (lib.rs:xxx)
22. `tests::pg_test_pg_rule_negation` (lib.rs:xxx)
23. `tests::pg_test_pg_rule_or` (lib.rs:xxx)

### Error Signature 2: `function mentat.mentat_query(unknown, jsonb) does not exist` (10 tests)
1. `tests::pg_test_pg_query_or` (lib.rs:442)
2. `tests::pg_test_pg_multi_clause` (lib.rs:xxx)
3. `tests::pg_test_pg_query_limit` (lib.rs:xxx)
4. `tests::pg_test_pg_query_not` (lib.rs:xxx)
5. `tests::pg_test_pg_query_order` (lib.rs:467)
6. `tests::pg_test_pg_query_with_inputs` (lib.rs:xxx)
7. `tests::pg_test_pg_fulltext_multi_term` (lib.rs:1177)
8. `tests::pg_test_pg_fulltext_non_fts_attribute` (lib.rs:1216)
9. `tests::pg_test_pg_rule_aggregation` (lib.rs:xxx)
10. `tests::pg_test_pg_rule_with_predicates` (lib.rs:xxx)

## Root Cause Analysis

All 33 failed tests call SQL functions (`mentat.mentat_transact()` or `mentat.mentat_query()`) via `Spi::run()` or `Spi::get_one()` inside their test bodies. The PostgreSQL error indicates these functions are not being registered in the `mentat` schema when the extension is loaded into the test database.

The extension installs successfully (75 SQL entities discovered: 2 schemas, 72 functions, 1 type), but the test functions call `mentat.mentat_transact(...)` and `mentat.mentat_query(...)` which are not found. This suggests the functions may be registered under a different schema or with different argument signatures than what the tests expect.

Key observations:
- The 5 EDN roundtrip tests pass because they only test the EDN type serialization/deserialization (no SQL function calls)
- The 7 planner/edn unit tests pass because they are pure Rust unit tests (no PostgreSQL interaction)
- All 33 failures happen at the SQL function call level, not in Rust code

## Compiler Warnings

1. `unused import: hooks::init_planner_hooks` at `pg_mentat/src/planner/mod.rs:18:9`
2. `function init_planner_hooks is never used` at `pg_mentat/src/planner/hooks.rs:167:15`

## Note on --no-schema Flag

The initial run with `--no-schema` failed completely (all 38 pg_test tests) because `cargo pgrx install` does not accept `--no-schema` as an argument. Removing that flag allowed the extension to install properly and the 5 EDN roundtrip tests to pass.

## Full Test Output

The complete test output is saved at: `/home/gburd/ws/pg_mentat/.tmp/test_output.txt`
