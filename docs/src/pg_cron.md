# Scheduled Maintenance via pg_cron

[pg_cron][cron] is the canonical scheduler extension for PostgreSQL:
cron-style schedules stored in `cron.job`, executed by a background
worker. Use it to schedule pg_mentat maintenance:
pg_partman partition rotation, narrow-datom-table VACUUM, materialized
view refreshes, and the like.

[cron]: https://github.com/citusdata/pg_cron

pg_cron is an **optional** dependency. Detect with
`mentat.has_pg_cron()`. Without it, the helpers raise a clear
missing-extension error rather than silently no-op'ing.

## Operational requirements

pg_cron requires:

1. **`shared_preload_libraries = 'pg_cron'`** (or
   `'pg_cron,pg_tre,...'` if you have other preloads). Cluster restart.
2. **`cron.database_name = '<your-db>'`** in `postgresql.conf` —
   pg_cron runs jobs in exactly **one** database per cluster.
3. **`CREATE EXTENSION pg_cron;`** in that database.

Without those steps `CREATE EXTENSION pg_cron` fails. The pg_mentat
helpers don't bypass this — they detect via `mentat.has_pg_cron()`
and refuse to install jobs if pg_cron is missing.

## Helpers

| Function | Purpose |
|:---|:---|
| `mentat.has_pg_cron()` | True if pg_cron is installed in this database. |
| `mentat.cron_schedule(job_name, schedule, command)` | Generic wrapper over `cron.schedule`. Returns the pg_cron job id. |
| `mentat.cron_unschedule(job_name)` | Cancel a job by name. Returns true on success. |
| `mentat.cron_schedule_partman_maintenance(schedule default '0 3 * * *')` | Convenience: schedule daily `mentat.partman_run_maintenance()`. |
| `mentat.cron_schedule_vacuum_datoms(schedule default '0 4 * * *')` | Convenience: schedule daily VACUUM ANALYZE on the 9 narrow datom tables. |

## Quick start

```sql
-- After enabling pg_cron in postgresql.conf and restarting:
CREATE EXTENSION pg_cron;
CREATE EXTENSION pg_mentat;

-- Schedule daily partition maintenance.
SELECT mentat.cron_schedule_partman_maintenance();
-- => job id

-- Schedule nightly VACUUM ANALYZE on datom tables.
SELECT mentat.cron_schedule_vacuum_datoms();
-- => job id

-- Custom job: refresh a materialized view at 02:30 UTC daily.
SELECT mentat.cron_schedule(
    'refresh-search-cache',
    '30 2 * * *',
    'REFRESH MATERIALIZED VIEW CONCURRENTLY app.search_index;'
);

-- Cancel one.
SELECT mentat.cron_unschedule('refresh-search-cache');
```

## Cron schedule syntax

pg_cron uses standard 5-field cron syntax (`minute hour day month dow`)
in **UTC**. Examples:

| Schedule | Meaning |
|:---|:---|
| `'* * * * *'` | Every minute. |
| `'0 * * * *'` | Top of every hour. |
| `'0 3 * * *'` | 03:00 UTC every day. |
| `'0 3 * * 0'` | 03:00 UTC every Sunday. |
| `'*/15 * * * *'` | Every 15 minutes. |

pg_cron also supports a 6-field "second-precision" form (PG 1.6+);
see the pg_cron docs.

## Common pg_mentat schedules

```sql
-- Hourly partman maintenance for high-volume stores.
SELECT mentat.cron_schedule_partman_maintenance('0 * * * *');

-- Vacuum just the high-churn fulltext attribute hourly.
SELECT mentat.cron_schedule(
    'mentat-vacuum-text',
    '0 * * * *',
    'VACUUM (ANALYZE) mentat.datoms_text_new;'
);

-- Drop dead tuples from PgQue's tx-emit triggers monthly.
SELECT mentat.cron_schedule(
    'mentat-vacuum-pgque-events',
    '0 4 1 * *',
    'VACUUM FULL pgque.event_1;'
);

-- Refresh the FDW caches (materialized view from postgres-fdw cookbook).
SELECT mentat.cron_schedule(
    'mentat-refresh-fdw',
    '*/30 * * * *',
    'REFRESH MATERIALIZED VIEW CONCURRENTLY remote_open_issues;'
);
```

## Inspecting jobs

```sql
SELECT jobid, jobname, schedule, command, active
FROM cron.job
WHERE jobname LIKE 'mentat-%';

-- Recent runs.
SELECT jobid, runid, start_time, end_time, status, return_message
FROM cron.job_run_details
ORDER BY start_time DESC LIMIT 20;
```

## Errors

| Error | Cause | Fix |
|:---|:---|:---|
| `:db.error/missing-extension pg_cron is not installed` | Helper called before pg_cron is loaded. | Add `pg_cron` to `shared_preload_libraries`, restart, `CREATE EXTENSION pg_cron;`. |
| `unrecognized configuration parameter "cron.database_name"` | pg_cron not in `shared_preload_libraries`. | Configure + restart. |
| `permission denied for schema cron` | User isn't a member of the `pg_cron` role / not the database owner. | Grant `USAGE ON SCHEMA cron`. |

## What this does NOT (yet) give you

- **Per-job notifications.** pg_cron writes results to
  `cron.job_run_details`; if you want alerts, set up a separate
  monitoring job that reads from there.
- **Multi-database scheduling.** pg_cron runs jobs in exactly one
  database (`cron.database_name`). For multi-tenant pg_mentat
  deployments where each store is its own database, see the
  [cross-database pattern][xdb] in the pg_cron docs.
- **Sub-minute schedules** in the standard 5-field form. Use the
  6-field form (pg_cron 1.6+) for second-precision schedules.

[xdb]: https://github.com/citusdata/pg_cron#creating-a-cron-job-in-a-different-database

## See also

- [pg_cron README](https://github.com/citusdata/pg_cron)
- [pg_partman integration](./pg_partman.md) — schedules
  partition maintenance via `mentat.cron_schedule_partman_maintenance`.
- [PgQue integration](./pgque.md) — uses pg_cron as the
  recommended ticker driver.
