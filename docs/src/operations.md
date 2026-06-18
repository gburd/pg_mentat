# Operations: Throughput, Bloat, and the Live Projection

This page covers the operational concerns that matter when pg_mentat is
the identity / knowledge-graph backbone of a production service: how to
make `mentat.t()` ingest fast, how to keep the narrow datom tables from
bloating, and how to read the "current value" of an attribute cheaply.

It is written against real production feedback (an 82 GB store used as a
community-stats identity backbone). The version that introduced the
accessors and autovacuum defaults described here is **1.4.0**.

## 1. `mentat.t()` throughput

### What costs time per call

Each `mentat.t()` call does, regardless of batch size:

1. Allocate a transaction id (`nextval` on the tx sequence) and insert
   the `mentat.transactions` row + the `:db/txInstant` datom.
2. Parse the EDN, resolve idents / tempids, validate constraints.
3. Per **cardinality-one** datom, look up the current value to decide
   assert / replace / skip.
4. Batch-insert the new datom rows (one INSERT per touched type table).

Step 3 used to run a **9-way `UNION ALL`** probe per datom. As of 1.4.0
it is a single indexed lookup on the one narrow table matching the
value's type — a measured **~1.8× speedup** on a cardinality-one
re-assertion workload (6.2 s → 3.4 s for 2000 calls in the project's
microbenchmark).

The residual per-call floor (~1–2 ms in that benchmark; higher under
production latency and replication) is **tx allocation + the txInstant
datom + savepoint setup**. It is *fixed per call*, so the way to amortize
it is fewer, larger transactions — not smaller batches.

### Make backfills fast: one tx, many facts

Batch as many assertions as possible into a **single** `mentat.t()`
call. The per-call overhead is paid once; the per-datom cost scales
linearly and is cheap. A 250k-fact backfill batched at, say, 5,000
facts/tx is 50 calls — not 1,250.

```sql
-- Good: 5000 facts, ONE tx, ONE tx-allocation overhead.
SELECT mentat.t($edn$[
  {:db/id "c1" :contribution/key "..." :contribution/kind :commit ...}
  {:db/id "c2" ...}
  ... 4998 more ...
]$edn$);
```

The larger the batch, the closer you get to the per-datom floor. There
is no fixed upper bound other than statement memory; batches of several
thousand facts are routinely fine.

### Idempotent re-assertion is already a no-op for the data

If your sync re-asserts a cardinality-one fact whose value already
matches the current value, pg_mentat takes the **Skip** path: no new
datom is written, no retraction is written, the datom table is not
churned. You still pay the per-call tx overhead, so the same advice
applies — batch the no-op re-assertions into few large transactions and
the cost disappears into the per-call floor.

> **Tip.** If your nightly mirror is mostly no-ops (idempotent by a
> `:contribution/key`-style natural key), the cheapest thing you can do
> is widen the batch. The Skip path means the table doesn't grow; the
> only cost left is the per-`t()` tx allocation, which batching
> amortizes.

## 2. Autovacuum and bloat

### The default-scale-factor trap

PostgreSQL's default `autovacuum_vacuum_scale_factor = 0.2` means a
table is vacuumed only after 20% of its rows are dead. On a 50M-row
narrow table that is **10M dead tuples** of slack — autovacuum
effectively never fires, and the table (especially its PK and EAVT
index) bloats without bound. The instant-typed table is the worst case
because monotonic attributes (`:first-seen`, `:last-seen`,
`:observed-at`) are re-asserted on every sync, each generating a
retraction + assertion.

### What 1.4.0 ships

`CREATE EXTENSION pg_mentat` (and the 1.3.0→1.4.0 upgrade) now sets, on
**all nine** `datoms_*_new` tables and on `mentat.transactions`:

```
autovacuum_vacuum_scale_factor  = 0
autovacuum_vacuum_threshold     = 50000
autovacuum_analyze_scale_factor = 0
autovacuum_analyze_threshold    = 50000
```

