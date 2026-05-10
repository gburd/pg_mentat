# pg_mentat Status

_Last updated: after the Phase 0 + partial Phase 1 engineering pass on
`feat/phase-0-honesty-and-build`._

This document states, as plainly as possible, what pg_mentat does today,
what it does partially, and what it does not do. It exists to replace
marketing-voice claims in older README and "production readiness"
documents that overstated the state of the code.

The source of truth is the code in `pg_mentat/src/` and the SQL in
`pg_mentat/sql/`. When this document and the code disagree, the code
wins and this document is the bug.

## What works today

`CREATE EXTENSION pg_mentat` on PostgreSQL 13–18 (default build: pg16
via pgrx 0.17) installs the extension and produces:

- The `mentat` schema.
- **Nine narrow per-type storage tables** (`mentat.datoms_<type>_new`)
  installed by `sql/10_narrow_storage.sql`. Each has `(store_id, e, a,
  tx)` primary key, one non-NULL `v` column of the native type,
  covering EAVT / AEVT indexes with INCLUDE-clause for index-only
  scans, partial-`added` predicates, fillfactor tuning, aggressive
  autovacuum (5% scale factor, 2% analyze scale factor), a GIN
  fulltext index on `datoms_text_new`, and
  `CREATE STATISTICS (ndistinct, dependencies, mcv) ON (a, e)` so the
  planner knows about the attribute/entity correlation. This is where
  actual datom data lives.
- **A `mentat.datoms` compatibility VIEW** over those nine tables with
  INSTEAD OF INSERT / INSTEAD OF DELETE triggers. The view reproduces
  the old wide-row column shape
  (`e, a, value_type_tag, v_ref, v_bool, ..., tx, added`) so pre-Phase-1
  callers (tests, legacy PL/pgSQL helpers) keep working. The wide-row
  TABLE, its ten partitions, the CHECK constraint summing the
  nine nullable value columns, and the `dual_write_datoms_trigger`
  are GONE as of Phase 1.
- Bootstrap data for built-in idents (`:db/ident`, `:db.type/*`,
  `:db.cardinality/*`, `:db.unique/*`, and friends).
- The `mentat_transact`, `mentat_query`, `mentat_pull`,
  `mentat_pull_many`, `mentat_entity`, `mentat_schema`,
  `mentat_explain`, and related `mentat_*` SQL functions documented
  in `README.md`.

In terms of smoke tests that pass end-to-end against a real PostgreSQL
instance (`scripts/smoke.sh` / `.github/workflows/installcheck.yml`):

- Schema install is idempotent-adjacent (`DROP EXTENSION` + `CREATE
  EXTENSION` round-trips on a clean database).
- `mentat_transact` accepts EDN transactions for assertions, retractions,
  `:db.fn/retractEntity`, and tempid resolution with lookup refs. The
  per-store schema cache is invalidated after every successful
  transaction so newly asserted idents are visible to the next call.
- `mentat_query` executes Datalog `:find` / `:where` / `:in` with
  patterns, joins across shared variables, scalar/tuple/collection/
  relation find-specs, the aggregates listed below, and the predicate
  operators listed below. Value-typed predicates (`[(>= ?age 25)]`)
  correctly filter — an earlier bug made them compare against 0.
- Rules without predicate bodies work, including recursive rules for
  transitive-closure style queries.
