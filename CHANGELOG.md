# Changelog

All notable changes to pg_mentat are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/).

## [1.5.6] - 2026-07-06

### Fixed

- **Entity-id partition sequences are now bounded, and the tx band is no
  longer a time bomb.** The three partition sequences
  (`partition_db_seq`, `partition_user_seq`, `partition_tx_seq`) shipped
  with only `START WITH` and no `MAXVALUE`, so an exhausted partition
  silently issued ids that collided with the next partition's space -- a
  latent entid-space-corruption hazard. Worse, the tx band was
  `[1000000, 2000000)`: one tx id is consumed per `mentat.t`, so a
  write-heavy store (e.g. ~33k tx/day) exhausted it in weeks and then
  `mentat.t` began issuing ids outside its band.

  Fresh installs use a new, disjoint, generous layout, and every partition
  sequence is bounded to its band with `MINVALUE`/`MAXVALUE` so exhaustion
  fails loud (`nextval: reached maximum value of sequence`) instead of
  colliding:
  | partition | band | sequence bound |
  |---|---|---|
  | `db.part/db`   | `[0, 1e6)`     | `MAXVALUE 999999` |
  | `db.part/user` | `[1e6, 1e12)`  | `MAXVALUE 999999999999` |
  | `db.part/tx`   | `[1e12, 2e12)` | `MAXVALUE 1999999999999` |

  This also fixes the intermittent `concurrency_tests` failures
  (`multi_partition_interleaved_allocation`,
  `allocate_entid_uniqueness_per_partition`): the bands are now disjoint by
  construction, and the concurrency test setup resets the sequences so the
  shared-instance / non-transactional-sequence drift between pgrx tests can
  no longer perturb the assertions.

### Upgrading

```sql
ALTER EXTENSION pg_mentat UPDATE TO '1.5.6';
```

Existing stores cannot be re-banded in place (their user ids already sit
directly below the old tx band), so the migration does the safe subset:
it bounds `db`/`user` at their existing band ceilings (fail-loud on
exhaustion) and raises the `tx` ceiling far upward (to 1e12), removing the
exhaustion time bomb without moving any live id. No id is relocated; data
is untouched. Fresh installs get the full new layout.

## [1.5.5] - 2026-07-06

### Fixed

- **VAET index on all value tables.** The transact / lookup-ref resolution
  probe `SELECT e ... WHERE store_id=? AND a=? AND v=?` (resolve an entity
  id from a known attribute+value) fires once per resolvable ref/upsert
  value inside `mentat.t`, on every value type. Only `datoms_ref_new` and
  `datoms_keyword_new` shipped a VAET index `(store_id, v, a, e, tx)`; the
  other seven value tables (`text`, `long`, `double`, `instant`, `uuid`,
  `bytes`, `boolean`) resolved this by scanning the AEVT index on
  `(store_id, a)` and filtering by `v`. On a high-fanout attribute (millions
  of rows per `(store_id, a)`) that scan is pathological. A production
  operator measured ~30x on a 1.1M-row attribute; the probe runs inside
  `mentat.t`, so it directly dominated write-path latency. All nine value
  tables now carry the VAET index. The value column is already part of each
  table's primary key, so there is no new index-row-width risk for text or
  bytes. (Reported with measurements by the agora / pg.ddx.io operator.)
- Added a `schema_introspection` regression test asserting every value table
  has its VAET index, so the gap cannot silently return.

### Upgrading

```sql
ALTER EXTENSION pg_mentat UPDATE TO '1.5.5';
```

