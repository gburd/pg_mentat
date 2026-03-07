# Test Fix Plan for pg_mentat

**Date:** 2026-03-07
**Author:** result-analyzer
**Branch:** claude
**Based on:** Actual test execution results from `test_results_final.log`

---

## Executive Summary

Test execution succeeded against PostgreSQL 16.13 (pgrx-managed). The extension
compiles cleanly (2 warnings), installs 75 SQL entities (2 schemas, 72
functions, 1 type), and runs all 45 tests to completion.

**Actual results: 12/45 passed, 33/45 failed (26.7% pass rate)**

All 33 failures share a **single immediate root cause**: PostgreSQL cannot
resolve `mentat_transact(unknown)` or `mentat_query(unknown, jsonb)` because
the string literal arguments have type `unknown` rather than `text`. This is a
function signature resolution issue, not a logic bug.

However, fixing this type resolution issue will expose **deeper implementation
gaps** in the query translator, transaction processor, and temporal features.
This document provides the complete layered analysis.

---

## Test Results Summary

### Passing Tests (12/45)

| # | Test | Category |
|---|------|----------|
| 1 | `planner::hooks::tests::test_estimate_query_cost` | Unit |
| 2 | `planner::hooks::tests::test_suggest_index_attribute` | Unit |
| 3 | `planner::hooks::tests::test_suggest_index_attribute_value` | Unit |
| 4 | `planner::hooks::tests::test_suggest_index_entity` | Unit |
| 5 | `planner::hooks::tests::test_suggest_index_value` | Unit |
| 6 | `types::edn::tests::test_edn_value_validation` | Unit |
| 7 | `types::edn::tests::test_edn_value_size` | Unit |
| 8 | `tests::pg_test_edn_roundtrip_boolean` | Integration (EDN) |
| 9 | `tests::pg_test_edn_roundtrip_integer` | Integration (EDN) |
| 10 | `tests::pg_test_edn_roundtrip_string` | Integration (EDN) |
| 11 | `tests::pg_test_edn_roundtrip_vector` | Integration (EDN) |
| 12 | `tests::pg_test_edn_roundtrip_map` | Integration (EDN) |

**Why these pass:**
- 7 unit tests are pure Rust (no PostgreSQL interaction)
- 5 EDN roundtrip tests only exercise `edn_in`/`edn_out` (type I/O functions),
  which are in the correct schema and take/return proper types

### Failing Tests (33/45)

All failures produce one of two error messages:

| Error | Count | Tests Affected |
|-------|-------|----------------|
| `function mentat.mentat_transact(unknown) does not exist` | 22 | Tests that call `mentat_transact` first |
| `function mentat.mentat_query(unknown, jsonb) does not exist` | 11 | Tests that call `mentat_query` directly (no prior transact) |

**Root cause:** When test SQL calls `SELECT mentat.mentat_transact('...')`, the
string literal `'...'` has PostgreSQL type `unknown`. The function signature is
`mentat_transact(text)`, but PostgreSQL's function resolution requires an exact
type match or explicit cast for `unknown` -> `text`. The `::jsonb` cast on the
second argument of `mentat_query` works correctly, but the first `text`
argument still has type `unknown`.

---

## Failure Categories (Layered Analysis)

Once the type resolution issue (Layer 0) is fixed, the tests will encounter
deeper issues. This analysis predicts the cascading failures.

### Layer 0: Function Signature Resolution (blocks ALL 33 tests)

**Error:** `function mentat.mentat_transact(unknown) does not exist`

**Affected tests:** All 33 failing integration tests

**Root cause file:** `pg_mentat/src/lib.rs:92-370` (test SQL strings)

**Fix:** Add explicit `::TEXT` cast to the first argument in all test SQL
calls to `mentat_transact` and `mentat_query`:

```sql
-- Before (fails):
SELECT mentat.mentat_transact('[[:db/add ...]]')
SELECT mentat.mentat_query('[:find ...]', '{}'::jsonb)

-- After (works):
SELECT mentat.mentat_transact('[[:db/add ...]]'::TEXT)
SELECT mentat.mentat_query('[:find ...]'::TEXT, '{}'::jsonb)
```

**Alternatively:** Change the function signatures to accept `unknown`-compatible
types, or add an overload. But explicit casts are the standard pgrx approach.

