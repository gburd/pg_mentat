# pg_mentat Roadmap

A six-phase plan to move pg_mentat from "a PostgreSQL extension that
answers Datalog queries in the happy path" to "a system both the
PostgreSQL and Datomic communities would recognise as legitimate".

Phases are sequential. Each phase has a short "done" definition, a
rough effort estimate, and a "what each community says yes to" note.
Estimates are wall-clock person-weeks assuming one maintainer working
continuously; calendar time is typically 2–3x.

The truth about current state lives in `docs/STATUS.md`.

---

## Phase 0 — Honesty and cleanup

**Status:** in progress (this commit).

**Goal.** Stop shipping marketing-voice documentation and stop carrying
dead Rust crates in the workspace. Nothing in Phase 0 changes runtime
behaviour.

**Scope.**

- Delete fabricated "expert reviews" (`docs/EXPERT_REVIEWS.md`) and
  "production readiness" documents attributed to named industry people
  with future dates.
- Delete benchmark summaries (`benchmarks/LOAD_TEST_RESULTS.md`,
  `benchmarks/results/BASELINE_SUMMARY.md`) that report numbers without
  CSVs, flamegraphs, or a reproducible harness.
- Replace the README's all-green feature table with
  `Works` / `Partial` / `Not implemented` statuses grounded in code.
- Rewrite the README opening pitch to describe the project as it is.
- Publish `docs/STATUS.md` enumerating what works, what partially
  works, and what is missing, with references to specific error codes
  in `pg_mentat/src/functions/query.rs`.
- Publish this `docs/ROADMAP.md`.
- Prune the Cargo workspace to the crates consumed by `pg_mentat` and
  `mentatd`. Remove the root `[package] name = "mentat"` so the root
  `Cargo.toml` is a pure workspace manifest.
- Move unapplied `sql/migrate_*.sql` files to `sql/archive/` with a
  README explaining they were never shipped.

**Done criteria.**

- `rg -n "production-ready|comprehensive|robust|elegant" README.md
  docs/` returns no hits in prose describing pg_mentat.
- `cargo build --no-default-features --features pg16 -p pg_mentat`
  succeeds.
- `CREATE EXTENSION pg_mentat` still works on a fresh pg16 cluster.
- No file in `docs/` attributes a review to a third party who did not
  write one.

**Effort.** 0.5–1 week.

**What the communities say yes to.**

- Postgres: "the repository stops lying about its own state."
- Datomic: "okay, now I can tell what's actually missing."

---

## Phase 1 — Storage unification

**Goal.** One datom storage path, not two. Today `mentat.datoms` (wide
row) and `mentat.datoms_narrow` (narrow per-type tables) coexist, with
a dual-write bridge. Queries read from the wide table in most paths.

**Scope.**

- Finish the narrow-table migration: every value-type write lands in
  the narrow per-type table, with a generated/derived projection
  preserving the `mentat.datoms` view shape for as long as compatibility
  is needed.
- Switch `mentat_query`, `mentat_pull`, `mentat_entity` to read
  exclusively from narrow tables.
- Fix `store_id` width: change `INT` to `INT8` in
  `sql/10_narrow_storage.sql` and in every Rust site that builds SQL
  referring to `store_id`.
- Drop the wide-row `mentat.datoms` and the dual-write trigger.
  Replace the old table with a compatibility `VIEW mentat.datoms` if
  downstream tooling relies on it.
- Add an `ALTER EXTENSION pg_mentat UPDATE` script that does this
  transition in-place on an existing database, backfilling narrow
  rows from the wide table and then swapping reads before dropping
  writes.
- Regression test: round-trip `pg_dump` + `pg_restore` on a database
  with ~100k datoms.

**Done criteria.**

- `grep -n "datoms " pg_mentat/src/functions/*.rs` shows no reads from
  a wide-row `mentat.datoms` table (view use is allowed but must be
  documented).
- `store_id` is `BIGINT` everywhere it appears.
- An upgrade script named `upgrade--1.1.0--1.2.0.sql` (or equivalent
  version number) lives next to the other upgrade scripts and is
  exercised by a pgrx test that installs the old version, loads
  datoms, runs `ALTER EXTENSION ... UPDATE`, and queries the upgraded
  database.

**Effort.** 3–4 weeks.

**What the communities say yes to.**

- Postgres: "the storage schema is legible, has correct column widths,
  and ships with a real upgrade script."
- Datomic: "datoms are datoms; I do not need to know which table holds
  them."

---

## Phase 2 — One real benchmark, published

**Goal.** Make one benchmark claim that another human can reproduce.
This replaces the old habit of citing TPS numbers with no artifacts.

**Scope.**

- Harness: a `benchmarks/phase2/` directory containing the exact SQL
  dataset generator, the exact workload driver (pgbench-style or a
  Rust driver), a `run.sh` that loads, warms, measures, and writes
  `results/<hostname>-<timestamp>/*.csv`.
