# pg_mentat Test Suite Documentation

Comprehensive documentation of the pg_mentat test suite, covering all test
categories, Datomic compatibility verification, and test execution instructions.

## Overview

The pg_mentat test suite contains **1,637 tests** across **68 test files**, all
implemented as pgrx `#[pg_test]` integration tests that run inside a real
PostgreSQL instance. This ensures every test exercises the actual extension
behavior including SPI calls, SQL execution, and PostgreSQL type handling.

### Test Architecture

Tests use the pgrx testing framework:

- All test modules are gated with `#[cfg(any(test, feature = "pg_test"))]`
- Each module is annotated with `#[pgrx::pg_schema]`
- Individual tests use `#[pg_test]` (runs inside a PostgreSQL backend process)
- Each test gets its own transaction that is rolled back after completion
- Tests call `setup()` / `bootstrap_schema()` to initialize the mentat schema

### Running Tests

```bash
# Run all tests (requires PostgreSQL and pgrx installed)
cargo pgrx test

# Run tests for a specific PostgreSQL version
cargo pgrx test pg17

# Run a specific test module
cargo pgrx test pg17 -- speculative_transaction_tests

# Run a single test
cargo pgrx test pg17 -- test_with_returns_valid_json
```

## Test Categories

### 1. Transaction Tests (263 tests)

Tests covering the core transaction processing pipeline.

| File | Tests | Description |
|------|-------|-------------|
| `transact_unit_tests.rs` | 45 | Core transact operations: add, retract, map forms |
| `transaction_comprehensive_tests.rs` | 34 | All operation types, formats, error cases |
| `transaction_lifecycle_tests.rs` | 32 | Full entity lifecycle: create, update, retract |
| `transaction_report_tests.rs` | 28 | TxReport JSON format: tempids, tx-data, basis-t |
| `transaction_safety_tests.rs` | 10 | Advisory locks, serialization failure handling |
| `cas_tests.rs` | 15 | Compare-and-swap: success, failure, nil, cardinality |
| `tempid_tests.rs` | 15 | Tempid allocation, consistency, cross-reference |
| `batch_operation_tests.rs` | 12 | Multi-entity batch transactions |
| `multi_transaction_workflow_tests.rs` | 7 | Sequential transaction workflows |
| `idempotency_tests.rs` | 11 | Idempotent assertion behavior |
| `retraction_tests.rs` | 15 | `:db/retract` and `:db.fn/retractEntity` |
| `comprehensive_retract_tests.rs` | 22 | Component cascading, partial retraction |
| `speculative_transaction_tests.rs` | 19 | `mentat_with` / `d/with` speculative transactions |

**Key scenarios tested:**
- EDN vector format `[:db/add ...]` and map format `{:db/id ...}`
- Tempid resolution: string tempids, cross-references, consistency
- Transaction report: `db-before`, `db-after`, `tx-data`, `tempids`
- CAS (compare-and-swap): success, mismatch, nil old-value, cardinality-many rejection
- RetractEntity: simple entities, multi-attribute, component cascading
- Speculative transactions: no persistence, identical reports, constraint enforcement
- Advisory lock acquisition and release (no deadlocks)
- Serialization failure detection (SQLSTATE 40001)

### 2. Schema Tests (127 tests)

Tests covering schema definition, evolution, and introspection.

| File | Tests | Description |
|------|-------|-------------|
| `schema_comprehensive_tests.rs` | 29 | Schema definition across all value types |
| `schema_operation_tests.rs` | 25 | Schema install, update, validation |
| `schema_attribute_tests.rs` | 41 | Individual attribute properties |
| `schema_evolution_tests.rs` | 8 | Schema changes over time |
| `schema_introspection_tests.rs` | 24 | Schema querying and discovery |

**Key scenarios tested:**
- All 9 value types: ref, boolean, long, double, string, keyword, instant, uuid, bytes
- Cardinality: one vs many, switching cardinality
- Unique constraints: `:db.unique/value` and `:db.unique/identity`
- Component attributes: `:db/isComponent`
- Fulltext attributes: `:db/fulltext`
- Indexed attributes: `:db/index`
- No-history attributes: `:db/noHistory`
- Schema-in-same-transaction: define schema and use it atomically
- Schema introspection queries

