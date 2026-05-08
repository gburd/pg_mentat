# pg_mentat — Session Summary

This file records exactly what one focused session of engineering work
accomplished against the six-phase plan in `docs/ROADMAP.md`.

If you're looking for "what's done vs what's not", read this first.

## Starting state (commit `871bd17 spring cleaning`)

- `cargo build -p pg_mentat` failed with a duplicate-symbol link error.
- `CREATE EXTENSION pg_mentat` failed on a clean database with at
  least three independent errors (schema ownership conflict; missing
  `mentat.stores` table; `ROUND(double precision, int)` does not
  exist in PostgreSQL).
- Four `docs/EXPERT_REVIEWS.md`-style documents attributed reviews to
  named real engineers (Marco Slot, "the Mozilla Mentat team", etc.)
  with dates in the future. No evidence any of them read the code.
- `benchmarks/LOAD_TEST_RESULTS.md` claimed 358–626 TPS against a real
  server. `benchmarks/results/` contained only template files.
- The workspace included ~15 crates inherited from the SQLite-era
  upstream (`tolstoy`, `ffi`, `query-projector`, ...) that `pg_mentat`
  did not consume. Root `Cargo.toml` still declared itself a
  publishable crate called `mentat` with `bundled_sqlite3` as a
  default feature.
- `pg_mentat/src/lib.rs.backup` was checked in.
- README marked every feature as "Complete" while in-code
  `NotYetImplemented` errors contradicted that claim.

## Ending state (branch `feat/phase-0-honesty-and-build`, 5 commits)

### Phase 0 — Honesty and cleanup (DONE)

- Deleted: `docs/EXPERT_REVIEWS.md`,
  `docs/PRODUCTION_READINESS_ASSESSMENT.md`,
  `docs/PRODUCTION_READINESS_UPDATE.md`,
  `docs/PRODUCTION_COMPLETION_SUMMARY.md`,
  `benchmarks/LOAD_TEST_RESULTS.md`,
  `benchmarks/results/BASELINE_SUMMARY.md`,
  `pg_mentat/src/lib.rs.backup`.
- Added: `docs/STATUS.md` (what works / partial / missing),
  `docs/ROADMAP.md` (six-phase plan with done-criteria and effort
  estimates), `docs/CI.md` (contributor pre-push workflow).
- README pitch rewritten; feature table now has three statuses
  (`Works`, `Partial`, `Not implemented`). The words "comprehensive",
  "robust", "production-ready", "elegant", "significant" no longer
  appear.
- Workspace pruned from ~20 crates to 5: `edn`, `core-traits`,
  `core`, `pg_mentat`, `mentatd`. Root `Cargo.toml` is now a pure
  workspace manifest.
- Migration scripts moved to `pg_mentat/sql/archive/` with README
  explaining they are historical.

### Phase 1 — Install path + storage unification (PARTIAL)

**Done:**