Scale-factor 0 + a fixed 50k-dead-tuple threshold means autovacuum
fires on a **constant** amount of dead tuples regardless of table size.
High-churn deployments can lower the threshold further per table:

```sql
ALTER TABLE mentat.datoms_instant_new
  SET (autovacuum_vacuum_threshold = 10000);
```

### Reclaiming existing bloat

Storage params change *future* triggering; they do not shrink a table
that is already bloated. To reclaim:

```sql
-- Online, no exclusive lock, needs pg_repack installed:
pg_repack -t mentat.datoms_instant_new -d yourdb

-- Or, during a maintenance window (takes an ACCESS EXCLUSIVE lock):
VACUUM FULL mentat.datoms_instant_new;
```

Schedule a periodic VACUUM via [pg_cron](./pg_cron.md):

```sql
SELECT mentat.cron_schedule_vacuum_datoms('0 4 * * *');
```

### Monitoring before it bites

`mentat.attribute_health()` reports live datom counts and the
dead-tuple % of each backing table:

```sql
SELECT * FROM mentat.attribute_health() ORDER BY dead_pct DESC;
```

```
   attr_ident    | value_type |    backing_table      | live_datoms | dead_pct
-----------------+------------+-----------------------+-------------+----------
 :person/seen    | instant    | mentat.datoms_instant_new | 124032  |     31.4
 :person/email   | string     | mentat.datoms_text_new    | 41200   |      2.1
 ...
```

Alert on `dead_pct > 25` to catch bloat before it costs you query
latency or disk.

> **Note on `:last-seen`-style attributes.** Keeping full history of a
> value that changes every sync is inherently bloat-generating: each
> change is a retraction + an assertion. If you do not need the history
> of a monotonic timestamp, mark the attribute **`:db/noHistory true`**
> (see below) — noHistory attributes keep only the current value, so
> they generate no history trail and cannot bloat.

### `:db/noHistory` — non-historical attributes

As of 1.5.0 the datom log is **append-only**: a retraction is a new
immutable datom, never an in-place flip of the prior assertion. That
makes history exact, but it also means an attribute whose value changes
every sync (the `:observed-at` / `:last-seen` class) accumulates one
assert + one retract datom per change — unbounded growth.

Mark such an attribute `:db/noHistory true` to opt out of history:

```clojure
{:db/ident :host/last-seen
 :db/valueType :db.type/instant
 :db/cardinality :db.cardinality/one
 :db/noHistory true}
```

For a `:db/noHistory` attribute, each assertion **physically replaces**
the prior value in the log (and the projection) instead of appending a
retraction + assertion. The log holds exactly the current value:

```sql
-- After 10 updates to a noHistory :host/last-seen:
SELECT count(*) FROM mentat.datoms_instant_new
  WHERE e = :host AND a = mentat.attr_id(':host/last-seen');
-- => 1   (a normal attribute would have ~20 rows: 10 asserts + 10 retracts)
```

Semantics:

- **Current-time queries are unchanged** — `[?h :host/last-seen ?t]`
  returns the current value exactly as for a normal attribute.
- **`:as-of` / history queries see only the current value**, because no
  prior versions are retained. This is the deliberate trade: you give
  up time-travel on that attribute in exchange for zero bloat.
- **Per-attribute, not global** — a noHistory attribute and a
  full-history attribute on the same entity each behave correctly.
- This is Datomic-compatible: Datomic's `:db/noHistory` has the same
  "keep only the current value" meaning.

Use it for high-churn, history-irrelevant values (heartbeats,
last-seen timestamps, observed counters). Do **not** use it for
attributes whose history you audit or time-travel.

## 3. The live projection: `mentat.current()` and `mentat.attr_id()`

### The problem with DISTINCT ON / LATERAL views

A view that resolves "the latest value of attribute A for entity E"
typically looks like:

```sql
SELECT DISTINCT ON (e) e, v
FROM mentat.datoms_text_new
WHERE a = <attr> AND added
ORDER BY e, tx DESC
```