### 3. Query Tests (353 tests)

Tests covering Datalog query compilation and execution.

| File | Tests | Description |
|------|-------|-------------|
| `query_comprehensive_tests.rs` | 36 | Core query patterns |
| `query_pattern_tests.rs` | 57 | WHERE clause patterns and bindings |
| `query_edge_tests.rs` | 29 | Edge cases: empty results, nil, special chars |
| `query_join_tests.rs` | 40 | Multi-clause joins, implicit/explicit |
| `query_predicate_exhaustive_tests.rs` | 47 | All predicate operators |
| `predicate_tests.rs` | 16 | Comparison predicates in WHERE clauses |
| `edge_case_query_tests.rs` | 24 | Unusual query patterns |
| `find_spec_tests.rs` | 22 | Find specifications: relation, tuple, scalar, collection |
| `find_spec_exhaustive_tests.rs` | 44 | Exhaustive find spec combinations |
| `aggregate_tests.rs` | 10 | Aggregate functions: count, sum, min, max, avg |
| `input_parameter_tests.rs` | 8 | `:in` clause parameter binding |
| `parameterized_value_tests.rs` | 60 | Value type handling in query parameters |
| `tests/rule_predicate_tests.rs` | 5 | Rule-based predicate queries |

**Key scenarios tested:**
- Find specifications: `[:find ?e ...]` (relation), `[:find ?e .]` (scalar),
  `[:find [?e ...]]` (collection), `[:find ?e ?v]` (tuple)
- WHERE clause patterns: `[?e :attr ?v]`, `[?e :attr value]`, `[?e :attr]`
- Multi-clause joins: implicit join on shared variables
- Predicates: `<`, `>`, `<=`, `>=`, `!=`, `=`, `not=`
- Aggregates: `(count ?e)`, `(sum ?v)`, `(min ?v)`, `(max ?v)`, `(avg ?v)`,
  `(count-distinct ?v)`, `(sample N ?v)`
- Input parameters: `:in $` (default db), `:in $ ?param`, `:in $ [?param ...]`
- Parameterized values across all types: string, long, double, boolean, keyword, ref, instant, uuid
- Empty result handling: no matches, nil values
- Special character handling in string values
- Large result sets

### 4. Value Type Tests (149 tests)

Tests covering type-specific storage and retrieval.

| File | Tests | Description |
|------|-------|-------------|
| `typed_value_tests.rs` | 49 | Type encoding/decoding roundtrips |
| `value_type_exhaustive_tests.rs` | 50 | Every type through store/query cycle |
| `generated_value_tests.rs` | 49 | Generated/computed value handling |
| `boundary_value_tests.rs` | 33 | Boundary values: min/max int, empty string, etc. |

**Key scenarios tested:**
- All 9 Datomic value types: ref, boolean, long, double, string, keyword,
  instant, uuid, bytes
- Roundtrip fidelity: store a value, query it back, verify exact match
- Boundary values: `i64::MIN`, `i64::MAX`, `0`, `-1`, empty string `""`,
  very long strings, `NaN`, `Infinity`, `-Infinity`, epoch instants,
  nil UUID, max UUID
- Type tag encoding: correct `value_type_tag` for each type
- Cross-type queries: joining entities with different value types

### 5. Upsert and Identity Tests (26 tests)

Tests covering Datomic's unique identity upsert semantics.

| File | Tests | Description |
|------|-------|-------------|
| `upsert_tests.rs` | 9 | Basic upsert: identity resolution, merging |
| `comprehensive_upsert_tests.rs` | 17 | Complex upsert scenarios |

**Key scenarios tested:**
- `:db.unique/identity` upsert: new tempid resolves to existing entity
- `:db.unique/value` constraint: duplicate value rejection
- Tempid-to-existing merge: single and multi-attribute
- Cross-tempid upsert: two tempids resolving to same entity
- Upsert with additional attributes in same transaction
- Conflict detection: different tempids, same unique value, conflicting attributes

### 6. Entity and Lifecycle Tests (62 tests)

Tests covering entity creation, modification, and querying.

| File | Tests | Description |
|------|-------|-------------|
| `entity_tests.rs` | 14 | Basic entity operations |
| `entity_lifecycle_tests.rs` | 34 | Full CRUD lifecycle |
| `cross_entity_tests.rs` | 14 | Cross-entity references and graphs |