The migration creates the seven missing VAET indexes with
`CREATE INDEX IF NOT EXISTS`. **`ALTER EXTENSION` runs in a transaction, so
these cannot be `CONCURRENTLY`** and a plain `CREATE INDEX` takes a write-
blocking `SHARE` lock for the build. On installs with large existing value
tables under heavy ingest, build the indexes `CONCURRENTLY` out-of-band
first (then the migration's `IF NOT EXISTS` builds are no-ops):

```sql
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_datoms_text_new_vaet
    ON mentat.datoms_text_new (store_id, v, a, e, tx) WHERE added;
-- repeat for long/double/instant/uuid/bytes/boolean as your data warrants
ALTER EXTENSION pg_mentat UPDATE TO '1.5.5';
```

## [1.5.4] - 2026-06-29

### Fixed

- `clippy::let_and_return` in `lookup_by_ident` (the 1.5.3 read-only-SPI
  conversion left a redundant `let result = ...; result`). The 1.5.3 tag's
  `clippy -D warnings` CI job failed on this; the test suites and builds
  passed. CI-only — the compiled module is functionally identical to 1.5.3.

### Upgrading

```sql
ALTER EXTENSION pg_mentat UPDATE TO '1.5.4';
```

No-op migration (no schema change).

## [1.5.3] - 2026-06-29

### Fixed

Completes and corrects the hot-standby read path begun in 1.5.2. No schema
or SQL-object change; compiled-module only.

- **Restored `mentat.q` / `mentat_query` on the primary.** 1.5.2 routed the
  query path through read-only SPI, which flips the transaction read-only
  before `apply_optimizer_hints` runs; its `SET LOCAL` resource hints
  (issued through SPI) were then rejected with "SET is not allowed in a
  non-volatile function", breaking every Datalog query. The resource-limit
  GUCs (`statement_timeout`, `temp_file_limit`, `enable_seqscan`,
  `work_mem`) are now set via `pg_sys::set_config_option` with
  `GUC_ACTION_LOCAL` instead of a SQL `SET`, which is permitted in a
  read-only / recovery transaction and reverts at transaction end.
- **Completed standby coverage of the read path.** The remaining read-side
  store-id / lookup resolutions now use read-only SPI, so they run on a
  hot-standby too: `mentat.pull`, `mentat.entity`, `:as-of` / `:since`
  time-travel queries, `mentat.lookup_by_ident`, and the `has_<ext>()`
  extension-detection helpers. Write paths (transactions, excision,
  entid allocation, cache-generation bump) keep mutable SPI; they cannot
  run on a standby regardless.

### Upgrading

```sql
ALTER EXTENSION pg_mentat UPDATE TO '1.5.3';
```

No-op migration (no schema change); it exists only so the `ALTER EXTENSION`
command succeeds and the recompiled module is picked up.

## [1.5.2] - 2026-06-29

### Changed

Makes the Datalog read query path (`mentat_query` / `mentat.q` / the view
helpers) run on a PostgreSQL hot-standby (read-only replica). The read path
previously used pgrx's mutable SPI (`Spi::connect_mut`), which assigns a
transaction id and fails on a standby with "cannot assign TransactionIds
during recovery". It now uses read-only SPI.

Compiled-module only: no schema or SQL-object change. (Note: 1.5.2's
resource-hint handling broke `mentat.q` on the primary; fixed in 1.5.3 —
prefer 1.5.3.)

### Upgrading

```sql
ALTER EXTENSION pg_mentat UPDATE TO '1.5.2';
```

## [1.5.1] - 2026-06-18

### CI / maintenance release

No schema, SQL-object, or query/transaction behavior changes relative to
1.5.0; the recompiled extension is functionally identical. This release
greens the CI pipeline and refreshes tooling.

### Changed

- CI Rust toolchain pinned to `1.90.0` (was `1.88.0`). pgrx 0.17 uses
  `NonNull::from_mut`, stable since 1.89, so 1.88 failed to compile pgrx.
- `cargo fmt --all` applied across the workspace (the CI format check had
  drifted on ~115 files).
- `tokio-postgres` → 0.7.18 and `postgres-protocol` → 0.6.12 in the
  `mentatd` client, closing RUSTSEC-2026-0178/0179/0180 (DoS). These are
  not in the PostgreSQL extension's runtime.
- Added per-crate `license = "Apache-2.0"` to `mentat_core` and
  `core_traits`.

### Fixed (CI)

- 1.90 clippy lints in production code (`doc_overindented_list_items`,
  `neg_cmp_op_on_partial_ord`; the latter keeps NaN rejection in the
  `geom-within` radius check).
- GitHub `docs` workflow: version-pinned the mdBook download URL; enabled
  GitHub Pages on the mirror.
- Container workflow: build `Dockerfile` (not the nonexistent
  `Containerfile`), added the `demo.sql` the image references, glob the
  versioned base SQL, and test by querying the running image (the lean
  runtime image has no Rust toolchain).
- `cargo pgrx test` jobs (GitHub and Nix): install into a writable
  pgrx-managed PostgreSQL rather than a root-owned system / read-only
  nix-store one, which had caused every test to abort.
- Security-audit job: added `deny.toml` (license allow-list + advisory
  triage) and `--ignore` for the four advisories with no clean fix
  (transitive via pgrx / prometheus).
- Optional-extension test suites (pgvector, rum, fuzzystrmatch, pg_trgm):
  route the speculative `CREATE EXTENSION` through a subtransaction helper
  so a missing third-party extension skips cleanly instead of poisoning
  the test transaction.
- Nix flake: `export -f` the dev-shell helpers; writable `CARGO_HOME` /
  `PGDATA`; provide `pg_config` via `postgresql.pg_config`; add
  readline/zlib/icu `.dev` outputs for from-source PG builds.

### Upgrading

```sql
ALTER EXTENSION pg_mentat UPDATE TO '1.5.1';
```

The migration is a no-op (no schema change); it exists only so the
`ALTER EXTENSION` command succeeds.


### The "Append-Only Datom Log" release

Makes the datom log a true immutable append-only log and adds the
Datomic-compatible `:db/noHistory` attribute class. Driven by the same
production feedback as 1.4.0: the instant-datom bloat was rooted in
(a) an in-place `added`-flip on retraction that violated the datom
model and (b) keeping full history of monotonic timestamps that change
every sync. Both are now fixed structurally.

This is a **storage-model change** with a required upgrade migration
(see Upgrading). Current-time query results are unchanged; the
internal representation of history is not.

### Changed — append-only datom log

- **Retraction no longer flips the prior assertion in place.** A
  retraction is now a new immutable `(e, a, v, tx, false)` datom; the
  original `(e, a, v, tx0, true)` row is preserved unchanged. This
  resolves the 1.4.0 "redundant retraction row" Known Issue at its
  root — the in-place flip *was* the bug; appending the retraction is
  the correct Datomic behavior.
- **Current-time queries read a maintained current-state projection**
  (nine `mentat.current_<type>` tables) instead of resolving
  latest-tx-wins over the full log. `:as-of` / `:since` / history
  queries continue to read the append-only log.
- **`fillfactor` 85/90 → 100** on the nine `datoms_*_new` log tables.
  Append-only tables never update in place, so the reserved
  HOT-update space the old flip required is pure waste.

### Added — current-state projection (the read path)

- Nine `mentat.current_<type>` tables holding only live datoms,
  maintained in lock-step with the log inside each transaction.
- `mentat.current_datoms` view (union over the nine, legacy
  `datoms`-shaped columns) for callers needing current state.
- `mentat.rebuild_current_projection(store)` — repopulate from the log
  (used by the upgrade and for recovery).
- `mentat.verify_current_projection(store)` — returns the count of
  rows where the projection disagrees with a fresh latest-tx-wins
  resolution of the log; `0` means consistent. Used as the cutover
  safety gate and in tests.

### Added — `:db/noHistory` attributes

- Datomic-compatible `:db/noHistory true` attribute flag. A noHistory
  attribute keeps **only the current value**: each assertion
  physically replaces the prior value in the log and projection
  instead of appending a retraction + assertion. The structural fix
  for monotonic-attribute bloat (`:last-seen` / `:observed-at`):
  10 updates leave 1 log row, not ~20.
- Per-attribute and per-cardinality (one and many). Current-time
  queries behave identically to a normal attribute; `:as-of` sees
  only the current value (the trade for zero bloat).

### Fixed (exposed by the conversion)

- `:db.fn/cas` read the current value via `datoms WHERE added=true`,
  which in the append-only model returns superseded historical
  assertions too. CAS now reads `mentat.current_datoms`.
- `batch_insert_datoms` dedups by full PK `(e,a,v,tx,added)`: CAS
  queues a retraction and the cardinality-one replace path
  independently queues the same one; a single `INSERT ... ON CONFLICT`
  cannot list a key twice.
- `is_duplicate_cardinality_many` reads the projection (presence ==
  live) rather than an `added=true` log scan.
- `pull`, `(fulltext)`, and the extension-search where-fns
  (`(fuzzy-match)`/pg_tre, `(similar-to)`/pg_trgm, `(rum-fulltext)`/rum,
  `(infer-near)`/pg_infer) read the current-state projection, so they
  no longer return values that were replaced or retracted. The
  extension index helpers (`create_trgm_index`, `create_rum_fulltext_index`,
  …) now build on `mentat.current_text` rather than the log table.
- Reverse-reference (`:ns/_attr`) and recursive-reference pull
  traversals read the projection instead of the append-only log.

### Fixed — query/transaction correctness (fail-loud)

- `(pull ?e [...])` inside a `:find` clause is now implemented; it
  previously produced a NULL column. The result nests as a JSON object.
- The `mentat.edn` type's text input parsed nothing (returned NULL for
  all valid EDN); it now round-trips raw EDN via an explicit I/O impl.
