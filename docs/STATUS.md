# pg_mentat Status

_Last updated: Phase 0 cleanup pass._

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
- A wide-row datom store at `mentat.datoms` with EAVT / AEVT / AVET /
  VAET covering indexes.
- A narrow-row store introduced by `sql/10_narrow_storage.sql`
  (`mentat.datoms_narrow` plus per-type value tables). See
  "In-flight" below for what is not yet true of this table.
- Bootstrap data for built-in idents (`:db/ident`, `:db.type/*`,
  `:db.cardinality/*`, `:db.unique/*`, and friends).
- The `mentat_transact`, `mentat_query`, `mentat_pull`,
  `mentat_pull_many`, `mentat_entity`, `mentat_schema`,
  `mentat_explain`, and related `mentat_*` SQL functions documented
  in `README.md`.

In terms of smoke tests that pass end-to-end against a real PostgreSQL
instance (via `cargo pgrx test pg16`):

- Schema install is idempotent-adjacent (`DROP EXTENSION` + `CREATE
  EXTENSION` round-trips on a clean database).
- `mentat_transact` accepts EDN transactions for assertions, retractions,
  `:db.fn/retractEntity`, and tempid resolution with lookup refs.
- `mentat_query` executes Datalog `:find` / `:where` / `:in` with
  patterns, joins across shared variables, scalar/tuple/collection/
  relation find-specs, the aggregates listed below, and the predicate
  operators listed below.
- Rules without predicate bodies work, including recursive rules for
  transitive-closure style queries.
- Pull (`mentat_pull`, `mentat_pull_many`) supports attribute lists,
  wildcards, recursive nested pulls on ref attributes, reverse lookups
  (`:ns/_attr`), `:limit`, and `:default`.
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
- **Store IDs.** Tables carry a `store_id` column, but it is declared
  `INT` (32-bit) in `sql/10_narrow_storage.sql`. The surface API treats
  it as if it were wide. Mixing many stores is untested at scale.
- **Schema-cache invalidation.** Schema edits invalidate the cache
  coarsely (effectively: on any transaction that touches `:db/ident` or
  `:db.install/attribute`). Fine-grained per-attribute invalidation is
  not implemented.
- **`mentat_explain`.** Returns PostgreSQL `EXPLAIN` output for the
  compiled SQL. It does not yet expose a Datalog-level plan (clause
  reordering, join strategy, index choice).

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
- **`ground`, `get-else`, `tuple`.** Despite being mentioned in the
  older README feature table, these where-functions are not
  implemented. The README's "Predicates and functions" row is being
  corrected as part of this Phase 0 pass.
- **Attribute predicates** (`[(attribute ?a :db/unique)]` style). Not
  wired.
- **`d/entity` API.** `mentat_entity` returns a JSON blob of all
  attributes for an entity ID. It is not a lazy entity-map navigator
  with transparent ref traversal.

## Known limitations

- **Single-writer bottleneck.** `mentat_transact` serialises through
  PostgreSQL like any other transaction. Heavy concurrent writers
  contend on the transaction-log allocation path and on the datom
  indexes. There is no sharded writer.
- **`store_id` is `INT` (4 bytes), not `BIGINT`.** Documented above;
  repeated here because it is load-bearing.
- **Schema cache invalidation is coarse.** A schema edit clears more
  cached parses than strictly necessary.
- **No published load-test results above 1K datoms.** The old
  `benchmarks/LOAD_TEST_RESULTS.md` and
  `benchmarks/results/BASELINE_SUMMARY.md` asserted TPS numbers with no
  CSVs, flamegraphs, or reproducible harness. Both have been deleted in
  this pass. Phase 2 in `docs/ROADMAP.md` commits to one real benchmark
  (10M datoms, CSV + flamegraph) before any performance claim returns
  to the README.
- **pgrx-tests coverage.** `cargo pgrx test pg16` runs, but many of the
  test files under `pg_mentat/src/*_tests.rs` are end-to-end SQL round
  trips, not unit tests of internal functions. Coverage of the
  algebrizer and planner as libraries is thin.
- **No logical-replication testing.** Behaviour of `mentat.datoms`
  under `pglogical` / native logical replication has not been
  verified.
- **No `pg_upgrade` testing.** `ALTER EXTENSION pg_mentat UPDATE` has
  one stub upgrade script (`upgrade--1.0.0--1.1.0.sql`); multi-step
  upgrade paths are untested.

## In-flight

- **Storage unification.** `sql/10_narrow_storage.sql` installs narrow
  per-type value tables alongside the wide `mentat.datoms` table. The
  current intent is that `mentat_transact` dual-writes both and that
  `mentat_query` reads from the narrow tables; once parity is verified
  on a real dataset, the wide table and the dual-write trigger are to
  be dropped. This is Phase 1 on the roadmap.
- **Workspace prune.** The root `Cargo.toml` has been trimmed to the
  crates actually consumed by `pg_mentat` and `mentatd` (`edn`,
  `core-traits`, `core`, `db-traits`, `db`, `pg_mentat`, `mentatd`).
  Removed crates are listed in the Phase 0 commit message.