**Key scenarios tested:**
- Entity creation with tempids
- Entity update (overwrite cardinality-one, accumulate cardinality-many)
- Entity retraction (full and partial)
- Cross-entity references via `:db.type/ref`
- Entity graphs: trees, cycles, DAGs
- Entity resurrection (retract then re-assert)

### 7. Pull API Tests (39 tests)

Tests covering the pull pattern (entity projection).

| File | Tests | Description |
|------|-------|-------------|
| `pull_tests.rs` | 21 | Pull patterns: attributes, wildcards, nesting |
| `pull_api_tests.rs` | 18 | Pull API edge cases and error handling |

**Key scenarios tested:**
- Simple pull: `[:person/name :person/age]`
- Wildcard pull: `[*]`
- Nested pull (component references): `[:person/name {:person/friends [...]}]`
- Reverse references: `[:person/_friends]`
- Missing attributes: nil/absent in result
- Default values in pull patterns
- Cardinality-many in pull results

### 8. Ref Graph Tests (34 tests)

Tests covering reference traversal and graph patterns.

| File | Tests | Description |
|------|-------|-------------|
| `ref_graph_tests.rs` | 34 | Reference chains, cycles, reverse refs |

**Key scenarios tested:**
- Forward reference traversal: `[?e :person/friend ?f]`
- Reverse reference traversal
- Reference chains: A -> B -> C
- Circular references: A -> B -> A
- Multi-hop joins
- Ref cardinality-many (sets of references)

### 9. Temporal Tests (22 tests)

Tests covering time-travel queries (as-of, since, history).

| File | Tests | Description |
|------|-------|-------------|
| `temporal_tests.rs` | 13 | as-of, since, history query basis |
| `history_tests.rs` | 9 | Full history including retractions |

**Key scenarios tested:**
- `as-of(t)`: query database state at specific transaction
- `since(t)`: query changes since a transaction
- `history`: query full history including retractions
- Basis-t tracking: `db-before` and `db-after` in transaction reports
- Temporal consistency: queries at different points return different results

### 10. Namespace Tests (16 tests)

| File | Tests | Description |
|------|-------|-------------|
| `namespace_tests.rs` | 16 | Keyword namespace handling |

**Key scenarios tested:**
- Namespaced keywords: `:person/name`, `:db.type/string`
- Non-namespaced keywords
- Namespace resolution in queries
- Schema ident namespace conventions

### 11. Lookup Ref Tests (8 tests)

| File | Tests | Description |
|------|-------|-------------|
| `lookup_ref_tests.rs` | 8 | Lookup references by unique attribute |

**Key scenarios tested:**
- Lookup ref syntax: `[:person/email "alice@example.com"]`
- Lookup refs in `:db/add` entity position
- Lookup refs in query `:in` parameters
- Missing lookup ref handling

### 12. Error Handling Tests (67 tests)

Tests covering error messages, validation, and edge cases.

| File | Tests | Description |
|------|-------|-------------|
| `error_handling_tests.rs` | 44 | Error message quality and categories |
| `error_regression_tests.rs` | 23 | Regression tests for previously-fixed bugs |

**Key scenarios tested:**
- Invalid EDN parsing errors
- Unknown attribute errors with helpful suggestions
- Type mismatch errors (e.g., string value for long attribute)
- Cardinality violations
- Unique constraint violations
- Malformed transaction structures
- MentatError categories: InvalidTransaction, SchemaNotFound, CasFailed, etc.

### 13. Performance and Scale Tests (34 tests)

Tests covering performance baselines and scaling behavior.

| File | Tests | Description |
|------|-------|-------------|
| `performance_benchmark_tests.rs` | 21 | Timing benchmarks at 1K/10K/100K scale |
| `stress_scale_tests.rs` | 13 | Stress tests: large transactions, many entities |

**Key scenarios tested:**
- Query performance at 1K, 10K, 100K entity scales
- Transaction throughput: entities per second
- Batch transaction performance
- Large single-transaction performance
- Memory pressure under high entity counts

### 14. Security Tests (25 tests)

