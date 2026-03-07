# pg_mentat -- Current Status

**Last updated:** 2026-03-07 (post actual test execution)
**Branch:** `claude`

---

## Overall Assessment

pg_mentat is a PostgreSQL extension (built with pgrx) that brings Mentat's
Datalog query engine to PostgreSQL, plus a companion `mentatd` HTTP server
that speaks the Datomic wire protocol.

**Honest completion estimate: ~55%**

The project has solid architecture, compiles cleanly (2 warnings), and installs
75 SQL entities into PostgreSQL 16.13. Tests were successfully executed via
`cargo pgrx test pg16`.

**Actual test results: 12/45 passed (26.7%)**

- 7 pure Rust unit tests pass (planner hooks, EDN validation)
- 5 EDN roundtrip integration tests pass (type I/O)
- 33 integration tests fail -- all with the same immediate error: PostgreSQL
  cannot resolve function signatures due to untyped string literal arguments
- Beneath this surface error lie deeper gaps: missing PL/pgSQL helpers in test
  setup, schema mismatch (tests use `mentat.EdnValue` but code expects `BYTEA`),
  transaction processor doesn't update schema/idents tables, and the query
  translator lacks NOT, predicates, rules, fulltext, temporal, ORDER, LIMIT,
  and aggregate support.

---

## Component Status

| Component | Status | Completion | Notes |
|-----------|--------|------------|-------|
| pg_mentat schema (SQL) | Done | 100% | Tables, indexes, constraints, bootstrap |
| EDN custom type | Done | 95% | Text I/O works; CBOR path stubbed |
| EDN operators | Done | 95% | =, <>, get, nth, count, contains, keys, values, type predicates |
| `mentat_transact()` | Partial | 70% | Works for basic assertions; SQL injection risk in format strings |
| `mentat_query()` | Partial | 60% | Simple patterns work; complex patterns (rules, predicates) fragile |
| `mentat_entity()` | Partial | 75% | Retrieves datoms; security fixes applied |
| `mentat_pull()` | Partial | 50% | Rewritten but not validated against live DB |
| `mentat_schema()` | Done | 90% | Returns schema as JSON |
| `initialize_schema()` | Done | 95% | Creates datom tables and indexes |
| mentatd server (HTTP) | Partial | 60% | Handlers wired to pg_mentat calls but not integration-tested |
| mentatd protocol parsing | Done | 90% | EDN request parsing, Datomic anomaly model |
| Test migration | Done | 100% | 38 tests in src/lib.rs using #[pg_test] |
| Nix flake | Done | 95% | Provides reproducible dev environment |
| CI/CD workflows | Draft | 40% | GitHub Actions files exist, not validated |
| WASM support | Design only | 5% | Architecture doc exists, no implementation |
| Documentation | Needs consolidation | 70% | Many overlapping files, inconsistent status claims |

---

## Phase Breakdown

### Phase 1: Foundation -- DONE

- Schema design (datoms, schema, idents, partitions, transactions tables)
- Four covering indexes (EAVT, AEVT, AVET, VAET)
- EdnValue custom PostgreSQL type with text I/O
- EDN operators and type predicates
- Storage helper functions (allocate_entid, resolve_ident, lookup_entity)
- Extension compiles with 0 errors

### Phase 2: Core Functions -- MOSTLY DONE (known gaps)

- `mentat_transact()` -- processes EDN transaction data, persists datoms
  - **Gap:** Uses `format!()` string interpolation instead of parameterized queries in some paths (SQL injection risk)
  - **Gap:** Limited type support (boolean, long, string, keyword); missing ref, double, instant, uuid, bytes
- `mentat_query()` -- translates Datalog to SQL and executes
  - **Gap:** Uses `format!("{:?}", ...)` producing Rust Debug output in SQL
  - **Gap:** No support for query input bindings
  - **Gap:** Complex patterns (rules, predicates, nested or/not) may produce incorrect SQL
- `mentat_entity()` -- retrieves all datoms for an entity
- `mentat_schema()` -- returns schema introspection as JSON
- `mentat_pull()` -- rewritten to fetch real data but not validated
- Full-text search via tsvector/tsquery

### Phase 3: Test Migration & Execution -- EXECUTED, 12/45 PASS (26.7%)

- All 38 pgrx integration tests migrated to `src/lib.rs` -- COMPLETE
- 7 additional unit tests exist (planner hooks, EDN validation)
- Old test files in `tests/` directory removed -- COMPLETE
- Extension compiles with 0 errors, 2 warnings -- COMPLETE
- Tests execute successfully via `cargo pgrx test pg16` -- COMPLETE
- **Actual results (from test_results_final.log):**
  - Total: 45 tests (7 unit + 38 integration)
  - Passed: 12 (7 unit + 5 EDN roundtrip)
  - Failed: 33 (all other integration tests)
  - Pass rate: 26.7%
