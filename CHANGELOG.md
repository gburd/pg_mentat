# Changelog

All notable changes to pg_mentat are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/).

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