| File | Tests | Description |
|------|-------|-------------|
| `security_tests.rs` | 25 | SQL injection, input sanitization |

**Key scenarios tested:**
- SQL injection via entity values (string, keyword)
- SQL injection via attribute names
- SQL injection via tempid strings
- EDN injection via crafted input
- Schema name validation (store names)
- Oversized input handling

### 15. Concurrency Tests (10 tests)

| File | Tests | Description |
|------|-------|-------------|
| `concurrency_tests.rs` | 10 | Concurrent transaction behavior |

**Key scenarios tested:**
- Concurrent writes to different entities
- Concurrent writes to same entity (last-writer-wins for cardinality-one)
- Advisory lock serialization
- Transaction ordering guarantees

### 16. Data Integrity Tests (28 tests)

| File | Tests | Description |
|------|-------|-------------|
| `data_integrity_tests.rs` | 28 | Data consistency and integrity checks |

**Key scenarios tested:**
- Datom consistency: stored values match queried values
- Transaction log consistency: tx-data matches actual datoms
- Index consistency: indexed lookups match scan results
- Cardinality enforcement: one vs many invariants
- Unique constraint enforcement after concurrent operations

### 17. Bootstrap and Cache Tests (32 tests)

| File | Tests | Description |
|------|-------|-------------|
| `bootstrap_tests.rs` | 21 | Schema bootstrap and initialization |
| `cache_tests.rs` | 11 | Schema cache warming, invalidation, lookup |

**Key scenarios tested:**
- Bootstrap schema creation: all system tables and sequences
- Bootstrap idempotency: calling bootstrap twice doesn't fail
- Cache warming: first access triggers bulk load
- Cache invalidation: schema changes clear the cache
- Cache hit paths: subsequent lookups are in-memory

### 18. Mixed Operation and Regression Tests (56 tests)

| File | Tests | Description |
|------|-------|-------------|
| `mixed_operation_tests.rs` | 17 | Combined operations in single transactions |
| `regression_tests.rs` | 22 | Bug fixes verified by regression tests |
| `property_tests.rs` | 34 | Property-based / randomized tests |

### 19. Other Specialized Tests

| File | Tests | Description |
|------|-------|-------------|
| `cardinality_tests.rs` | 17 | Cardinality-one vs cardinality-many behavior |
| `lib.rs` (inline) | 100 | Core integration tests embedded in lib.rs |
| `functions/edn_helpers.rs` | 11 | EDN helper function unit tests |
| `functions/pull.rs` | 20 | Pull function implementation tests |
| `storage.rs` | 1 | Storage layer test |

## Datomic Compatibility Verification

The following table maps Datomic API operations to their pg_mentat implementations
and test coverage status.

### Core Operations

| Datomic Operation | pg_mentat Function | Test Coverage | Status |
|---|---|---|---|
| `d/transact` | `mentat_transact()` | 263 transaction tests | Verified |
| `d/q` (query) | `mentat_query()` | 353 query tests | Verified |
| `d/pull` | `mentat_pull()` | 39 pull tests | Verified |
| `d/with` | `mentat_with()` | 19 speculative tests | Verified |
| `d/db` | `mentat_db()` | Bootstrap tests | Verified |
| `d/entity` | Entity queries | 62 entity tests | Verified |

### Schema Operations

| Datomic Feature | pg_mentat Support | Test Coverage | Status |
|---|---|---|---|
| `:db/valueType` (all 9 types) | All 9 types supported | 149 type tests | Verified |
| `:db/cardinality` (one/many) | Supported | 17 cardinality tests | Verified |
| `:db.unique/identity` | Supported with upsert | 26 upsert tests | Verified |
| `:db.unique/value` | Supported | Included in upsert tests | Verified |
| `:db/index` | Supported | Schema tests | Verified |
| `:db/fulltext` | Supported (ts_rank_cd) | FTS tests in query suite | Verified |
| `:db/isComponent` | Supported | Retraction cascade tests | Verified |
| `:db/noHistory` | Supported | Schema tests | Verified |
| `:db/doc` | Supported | Schema tests | Verified |

### Transaction Functions