- A scalar supplied through `:in` and used only in a predicate (e.g.
  `[(>= ?age ?min)]`) was rejected as unbound; it now binds correctly.
- An unknown transaction op, a malformed assertion, an incomplete
  schema-attribute definition, an unbound `:find` variable, and an
  unknown attribute in a query now all fail loud with a
  `:db.error/*` message instead of silently returning wrong or empty
  results. A bare `:db/ident` (naming a non-attribute entity, e.g. an
  enum value) is no longer mis-flagged as an incomplete attribute.

### Tests

- `current_projection_tests` (8), `no_history_tests` (6) — all green.
- `history_tests::test_hi_many_retract_history` (failing on 1.4.0)
  now passes.
- **Full suite is green: 1829 passed, 0 failed.** The pre-existing
  test debt (108 failures across ~30 suites — obsolete `idx_datoms_*`
  introspection from the storage redesign plus scattered functional
  rot) has been cleared, partly by retargeting stale tests to the
  narrow-table / projection model and partly by the fail-loud fixes
  above (several failures were correct tests guarding real bugs).
- The `1.4.0 → 1.5.0` in-place upgrade is qualified end to end:
  install 1.4.0, load data, `ALTER EXTENSION … UPDATE TO '1.5.0'`,
  then `verify_current_projection(0) = 0` and current-time queries
  return results identical to pre-upgrade.