…with a `LEFT JOIN LATERAL (... ORDER BY tx DESC LIMIT 1)` per extra
attribute. That recomputes the latest-per-`(e, a)` on every read and
fans out one lateral per attribute — the dominant cost when refreshing a
materialized view that joins many attributes.

### `mentat.current(e, a)`

`mentat.current()` returns the current value of one attribute for one
entity as text, with a single indexed lookup on the
`(store_id, e, a, tx DESC) WHERE added` covering index. It dispatches on
the attribute's declared value type so only one narrow table is touched.

```sql
-- By attribute keyword (resolves the entid for you):
SELECT mentat.current(12345, ':person/canonical-email');

-- Or by attribute entid, if you already have it:
SELECT mentat.current(12345, mentat.attr_id(':person/canonical-email'));
```

Use it in a view to replace the DISTINCT ON / LATERAL machinery:

```sql
CREATE VIEW community.persons AS
SELECT
    e AS person_id,
    mentat.current(e, ':person/canonical-email') AS email,
    mentat.current(e, ':person/name')            AS name,
    mentat.current(e, ':person/employer')        AS employer
FROM (
    -- the set of person entities (one row per entity)
    SELECT DISTINCT e FROM mentat.datoms_text_new
    WHERE a = mentat.attr_id(':person/canonical-email') AND added
) p;
```

Each `mentat.current()` call is an index lookup; the planner folds the
STABLE function once per row. This is dramatically cheaper than a
per-attribute LATERAL on a large fan-out.

### A maintained current-state index

For the absolute hottest read paths, back `mentat.current()` with a
covering index per attribute type (most projections read text/keyword):

```sql
-- Already shipped: the *_tx covering index supports the lookup:
--   (store_id, tx DESC) INCLUDE (e, a, v) WHERE added
-- For a per-attribute hot path, add a partial index:
CREATE INDEX idx_person_email_current
  ON mentat.datoms_text_new (e, tx DESC)
  WHERE a = <:person/canonical-email entid> AND added;
```

A fully maintained "current datoms" table (kept in sync on `t()`) is on
the roadmap; today the covering-index + `mentat.current()` combination
gives index-backed reads without it.

### `mentat.attr_id()`

`mentat.attr_id(':ns/name')` resolves an attribute keyword to its entid
for use in SQL and view definitions, so generated viewdefs read
`a = mentat.attr_id(':person/name')` instead of an opaque
`a = 1308861`. It is STABLE, so the planner evaluates it once.

## 4. Health dashboard

| Function | Returns |
|:---|:---|
| `mentat.attr_id(':ns/n')` | The attribute's entid (BIGINT), or NULL. |
| `mentat.current(e, a)` | Current value of attribute for entity, as TEXT. |
| `mentat.attribute_health()` | Per-attribute live datom count + backing-table dead %. |
| `mentat.storage()` | Per-table size + row estimates (pre-existing). |
| `mentat.stats()` | Query-execution statistics (pre-existing). |

A minimal alerting query:

```sql
SELECT attr_ident, live_datoms, dead_pct
FROM mentat.attribute_health()
WHERE dead_pct > 25
ORDER BY dead_pct DESC;
```

## What is NOT solved by auto-indexing

A common first instinct for "pg_mentat is slow" is *missing indexes*.
It is worth stating plainly: the narrow datom tables already carry
EAVT / AEVT / VAET / tx covering indexes plus an FTS GIN index. The
costs that actually hurt in production are:

- **per-`t()` transaction overhead** — solved by batching, not indexing;
- **history-resolution on reads** — solved by `mentat.current()` + a
  per-attribute partial index, not by adding more general indexes;
- **dead-tuple bloat** — solved by autovacuum tuning + scheduled
  vacuum, not by indexing.

Adding indexes beyond the shipped set will not move these numbers and
will slow `t()` further (every index is maintained on write).
