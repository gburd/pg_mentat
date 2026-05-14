# Postgres Extension Integrations — Plan

This file enumerates Postgres extensions that pg_mentat **could**
integrate with, in the same opt-in / soft-dependency shape as the
already-shipped pg_tre integration. Each entry says what it adds, the
integration shape (Datalog where-fn? new value type? helper SQL? index
type on narrow tables?), license/build compatibility, effort, and
whether it should land.

This is a planning document, not a marketing list. Extensions that don't
add real Datalog value have been excluded. The file is the source of
truth for "what integrations does pg_mentat ship?" — when an integration
lands, mark it `Done` and link to the cookbook page.

## Reference: the pg_tre integration (already shipped)

Use this as the template for everything below.

- **Soft dependency.** `CREATE EXTENSION pg_mentat` does not require
  pg_tre. `mentat.has_pg_tre()` returns `true` when the extension is
  loaded; helper functions error with a specific `:db.error/*` code
  pointing at install instructions when it isn't.
- **One Datalog where-fn.** `(fuzzy-match $ :attr "pattern" k)` mirrors
  the shape of `(fulltext ...)` and slots into the same dispatch
  (`build_fuzzy_match_join` next to `build_fulltext_join`).
- **One helper SQL.** `mentat.create_tre_index('<:attr>')` builds a
  partial pg_tre index on the right narrow table.
- **Tests.** Happy path and every error path, gated on
  `mentat.has_pg_tre()` so the suite passes whether or not the test
  cluster has the extension installed.
- **Docs.** `docs/src/fuzzy-search.md` covers prerequisites, worked
  example, error table, explicit non-features.

Total cost: ~600 lines of Rust + SQL + docs + tests. One commit, one
review pass. This is the cost ceiling for a single integration.

---

## Tier 1 — high-value, well-scoped, should land

### pgvector — `:db.type/vector` + `(nearest-neighbor ...)`
**Status:** Open. **Effort:** 2 weeks. **Maintainer:** pgvector/pgvector. **License:** PostgreSQL.

The single most valuable integration in 2026. Nothing else in the
Postgres-Datalog space stores embeddings as datoms.

Integration shape:

- New value type `:db.type/vector`. Encodes dimension as part of the
  attribute schema (`:db/vector-dim 1536`); rejects insert of mismatched
  vectors with `:db.error/wrong-vector-dim`.
- New tenth narrow table `mentat.datoms_vector_new (store_id BIGINT, e
  BIGINT, a BIGINT, v vector, tx BIGINT, added BOOLEAN)`. Per-attribute
  HNSW or IVFFlat index via a new helper
  `mentat.create_vector_index('<:attr>', 'hnsw'|'ivfflat', '<opts>')`.
- Datalog where-fn `(nearest-neighbor $ :attr ?vec k)` returning the k
  nearest entities by cosine distance. Distance op is configurable per
  index (cosine / L2 / inner-product); pick from the attribute's index
  metadata.
- The `:db/valueType` enum gets a new entry: `:db.type/vector`.
  Existing bootstrap entids 70–78 are taken; assign `:db.type/vector`
  entid 79 (free).

Hard parts:

- New value type means new code paths in `transact.rs` (encoding),
  `query.rs` (the UNION subquery, the value-decode CASE, predicate
  comparison), `pull.rs` (decode), the dual-write trigger if it still
  exists by the time we land this. ~150–200 line touchpoints.
- Storing dimension in the attribute schema requires either a new
  `:db/vector-dim` meta-attribute (preferred) or schema
  `value_type` becoming `value_type | NULL` plus a new column. Pick
  the meta-attribute approach.

Done criteria:

```
SELECT mentat.t('[
  {:db/ident :doc/embedding
   :db/valueType :db.type/vector
   :db/vector-dim 1536
   :db/cardinality :db.cardinality/one}]');
SELECT mentat.create_vector_index(':doc/embedding', 'hnsw',
                                   'm=16,ef_construction=64');
SELECT mentat.q('[:find ?e ?dist
                  :where [(nearest-neighbor $ :doc/embedding
                           [0.0,0.0,...] 10) [[?e ?dist]]]]');
```

### PostGIS — `:db.type/geometry` + `(within ...)` / `(near ...)`
**Status:** Open. **Effort:** 2–3 weeks. **Maintainer:** OSGeo/postgis. **License:** GPL-2.0.

