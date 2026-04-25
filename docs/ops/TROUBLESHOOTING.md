# pg_mentat Troubleshooting Guide

## Slow Queries

### Symptom
Query latency increases; `mentatd_query_duration_seconds` p95 exceeds 100ms.

### Diagnosis

```sql
-- Check for missing indexes or sequential scans on datoms
EXPLAIN (ANALYZE, BUFFERS) SELECT * FROM mentat.datoms
WHERE e = 12345 AND a = 10 AND added = TRUE;

-- Identify slowest queries (requires pg_stat_statements)
SELECT query, calls, mean_exec_time, total_exec_time
FROM pg_stat_statements
WHERE dbid = (SELECT oid FROM pg_database WHERE datname = 'mentat')
ORDER BY mean_exec_time DESC
LIMIT 10;

-- Check if the query cache is helping
-- Scrape /metrics and look at mentatd_cache_hit_rate
curl -s http://localhost:8080/metrics | grep cache_hit_rate
```

### Solutions

1. **Missing index**: The datoms table uses type-specific partial indexes. Verify the expected
   index is being used with `EXPLAIN ANALYZE`. The EAVT index (`idx_datoms_eavt`) should be
   used for entity lookups, AEVT (`idx_datoms_aevt`) for attribute scans, and the type-specific
   AVET indexes for value range queries.

2. **Stale statistics**: Run `ANALYZE mentat.datoms;` to update planner statistics.

3. **High dead tuple ratio**: Check `n_dead_tup` in `pg_stat_user_tables`. The datoms table
   has aggressive autovacuum settings (`autovacuum_vacuum_scale_factor = 0.05`) but under
   heavy write load, manual vacuum may be needed:
   ```sql
   VACUUM (VERBOSE) mentat.datoms;
   ```

4. **Increase work_mem**: For queries with large sorts or hash joins:
   ```sql
   SET work_mem = '128MB';
   -- Or globally in postgresql.conf
   ```

5. **Enable query cache**: If the cache hit rate is low or caching is disabled, enable it:
   ```
   MENTATD_CACHE_ENABLED=true
   MENTATD_CACHE_CAPACITY=5000
   MENTATD_CACHE_TTL=300
   ```

## High Memory Usage

### Symptom
PostgreSQL or mentatd consuming excessive memory; OOM killer triggers.

### Diagnosis

```bash
# Check mentatd memory usage
ps aux | grep mentatd

# Check PostgreSQL memory
SELECT pg_size_pretty(sum(pg_total_relation_size(c.oid)))
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'mentat';

# Check work_mem usage (sorts spilling to disk)
SELECT * FROM pg_stat_activity
WHERE wait_event_type = 'IO' AND wait_event = 'DataFileRead';
```

### Solutions

1. **Reduce connection pool size**: Each connection consumes ~10MB of PostgreSQL memory.
   Lower `DATABASE_POOL_SIZE` if many connections are idle:
   ```
   DATABASE_POOL_SIZE=50
   ```

2. **Reduce work_mem**: High `work_mem` multiplied by many concurrent queries can exhaust
   memory. Start at 64MB and adjust based on `EXPLAIN ANALYZE` output.

3. **Add temp_file_limit**: Prevent runaway queries from consuming all disk with temporary
   files:
   ```sql
   ALTER SYSTEM SET temp_file_limit = '1GB';
   SELECT pg_reload_conf();
   ```

4. **Check for long-running queries**: Idle transactions hold locks and prevent vacuum:
   ```sql
   SELECT pid, now() - xact_start AS duration, query
   FROM pg_stat_activity
   WHERE state = 'idle in transaction'
   ORDER BY duration DESC;
   ```

## Connection Pool Exhaustion

### Symptom
Requests fail with "Database unavailable" errors. `mentatd_connection_pool_available` drops
to 0. `mentatd_connection_pool_waiting` is > 0.

### Diagnosis

```bash
# Check pool metrics
curl -s http://localhost:8080/metrics | grep connection_pool

# Check PostgreSQL connection count
psql -c "SELECT count(*) FROM pg_stat_activity WHERE datname = 'mentat';"

# Check for blocked connections
psql -c "SELECT pid, wait_event_type, wait_event, state, query
FROM pg_stat_activity WHERE datname = 'mentat' AND state != 'idle';"
```

### Solutions

1. **Increase pool size**: Raise `DATABASE_POOL_SIZE`. Ensure PostgreSQL `max_connections`
   is higher than the total pool size across all mentatd instances.

2. **Kill long-running queries**:
   ```sql
   -- Find and terminate queries running longer than 60 seconds
   SELECT pg_terminate_backend(pid)
   FROM pg_stat_activity
   WHERE datname = 'mentat'
     AND state = 'active'
     AND now() - query_start > interval '60 seconds';
   ```

3. **Check for lock contention**:
   ```sql
   SELECT blocked.pid AS blocked_pid,
          blocked.query AS blocked_query,
          blocking.pid AS blocking_pid,
          blocking.query AS blocking_query
   FROM pg_stat_activity blocked
   JOIN pg_locks bl ON bl.pid = blocked.pid
   JOIN pg_locks l ON l.locktype = bl.locktype
     AND l.database = bl.database
     AND l.relation = bl.relation
     AND l.pid != bl.pid
   JOIN pg_stat_activity blocking ON blocking.pid = l.pid
   WHERE NOT bl.granted;
   ```

4. **Restart mentatd**: As a last resort, restart mentatd to reset the pool.

## Extension Won't Load

### Symptom
`CREATE EXTENSION pg_mentat` fails with an error.

### Diagnosis