- Dataset: 10 million datoms across schemas representative of typical
  Datomic use (an issue tracker and a product catalog, or similar).
- Measurements: per-query p50/p95/p99 latency, throughput, CPU
  utilisation, per-index bloat, `pg_stat_statements` output. Capture
  `perf record` on the hot path and publish a flamegraph SVG.
- Comparison: at minimum, measure narrow-table reads vs the equivalent
  hand-written SQL against a plain EAV table. Report both.
- Publication: one Markdown write-up under `docs/benchmarks/phase2.md`
  committing exact hardware, exact PostgreSQL config, exact
  `pg_mentat` version, and linking to the CSVs and flamegraphs.

**Done criteria.**

- `benchmarks/phase2/run.sh` produces the same CSV columns on a fresh
  machine.
- `docs/benchmarks/phase2.md` contains numbers, not adjectives.
- No TPS number appears in `README.md` without a hyperlink to the CSV
  it came from.

**Effort.** 2–3 weeks. Most of the cost is harness plumbing and
write-up, not runtime.

**What the communities say yes to.**

- Postgres: "the performance claims are testable, reproducible, and
  tied to a specific commit."
- Datomic: "even if the numbers are not Datomic-on-DynamoDB, I know
  what I am comparing to."

---

## Phase 3 — Datalog completeness

**Goal.** Close the `:db.error/unsupported-*` gaps called out in
`docs/STATUS.md`.

**Scope.**

- **`or` with predicates** beyond the supported operator set inside
  any OR branch. Currently rejected by
  `pg_mentat/src/functions/query.rs` around the
  `"not supported inside OR branches"` error.
- **Rules with predicates** beyond the six comparison operators and
  four arithmetic functions. Currently rejected with
  `:db.error/unsupported-rule-fn` and `:db.error/unsupported-rule-body`.
- **Collection, tuple, and relation bindings in `:in`.** Implement
  correlated subqueries (or `UNNEST` / `VALUES`) so `[?x ...]`,
  `[?x ?y]`, and `[[?x ?y]]` produce the expected join semantics.
- **`ground`, `get-else`, `tuple` where-functions.** These are
  expected by any non-trivial Datalog corpus; implement them as SQL
  generation rules over typed bound values.
- **Attribute predicates.** `[(attribute ?a :db/unique)]` and
  friends, reading the schema cache rather than `mentat.datoms`.
- **Multiple `or-join` clauses in a single query.** Currently
  rejected; plan is to compile each into an independent
  `LEFT JOIN LATERAL` and `UNION`-combine results.
- **`not` with predicates and function calls.** Permit predicates
  inside `not` when the variables they reference are bound in the
  outer scope.

**Done criteria.**

- Every `:db.error/unsupported-query`, `:db.error/unsupported-rule-*`,
  and `:db.error/unsupported-pred-*` error in `functions/query.rs`
  either goes away or has a test pinning its existence as an
  intentional, documented limitation.
- A Datomic query corpus (drawn from public Datomic tutorials with
  syntax adjusted where required) runs green.
- `docs/STATUS.md` "What is not implemented" shrinks to a short
  reviewed list instead of a running catalog.

**Effort.** 6–8 weeks. Most of this is the `:in` work and the rule
predicate work; the rest is narrower.

**What the communities say yes to.**

- Postgres: "this looks like a normal Datalog-to-SQL compiler; I can
  read the generated SQL."
- Datomic: "I can port my application's queries without rewriting
  them."

---

## Phase 4 — Postgres citizenship

**Goal.** Make pg_mentat behave like a PostgreSQL extension, not a Rust
project that happens to run inside the backend.

**Scope.**

- **Regression tests in the Postgres style.** Add a `test/expected/`
  tree with expected output files for each `.sql` script in
  `pg_mentat/test/sql/`. Wire `make installcheck` so an out-of-tree
  cluster can run the suite.
- **Real `mentat_explain`.** Return a JSON document with the parsed
  Datalog plan (clause order, join variables, index choices) _and_ the
  PostgreSQL `EXPLAIN (ANALYZE, BUFFERS)` output of the compiled SQL.
  Two layers, both inspectable.
- **Logical-replication compatibility.** Verify that `mentat.datoms`
  (or its narrow successor) replicates correctly under native logical
  replication and under `pglogical`. Document REPLICA IDENTITY and
  column-list configuration.
- **`pg_upgrade` story.** Test `pg_upgrade` from pg14 → pg16 and
  pg16 → pg18 with pg_mentat installed. Document any required
  pre-upgrade steps.
- **Proper extension upgrade scripts.** Every version bump ships a
  `upgrade--X.Y.Z--X.Y.(Z+1).sql`. No upgrade ever does
  `DROP ... CASCADE` on user data. CI verifies chained upgrades.
