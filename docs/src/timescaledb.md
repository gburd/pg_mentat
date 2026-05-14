# Time-Series Storage via TimescaleDB

[TimescaleDB][tsdb] (Apache 2.0 OSS edition) converts regular
PostgreSQL tables into "hypertables" — automatically partitioned by a
time column with chunk-level operations for fast inserts, time-range
queries, retention, and compression.

[tsdb]: https://docs.timescale.com/

pg_mentat's transaction log (`mentat.transactions`) and the
instant-typed datoms (`mentat.datoms_instant_new`) are natural fits:
both have a meaningful time axis, and time-window queries are common
on the audit / time-travel paths. Treat as a SOFT dependency.

## When to use this

| Use case | Pattern |
|:---|:---|
| Multi-year tx history with time-window queries | Hypertable on `mentat.transactions(tx_instant)`. |
| Audit retention (drop tx history > N days/months) | Hypertable + retention policy. |
| Aggregate time-bucketed datom counts | Hypertable + continuous aggregates (raw SQL). |
| High-volume `:db.type/instant` attributes | Hypertable on `mentat.datoms_instant_new(v)`. |

If the store is small (< 10M txs) and history is short (< 1 year),
plain PostgreSQL handles it fine and TimescaleDB adds operational
overhead without measurable benefit.

## Helpers

| Function | Effect |
|:---|:---|
| `mentat.has_timescaledb()` | True if `timescaledb` extension is installed. |
| `mentat.timescale_attach_transactions(chunk_interval default '1 month')` | Convert `mentat.transactions` into a hypertable partitioned on `tx_instant`. Idempotent. Returns hypertable id. |
| `mentat.timescale_attach_instant_datoms(chunk_interval default '1 month')` | Convert `mentat.datoms_instant_new` into a hypertable on `v`. Idempotent. |
| `mentat.timescale_set_transaction_retention(keep_for INTERVAL)` | Add a retention policy: drop chunks older than `keep_for`. **Destructive** — affects time-travel queries. |

## Quick start

```bash
# Install TimescaleDB (Debian/Ubuntu example).
sudo apt-get install timescaledb-2-postgresql-16
# Add to postgresql.conf: shared_preload_libraries = 'timescaledb'
# Restart postgres.
```

```sql
CREATE EXTENSION pg_mentat;
CREATE EXTENSION timescaledb;

-- Convert the transaction log to a hypertable. Default chunk = 1 month.
SELECT mentat.timescale_attach_transactions();

-- Optionally attach the instant datom table too.
SELECT mentat.timescale_attach_instant_datoms();

-- Drop tx history older than 90 days. WARNING: time-travel queries
-- with :as-of will fail for txs older than this.
SELECT mentat.timescale_set_transaction_retention(INTERVAL '90 days');
```

## Caveats

1. **Retention is destructive.** TimescaleDB's `add_retention_policy`
   physically drops chunks. Datoms inserted in those chunks are gone.
   pg_mentat's `:as-of` time-travel queries will fail for txs older
   than the retention window. **Pin retention to your audit
   requirements before enabling.**

2. **Chunk size affects insert latency.** Smaller chunks (1 day vs 1
   month) mean more chunks, more catalog overhead per insert. For
   most pg_mentat workloads — tx counts in the millions per month —
   monthly chunks are a good default.

3. **CLUSTER doesn't apply.** Hypertable chunks are individually
   `CLUSTER`'able but not as a unit. The narrow datom tables remain
   plain (only `mentat.transactions` and `datoms_instant_new` become
   hypertables).

4. **Compression is opt-in.** TimescaleDB's columnar compression
   (`alter_table ... SET (timescaledb.compress = true)` plus
   `add_compression_policy`) is not wrapped here. Use it directly
   via raw SQL if cold-tier compression matters.

5. **Distributed hypertables not supported here.** Multi-node
   TimescaleDB is out of scope for this integration.

## What this does NOT (yet) give you

- **Continuous aggregates wrapped in Datalog.** TimescaleDB's
  `CREATE MATERIALIZED VIEW ... WITH (timescaledb.continuous)` is
  the right tool for "datoms-per-day" rollups; use it directly via
  SQL.
- **Compression policies.** Wrap `add_compression_policy` directly.
- **Retention helpers for non-time tables.** Only the time-axis
  tables (`transactions`, `datoms_instant_new`) get hypertable
  treatment.

## See also

- [TimescaleDB Hypertable docs](https://docs.timescale.com/use-timescale/latest/hypertables/)
- [pg_partman integration](./pg_partman.md) — declarative
  partitioning without TimescaleDB; PostgreSQL-native.