- Pull (`mentat_pull`, `mentat_pull_many`) supports attribute lists,
  wildcards, recursive nested pulls on ref attributes, reverse lookups
  (`:ns/_attr`), `:limit`, and `:default`. Ref-typed values whose
  target has `:db/ident` render as the ident keyword (matching
  Datomic's `d/entity` display); other refs render as `{":db/id": N}`.
- `mentat_entity` returns the same Datomic-shaped ident-resolved view
  as `mentat_pull '[*]'`.
- `mentat_explain` returns the Datalog query, the generated SQL, and
  the Postgres plan (including index usage) for debugging.
- Time-travel input options (`asOf`, `since`, `history`) apply to
  `mentat_query` and change which datoms are visible.
- Cardinality-many attributes are stored with set semantics.
Aggregates implemented: `count`, `sum`, `avg`, `min`, `max` (see
`pg_mentat/src/functions/query.rs` around the `unsupported-aggregate`
error message).

Predicate operators implemented (top level and inside simple rule
bodies): `<`, `>`, `<=`, `>=`, `=`, `!=`. Arithmetic where-functions:
`*`, `+`, `-`, `/`. Full-text: `fulltext` is recognised and compiled
to a PostgreSQL `tsvector` / GIN query.

Value types encoded: `:db.type/ref`, `:db.type/boolean`, `:db.type/long`,
`:db.type/double`, `:db.type/instant`, `:db.type/string`,
`:db.type/keyword`, `:db.type/uuid`, `:db.type/bytes`. Value-type tag
`5` (BigInteger) is rejected with `:db.error/unsupported-constant`; this
matches Datomic's historical behaviour but is called out here for
completeness.

## What is partially implemented

These features exist in some form and are exercised by tests, but have
known gaps. The gaps are all returned to the caller as
`:db.error/unsupported-*` Datalog error codes rather than silent wrong
answers.

- **OR / OR-join clauses.** Single top-level `or` / `or-join` works.
  Multiple `or-join` clauses in one query are rejected
  (`:db.error/unsupported-query Multiple OR-join clauses …`). Inside
  OR branches: patterns and predicates work; full-text works; `not` /
  `not-join` and rule invocations are rejected.
- **NOT / not-join clauses.** Pattern clauses inside `not` /
  `not-join` work with a groundedness check; predicates and function
  calls inside `not` are rejected.
- **Rules.** Data patterns, `<`/`>`/`<=`/`>=`/`=`/`!=` predicates, the
  four arithmetic where-functions, and recursive rule invocations work
  inside rule bodies. Anything else in a rule body returns
  `:db.error/unsupported-rule-body`.
- **`:in` bindings.** Scalar bindings work. Collection bindings
  (`[?x ...]`), tuple bindings (`[?x ?y]`), and relation bindings
  (`[[?x ?y]]`) are not implemented end-to-end in the extension
  function surface; see "Not implemented" below.
- **Store IDs.** Tables carry a `store_id BIGINT` column and the
  Rust side uses `i64` end-to-end. Multi-store is schematically
  supported but still untested at scale.
- **Schema-cache invalidation.** Schema edits invalidate the cache
  coarsely (effectively: on any transaction that touches `:db/ident` or
  `:db.install/attribute`). Fine-grained per-attribute invalidation is
  not implemented.
- **`mentat_explain`.** Returns the Datalog query, the generated SQL,
  and the Postgres `EXPLAIN (VERBOSE, FORMAT TEXT)` output (including
  index usage). It does not yet expose a Datalog-level plan (clause
  reordering, join strategy, index choice).
- **Planner hints.** `mentat.enable_optimizer_hints` defaults to OFF.
  Per-query `SET LOCAL enable_seqscan = off` is available as an escape
  hatch (`SET mentat.enable_optimizer_hints = on`) but no longer the
  default behaviour — the narrow-table indexes + extended statistics
  usually make the planner pick the right plan on its own.

## What is not implemented

The following Datomic / Mentat features are _absent_. Callers should
treat them as missing rather than broken.

- **A Clojure peer library.** `pg-mentat-client/` contains a stub that
  talks to `mentatd` over HTTP but does not implement the Datomic peer
  API (`d/connect`, `d/db`, `d/entity`, `d/pull`, value-of-db
  semantics, `datomic.api`-shaped queries).
- **Excision.** There is no `:db/excise` / `d/excise` operation. All
  retractions remain in the history.
- **Collection / tuple / relation bindings in `:in`.** The Datalog
  parser accepts them; the extension-level query executor does not yet
  materialise them as correlated inputs.
- **Predicates in OR branches beyond the supported operator set.**
  Arbitrary where-functions inside `or` are rejected
  (`:db.error/unsupported-query Function '…' is not supported inside
  OR branches`).
- **Predicates in rule bodies beyond the listed operators.** Anything
  other than `<`/`>`/`<=`/`>=`/`=`/`!=` and `*`/`+`/`-`/`/` is rejected
  (`:db.error/unsupported-rule-fn`, `:db.error/unsupported-rule-body`).
- **`ground`.** Returns a specific
  `:db.error/unsupported-where-fn ground is not yet implemented`
  error that points callers at the `:in ?x ... inputs` workaround.
  A naive implementation was tried and reverted — it silently returned
  wrong results because the binding did not propagate into pattern-value
  positions. Correct implementation is Phase 3.
- **`get-else`, `tuple`, `missing?`, `untuple`, `vector`.** Not
  implemented. The README's "Predicates and functions" row is being
  corrected as part of this Phase 0 pass.
- **Attribute predicates** (`[(attribute ?a :db/unique)]` style). Not
  wired.
- **`d/entity` API.** `mentat_entity` returns a JSON blob of all
  attributes for an entity ID, now with ref values resolved to their
  `:db/ident` keyword when available. It is not yet a lazy entity-map
  navigator with transparent ref traversal and programmatic attribute
  access.

## Known limitations

- **Single-writer bottleneck.** `mentat_transact` serialises through
  PostgreSQL like any other transaction. Heavy concurrent writers
  contend on the transaction-log allocation path and on the datom
  indexes. There is no sharded writer.
- **Schema cache invalidation is coarse.** A schema edit clears more
  cached parses than strictly necessary.
- **No published load-test results above 100K datoms.** The initial
  Phase 2 benchmark (`docs/benchmarks/phase2.md`) covers 120K datoms
  on a developer laptop with 4 query shapes and an EAV baseline.
  Mentat overhead is 1.7x–3.6x vs raw EAV SQL depending on query
  shape. The full 10M-datom hermetic benchmark (dedicated host, CSV +
  flamegraph, scaling curves) remains open work per `docs/ROADMAP.md`.
- **pgrx-tests coverage.** `cargo pgrx test pg16` runs, but many of the
  test files under `pg_mentat/src/*_tests.rs` are end-to-end SQL round
  trips, not unit tests of internal functions. Coverage of the
  algebrizer and planner as libraries is thin.
- **No logical-replication testing.** Behaviour of `mentat.datoms`
  under `pglogical` / native logical replication has not been
  verified.
- **No tested `ALTER EXTENSION pg_mentat UPDATE` path.** Greenfield
  `CREATE EXTENSION` is exercised by CI; upgrading from an older
  installed version is not. This is deliberate: `pg_mentat` has no
  external deployments yet, so there is no pre-existing schema to
  migrate from. When the first deployment happens, the upgrade
  script for that specific transition will be written and tested
  then.

## In-flight

- **Workspace prune.** The root `Cargo.toml` has been trimmed to the
  crates actually consumed by `pg_mentat` and `mentatd` (`edn`,
  `core-traits`, `core`, `db-traits`, `db`, `pg_mentat`, `mentatd`).
  Removed crates are listed in the Phase 0 commit message.

## Done (previously in-flight)

- **Phase 2 benchmark harness.** `benchmarks/phase2/run.sh` generates
  a deterministic dataset, loads into both pg_mentat and a plain-EAV
  baseline, runs 4 query shapes x 30 repetitions, and writes
  timestamped CSV + EXPLAIN plans. Results:
  [`docs/benchmarks/phase2.md`](benchmarks/phase2.md). Overhead
  ranges from 1.7x (predicate joins) to 3.6x (point lookups) vs raw
  EAV at 120K datoms. The hermetic 10M-datom run remains open.
- **Beta hardening.** Four code-quality fixes shipped: (1) predicate
  constants parameterized via `SqlBuilder::bind_*` (eliminates the
  last SQL-injection surface), (2) type-tag constants extracted to a
  single shared module, (3) LRU eviction (cap=256) on the prepared
  statement cache, (4) `parking_lot::RwLock` replaces
  `std::sync::RwLock` in the schema cache (no lock poisoning).
- **Storage unification (Phase 1).** `sql/10_narrow_storage.sql`
  installs the narrow per-type tables. The wide-row `mentat.datoms`
  TABLE has been dropped; `mentat.datoms` is now a compatibility VIEW
  over the narrow tables with INSTEAD OF INSERT / DELETE triggers.
  Data lives in exactly one place (the narrow tables). The dual-write
  trigger is gone. Smoke test `scripts/smoke.sh` verifies the view
  shape and both triggers on every install. See
  `benchmarks/BENCHMARKS.md` for the read-path delta (noise); write
  improvement is expected but not yet measured against a pre-Phase-1
  baseline.
