# Declarative Partitioning via pg_partman

[pg_partman][partman] (PostgreSQL license) is the canonical
declarative partition-management extension. It builds on PostgreSQL's
native partitioning to automate creation, rotation, and retention of
time- and range-based partitions.

[partman]: https://github.com/pgpartman/pg_partman

pg_mentat's natural fit is partitioning `mentat.transactions` on
`tx_instant` — pg_partman creates monthly/weekly partitions
automatically, so old tx history can be dropped or archived without
manual maintenance. Treat as a SOFT dependency.

## pg_partman vs TimescaleDB

Both partition `mentat.transactions` on `tx_instant`. Differences:

| | pg_partman | TimescaleDB |
|---|:---|:---|
| Implementation | PG-native partitioning + management functions | Custom hypertable with chunks |
| `shared_preload_libraries` | not required (with `NO_BGW=1` build) | required |
| Compression policies | no | yes |
| Continuous aggregates | no | yes |
| Multi-node | no | yes (commercial) |
| License | PostgreSQL | Apache 2.0 (OSS) / TSL (commercial) |
| Operational footprint | low | medium |

pg_partman is the lighter choice for plain time-based retention. Use
TimescaleDB when you need its analytic features on top.

## Helpers

| Function | Purpose |
|:---|:---|
| `mentat.has_pg_partman()` | Detection. |
| `mentat.partman_attach_transactions(interval default '1 month', premake INT default 4)` | Register `mentat.transactions` with pg_partman as a native-partitioned parent. Idempotent. |
| `mentat.partman_set_transaction_retention(keep_for TEXT)` | Set retention on the registered transactions parent. `keep_for` like `'90 days'`, `'6 months'`. |
| `mentat.partman_run_maintenance()` | Run pg_partman maintenance on all `mentat.*` parents. Schedule via pg_cron for hands-off operation. |

## One-time conversion of mentat.transactions

`mentat.partman_attach_transactions` requires `mentat.transactions`
to **already be a partitioned table**. The default pg_mentat install
ships it as a plain table to keep installation simple. Converting is
a one-time manual step:

```sql
-- 1. Lock the table to prevent concurrent writes.
LOCK TABLE mentat.transactions IN ACCESS EXCLUSIVE MODE;

-- 2. Rename + recreate as partitioned root.
ALTER TABLE mentat.transactions RENAME TO transactions_old;

CREATE TABLE mentat.transactions (
    tx         BIGINT PRIMARY KEY,
    tx_instant TIMESTAMPTZ NOT NULL DEFAULT now()
) PARTITION BY RANGE (tx_instant);

-- 3. Create one initial partition covering existing data.
--    pg_partman will manage future partitions.
CREATE TABLE mentat.transactions_default
    PARTITION OF mentat.transactions DEFAULT;

-- 4. Migrate rows.
INSERT INTO mentat.transactions SELECT * FROM mentat.transactions_old;
DROP TABLE mentat.transactions_old;

-- 5. Now register with pg_partman.
SELECT mentat.partman_attach_transactions('1 month', premake => 4);
```

After registration, pg_partman's maintenance functions handle
everything else.

## Quick start

```bash
# Install pg_partman (NO_BGW=1 skips the bgw build, so no preload required).
cd pg_partman
PG_CONFIG=/path/to/pg_config make NO_BGW=1
PG_CONFIG=/path/to/pg_config make NO_BGW=1 install
```

```sql
CREATE SCHEMA partman;
CREATE EXTENSION pg_partman SCHEMA partman;

-- Convert mentat.transactions (one-time manual step above).

-- Register.
SELECT mentat.partman_attach_transactions('1 month');

-- Set retention to 90 days.
SELECT mentat.partman_set_transaction_retention('90 days');

-- Run maintenance now to materialize partitions; pg_cron schedules
-- this normally (see docs/src/pg_cron.md).
SELECT mentat.partman_run_maintenance();
```

## Periodic maintenance

pg_partman needs `run_maintenance` called regularly — typically
every few hours — to:

- Pre-create upcoming partitions (controlled by `premake`).
- Drop expired partitions (controlled by retention).
- Update analyze stats on each partition.

Schedule via [pg_cron](./pg_cron.md):

```sql
SELECT mentat.cron_schedule_partman_maintenance('0 3 * * *');
-- Daily at 03:00 UTC; idempotent.
```

## Errors

| Error | Cause | Fix |
|:---|:---|:---|
| `:db.error/missing-extension pg_partman is not installed` | Helper called before `CREATE EXTENSION pg_partman`. | Install + `CREATE EXTENSION pg_partman SCHEMA partman;`. |
| `:db.error/manual-step mentat.transactions is not a partitioned table` | Trying to register without first converting `mentat.transactions` to a partitioned root. | Follow the manual conversion above. |
| `:db.error/missing-config mentat.transactions is not registered with pg_partman` | Called `partman_set_transaction_retention` before `partman_attach_transactions`. | Attach first. |

## What this does NOT (yet) give you

- **Auto-conversion of mentat.transactions.** The one-time partition
  conversion is manual; the helper refuses to do it for you to
  prevent silent data loss.
- **Per-store partitioning** in multi-tenant deployments. Add
  `store_id` to the partition key by hand-editing the partition
  layout.
- **Datom table partitioning.** Only the transaction log gets
  pg_partman treatment; datom tables are typically high-throughput
  with low time-axis selectivity, so per-tx partitioning is the
  better approach via TimescaleDB hypertables on `datoms_instant_new`.

## See also

- [pg_partman docs](https://github.com/pgpartman/pg_partman/blob/master/doc/pg_partman.md)
- [pg_cron integration](./pg_cron.md) for scheduling maintenance.
- [TimescaleDB integration](./timescaledb.md) for the
  hypertable alternative.