- **Failure root cause:** All 33 failures are `function mentat.mentat_transact(unknown) does not exist` or `function mentat.mentat_query(unknown, jsonb) does not exist` -- PostgreSQL type resolution issue with untyped string literals
- **Beneath surface error:** Schema mismatch, missing PL/pgSQL helpers, incomplete query translator, missing ref type encoding
- **Detailed fix plan:** See `TEST_FIX_PLAN.md` (18 prioritized fixes)

### Phase 4: mentatd Server -- PARTIALLY DONE

- HTTP server with axum and connection pooling (deadpool-postgres)
- Configuration via TOML and environment variables
- EDN request parsing for all Datomic operations
- **Gap:** Handlers previously returned fake/stub data; now wired to call pg_mentat functions but the integration has not been tested end-to-end
- 12/12 unit tests pass (protocol-level)
- 21 integration tests exist but require a running PostgreSQL instance

### Phase 5: Environment & CI -- IN PROGRESS

- Nix flake provides Rust 1.90, PostgreSQL 13-17, LLVM/Clang 18
- Helper commands: `setup-pgrx`, `test-pg16`, `build-extension`, `install-extension`
- GitHub Actions workflow files drafted
- **Blocker:** Testing on current system blocked by filesystem restrictions and missing pgrx initialization

### Phase 6: WASM Support -- NOT STARTED

- Architecture document exists (`docs/architecture/wasm_design.md`)
- No implementation code

---

## Known Issues

### Security

1. **SQL injection in query construction** -- Several functions in `pg_mentat/src/functions/` use `format!()` to build SQL strings instead of parameterized queries via `Spi::run_with_args()`. Files affected: `transact.rs`, `query.rs`, `entity.rs`, `storage.rs`. Some paths have been fixed; audit is incomplete.

### Correctness

2. **Query translation fragility** -- `mentat_query()` generates SQL using Rust's `Debug` formatting (`{:?}`), which produces quoted strings with escape sequences that may not be valid SQL identifiers. Complex Datalog patterns (rules, recursive queries, aggregates) are likely to produce incorrect SQL.

3. **Limited type coverage** -- Only 4 of 9 Mentat value types are handled in transaction and query paths: boolean, long, string, keyword. Missing: ref, double, instant, uuid, bytes.

4. **mentatd stub concern** -- The mentatd handlers were rewritten from stubs to real calls, but this has never been integration-tested. The data path `HTTP request -> mentatd -> pg_mentat -> PostgreSQL -> response` is unverified.

### Environment

5. **Testing blocked** -- `cargo pgrx test` cannot run on the current system due to missing pgrx initialization and library compatibility issues. The Nix flake is designed to solve this but has not been validated on a system with Nix installed.

---

## Test Results (Actual Execution)

**Actual pass rate: 12/45 (26.7%)**

| Category | Result | Notes |
|----------|--------|-------|
| Unit tests (planner, validation) | 7/7 PASS | Pure Rust, no PostgreSQL |
| EDN Type roundtrips | 5/5 PASS | Self-contained, no schema deps |
| Core Queries | 0/11 FAIL | Function signature resolution error |
| Time-Travel | 0/7 FAIL | Function signature resolution error |
| Rules | 0/8 FAIL | Function signature resolution error |
| Full-Text | 0/7 FAIL | Function signature resolution error |

**Immediate error (all 33 failures):**
- `function mentat.mentat_transact(unknown) does not exist` (22 tests)
- `function mentat.mentat_query(unknown, jsonb) does not exist` (11 tests)

**Layered blockers (in priority order):**
1. **Layer 0:** String literals have type `unknown`, need `::TEXT` casts (~60 sites)
2. **Layer 1:** Missing `allocate_entid`/`resolve_ident` PL/pgSQL functions in test setup
3. **Layer 2:** Schema mismatch: tests use `v mentat.EdnValue` but code expects `v BYTEA` + `value_type_tag SMALLINT`
4. **Layer 3:** `mentat_transact` doesn't update `mentat.schema`/`mentat.idents` when processing schema assertions
5. **Layer 4:** Ref type (tag 0) not encoded in `encode_value`
6. **Layer 5:** Query translator lacks NOT, predicates, ORDER, LIMIT, aggregates, rules, FTS, temporal features

See `TEST_FIX_PLAN.md` for the complete prioritized fix plan with 18 specific fixes.

## What Works (High Confidence)

- Extension compiles with 0 errors, 2 warnings (planner hooks unused)
- 415/415 original core Mentat tests pass (SQLite backend, unrelated to pg_mentat)
- 12/12 mentatd unit tests pass
- EDN type parsing and serialization (edn_in, edn_out)
- SQL schema validated against live PostgreSQL (15/15 smoke tests pass)
- Nix flake structure (syntactically valid, dependencies specified)

## What Needs Validation (Medium Confidence)