PostGIS is the most-used Postgres extension after pgcrypto. Geographic
data in EAV is a common shape (places-of-business, asset locations,
event coordinates).

Integration shape:

- New value types `:db.type/geometry` and `:db.type/geography`.
- New eleventh and twelfth narrow tables for each (or one shared with a
  type-tag column; pick whichever fits better with PostGIS's GiST index
  semantics).
- Per-attribute GiST index via `mentat.create_spatial_index('<:attr>')`.
- Datalog where-fns:
  - `(within $ :attr ?shape)` — datoms whose value is within the given
    geometry. Compiles to `ST_Within`.
  - `(near $ :attr ?point ?radius_m)` — within radius (meters), uses
    `<->` operator on the GiST index for k-NN ordering when used with
    `:order`.
  - `(distance $ :attr ?point) ?d` — bind ?d to the meters distance
    for use in predicates.

Hard parts:

- GPL-2.0 license. pg_mentat is Apache-2.0. PostGIS is a runtime
  dependency, not a derivative-work issue, so this is fine for the
  integration; document the license boundary.
- WKT vs WKB input. Decide one canonical form for EDN-encoded geometry
  literals. Probably WKT inside a `#geom "POINT(1 2)"` tagged literal,
  with a constructor function `mentat.geom('WKT')` for SQL callers.

Done criteria: a cookbook page that does point-in-polygon queries
mixing datoms and a GeoJSON polygon parameter.

### TimescaleDB — `mentat.transactions` and `datoms_instant_new` as hypertables
**Status:** Open. **Effort:** 1 week. **Maintainer:** timescale/timescaledb. **License:** Apache-2.0 (TSL parts: TSL).

The transaction log and the instant-typed datom table are both
naturally time-partitioned. Timescale-style hypertables would give:

- Automatic time-bucketed partitioning with proper chunk pruning.
- Continuous aggregates for time-series Datalog queries (count of
  retractions per hour, per attribute).
- `time_bucket` available inside generated SQL for queries that group
  by time window.

Integration shape:

- Helper SQL `mentat.enable_timescale(target text)` where `target ∈
  {'transactions', 'datoms_instant_new', 'all'}`. Calls
  `create_hypertable` with sensible chunk-time-interval defaults
  (probably one week).
- A `mentat.transaction_window(?from, ?to)` Datalog where-fn that
  generates SQL using `time_bucket` when the underlying table is a
  hypertable, falling back to a plain range scan otherwise.
- No new value types or narrow tables. Pure operational integration.

Hard parts:

- Hypertables impose constraints on referenced rows (no foreign keys
  pointing into them, etc.). Verify the dual-write VIEW on
  `mentat.datoms` still works once `datoms_instant_new` is a
  hypertable.
- TimescaleDB's TSL license vs the Apache-2.0 community edition. We
  should target the community edition to avoid licensing surprises.

Done criteria: `enable_timescale('transactions')` succeeds; a
`time_bucket('1 day', tx_instant)` query against `mentat.transactions`
runs against chunks; benchmark shows retraction-window queries scale
linearly with window size, not total tx count.

### fuzzystrmatch — `(soundex ...)`, `(levenshtein ...)`, `(metaphone ...)`
**Status:** Open. **Effort:** 1–2 days. **Maintainer:** PostgreSQL contrib. **License:** PostgreSQL.

Built-in `contrib` module. PG13+. No `shared_preload_libraries`. No
runtime cost when not used. The cheap, no-friction alternative to
pg_tre for name-search and other typo-tolerant queries on PG13–17.

Integration shape:

- Three new Datalog where-fns:
  - `[(levenshtein ?a ?b) ?d]` — bind ?d to the integer edit distance
    between two text values. Pure function, can be used in predicates
    (e.g. `[(< ?d 3)]` for "within 2 edits").
  - `[(soundex ?s) ?code]` — bind ?code to the Soundex hash for
    name matching.
  - `[(metaphone ?s ?max) ?code]` — bind ?code to the Metaphone
    phonetic encoding.
- All three compile to function calls with `extension_required` =
  `fuzzystrmatch`. Helper `mentat.has_fuzzystrmatch()` returns true if
  the extension is loaded.
- No index needed; these are scalar functions, called per row.

Hard parts: none. Smallest integration on the list.

### pg_partman — declarative partition management of retraction tombstones
**Status:** Open. **Effort:** 1 week. **Maintainer:** pgpartman/pg_partman. **License:** PostgreSQL.