| Datomic Function | pg_mentat Support | Test Coverage | Status |
|---|---|---|---|
| `:db/add` | Supported (vector + map) | Transaction tests | Verified |
| `:db/retract` | Supported | 37 retraction tests | Verified |
| `:db.fn/retractEntity` | Supported | Retract + speculative tests | Verified |
| `:db.fn/cas` | Supported | 15 CAS tests + speculative | Verified |

### Query Features

| Datomic Feature | pg_mentat Support | Test Coverage | Status |
|---|---|---|---|
| `:find` (relation) | Supported | find_spec tests | Verified |
| `:find` (scalar `.`) | Supported | find_spec tests | Verified |
| `:find` (collection `[...]`) | Supported | find_spec tests | Verified |
| `:find` (tuple) | Supported | find_spec tests | Verified |
| `:where` patterns | Supported | 57 pattern tests | Verified |
| `:in` parameters | Supported | 68 parameter tests | Verified |
| Predicates (`<`, `>`, etc.) | Supported | 63 predicate tests | Verified |
| Aggregates | Supported | 10 aggregate tests | Verified |
| Rules | Supported | 5 rule tests | Partial |

### Time-Travel

| Datomic Feature | pg_mentat Support | Test Coverage | Status |
|---|---|---|---|
| `d/as-of` | Supported | 13 temporal tests | Verified |
| `d/since` | Supported | temporal tests | Verified |
| `d/history` | Supported | 9 history tests | Verified |
| Basis-t tracking | Supported | Transaction report tests | Verified |

### Wire Protocol (mentatd)

| Datomic Feature | pg_mentat Support | Test Coverage | Status |
|---|---|---|---|
| Transit+JSON encoding | Supported | Client library tests | Verified |
| WebSocket sessions | Supported | Protocol tests | Verified |
| `:op` dispatch | 13 operations | See `datomic_compatibility/README.md` | Verified |
| cognitect.anomalies errors | Supported | Protocol tests | Verified |

### Known Differences from Datomic

1. **Storage**: PostgreSQL tables instead of DynamoDB/Cassandra/SQL Server
2. **Partitions**: Not supported (user partition only)
3. **Excision**: Not supported (hard delete of historical data)
4. **Log API**: Partial (tx-range supported, no raw log iteration)
5. **Analytics**: No built-in analytics support
6. **Peer model**: No peer library (all access via client protocol or SQL)
7. **Memory index**: No in-memory index segment (PostgreSQL manages indexes)
8. **Custom transaction functions**: Not supported (only built-in `:db.fn/cas`
   and `:db.fn/retractEntity`)

## Test Data Fixtures

### Standard Test Schema

Most test modules define a local schema for isolation. The common pattern:

```sql
-- String + Long attributes (cardinality one)
SELECT mentat_transact('[
    {:db/id "n" :db/ident :test/name :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
    {:db/id "v" :db/ident :test/val :db/valueType :db.type/long
     :db/cardinality :db.cardinality/one}
]');
```

### All-Type Schema

For comprehensive type testing:

```sql
SELECT mentat_transact('[
    {:db/id "r"  :db/ident :t/ref     :db/valueType :db.type/ref
     :db/cardinality :db.cardinality/one}
    {:db/id "b"  :db/ident :t/bool    :db/valueType :db.type/boolean
     :db/cardinality :db.cardinality/one}
    {:db/id "l"  :db/ident :t/long    :db/valueType :db.type/long
     :db/cardinality :db.cardinality/one}
    {:db/id "d"  :db/ident :t/double  :db/valueType :db.type/double
     :db/cardinality :db.cardinality/one}
    {:db/id "s"  :db/ident :t/str     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
    {:db/id "k"  :db/ident :t/kw      :db/valueType :db.type/keyword
     :db/cardinality :db.cardinality/one}
    {:db/id "i"  :db/ident :t/inst    :db/valueType :db.type/instant
     :db/cardinality :db.cardinality/one}
    {:db/id "u"  :db/ident :t/uuid    :db/valueType :db.type/uuid
     :db/cardinality :db.cardinality/one}
    {:db/id "by" :db/ident :t/bytes   :db/valueType :db.type/bytes
     :db/cardinality :db.cardinality/one}
]');
```

### Unique Identity Schema (for Upsert tests)

```sql
SELECT mentat_transact('[
    {:db/id "u" :db/ident :test/uid :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
    {:db/id "n" :db/ident :test/name :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
]');
```

