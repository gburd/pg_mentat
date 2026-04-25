# Runbook: Disk Full

## Severity: P1

## Trigger

Disk usage exceeds 90% on the PostgreSQL data volume, or PostgreSQL reports errors
writing to disk (WAL, temporary files, or table data).

## Symptoms

- PostgreSQL errors: `could not write to file`, `No space left on device`
- Transactions failing
- WAL archiving failures
- Autovacuum unable to run

## Investigation Steps

### 1. Check Disk Usage

```bash
# Overall disk usage
df -h /var/lib/postgresql

# PostgreSQL data directory size
du -sh /var/lib/postgresql/16/main/

# WAL size
du -sh /var/lib/postgresql/16/main/pg_wal/
```

### 2. Identify Space Consumers

```sql
-- Database size
SELECT pg_size_pretty(pg_database_size('mentat'));

-- Table and index sizes in mentat schema
SELECT
    relname,
    pg_size_pretty(pg_total_relation_size(c.oid)) AS total,
    pg_size_pretty(pg_relation_size(c.oid)) AS data,
    pg_size_pretty(pg_indexes_size(c.oid)) AS indexes
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'mentat'
ORDER BY pg_total_relation_size(c.oid) DESC;

-- Temporary file usage
SELECT pg_size_pretty(temp_bytes) AS temp_used
FROM pg_stat_database
WHERE datname = 'mentat';

-- Check for table bloat
SELECT relname, n_dead_tup, n_live_tup,
       pg_size_pretty(pg_relation_size(relid)) AS size
FROM pg_stat_user_tables
WHERE schemaname = 'mentat'
ORDER BY n_dead_tup DESC;
```

### 3. Check WAL Accumulation

```sql
-- WAL file count
SELECT count(*) FROM pg_ls_waldir();

-- Check replication slots that might prevent WAL cleanup
SELECT slot_name, active, pg_size_pretty(pg_wal_lsn_diff(pg_current_wal_lsn(), restart_lsn)) AS retained
FROM pg_replication_slots;
```

## Remediation

### Immediate: Free Space

```bash
# 1. Remove old PostgreSQL logs
find /var/log/postgresql -name "*.log" -mtime +7 -delete

# 2. Remove old WAL archives (if archived copies are confirmed)
# WARNING: Only delete archived WAL files that have been confirmed archived
find /backup/wal -name "0000*" -mtime +3 -delete
```

### Reclaim Table Bloat

```sql
-- Regular vacuum (non-blocking, reclaims space for reuse)
VACUUM (VERBOSE) mentat.datoms;

-- VACUUM FULL (blocking, returns space to OS)
-- Schedule during maintenance window
VACUUM FULL mentat.datoms;
```

### Reduce WAL Retention

```sql
-- If replication slots are retaining WAL
SELECT pg_drop_replication_slot('unused_slot_name');

-- Reduce max_wal_size
ALTER SYSTEM SET max_wal_size = '1GB';
SELECT pg_reload_conf();

-- Force a checkpoint to clean up WAL
CHECKPOINT;
```

### Reduce Temporary File Usage

```sql
-- Set temp_file_limit to prevent runaway queries
ALTER SYSTEM SET temp_file_limit = '1GB';
SELECT pg_reload_conf();

-- Kill queries using large temp files
SELECT pid, query FROM pg_stat_activity
WHERE state = 'active' AND datname = 'mentat';
-- Use pg_terminate_backend() on suspicious long-running queries
```

### Add Storage (if possible)

```bash
# Kubernetes: expand PVC (if storage class supports it)
kubectl patch pvc postgres-data -p '{"spec":{"resources":{"requests":{"storage":"50Gi"}}}}'

# VM/bare metal: extend the filesystem
lvextend -L +20G /dev/vg0/pg_data
resize2fs /dev/vg0/pg_data
```

## Prevention

- Monitor disk usage with alerts at 80% and 90% thresholds
- Set `temp_file_limit` to prevent query-driven disk exhaustion
- Configure WAL archiving with `archive_cleanup_command`
- Run regular VACUUM to prevent table bloat
- Schedule periodic `VACUUM FULL` during maintenance windows
- Size persistent volumes with 2x headroom over current usage
- Monitor `n_dead_tup` in `pg_stat_user_tables` for the datoms table

## Escalation

If disk space cannot be freed:
- Expand storage capacity
- Consider table partitioning to distribute data across volumes
- Archive historical data (old transactions) to cold storage
- Review data retention policies