**Effort:** Low -- mechanical search-and-replace across ~60 SQL call sites in
`lib.rs` test functions.

---

### Layer 1: Transaction Setup (blocks 26 tests)

Once type resolution is fixed, `mentat_transact` will execute but will fail
because it calls PL/pgSQL helper functions that don't exist in the test schema.

**Error (predicted):** `function mentat.allocate_entid(unknown) does not exist`

**Affected tests:** All 26 tests that call `mentat_transact` (either directly
or via setup helpers like `setup_temporal_data`, `setup_family_schema`, etc.)

**Root cause:** `mentat_transact()` in `transact.rs:26` calls:
```sql
SELECT mentat.allocate_entid('db.part/tx')
```
But `setup_test_db()` in `lib.rs:92-151` does not create the
`mentat.allocate_entid()` or `mentat.resolve_ident()` PL/pgSQL functions.
These are defined in `sql/05_functions.sql` but never loaded during test setup.

**Root cause file:** `pg_mentat/src/lib.rs:92-151` (`setup_test_db`)

**Fix:** Add the required PL/pgSQL helper functions to `setup_test_db()`:

```sql
CREATE OR REPLACE FUNCTION mentat.allocate_entid(partition_name TEXT)
RETURNS BIGINT AS $$
DECLARE new_entid BIGINT;
BEGIN
    UPDATE mentat.partitions
    SET start_id = start_id + 1
    WHERE part = partition_name
    RETURNING start_id - 1 INTO new_entid;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Partition % not found', partition_name;
    END IF;
    RETURN new_entid;
END; $$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION mentat.resolve_ident(keyword TEXT)
RETURNS BIGINT AS $$
BEGIN
    RETURN (SELECT entid FROM mentat.idents WHERE ident = keyword);
END; $$ LANGUAGE plpgsql;
```

**Important schema mismatch:** The production schema (`sql/02_tables.sql`) uses:
- `partitions.name` / `partitions.next_entid` column names
- `value_type mentat.value_type` (enum type)
- `v BYTEA` + `value_type_tag SMALLINT` in datoms

The test schema (`setup_test_db`) uses:
- `partitions.part` / `partitions.start_id` column names
- `value_type INTEGER`
- `v mentat.EdnValue` (custom type, NO `value_type_tag` column)

This schema mismatch means the PL/pgSQL functions need to be adapted for the
test schema's column names, AND the datoms table needs `value_type_tag SMALLINT`
added (since `transact.rs` inserts into that column).

**Recommended approach:** Change `setup_test_db()` to match the production
schema more closely:
- Use `v BYTEA NOT NULL` + `value_type_tag SMALLINT NOT NULL` instead of
  `v mentat.EdnValue`
- Use production column names in partitions table
- Load PL/pgSQL functions adapted from `sql/05_functions.sql`

**Effort:** Medium -- requires careful alignment of test schema with production
schema and function expectations.

---

### Layer 2: Schema Attribute Resolution (blocks 22 tests)

Even with `allocate_entid` working, transactions that define custom attributes
(like `:person/name`, `:family/child`) will fail because `resolve_ident` won't
find them in `mentat.idents`.

**Error (predicted):** `Unknown keyword ident: :person/name` or
`Failed to resolve attribute`

**Affected tests:** All 22 tests that transact custom schema attributes

**Root cause:** `resolve_attribute()` in `transact.rs:174-190` calls
`mentat.resolve_ident($1)` to find the attribute's entid. But when a test
transacts `[:db/add "name-attr" :db/ident :person/name]`, this creates a
datom, not an idents table entry. The `mentat.idents` table is only populated
during bootstrap, not by `mentat_transact`.

The test expects that transacting `:db/ident` assertions will make new
attributes resolvable, but `mentat_transact` doesn't update `mentat.idents`
or `mentat.schema` when processing `:db/ident`, `:db/valueType`, etc.

**Root cause file:** `pg_mentat/src/functions/transact.rs:14-135`

**Fix:** After processing `:db/add` assertions that target schema attributes
(`:db/ident`, `:db/valueType`, `:db/cardinality`), update the `mentat.schema`
and `mentat.idents` tables accordingly. This is the "schema alteration via
transaction" feature that Datomic/Mentat supports.