- `CREATE EXTENSION pg_mentat` now succeeds on a clean PostgreSQL 16.
  The fix required:
  - Removing the duplicate `#[pg_extern]` `edn_in/out/send/recv` that
    collided with pgrx's auto-generated I/O funcs from
    `#[derive(PostgresType)]` on `Edn`.
  - Dropping `schema = mentat` from the control file (it fought
    pgrx's generated `CREATE SCHEMA` from `#[pg_schema] mod mentat`).
  - Adding `mentat.stores`, `mentat.subscriptions`, and
    `mentat.materialized_views` to the inline bootstrap — several
    Rust files depend on them but they were never created.
  - Fixing `ROUND(double precision, int)` → `ROUND(numeric, int)` in
    `monitoring_views.sql`.
  - Rewriting `monitoring_views.sql`'s `store_overview` to aggregate
    from the wide-row table via `value_type_tag` instead of joining
    `_new` tables that didn't exist.
- The narrow per-type storage model (`datoms_ref_new`,
  `datoms_long_new`, ..., nine tables with proper covering indexes,
  aggressive autovacuum settings, GIN full-text index on text
  values, fillfactor-tuned) now gets created at CREATE EXTENSION
  time via `sql/10_narrow_storage.sql`. A `dual_write_datoms`
  trigger keeps the wide-row and narrow-row tables in sync while
  legacy code paths migrate onto narrow tables directly.
- Schema cache invalidation is now wired: every successful
  `mentat_transact` calls `invalidate_store_cache` so newly asserted
  idents become visible to the next query. (The function existed but
  was never referenced — the self-review flagged this as a dead-code
  warning.)
- Real bugs fixed along the way:
  - `:db/id` was parsed as a namespaced keyword but looked up as
    `Keyword::plain("db/id")` and skipped via `kw.name() == "db/id"`;
    both silently fail, so every `{:db/id ... :attr v}` map-form
    transaction failed. Centralised in `is_db_id()`.
  - `value_type_tag` integer literals were emitted as `INTEGER`
    (Rust's `i32`) but the Rust side declared `i16`; cast to
    `::SMALLINT` in the UNION projections.
  - `find_current_value_for_ea` UNION had a `BIGINT` arm where every
    other arm was `TEXT`, causing upsert to fail.
  - Value-typed predicates always compared against 0 because
    `CASE WHEN v_bool THEN 1 ELSE 0 END` returns 0 (not NULL) when
    `v_bool IS NULL`, poisoning COALESCE. Fixed with IS NULL guards.
  - `mentat_explain` returned "EOF while parsing a value" because it
    used `client.select(&raw_str, ..., &params)` with parameterised
    SQL (silently returns 0 rows under pgrx 0.17) and
    `EXPLAIN (FORMAT JSON)` (returns a `json` column that neither
    `String` nor `JsonB` Datum conversion handles cleanly in 0.17).
    Now uses `client.prepare()` + `FORMAT TEXT`.

**Not done (deferred to ongoing Phase 1 work):**

- Dropping the wide-row `mentat.datoms` table entirely. Several Rust
  files (`bootstrap.rs`, `storage.rs`, `pull.rs` test helpers,
  `edn_helpers.rs`, `stats.rs`, `helpers.rs`, `recursive_queries.rs`)
  still read and write the wide-row table. The dual-write trigger
  keeps them consistent. Porting those call sites is roughly a week
  of mechanical work. Tracked in `ROADMAP.md`.
- `store_id` widening from `i32` (SERIAL) to `i64` (BIGSERIAL). Many
  files hard-code `i32`. Cross-cutting but low-risk change.

### Phase 2 — Real benchmark, published (NOT STARTED)

Requires hardware time to load 10M datoms and run workloads. Can be
done once Phase 1 storage unification lands.

### Phase 3 — Datalog completeness (PARTIAL)

**Done:**

- `mentat_explain` works end-to-end: returns the Datalog query, the
  generated SQL, and the full PostgreSQL plan (including which
  indexes are being used).
- Value predicates (`[(>= ?age 25)]`) now actually filter.

**Not done:**

- Predicates inside `or` and `and` clauses — blocked on non-trivial
  changes to `build_or_union_sql`.
- Predicates inside rule bodies.
- `ground`, `get-else`, `tuple`, `untuple`, `vector`, `missing?`
  functions.
- Collection bindings in `:in` (`[?name ...]` syntax). The EDN
  grammar in `edn/src/lib.rs` does not accept this form at all;
  needs parser + algebrizer changes.
- Attribute predicates (`[(pred ?a :db/doc)]`).

### Phase 4 — Postgres citizenship (PARTIAL)

**Done:**

- CI scaffolding: `.github/workflows/installcheck.yml` (fast path,
  every push) + `.github/workflows/ci.yml` (full gate, every PR with
  build, clippy, pgrx test matrix). Both use a real Postgres 16
  container and run the smoke test against it.
- `pg_mentat/tests/smoke.sql` — 11-step end-to-end regression test
  that asserts CREATE EXTENSION produces the expected schema,
  `mentat_transact` + `mentat_query` round-trip a fact, and
  `DROP EXTENSION CASCADE` cleans up. Single script, exits non-zero
  on any step failure. Passes against pgrx-managed PG 16 locally.
- `scripts/smoke.sh` + `make smoke` — local reproduction.
- `mentat_explain` for debugging slow Datalog queries.

**Not done:**

- Upgrade scripts tested in CI.
- Logical replication compatibility verified.
- `pg_upgrade` story documented and tested.
- `SET LOCAL enable_seqscan = off` made optional instead of default.

### Phase 5 — Datomic community (NOT STARTED)

Clojure peer library, session-based connections, db values,
`d/entity` API, Datalog-friendly errors. All require Clojure
tooling. None in this session.

### Phase 6 — Differentiators (NOT STARTED)

Cross-table Datalog+SQL joins, pgvector as a value type, RLS
tenancy, FDW, CDC of the datom log via logical replication. All
require Phase 4 landed first.

## Verification

```
$ cd /home/gburd/ws/pg_mentat
$ CARGO_HOME=$HOME/.cargo_pg_mentat bash scripts/smoke.sh
smoke: local mode (PGHOST=/home/gburd/.pgrx PGPORT=28816)
...
=== pg_mentat smoke test: PASS ===
smoke: PASS (local mode)
```

11/11 steps green.

## Commits on this branch

```
d22a5a7 fix(smoke): honour caller-provided CARGO_HOME in scripts/smoke.sh
4cc23bc chore(workspace): prune to crates pg_mentat/mentatd actually use
198fe06 ci: add installcheck + full CI workflows and smoke-test scaffolding
2b5a58f fix(query): value predicates and mentat_explain
bf0f6e0 fix(install): make CREATE EXTENSION pg_mentat actually work
```

## Diff summary vs `main`

```
202 files changed, 2703 insertions(+), 60628 deletions(-)
```

The ~58K net deletion comes almost entirely from pruning unused
SQLite-era workspace crates and fabricated review documents.

## Still needed before both communities say "yes"

A realistic accounting of what remains:

- **Phase 1 finish** (storage unification): 1 person-week
- **Phase 2** (real benchmark on 10M datoms with flamegraphs):
  1 person-week including hardware time
- **Phase 3 finish** (Datalog features above): 3 person-weeks
- **Phase 4 finish** (Postgres upgrade/CDC/pg_upgrade stories):
  2 person-weeks
- **Phase 5** (Clojure peer library, sessions, d/entity): 3
  person-weeks
- **Phase 6 first slice** (cross-table joins + pgvector value
  type): 2 person-weeks

Total: **12 person-weeks** of focused work.

This session delivered roughly the first two of those.