Operational rather than feature work. Long-running pg_mentat
deployments accumulate retracted-datom tombstones (`added = false`
rows) that grow indefinitely. pg_partman partitions a parent table by
range/list/hash and runs maintenance jobs to drop old partitions or
move them to cheap storage.

Integration shape:

- Per-narrow-table opt-in: `mentat.enable_partman(table_name,
  partition_by, retention)`. For example:
  ```
  SELECT mentat.enable_partman('datoms_instant_new', 'tx', '90 days');
  ```
  partitions `datoms_instant_new` by tx range and drops partitions
  older than 90 days of tx history.
- The narrow tables already have `(store_id, e, a, tx)` primary keys;
  `tx` is monotonic so range-partitioning by `tx` is straightforward.
- A pg_cron job (see entry below) runs `partman.run_maintenance()` on
  a schedule.

Hard parts:

- Excision (the GDPR "forget me" path) and partition drops have to be
  coordinated; otherwise an excise call may target a partition that's
  about to be dropped, or vice versa. Document the order.
- Existing data in non-partitioned narrow tables has to be migrated
  in-place — pg_partman has tooling for this but it's not free.

Done criteria: partman maintenance on `datoms_instant_new` drops
partitions older than the configured retention; a benchmark shows
storage growth flattens after the retention window.

---

## Tier 2 — useful and reasonably scoped, should land after Tier 1

### pg_trgm — `(similar-to ...)` where-fn
**Status:** Open. **Effort:** 3–5 days. **License:** PostgreSQL (contrib).

Built-in. Trigram similarity (set-based, not edit-distance). Different
semantics from pg_tre and from fuzzystrmatch's Levenshtein — ranks by
overlap of trigrams, not by edit distance. Useful for "rows whose
attribute is *kind of like* this string" queries.

Shape:
- `[(similar-to $ :attr "needle" threshold) [[?e ?val ?score]]]`.
- Helper `mentat.create_trgm_index('<:attr>')` creates a partial
  `gin_trgm_ops` or `gist_trgm_ops` index on the appropriate narrow
  table.
- Coexists with `(fulltext ...)` and `(fuzzy-match ...)`. Document the
  distinction in `docs/src/text-search-comparison.md`.

### pg_jsonschema — `:db/jsonschema` constraint
**Status:** Open. **Effort:** 3–5 days. **Maintainer:** supabase/pg_jsonschema. **License:** Apache-2.0.

Validates JSON against a JSON Schema. pgrx-based, like pg_mentat.

Shape:
- New attribute-schema option `:db/jsonschema "<schema-text>"`.
- On `mentat_transact`, when an attribute has `:db/jsonschema` set and
  the value is a string, validate the parsed JSON against the schema
  before inserting. Reject with `:db.error/jsonschema-violation`.
- Document why this isn't `:db/predicate "<EDN expression>"`: JSON
  Schema is an industry standard with well-defined semantics, EDN
  predicates aren't.

### pg_cron — scheduled compaction and maintenance
**Status:** Open. **Effort:** 2–3 days. **Maintainer:** citusdata/pg_cron. **License:** PostgreSQL.

Schedule periodic operations: VACUUM on hot narrow tables, partition
maintenance for pg_partman, retraction GC for tombstoned datoms older
than retention, refresh of materialized monitoring views.

Shape:
- Helper SQL `mentat.schedule_maintenance(name, cron_expr, sql_body)`
  that registers a pg_cron job in the right database.
- Built-in jobs: `mentat.scheduled_vacuum`,
  `mentat.scheduled_excision_gc`,
  `mentat.scheduled_partman_run` (registers the partman job).
- Soft dep, gated on `mentat.has_pg_cron()`.

### postgres_fdw + cookbook — query datoms across PG instances
**Status:** Open. **Effort:** 2 days (mostly docs). **License:** PostgreSQL (contrib).

Built-in. No code change in pg_mentat needed; this is a cookbook page
showing how to set up a foreign server pointing at another pg_mentat
instance, import its `datoms_*_new` tables as foreign tables, and run
Datalog-shaped queries that JOIN them.

Shape:
- `docs/src/cookbook-multi-instance-fdw.md`.
- The `mentat_query` function operates on local tables only; cross-
  instance queries are written as plain SQL that JOINs local Datalog
  results with foreign datom tables.
- Optional helper `mentat.import_remote_store(server, store_name)` that
  generates the CREATE FOREIGN TABLE statements for one remote store.