### Upgrading

```sql
ALTER EXTENSION pg_mentat UPDATE TO '1.5.0';
```

The migration creates the projection tables, retunes the log tables to
`fillfactor=100`, and — **required** — runs
`mentat.rebuild_current_projection(0)` to populate the projection from
the existing log. Without that population step, current-time queries
return nothing. The migration handles it automatically; if you build a
store by other means, call `rebuild_current_projection` yourself.

Pre-1.5.0 history was flip-based; those rows remain as-is. The
projection is rebuilt by latest-tx-wins resolution, which is correct
against both flip-era and append-only-era history. All retractions
going forward are appended, never flipped.

## [1.4.0] - 2026-06-16

### The "Production Throughput & Bloat" release

Driven by production feedback from an 82 GB store used as a
community-stats identity backbone. Focus: `mentat.t()` ingest
throughput, narrow-table autovacuum, and a cheap live-projection read
path. No new query surface; no breaking changes.

### Performance

- **Cardinality-one assertion fast path.** `mentat.t()` previously ran a
  9-way `UNION ALL` probe per cardinality-one datom to find the current
  value (to decide assert / replace / skip). Because the new value's
  type is always known and a cardinality-one attribute's type is fixed,
  the current value lives in exactly one narrow table. The probe is now
  a single indexed lookup on that table's `(store_id, e, a, tx DESC)
  WHERE added` covering index. Measured **~1.8x speedup** (6.2 s -> 3.4
  s) on a 2,000-call cardinality-one re-assertion microbenchmark. The
  residual per-call cost is fixed tx-allocation overhead, amortized by
  batching more facts per `t()`.

### Operations

- **Autovacuum retune on all narrow tables + `transactions`.** The
  previous `autovacuum_vacuum_scale_factor = 0.05` (and the PG default
  0.2) effectively stops triggering on large tables, so they bloat
  without bound — most visibly `datoms_instant_new`, where monotonic
  attributes (`:first-seen` / `:last-seen` / `:observed-at`) are
  re-asserted every sync. All nine `datoms_*_new` tables and
  `mentat.transactions` now ship with `scale_factor = 0` + a fixed
  `threshold = 50000`, so autovacuum fires on a constant dead-tuple
  count regardless of table size. Applied by `CREATE EXTENSION` and the
  1.3.0->1.4.0 upgrade.

### Added — operational + read-path accessors

- `mentat.attr_id(':ns/name')` — resolve an attribute keyword to its
  entid for use in SQL / views (STABLE), so generated viewdefs read
  `a = mentat.attr_id(':person/name')` instead of `a = 1308861`.
- `mentat.current(e, a)` and `mentat.current(e, ':ns/name')` —
  index-backed "current value of attribute A for entity E" as TEXT,
  dispatching on the declared value type so only one narrow table is
  touched. Replaces per-query `DISTINCT ON` / `LATERAL` fan-out in
  consumer views.
- `mentat.attribute_health()` — per-attribute live datom count plus the
  backing narrow table's dead-tuple %, so operators can alert on bloat
  before it bites.

### Documentation

- New `docs/src/operations.md`: throughput (batching strategy, the
  cardinality-one fast path, idempotent-reassert no-op), bloat (the
  default-scale-factor trap, the 1.4.0 autovacuum defaults, reclaiming
  existing bloat, monitoring with `attribute_health()`), and the live
  projection (`mentat.current` / `mentat.attr_id`, a maintained
  current-state partial-index pattern). Explicitly addresses the
  "is this an auto-indexing problem?" question: it is not — the indexes
  already exist; the costs are per-tx overhead, history resolution, and
  bloat.

### Fixed

- Removed a dead `insert_typed_datom` function (superseded by the
  batch-insert path) to restore the zero-warnings build.

### Known issue (reported, not yet changed — needs a semantics decision)

- A cardinality-one **replace** writes a redundant `(e, a, old_v, tx,
  false)` retraction datom *in addition to* flipping the original
  assertion row's `added` flag in place. This double-counts retraction
  churn (one extra dead row per replace) and contributes to the
  instant-datom bloat above. Fixing it changes history-replay semantics
  (whether the tx log carries an explicit retraction datom), so it is
  deliberately left for a maintainer decision rather than changed
  silently. Tracked for 1.5.0.

### Tests

- `operational_accessors_tests` (6 `#[pg_test]`): `attr_id` resolution,
  `current()` latest-value + NULL-absent + post-replace, the
  cardinality-one fast-path correctness (exactly one live datom after
  repeated replaces), idempotent-reassert no-churn, and
  `attribute_health()` counts.