```sql
-- Check if the extension files are installed
SELECT * FROM pg_available_extensions WHERE name = 'pg_mentat';

-- Check the extension directory
-- (run from shell)
ls $(pg_config --sharedir)/extension/pg_mentat*
ls $(pg_config --pkglibdir)/pg_mentat*
```

### Solutions

1. **Extension files not found**: Reinstall the extension:
   ```bash
   cd pg_mentat
   cargo pgrx install --release --pg-config=$(which pg_config)
   ```

2. **Wrong PostgreSQL version**: pg_mentat is compiled for a specific PostgreSQL major version.
   Verify with:
   ```bash
   pg_config --version
   # Must match the feature flag used during build (pg13-pg18)
   ```

3. **Shared library load error**: Check PostgreSQL logs for details:
   ```bash
   tail -50 /var/log/postgresql/postgresql-16-main.log
   ```
   Common causes: missing shared libraries (libclang, OpenSSL), ABI mismatch.

4. **Permission denied**: Ensure the PostgreSQL user can read the extension files:
   ```bash
   ls -la $(pg_config --pkglibdir)/pg_mentat.so
   ```

## Transaction Conflicts

### Symptom
Transactions fail with serialization errors or constraint violations.
`mentatd_errors_total` increases.

### Diagnosis

```sql
-- Check recent transactions
SELECT tx, tx_instant FROM mentat.transactions
ORDER BY tx DESC LIMIT 20;

-- Check for unique constraint violations
SELECT e, a, count(*)
FROM mentat.datoms
WHERE added = TRUE
GROUP BY e, a
HAVING count(*) > 1;

-- Check for conflicting concurrent transactions
SELECT * FROM pg_locks WHERE NOT granted;
```

### Solutions

1. **Implement client-side retry logic**: Serialization failures are expected under
   concurrent writes. Clients should retry with exponential backoff.

2. **Reduce transaction batch size**: Large transactions hold locks longer and increase
   conflict probability. Break large imports into smaller batches.

3. **Check uniqueness constraints**: If a `:db.unique/identity` attribute has duplicate
   values, resolve the data conflict.

## Cache Not Helping

### Symptom
`mentatd_cache_hit_rate` is consistently below 50%.

### Diagnosis

```bash
# Check cache metrics
curl -s http://localhost:8080/metrics | grep mentatd_cache

# Key metrics to check:
# mentatd_cache_entries          -- Are entries being stored?
# mentatd_cache_hits_total       -- Are we getting any hits?
# mentatd_cache_misses_total     -- Mostly misses?
# mentatd_cache_full_invalidations_total -- Too many full clears?
# mentatd_cache_tracked_entries  -- Are entries tracked for targeted invalidation?
```

### Solutions

1. **Increase cache capacity**: If the cache is frequently full (entries == capacity),
   increase `MENTATD_CACHE_CAPACITY`.

2. **Increase TTL**: If entries expire before they can be reused, increase
   `MENTATD_CACHE_TTL`.

3. **Reduce write frequency**: Every write transaction invalidates untracked cache entries.
   The cache works best with read-heavy workloads.

4. **Check for full invalidations**: If `mentatd_cache_full_invalidations_total` is high,
   there may be transactions that don't provide entity-level dependency information.
   Tracked entries (with entity dependency sets) survive unrelated transactions.

## mentatd Won't Start

### Symptom
mentatd exits immediately after launch.

### Diagnosis

```bash
# Run with debug logging
RUST_LOG=debug mentatd

# Check if the port is already in use
ss -tlnp | grep 8080

# Check if PostgreSQL is accessible
psql "postgresql://mentat:password@localhost:5432/mentat" -c "SELECT 1;"
```

### Solutions

1. **Port in use**: Change `MENTATD_PORT` or stop the conflicting process.

2. **Database unreachable**: Verify the `DATABASE_URL` and that PostgreSQL is running
   and accepting connections.

3. **Invalid configuration**: mentatd exits with an error if the config file is malformed.
   Check the log output for `Config error:`.

4. **Extension not installed**: mentatd requires the `mentat` schema to exist. Run
   `CREATE EXTENSION pg_mentat;` in the target database.

## High Disk Usage

### Symptom
Disk space filling up; PostgreSQL WAL or table data growing unexpectedly.

### Diagnosis

```sql
-- Check table sizes
SELECT
    relname,
    pg_size_pretty(pg_total_relation_size(c.oid)) AS total,
    pg_size_pretty(pg_relation_size(c.oid)) AS data,
    pg_size_pretty(pg_indexes_size(c.oid)) AS indexes
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'mentat'
ORDER BY pg_total_relation_size(c.oid) DESC;

-- Check WAL size
SELECT pg_size_pretty(sum(size)) FROM pg_ls_waldir();

-- Check dead tuples (bloat)
SELECT relname, n_dead_tup, n_live_tup,
       round(100.0 * n_dead_tup / GREATEST(n_live_tup + n_dead_tup, 1), 1) AS dead_pct
FROM pg_stat_user_tables
WHERE schemaname = 'mentat'
ORDER BY n_dead_tup DESC;
```

### Solutions

1. **Run VACUUM FULL**: Reclaims space from bloated tables (causes downtime):
   ```sql
   VACUUM FULL mentat.datoms;
   ```

2. **Tune autovacuum**: The datoms table already has aggressive settings. For very high
   write rates, consider:
   ```sql
   ALTER TABLE mentat.datoms SET (
       autovacuum_vacuum_scale_factor = 0.02,
       autovacuum_vacuum_cost_delay = 2
   );
   ```

3. **Archive old WAL files**: If WAL accumulates, check that `archive_command` is working
   or adjust `max_wal_size`.

4. **Clean up old data**: If historical datoms (`added = FALSE`) are not needed,
   consider excision (if supported by partition settings).