### h3-pg — geographic indexing for PostGIS users
**Status:** Open. **Effort:** 2 days, only after PostGIS lands. **Maintainer:** zachasme/h3-pg. **License:** Apache-2.0.

Uber's H3 hexagonal hierarchical spatial index. Pairs with PostGIS for
fast point-in-region queries at scale. Niche but well-loved.

Shape:
- Helper `mentat.h3_index('<:attr>', resolution)` that adds an H3 cell
  index to a `:db.type/geometry` attribute.
- Datalog where-fn `(in-h3-cell $ :attr ?cell)`.

---

## Tier 3 — interesting, lower priority

### pg_search (ParadeDB) — Tantivy-backed BM25 full-text

**Status:** rejected on license grounds. pg_search is AGPL-3.0; this
is a non-starter for many commercial deployments. **Use `rum`
instead** — it's PostgreSQL-licensed and provides positional ranked
search that is close to BM25 in spirit. See `docs/src/rum.md`.

### pg_duckdb — DuckDB columnar engine inside Postgres
**Effort:** 2–3 weeks. **License:** MIT.

Real value for analytical workloads on the historical datom log.
Speculative until pg_duckdb's API stabilizes; today (mid-2026) it's
still moving fast. Re-evaluate annually.

### citus — sharded multi-store
**Effort:** 4–6 weeks. **License:** AGPL-3.0 (some pieces) / PostgreSQL.

The path to "datoms over many machines." Requires significant rework of
the narrow-table primary keys to include a shard-key column. Phase
A-grade work; not a single integration. Tracked here so it doesn't get
re-discovered every six months.

### pglogical / wal2json — CDC of the datom log
**Effort:** 1 week (smoke test only). **License:** PostgreSQL-style.

The narrow tables are append-mostly (retractions are also INSERTs with
`added = false`); they replicate cleanly via either path. The work is
operational documentation more than code. Already on the existing
roadmap as "Phase 6 CDC."

### plv8 / plpython3u — user-defined Datalog where-fns
**Effort:** 2 weeks. **License:** PostgreSQL.

Lets users define custom predicates and functions in JS or Python. The
sandboxing story is the question — `plpython3u` is `u` (untrusted) for
a reason. Probably not worth the security review unless someone is
asking.

### pg_stat_statements — query observability
**Effort:** already integrated as opt-in. **License:** PostgreSQL.

The Phase 2 benchmark harness uses pg_stat_statements when available
to capture top-10 slowest statements. Done.

### pgaudit — DDL/DML audit logging
**Effort:** 1 day (cookbook). **License:** PostgreSQL.

pg_mentat already has datom-level audit semantics built in. pgaudit
adds defense-in-depth at the SQL level (catches direct writes to
narrow tables that bypass `mentat_transact`). Cookbook page is enough.

---

## Anti-list — extensions explicitly NOT worth integrating

| Extension | Reason |
|---|---|
| `pgcrypto` | Used internally where useful; nothing to expose to Datalog. |
| `pg_uuidv7` / `pg_idkit` | `:db.type/uuid` exists; users supply their own UUIDs. |
| `zhparser` and other tsvector-language plugins | One-line config of `:db/fts-language`; not an integration. |
| `pgrouting` | Datalog rules already do recursive graph traversal. |
| `pg_squeeze` / `pg_repack` | Operational tools; not Datalog-visible. |
| `pg_anonymizer` | Datomic-style retraction + RLS already cover the use case. |
| `pgsql-http` / `pg_net` | Side-effecting from queries is anti-Datalog by design. |

---

## Process

When implementing an entry:

1. Open a tracking issue with the entry's section as the description.
2. Branch `feat/integration-<name>`.
3. Build the SQL helper file `pg_mentat/sql/<NN>_<name>_helpers.sql`
   following the pattern in `11_pg_tre_helpers.sql`.
4. Add Datalog where-fn(s) in `pg_mentat/src/functions/query.rs`
   following the pattern around `build_fuzzy_match_join`.
5. Tests in `pg_mentat/src/<name>_tests.rs`. Gate happy-path tests on
   the `mentat.has_<extension>()` check; assert error paths
   unconditionally.
6. Cookbook page at `docs/src/<name>.md`. Link from `SUMMARY.md`.
7. Update this file: change `Status:` to `Done` with the commit hash
   and link to the cookbook page.