- Regression: `comprehensive_upsert_tests` (17/17),
  `comprehensive_retract_tests` (22/22), `cross_entity_tests` (14/14)
  all green — the fast path preserves cardinality / upsert / retract
  semantics.

### Upgrading

```sql
ALTER EXTENSION pg_mentat UPDATE TO '1.4.0';
```

The upgrade retunes autovacuum and installs the new accessors. To
reclaim *existing* bloat (storage params only affect future
triggering), run `VACUUM FULL` or `pg_repack` on the affected tables —
see `docs/src/operations.md`.

## [1.3.0] - 2026-05-14

### The "Postgres Extension Family" release

This release lands ten extension integrations that turn pg_mentat into a
Datalog hub for the Postgres ecosystem. Every integration is a SOFT
dependency: nothing pg_mentat ships requires the upstream extension; each
integration gates on a `mentat.has_<ext>()` detection helper. Where-fns
generate SQL that calls the upstream extension's operators directly;
queries fail at execution (not compilation) when the extension isn't
loaded.

### Added — Datalog where-fns

Search and ranking:

- `(rum-fulltext $ :attr "term") [[?e ?val ?score]]` — BM25-style ranked
  fulltext via [rum](https://github.com/postgrespro/rum) (PostgreSQL
  license; the permissive alternative to ParadeDB's AGPL `pg_search`).
- `(similar-to $ :attr "needle" threshold) [[?e ?val ?score]]` —
  trigram similarity via [pg_trgm](https://www.postgresql.org/docs/current/pgtrgm.html).
- `(levenshtein ?a ?b) ?d`, `(soundex ?s) ?c`, `(metaphone ?s ?n) ?c`,
  `(daitch-mokotoff ?s) ?c` — phonetic and edit-distance functions
  via [fuzzystrmatch](https://www.postgresql.org/docs/current/fuzzystrmatch.html).

Vector & semantic:

- `(vector-near $ :attr "[v1,v2,...]" k [:cosine|:l2|:inner]) [[?e ?dist]]`
  — KNN via [pgvector](https://github.com/pgvector/pgvector). Side-table
  aux pattern (`mentat.attach_vector_attribute`, `set_vector`,
  `del_vector`, `create_hnsw_vector_index`).
- `(infer-near $ :attr "text" k [:model]) [[?e ?dist]]` — top-K KNN by
  model knowledge via [pg_infer](https://codeberg.org/gregburd/pg_infer)'s
  `<~>` operator.
- `(infer-similar a b) ?score`, `(infer-implies a b) ?bool` — scalar
  pg_infer model functions.
- `(infer-walk "prompt" top) [[?layer ?feature ?score ?concept]]`,
  `(infer-describe "entity") [[?relation ?target ?score ?layer]]`,
  `(infer-predict "prompt" top) [[?token ?prob ?rank]]` — set-returning
  pg_infer verbs.

Geospatial:

- `(geom-near $ :attr "WKT" k) [[?e ?dist]]` — KNN by `ST_Distance`.
- `(geom-within $ :attr "WKT" radius) [[?e ?dist]]` — within-distance
  via `ST_DWithin`.
- `(geom-contains $ :attr "WKT") [[?e]]` — `ST_Contains`.
- `(geom-intersects $ :attr "WKT") [[?e]]` — `ST_Intersects`.
  All via [PostGIS](https://postgis.net/), with side-table aux pattern
  (`attach_geometry_attribute`, `set_geometry`, `del_geometry`,
  `create_gist_geometry_index`) and automatic SRID detection from
  `geometry_columns`.

### Added — SQL helpers (no Datalog surface)

Eleven new SQL-helper modules (`pg_mentat/sql/12_*.sql` through
`pg_mentat/sql/22_*.sql`). Each declares a `mentat.has_<ext>()` detection
function plus extension-specific helpers (index management, side-table
attachment, etc.). The full helper inventory:

| Extension | Detection | Headline helpers |
|:---|:---|:---|
| pg_tre | `has_pg_tre` | (existing) |
| fuzzystrmatch | `has_fuzzystrmatch` | (where-fns only) |
| pg_trgm | `has_pg_trgm` | `create_trgm_index`, `drop_trgm_index` |
| rum | `has_rum` | `create_rum_fulltext_index`, `drop_rum_fulltext_index` |
| pgvector | `has_pgvector` | `attach_vector_attribute`, `set_vector`, `del_vector`, `create_hnsw_vector_index` |
| PgQue | `has_pgque` | `pgque_emit_tx`, `pgque_disable_tx`, `pgque_register_consumer` |
| pg_infer | `has_pg_infer` | `create_infer_index`, `drop_infer_index` |
| PostGIS | `has_postgis` | `attach_geometry_attribute`, `set_geometry`, `del_geometry`, `create_gist_geometry_index`, `detach_geometry_attribute` |
| PG19 SQL/PGQ | `has_pg19_graph` | `create_vertex_view`, `create_edge_view`, `drop_*_view`, `create_property_graph_ddl` |
| TimescaleDB | `has_timescaledb` | `timescale_attach_transactions`, `timescale_attach_instant_datoms`, `timescale_set_transaction_retention` |
| pg_partman | `has_pg_partman` | `partman_attach_transactions`, `partman_set_transaction_retention`, `partman_run_maintenance` |
| pg_cron | `has_pg_cron` | `cron_schedule`, `cron_unschedule`, `cron_schedule_partman_maintenance`, `cron_schedule_vacuum_datoms` |

### Added — transactional event stream

PgQue (NikolayS/PgQue) integration: `mentat.pgque_emit_tx('queue')`
attaches a deferred constraint trigger to `mentat.transactions` that
emits one `mentat.tx`-typed PgQue event per pg_mentat transaction. Event
payload is JSON: `tx`, `tx_instant`, `store_id`, `datom_count`, plus a
full `datoms[]` array with `(e, a, v, vt, tx, added)`. PgQue is
pure-PL/pgSQL — works on managed Postgres providers without
`shared_preload_libraries` or restarts.

### Added — documentation

Twelve new cookbook pages under `docs/src/`:

- `fuzzy-search.md` (pg_tre — pre-existing, polished)
- `fuzzystrmatch.md`, `pg-trgm.md`, `rum.md`, `pgvector.md`,
  `postgres-fdw.md`, `pgque.md`, `pg_infer.md`, `postgis.md`,
  `pg19_graph.md`, `timescaledb.md`, `pg_partman.md`, `pg_cron.md`

`docs/INTEGRATIONS.md` was the planning doc at the start of this work
and is now maintained as the integration tracker, with everything in
this release moved from the Tier 1 / Tier 2 / Tier 3 buckets to Done.

### Fixed

- **FtsJoin entity-binding bug.** The pre-existing FTS where-fns
  (`fulltext`, `fuzzy-match`) bound their entity variable into
  `extra_var_bindings` only, not `var_to_alias`. Subsequent EAV
  patterns referencing the same `?e` failed to JOIN; cartesian
  products were silently masked by `SELECT DISTINCT` whenever the
  projected columns happened to collapse identically. Verified on a
  query that returned 9 rows when 3 were correct; the fix returns 3.
  All five FTS-style builders (`fulltext`, `fuzzy-match`, `similar-to`,
  `rum-fulltext`, `vector-near`, `infer-near`, `geom-near`,
  `geom-within`, `geom-contains`, `geom-intersects`) now propagate
  their entity binding into `var_to_alias` via a new
  `FtsJoin.entity_alias` field, populated before pattern processing.

### Tests

| Integration | Tests | Result |
|:---|:---:|:---:|
| pg_tre (fuzzy-match) | 7 | 7/7 |
| fuzzystrmatch | 7 | 7/7 |
| pg_trgm | 7 | 7/7 |
| rum | 6 | 6/6 |
| pgvector | 9 | 9/9 |
| PgQue | 5 | 5/5 |
| pg_infer | 10 | 10/10 |
| PostGIS | 10 | 10/10 |
| Infra (pg19, ts, partman, cron) | 13 | 13/13 |
| **Total integration tests** | **74** | **74/74** |

Smoke (`scripts/smoke.sh`): 11/11 PASS throughout.

### Upgrading

`pg_mentat--1.2.1--1.3.0.sql` ships all eleven new helper-SQL modules
as a single forward-only migration. The where-fn additions live in the
loadable library and require no SQL upgrade.

```sql
ALTER EXTENSION pg_mentat UPDATE TO '1.3.0';
```

### License notes

- **rum**: PostgreSQL license. Use this instead of ParadeDB's AGPL
  `pg_search` for BM25-style ranking in commercial deployments.
- **PostGIS**: GPL-2.0+. Same as previous releases.
- **PgQue**: Apache 2.0.
- **pg_infer**: Apache 2.0. Experimental; PG18+; no managed-Postgres
  provider ships it yet.

## [1.2.1] - 2026-05-13

Storage redesign + pg_tre integration. Wide-row `mentat.datoms` is now
a VIEW over 9 narrow per-type tables with INSTEAD OF INSERT/DELETE
triggers; `store_id` widened to BIGINT. pg_tre integration shipped
with `(fuzzy-match)` where-fn for approximate-regex search.

See `git log v1.2.0..v1.2.1` for full detail.

## Earlier

For releases prior to 1.2.1, see `git log` and the migration scripts
in `pg_mentat/sql/`.