- **Control file hygiene.** `pg_mentat.control` declares `relocatable`,
  `trusted`, and `schema` correctly; function definitions respect
  `search_path` and do not leak into `public`.
- **Session GUC settings.** Expose `mentat.query_timeout_ms`,
  `mentat.explain_format`, etc. as PostgreSQL GUCs.

**Done criteria.**

- `make installcheck` runs out-of-tree against a stock PostgreSQL
  build.
- `mentat_explain` returns both a Datalog plan and a Postgres plan.
- CI chains `pg_upgrade` across two major versions with data loaded.
- A Postgres extension maintainer reading `pg_mentat.control` and the
  upgrade tree can describe what every file is for without guessing.

**Effort.** 4–6 weeks.

**What the communities say yes to.**

- Postgres: "this is a well-behaved extension that I could recommend."
- Datomic: "the operational model is the standard Postgres one; I do
  not need a bespoke ops playbook."

---

## Phase 5 — Datomic community

**Goal.** Give existing Datomic users a library and a vocabulary they
already know.

**Scope.**

- **Clojure peer library.** A real Clojure artifact (`pg-mentat/core`)
  exposing `d/connect`, `d/db`, `d/transact`, `d/q`, `d/pull`,
  `d/entity`, `d/as-of`, `d/since`, `d/history`. Connects via JDBC
  directly by default, optionally via `mentatd` for Transit-native
  clients.
- **Connection sessions.** `d/connect` returns a connection value
  that holds a PostgreSQL connection pool and a schema cache, not a
  URL string.
- **Db values.** `d/db` returns an immutable value identified by a
  (store, tx) pair. Subsequent operations see that value consistently
  even if the live database has advanced.
- **`d/entity`.** Lazy entity-map navigation with transparent ref
  traversal. Backed by the `mentat_pull` SPI under the hood, but
  exposed as a Clojure lazy map.
- **Datalog-friendly errors.** Every `:db.error/*` code emitted by the
  extension maps to a Clojure ex-info with structured data. No stack
  traces through `#error` printers.
- **Docs aimed at Datomic users.** A migration guide that maps
  Datomic concepts 1:1 to pg_mentat, including the things that do not
  map (no transactor, no peer-side caching, no excision).

**Done criteria.**

- A representative Datomic tutorial app (something like the
  "learn-datalog-today" exercises) runs against pg_mentat with only
  connection-string changes.
- `d/entity` navigation, `d/as-of`, and `d/pull` all work from
  Clojure.
- Every error from pg_mentat reaches Clojure as `ex-info` with
  `:cognitect.anomalies/category` and `:db/error` keys.

**Effort.** 6–10 weeks, because Clojure library quality and
documentation dominate the budget.

**What the communities say yes to.**

- Postgres: "the Clojure story does not compromise the extension; it
  is just another client."
- Datomic: "I can keep writing Datomic code."

---

## Phase 6 — Differentiators

**Goal.** Do things Datomic cannot and things plain PostgreSQL cannot,
using the fact that we are both.

**Scope.**

- **Cross-table Datalog + SQL joins.** Let a Datalog query reference a
  regular PostgreSQL table as a relation and let a SQL query join
  against `mentat_query(...)` as a function. Spec out semantics for
  attribute-to-column binding.
- **pgvector as a value type.** Accept `:db.type/vector` backed by
  pgvector; allow predicates like `(<-> ?v #vec [...])` in Datalog
  queries.
- **Row-level security / multi-tenancy.** Use native PostgreSQL RLS
  on the narrow datom tables to enforce per-tenant visibility. Expose
  a `mentat.current_tenant` GUC.
- **Foreign Data Wrapper.** Present datoms from a remote pg_mentat
  cluster via FDW, so one cluster can query another.
- **CDC.** Provide a logical decoding output plugin (or reuse
  `pgoutput` with a standard schema) so downstream consumers receive
  `(entity, attribute, value, tx, added)` events, not raw row diffs.

**Done criteria.**

- At least two of the five listed sub-items ship with docs and tests.
  This is the "pick your battles" phase.
- A demo repo uses Datalog + pgvector to build something neither
  system alone can: for example, "find similar entities within a
  social-graph neighbourhood at a point in time".

**Effort.** Open-ended. Each sub-item is 2–4 weeks on its own and they
can be parallelised once Phase 4 is done.

**What the communities say yes to.**

- Postgres: "this is now a reason to use PostgreSQL I did not have
  before."
- Datomic: "this is now a reason to leave Datomic I did not have
  before."

---

## How to use this document

- Every PR that advances a phase links back to the relevant bullet
  above in its description.
- When a bullet ships, it is struck through here _and_ the matching
  entry in `docs/STATUS.md` moves from "Partial" or "Not implemented"
  to "Works".
- No new phase starts while an earlier phase still has open done
  criteria. Phase 0 is the exception because it is cleanup; all other
  phases block on their predecessors.