**Effort:** High -- requires implementing schema mutation logic in the
transaction processor.

---

### Layer 3: Query Translator Limitations (blocks 26 tests)

For the few tests that might survive Layers 1-2 (basic queries against
bootstrap schema data), the query translator has several unsupported features:

| Feature | Error Message | Tests Affected |
|---------|--------------|----------------|
| NOT clauses | "NOT / not-join clauses are not yet supported" | `test_pg_query_not`, `test_pg_rule_negation` (2) |
| Predicates | "Predicate clauses not yet supported" | `test_pg_rule_with_predicates`, `test_pg_rule_multi_clause` (2) |
| WhereFn | "Where-function clauses not yet supported" | All 7 fulltext tests, `test_pg_rule_bind` (8) |
| Rules | "Rule expressions not yet supported" | `test_pg_recursive_rule` (1) |
| ORDER BY | `parsed.order` field ignored | `test_pg_query_order`, `test_pg_fulltext_scoring` (2) |
| LIMIT | `parsed.limit` field ignored | `test_pg_query_limit` (1) |
| Aggregates | `(count ?x)` not translated | `test_pg_as_of_complex`, `test_pg_rule_aggregation` (2) |
| Input bindings | `_inputs` parameter ignored | `test_pg_query_with_inputs` (1) |
| asOf temporal | `_inputs` JSON not parsed | `test_pg_as_of`, `test_pg_as_of_future_entity`, `test_pg_as_of_complex` (3) |
| since temporal | `_inputs` JSON not parsed | `test_pg_since` (1) |
| history mode | `added = true` always applied | `test_pg_history`, `test_pg_history_retraction` (2) |

**Root cause files:**
- `pg_mentat/src/functions/query.rs:348-375` (unsupported clause checks)
- `pg_mentat/src/functions/query.rs:87-131` (`_inputs` parameter ignored)
- `pg_mentat/src/functions/query.rs:602-603` (`added = true` hardcoded)

### Layer 4: Type Encoding Gaps (blocks 8 tests)

The `encode_value()` function in `transact.rs:195-211` only handles 4 of 9
value types:

| Type | Tag | Supported | Tests Affected |
|------|-----|-----------|----------------|
| boolean | 1 | Yes | -- |
| long | 2 | Yes | -- |
| string | 7 | Yes | -- |
| keyword | 8 | Yes | -- |
| **ref** | **0** | **No** | `test_pg_simple_rule`, all family/rule tests (8) |
| double | 3 | No | -- |
| instant | 4 | No | -- |
| uuid | 10 | No | -- |
| bytes | 11 | No | -- |

When `:family/child` (type ref) is transacted with a tempid value like `"mom"`,
the value should be resolved to an entity ID and encoded as i64 (tag 0). Instead,
it's encoded as a string (tag 7), breaking ref-based joins.

**Root cause file:** `pg_mentat/src/functions/transact.rs:195-211`

### Layer 5: Ref Type Tag Inconsistency (blocks pull API)

`pull.rs:203` decodes refs at tag 5 (`2 | 5 =>`), but `query.rs` and
`transact.rs` use tag 0 for refs. If refs are properly encoded as tag 0, the
pull API will return an error for them.

**Root cause file:** `pg_mentat/src/functions/pull.rs:203`

---

## Prioritized Fix List

### Priority 1: CRITICAL -- Unblocks test execution (fixes 0 tests directly, enables all)

| # | Fix | File | Lines | Description |
|---|-----|------|-------|-------------|
| 1 | **Add `::TEXT` casts to test SQL** | `lib.rs` | 243-1370 | Add `::TEXT` to all `mentat_transact('...')` and first arg of `mentat_query('...'::TEXT, ...)` calls in test functions. ~60 call sites. |

### Priority 2: HIGH -- Fixes test infrastructure (potentially unblocks 7 tests)

| # | Fix | File | Lines | Description |
|---|-----|------|-------|-------------|
| 2 | **Align test schema with production** | `lib.rs` | 92-151 | Change `setup_test_db()` to use `v BYTEA NOT NULL, value_type_tag SMALLINT NOT NULL` instead of `v mentat.EdnValue`. Use production column names for partitions. |
| 3 | **Add PL/pgSQL helpers to test setup** | `lib.rs` | 92-151 | Add `allocate_entid()` and `resolve_ident()` functions to `setup_test_db()`. |