### Benchmark Schema

```sql
SELECT mentat_transact('[
    {:db/id "n"  :db/ident :bench/name    :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
    {:db/id "a"  :db/ident :bench/age     :db/valueType :db.type/long
     :db/cardinality :db.cardinality/one}
    {:db/id "e"  :db/ident :bench/email   :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
    {:db/id "s"  :db/ident :bench/score   :db/valueType :db.type/double
     :db/cardinality :db.cardinality/one}
    {:db/id "ac" :db/ident :bench/active  :db/valueType :db.type/boolean
     :db/cardinality :db.cardinality/one}
    {:db/id "tg" :db/ident :bench/tags    :db/valueType :db.type/string
     :db/cardinality :db.cardinality/many}
    {:db/id "ct" :db/ident :bench/cat     :db/valueType :db.type/keyword
     :db/cardinality :db.cardinality/one}
    {:db/id "mg" :db/ident :bench/manager :db/valueType :db.type/ref
     :db/cardinality :db.cardinality/one}
]');
```

## CI/CD Integration

### Prerequisites

- Rust toolchain (1.88+)
- PostgreSQL 17 (or matching pgrx version)
- pgrx installed: `cargo install cargo-pgrx`
- pgrx initialized: `cargo pgrx init`

### Test Pipeline

```bash
# 1. Compile the extension
cargo pgrx package

# 2. Run all pg_test tests
cargo pgrx test pg17

# 3. Run load tests (requires k6)
cd benchmarks && ./load_test.sh all

# 4. Run client library tests
cd clients/python && pytest tests/ -v
cd clients/nodejs && npm test
cd clients/clojure && clj -X:test
```

### Test Categories for CI

**Fast (< 2 min):** All `#[pg_test]` tests except `stress_scale_tests.rs`
and `performance_benchmark_tests.rs` (large-scale tests marked `#[ignore]`).

**Slow (< 10 min):** Include `performance_benchmark_tests.rs` at medium scale.

**Nightly (< 30 min):** Include `stress_scale_tests.rs`, large-scale benchmarks,
and load tests.

## Test Coverage Summary

| Category | Test Files | Test Count | Coverage |
|----------|-----------|------------|----------|
| Transactions | 13 | 263 | Comprehensive |
| Schema | 5 | 127 | Comprehensive |
| Queries | 13 | 353 | Comprehensive |
| Value Types | 4 | 149 | Exhaustive |
| Upsert/Identity | 2 | 26 | Comprehensive |
| Entity Lifecycle | 3 | 62 | Comprehensive |
| Pull API | 2 | 39 | Comprehensive |
| Ref Graphs | 1 | 34 | Comprehensive |
| Temporal | 2 | 22 | Good |
| Namespaces | 1 | 16 | Good |
| Lookup Refs | 1 | 8 | Basic |
| Error Handling | 2 | 67 | Comprehensive |
| Performance | 2 | 34 | Good |
| Security | 1 | 25 | Good |
| Concurrency | 1 | 10 | Basic |
| Data Integrity | 1 | 28 | Good |
| Bootstrap/Cache | 2 | 32 | Comprehensive |
| Mixed/Regression | 3 | 56 | Comprehensive |
| Other (lib, helpers) | 5 | 133 | Varies |
| **Total** | **68** | **1,637** | |

### Coverage Gaps and Recommendations

1. **Rules**: Only 5 tests for rule-based queries. Recommend expanding with
   recursive rules, rule composition, and rule-with-aggregation tests.
2. **Lookup Refs**: Only 8 tests. Recommend adding nested lookup refs and
   lookup refs in pull patterns.
3. **Concurrency**: Only 10 tests. Recommend adding multi-connection
   concurrent transaction tests (requires separate PostgreSQL connections,
   not possible within single pgrx test process).
4. **Named Stores**: Limited testing of multi-store operations
   (`mentat_create_store`, `mentat_transact_full`, `mentat.with`).
5. **Full-Text Search**: BM25 scoring (ts_rank_cd), language-specific
   stemming, and FTS query integration could use dedicated tests.
6. **Speculative + Schema**: Testing `mentat_with` with transactions that
   define new schema attributes (to verify schema rollback behavior).