- All 38 pgrx tests (migrated but never executed due to environment)
- `mentat_transact()` with real PostgreSQL
- `mentat_query()` with real PostgreSQL
- `mentat_pull()` rewrite
- mentatd -> pg_mentat integration
- Nix flake on a Nix-enabled system
- GitHub Actions workflows

## What Needs Implementation (Low Confidence)

- Test infrastructure: PL/pgSQL helpers in setup, schema alignment
- Query translator: NOT, predicates, ORDER, LIMIT, aggregates, rules, FTS
- Temporal queries: asOf, since, history
- Ref type encoding in transactions
- Cross-transaction tempid resolution
- Query input bindings
- WASM runtime
- Performance testing

---

## File Map

### Source Code

| Path | Description |
|------|-------------|
| `pg_mentat/src/lib.rs` | Extension entry point + 38 inline tests |
| `pg_mentat/src/types/edn.rs` | EdnValue PostgreSQL type |
| `pg_mentat/src/operators.rs` | EDN comparison, accessors, predicates |
| `pg_mentat/src/storage.rs` | SPI wrappers for entity/ident operations |
| `pg_mentat/src/functions/transact.rs` | `mentat_transact()` |
| `pg_mentat/src/functions/query.rs` | `mentat_query()` |
| `pg_mentat/src/functions/entity.rs` | `mentat_entity()` |
| `pg_mentat/src/functions/schema.rs` | `mentat_schema()` |
| `pg_mentat/src/functions/pull.rs` | `mentat_pull()` |
| `pg_mentat/sql/*.sql` | Schema DDL (tables, indexes, constraints, bootstrap) |
| `mentatd/src/server.rs` | HTTP handlers |
| `mentatd/src/config.rs` | Configuration |
| `mentatd/src/protocol/` | Datomic wire protocol parsing |

### Documentation (Key Files)

| Path | Description |
|------|-------------|
| `README.md` | Project overview (this repo) |
| `CURRENT_STATUS.md` | This file |
| `NIX_SETUP.md` | Nix development environment guide |
| `CONTRIBUTING.md` | Developer contribution guidelines |
| `QUICK_START.md` | Getting started guide |
| `pg_mentat/README.md` | Extension-specific documentation |
| `HONEST_STATUS.md` | Validator audit findings (2026-03-05) |
| `TEST_MIGRATION_COMPLETE.md` | Test restructuring report |

### Configuration

| Path | Description |
|------|-------------|
| `flake.nix` | Nix flake for reproducible builds |
| `.envrc` | direnv integration |
| `Cargo.toml` | Workspace root |
| `pg_mentat/Cargo.toml` | Extension crate |
| `mentatd/Cargo.toml` | Server crate |
| `.github/workflows/test.yml` | GitHub Actions CI |

---

## Immediate Priorities

1. **Resolve environment blocker** -- Use GitHub Actions, a container, or a
   fresh VM to get a writable filesystem where `cargo pgrx test pg16` can run.
   See `TEST_EXECUTION_BLOCKER.md` for options.

2. **Fix test infrastructure (Priority 1 from TEST_FIX_PLAN.md)** -- Add
   `allocate_entid` and `resolve_ident` PL/pgSQL functions to `setup_test_db()`.
   Fix schema mismatch (use `v BYTEA` + `value_type_tag SMALLINT` instead of
   `v mentat.EdnValue`).

3. **Add ref type encoding** -- Implement ref (tag 0) in `transact.rs:encode_value`.

4. **Implement temporal query options** -- Parse `asOf`, `since`, `history`
   from the JSON inputs parameter in `mentat_query()`.

5. **Add missing query features** -- NOT clauses, predicates, ORDER BY, LIMIT,
   aggregates, in the query translator.

6. **Implement full-text search in queries** -- Handle `fulltext` WhereFn.

7. **Implement rule expressions** -- Translate rules to recursive CTEs.

---

## Historical Note

This project has gone through several assessment cycles with varying completion
claims (95%, 85%, 60%). The HONEST_STATUS.md file from 2026-03-05 contains a
thorough validator audit that identified critical gaps. Subsequent work sessions
addressed some of those gaps (handler wiring, security fixes, pull rewrite, test
migration). The current 65% estimate accounts for unverified fixes and the fact
that no end-to-end testing has been performed.

---

*This document supersedes: HONEST_STATUS.md, POST_FIX_STATUS.md,
VALIDATION_STATUS.md, TESTS_FINAL_STATUS.md, FINAL_VALIDATION_REPORT.md,
SESSION_COMPLETE_SUMMARY.md, PROGRESS_REPORT.md*

*Related documents: TEST_FIX_PLAN.md (prioritized fix list),
TEST_EXECUTION_BLOCKER.md (environment issues),
MANUAL_SMOKE_TEST_RESULTS.md (SQL-layer validation)*