8. PR. Squash. Push. The integration ships exactly as opt-in.

Do not bundle multiple integrations in one PR. Each integration is
independently reviewable; bundling rebuilds the same merge-conflict
problem we keep running into elsewhere.

---

## Done

- **pg_tre** — approximate-regex search via the `(fuzzy-match ...)`
  where-fn. See `docs/src/fuzzy-search.md`. Commit `9ad7650`.
- **fuzzystrmatch** — phonetic and edit-distance scalar functions
  (`levenshtein`, `soundex`, `metaphone`, `daitch-mokotoff`) as
  Datalog where-fns. See `docs/src/fuzzystrmatch.md`. PG13+, no
  preload required.
- **pg_trgm** — trigram-similarity matching via the `(similar-to ...)`
  where-fn, plus partial-GIN index helpers
  (`mentat.create_trgm_index`, `mentat.drop_trgm_index`). See
  `docs/src/pg-trgm.md`. PG13+, no preload required.
- **rum** (postgrespro/rum, PostgreSQL license) — BM25-style ranked
  fulltext via the `(rum-fulltext $ :attr "term")` where-fn, with
  partial-RUM index helpers (`mentat.create_rum_fulltext_index`,
  `mentat.drop_rum_fulltext_index`). The permissive alternative to
  ParadeDB's AGPL `pg_search`. See `docs/src/rum.md`.
- **pgvector** (partial) — K-nearest-neighbor search via the
  `(vector-near $ :attr "[...]" k [op])` where-fn, plus aux-table
  helpers (`attach_vector_attribute`, `set_vector`, `del_vector`,
  `create_hnsw_vector_index`). Side-table design; full
  `:db.type/vector` schema integration is a follow-up. See
  `docs/src/pgvector.md`.
- **(query-side fix bundled with pgvector)** — the FTS-style join
  builders (`fulltext`, `fuzzy-match`, `similar-to`, `rum-fulltext`,
  `vector-near`) now properly JOIN their entity binding to subsequent
  patterns instead of relying on DISTINCT to mask cartesian products.
  Fixes a latent bug exposed by `vector-near`'s per-row varying score.
- **postgres_fdw cookbook** — cookbook page covering cross-database
  Datalog: foreign-table the remote `mentat` schema, push filters
  down via FDW, materialized-view caches, multi-tenant fan-out, and
  the `:in` clause pattern. Pure docs (no new where-fns) since
  postgres_fdw is built-in contrib. See `docs/src/postgres-fdw.md`.
- **PgQue** (NikolayS/PgQue, Apache 2.0) — transactional event
  stream: one `mentat.tx` event per transaction, emitted from a
  deferred constraint trigger on `mentat.transactions`. Helpers:
  `mentat.pgque_emit_tx('queue')`, `mentat.pgque_disable_tx`,
  `mentat.pgque_register_consumer`. Pure-PL/pgSQL queue, no
  extensions, PG14+. See `docs/src/pgque.md`.
- **pg_infer** (codeberg.org/gregburd/pg_infer, Apache 2.0) —
  model-knowledge search via the experimental pg_infer extension
  (PG18+). Where-fns: `(infer-near $ :attr "text" k [:model])`
  for top-K KNN ranked by model knowledge using `<~>`,
  `(infer-similar a b)` for scalar similarity,
  `(infer-implies a b)` for directional implication, plus three
  set-returning verbs `(infer-walk "prompt" top)`,
  `(infer-describe "entity")`, `(infer-predict "prompt" top)`
  with relation bindings. Helpers: `mentat.has_pg_infer`,
  `mentat.create_infer_index`, `mentat.drop_infer_index`. See
  `docs/src/pg_infer.md`.
- **PostGIS** (postgis.net, GPL-2.0+) — geospatial search via
  `(geom-near $ :attr "WKT" k)` (KNN by `ST_Distance`),
  `(geom-within $ :attr "WKT" radius)` (within-distance via
  `ST_DWithin`), `(geom-contains $ :attr "WKT")`
  (`ST_Contains`), `(geom-intersects $ :attr "WKT")`
  (`ST_Intersects`). Aux-table helpers:
  `mentat.attach_geometry_attribute`, `set_geometry`, `del_geometry`,
  `create_gist_geometry_index`, `detach_geometry_attribute`. SRID
  auto-detected from `geometry_columns` so input WKT is coerced to
  the column's projection. See `docs/src/postgis.md`.