### Priority 3: HIGH -- Fixes transaction processing (unblocks 22 tests)

| # | Fix | File | Lines | Description |
|---|-----|------|-------|-------------|
| 4 | **Schema mutation via transactions** | `transact.rs` | 14-135 | When processing `:db/add` for `:db/ident`, `:db/valueType`, `:db/cardinality`, update `mentat.schema` and `mentat.idents` tables. |
| 5 | **Ref type encoding** | `transact.rs` | 195-211 | When attribute's value_type is `ref`, resolve tempid values to entity IDs and encode as i64 (tag 0). |
| 6 | **Cross-transaction tempid resolution** | `transact.rs` | 37-38 | For `:db/retract`, resolve entity by attribute lookup instead of allocating new tempid. |

### Priority 4: MEDIUM -- Fixes query translator (unblocks 16 tests incrementally)

| # | Fix | File | Lines | Description | Tests Unblocked |
|---|-----|------|-------|-------------|-----------------|
| 7 | **Temporal query options (asOf/since)** | `query.rs` | 87-131 | Parse `_inputs` JSON for `asOf`/`since` keys, add tx filters | 4 |
| 8 | **History mode** | `query.rs` | 602-603 | Remove `added = true` when `history: true` | 2 |
| 9 | **ORDER BY** | `query.rs` | 641-648 | Append `ORDER BY` from `parsed.order` | 2 |
| 10 | **LIMIT** | `query.rs` | 641-648 | Append `LIMIT N` from `parsed.limit` | 1 |
| 11 | **NOT clauses** | `query.rs` | 359-361 | Translate NOT to `NOT EXISTS (subquery)` | 2 |
| 12 | **Predicate clauses** | `query.rs` | 362-364 | Translate `[(< ?age 30)]` to SQL `WHERE` | 2 |
| 13 | **Aggregate functions** | `query.rs` | 168-175 | Handle `(count ?x)` in find spec | 2 |
| 14 | **Input bindings** | `query.rs` | 87 | Parse `_inputs` JSON for query parameters | 1 |

### Priority 5: LOW -- Fixes advanced features (unblocks 9 tests)

| # | Fix | File | Lines | Description | Tests Unblocked |
|---|-----|------|-------|-------------|-----------------|
| 15 | **Fulltext WhereFn** | `query.rs` | 365-367 | Handle `fulltext` as special WhereFn using `ts_query`/`ts_rank` | 7 |
| 16 | **Rule expressions** | `query.rs` | 368-370 | Implement rules as recursive CTEs or inline expansion | 1 |
| 17 | **Bind/WhereFn arithmetic** | `query.rs` | 365-367 | Handle `[(* ?age 2) ?double-age]` | 1 |

### Priority 6: CLEANUP -- Fix type tag inconsistency

| # | Fix | File | Lines | Description |
|---|-----|------|-------|-------------|
| 18 | **Ref tag in pull.rs** | `pull.rs` | 203 | Change `2 | 5 =>` to `0 | 2 =>` so refs (tag 0) decode correctly |

---

## Estimated Impact of Fixes

| After Fixes | Tests Passing | Pass Rate | Improvement |
|-------------|---------------|-----------|-------------|
| Baseline (current) | 12/45 | 26.7% | -- |
| Fix #1 (type casts) | 12/45 | 26.7% | Tests reach function code, but still fail at PL/pgSQL |
| + Fix #2-#3 (schema + helpers) | 12/45 | 26.7% | Setup succeeds, but transactions fail at schema mutation |
| + Fix #4 (schema mutation) | ~15/45 | 33% | Basic schema-only queries may work (rel, scalar, tuple, coll, multi-clause) |
| + Fix #5-#6 (ref + tempid) | ~19/45 | 42% | Transaction-dependent tests start passing |
| + Fix #7-#8 (temporal) | ~25/45 | 56% | Time-travel tests pass |
| + Fix #9-#14 (query features) | ~33/45 | 73% | Most query tests pass |
| + Fix #15-#17 (FTS + rules) | ~42/45 | 93% | Advanced features work |
| + Fix #18 (ref tag) | 45/45 | 100% | All tests pass |

---

## Quick Wins vs. Structural Fixes

