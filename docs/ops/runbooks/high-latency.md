# Runbook: High Latency

## Severity: P2

## Trigger

Alert `MentatdHighQueryLatency` fires when:
- `histogram_quantile(0.95, rate(mentatd_query_duration_seconds_bucket[5m])) > 0.1`

Or users report slow query responses.

## Symptoms

- p95 query latency exceeds 100ms
- `mentatd_query_duration_seconds` histogram shows elevated values
- Client-side timeouts or degraded user experience

## Investigation Steps

### 1. Confirm the Issue

```bash
# Check current query latency
curl -s http://localhost:8080/metrics | grep mentatd_query_duration

# Check if the issue is isolated to specific operations
curl -s http://localhost:8080/metrics | grep mentatd_operation_duration
```

### 2. Check Connection Pool

```bash
curl -s http://localhost:8080/metrics | grep connection_pool
# If pool_available is 0, see runbook: connection-pool-full.md
```

### 3. Check PostgreSQL

```sql
-- Active queries
SELECT pid, now() - query_start AS duration, state, query
FROM pg_stat_activity
WHERE datname = 'mentat' AND state = 'active'
ORDER BY duration DESC;

-- Lock contention
SELECT blocked.pid, blocked.query, blocking.pid AS blocking_pid, blocking.query AS blocking_query
FROM pg_stat_activity blocked
JOIN pg_locks bl ON bl.pid = blocked.pid
JOIN pg_locks l ON l.locktype = bl.locktype AND l.database = bl.database
  AND l.relation = bl.relation AND l.pid != bl.pid
JOIN pg_stat_activity blocking ON blocking.pid = l.pid
WHERE NOT bl.granted;

-- Check for sequential scans on datoms
SELECT relname, seq_scan, idx_scan,
       round(100.0 * idx_scan / GREATEST(idx_scan + seq_scan, 1), 1) AS idx_pct
FROM pg_stat_user_tables
WHERE schemaname = 'mentat';

-- Slow queries (requires pg_stat_statements)
SELECT query, calls, mean_exec_time, max_exec_time
FROM pg_stat_statements
WHERE dbid = (SELECT oid FROM pg_database WHERE datname = 'mentat')
ORDER BY mean_exec_time DESC
LIMIT 10;
```

### 4. Check System Resources

```bash
# CPU and memory
top -b -n1 | head -20

# Disk I/O
iostat -x 1 3

# Check if PostgreSQL is swapping
vmstat 1 5
```

## Remediation

### If index is missing or not being used:

```sql
-- Update statistics
ANALYZE mentat.datoms;

-- Check that random_page_cost is appropriate for SSDs
SHOW random_page_cost;
-- Set to 1.1 for SSDs:
ALTER SYSTEM SET random_page_cost = 1.1;
SELECT pg_reload_conf();
```

### If work_mem is too low (sorts spilling to disk):

```sql
-- Temporarily increase for the session
SET work_mem = '256MB';

-- Or permanently
ALTER SYSTEM SET work_mem = '128MB';
SELECT pg_reload_conf();
```

### If pool is exhausted:

See [connection-pool-full.md](connection-pool-full.md).

### If a single long query is blocking:

```sql
-- Terminate the blocking query (use with caution)
SELECT pg_terminate_backend(<pid>);
```

### If table is bloated (high dead tuple count):

```sql
-- Non-blocking vacuum
VACUUM (VERBOSE) mentat.datoms;

-- If very bloated, schedule a VACUUM FULL during maintenance window
-- WARNING: VACUUM FULL locks the table
VACUUM FULL mentat.datoms;
```

## Prevention

- Monitor `mentatd_query_duration_seconds` p95 continuously
- Run `ANALYZE mentat.datoms` after large data changes
- Ensure autovacuum is running (check `last_autovacuum` in `pg_stat_user_tables`)
- Keep `random_page_cost` tuned for your storage (1.1 for SSDs)
- Review and optimize slow queries identified via `pg_stat_statements`

## Escalation

If latency persists after remediation:
- Escalate to the DBA team for query plan analysis
- Consider increasing PostgreSQL `shared_buffers` or adding read replicas
- Review whether the query pattern requires a new index