### Quick Wins (can fix now, low risk)

1. **Fix #1: Type casts** -- Pure mechanical change, no logic impact
2. **Fix #9-#10: ORDER BY / LIMIT** -- Append clauses to already-correct SQL
3. **Fix #18: Ref tag in pull.rs** -- One-line constant change

### Structural Fixes (require careful implementation)

1. **Fix #2-#3: Schema alignment** -- Must ensure test schema matches what `transact.rs` and `query.rs` expect (BYTEA columns, `value_type_tag`, production column names)
2. **Fix #4: Schema mutation** -- Core Mentat feature, must correctly update `schema` and `idents` tables during transactions
3. **Fix #5-#6: Ref encoding / tempid resolution** -- Requires looking up attribute value_type from schema during transaction processing

### Complex Features (significant new code)

1. **Fix #15: Fulltext queries** -- Need to integrate PostgreSQL `ts_query`/`ts_rank` with the Datalog translator
2. **Fix #16: Rule expressions** -- Recursive CTEs or rule inlining, potentially complex
3. **Fix #11-#13: NOT/predicates/aggregates** -- Each requires translating a different Datalog construct to SQL

---

## Key Observations

### 1. The Immediate Fix is Deceptively Simple

The FINAL_TEST_STATUS.md from the previous session claimed "the type casting fix
will resolve all 33 remaining failures." This is **incorrect**. The type casting
fix (Fix #1) will only allow the functions to be called -- it won't make the
tests pass. The tests will then fail at deeper layers (missing PL/pgSQL
functions, unsupported query features, etc.).

### 2. Schema Mismatch is a Fundamental Problem

The test schema (`setup_test_db`) uses `v mentat.EdnValue` while all the Rust
code (`transact.rs`, `query.rs`, `pull.rs`, `entity.rs`) operates on `v BYTEA`
with a `value_type_tag SMALLINT` column. These are incompatible. The test
schema MUST be changed to match the production schema's `BYTEA + value_type_tag`
approach. This is not optional.

### 3. Transaction Processing Doesn't Update Schema

When Mentat/Datomic processes transactions that include `:db/ident`,
`:db/valueType`, and `:db/cardinality` assertions, it's expected to modify the
schema. The current `mentat_transact` just inserts datoms without side effects.
This means no test can create custom attributes and then query them.

### 4. The Query Translator is Phase 1

The query translator handles basic patterns and OR-joins but explicitly rejects
NOT, predicates, where-functions (including fulltext), rules, and type
annotations. It also ignores ORDER BY, LIMIT, and temporal options. This means
at least 25 of 33 tests would still fail even with perfect transaction support.

### 5. Previous Completion Estimates Were Optimistic

The FINAL_TEST_STATUS.md estimated "92% complete" and claimed the type cast fix
would bring tests to ~95%. Based on this analysis, realistic estimates are:
- After Fix #1 alone: still 26.7% pass rate (no actual improvement)
- After all Priority 1-3 fixes: ~42% pass rate
- After all fixes including advanced features: ~100% pass rate
- The full fix effort spans 18 discrete changes across 4 files

---

## Files Requiring Changes

| File | Fixes | Priority |
|------|-------|----------|
| `pg_mentat/src/lib.rs` | #1 (type casts), #2 (schema), #3 (PL/pgSQL helpers) | P1-P2 |
| `pg_mentat/src/functions/transact.rs` | #4 (schema mutation), #5 (ref encoding), #6 (tempid resolution) | P3 |
| `pg_mentat/src/functions/query.rs` | #7-#14 (temporal, ORDER, LIMIT, NOT, predicates, aggregates, inputs, fulltext) | P4-P5 |
| `pg_mentat/src/functions/pull.rs` | #18 (ref tag fix) | P6 |

---

## Recommendation

Start with Fixes #1 through #3 as a single batch. This unblocks test
execution and reveals the true state of `mentat_transact` and `mentat_query`.
Then prioritize Fix #4 (schema mutation) since it's the gateway to all
transaction-dependent tests. The query translator improvements (#7-#17) can
be done incrementally, each unblocking a specific category of tests.

The estimated overall project completion, based on actual test results, is
**~55%** (matching the CURRENT_STATUS.md honest assessment), not the 92%
claimed in FINAL_TEST_STATUS.md.
