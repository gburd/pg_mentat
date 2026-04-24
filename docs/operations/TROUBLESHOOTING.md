# Troubleshooting Guide

Operational troubleshooting guide for pg_mentat and mentatd in production.

For schema, query, and transaction errors, see also: [Common Issues](../troubleshooting/COMMON_ISSUES.md) and [Debugging Guide](../troubleshooting/DEBUGGING.md).

## Slow Queries

### Symptoms

- `mentatd_query_duration_seconds` p99 exceeds 2 seconds
- Users report slow response times
- PostgreSQL CPU usage is high

### Diagnosis

**Step 1: Identify the slow query**

```sql
-- Enable pg_stat_statements if not already active
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

-- Find the slowest mentat-related queries
SELECT
  left(query, 120) AS query_preview,
  calls,
  round(mean_exec_time::numeric, 2) AS avg_ms,
  round(max_exec_time::numeric, 2) AS max_ms,
  rows
FROM pg_stat_statements
WHERE query LIKE '%mentat%'
ORDER BY mean_exec_time DESC
LIMIT 10;
```

**Step 2: Analyze the query plan**

```sql
EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT)
SELECT mentat_query(
  '<your slow query here>',
  '{}'::jsonb
);
```

**Step 3: Check for common causes**

| Observation | Cause | Fix |
|-------------|-------|-----|
| `Seq Scan on datoms` | Missing index | Add `:db/index true` to the attribute or create a custom PostgreSQL index |
| `actual rows` >> `estimated rows` | Stale statistics | Run `ANALYZE mentat.datoms;` |
| `Sort Method: external merge Disk` | Sort spills to disk | Increase `work_mem` in `postgresql.conf` |
| `Nested Loop (actual loops=100000)` | Bad join order | Reorder Datalog patterns: most selective first |
| `Buffers: shared read=50000` | Cold buffer pool | Increase `shared_buffers` or pre-warm with queries |

### Solutions

**Add an index to a frequently queried attribute:**
```sql
SELECT mentat_transact($$
[{:db/id :person/email
  :db/index true}]
$$);
```

**Update planner statistics:**
```sql
ANALYZE mentat.datoms;
ANALYZE mentat.schema;
ANALYZE mentat.transactions;
```

**Increase work_mem for sort-heavy queries:**
```sql
-- Session-level (for testing)
SET work_mem = '128MB';

-- Permanent (postgresql.conf)
-- work_mem = 64MB
```

**Reorder query patterns:**
```clojure
;; Before: broad scan first
[:find ?name
 :where
 [?e :person/name ?name]
 [?e :person/country "US"]]

;; After: selective pattern first (fewer entities have country=US)
[:find ?name
 :where
 [?e :person/country "US"]
 [?e :person/name ?name]]
```

## Connection Pool Issues

### Symptom: "Failed to get connection from pool"

This error means all pooled connections are in use and the timeout expired.

**Diagnosis:**

```bash
# Check mentatd metrics
curl -s http://localhost:8080/metrics | grep pool
# mentatd_connection_pool_size <current_count>
```

```sql
-- Check active PostgreSQL connections
SELECT
  state,
  count(*) AS count,
  avg(extract(epoch from (now() - state_change)))::int AS avg_age_seconds
FROM pg_stat_activity
WHERE application_name LIKE '%mentat%'
   OR usename = 'mentat'
GROUP BY state;

-- Find long-running queries holding connections
SELECT
  pid,
  state,
  query,
  extract(epoch from (now() - query_start))::int AS duration_seconds
FROM pg_stat_activity
WHERE application_name LIKE '%mentat%'
  AND state = 'active'
ORDER BY query_start;
```

**Solutions:**

1. **Increase pool size** (if PostgreSQL can handle more connections):
   ```toml
   [database]
   pool_size = 25  # was 10
   ```
   Also increase PostgreSQL `max_connections` if needed.

2. **Kill stuck queries:**
   ```sql
   -- Terminate a specific backend
   SELECT pg_terminate_backend(<pid>);

   -- Cancel (not terminate) long queries
   SELECT pg_cancel_backend(pid)
   FROM pg_stat_activity
   WHERE query LIKE '%mentat%'
     AND state = 'active'
     AND extract(epoch from (now() - query_start)) > 60;
   ```

3. **Set statement timeout** to prevent runaway queries:
   ```sql
   -- In postgresql.conf or per-session
   SET statement_timeout = '30s';
   ```

4. **Reduce connection lifetime** to recycle stale connections faster:
   ```toml
   [database]
   max_lifetime_secs = 900  # 15 minutes instead of 30
   ```

### Symptom: Connection Errors After PostgreSQL Restart

After PostgreSQL restarts, mentatd's pooled connections become invalid. The pool detects broken connections lazily (on next use), so you may see a burst of errors.

**Solution:**

Reduce `max_lifetime_secs` so connections are recycled regularly. With `max_lifetime_secs = 900`, all connections will be refreshed within 15 minutes.

For immediate recovery, restart mentatd:
```bash
systemctl restart mentatd
# or
kill -HUP $(pidof mentatd)
```

### Symptom: "Too many connections" in PostgreSQL

```sql
-- Check current vs maximum connections
SELECT
  count(*) AS current,
  setting::int AS maximum
FROM pg_stat_activity, pg_settings
WHERE pg_settings.name = 'max_connections'
GROUP BY setting;
```

**Solutions:**

1. Reduce mentatd `pool_size`
2. Increase PostgreSQL `max_connections` (requires restart)
3. Use PgBouncer between mentatd and PostgreSQL for connection multiplexing

## Cache Tuning

### Symptom: Low Cache Hit Ratio

Cache hit ratio below 30% means most queries bypass the cache.

**Diagnosis:**

```bash
curl -s http://localhost:8080/metrics | grep cache
# mentatd_cache_hits_total 120
# mentatd_cache_misses_total 880
# Hit ratio: 120/1000 = 12%
```

**Causes and solutions:**

| Cause | Diagnosis | Solution |
|-------|-----------|----------|
| High query cardinality | Many distinct `(query, args)` combinations | Increase `cache.capacity` |
| Frequent writes | High `mentatd_transactions_total` rate | Cache benefit is limited; reduce capacity to save memory |
| Short TTL | Entries expire before reuse | Increase `cache.ttl_secs` |
| Cache too small | Capacity reached, LRU evicting useful entries | Increase `cache.capacity` |

**Adjusting cache configuration:**

```toml
# Read-heavy: maximize caching
[cache]
enabled = true
capacity = 10000
ttl_secs = 600

# Write-heavy: minimize overhead
[cache]
enabled = true
capacity = 200
ttl_secs = 60
```

### Symptom: High Memory Usage from Cache

Each cached entry stores the full JSON string returned by PostgreSQL. Large result sets can consume significant memory.

**Estimate cache memory:**
```
cache_memory ~= capacity * average_result_size_bytes
```

If average query results are 10 KB and capacity is 10000, the cache could use ~100 MB.

**Solutions:**
- Reduce `capacity`
- Design queries to return smaller result sets (use aggregates, limit columns)
- Monitor process memory: `ps aux | grep mentatd`

### Symptom: Stale Data After Transaction

If queries return stale data after a transaction, the cache invalidation may have failed.

This should not happen in normal operation -- `mentatd/src/server.rs` calls `query_cache.invalidate()` after every successful transaction. However, if mentatd crashes between the PostgreSQL commit and the cache invalidation, the cache may be stale.

**Solutions:**
- Restart mentatd (clears the cache)
- Reduce `ttl_secs` to limit the window of staleness
- As a workaround, clients can add a unique no-op parameter to force a cache miss

## Memory Optimization

### mentatd Memory

mentatd memory usage consists of:
- **Base process:** ~50-100 MB (Tokio runtime, HTTP server, metrics)
- **Connection pool:** ~5-10 MB per connection
- **Query cache:** varies (see cache tuning above)
- **Per-request:** temporary allocations for JSON parsing and serialization

**Monitor:**
```bash
# Process memory
ps -o rss,vsz,pid,command -p $(pidof mentatd)

# /proc stats (Linux)
cat /proc/$(pidof mentatd)/status | grep -E 'VmRSS|VmSize|VmPeak'
```

**If memory usage is too high:**
1. Reduce cache `capacity`
2. Reduce `pool_size`
3. Set request `timeout` to prevent long-running queries from accumulating
4. Ensure clients are not sending unbounded queries (queries that return millions of rows)

### PostgreSQL Memory

**Check per-connection memory:**
```sql
-- Approximate memory per backend
SELECT
  pid,
  usename,
  state,
  query,
  pg_size_pretty(
    pg_backend_memory_contexts.total_bytes
  ) AS memory
FROM pg_stat_activity
JOIN pg_backend_memory_contexts ON true
WHERE usename = 'mentat'
LIMIT 10;
```

**Key PostgreSQL memory parameters:**

| Parameter | Impact | Recommendation |
|-----------|--------|----------------|
| `shared_buffers` | Global shared cache | 25% of system RAM |
| `work_mem` | Per-sort/hash memory | 32-64 MB for OLTP, 256 MB for OLAP |
| `maintenance_work_mem` | VACUUM, CREATE INDEX | 512 MB - 2 GB |
| `effective_cache_size` | Planner hint (no allocation) | 75% of system RAM |

**If PostgreSQL OOM-kills:**
1. Reduce `max_connections`
2. Reduce `work_mem`
3. Check for queries using excessive sorts: `EXPLAIN ANALYZE` and look for `Sort Method: external merge`
4. Enable `log_temp_files = 0` to log all temp file usage

## CPU Optimization

### Symptom: High CPU on PostgreSQL

**Diagnosis:**
```sql
-- Find CPU-intensive queries
SELECT
  pid,
  state,
  left(query, 100) AS query,
  extract(epoch from (now() - query_start))::int AS runtime_sec
FROM pg_stat_activity
WHERE state = 'active'
ORDER BY query_start;
```

**Common causes:**
1. **Full table scans** -- add indexes (see Slow Queries section)
2. **JIT compilation overhead** -- disable JIT for short queries: `SET jit = off;`
3. **Excessive parallelism** -- reduce `max_parallel_workers_per_gather`
4. **Frequent VACUUM** -- tune autovacuum settings

### Symptom: High CPU on mentatd

**Diagnosis:**
```bash
# CPU profiling with perf (Linux)
perf top -p $(pidof mentatd)

# Or generate a flamegraph
perf record -g -p $(pidof mentatd) -- sleep 30
perf script | stackcollapse-perf.pl | flamegraph.pl > mentatd.svg
```

**Common causes:**
1. **JSON serialization** -- use Transit+MessagePack (`Accept: application/transit+msgpack`) to reduce serialization work
2. **EDN parsing** -- malformed requests cause repeated parse retries
3. **Cache lock contention** -- the query cache uses a `Mutex`; very high concurrency can cause contention

## Common Production Issues

### Issue: mentatd Fails to Start

**Check:**
1. PostgreSQL is running: `pg_isready`
2. Connection string is valid: `psql $DATABASE_URL`
3. Port is available: `lsof -i :8080`
4. Config file is valid: review mentatd startup logs

**Common errors:**

| Error | Cause | Fix |
|-------|-------|-----|
| `Address already in use` | Port 8080 in use | Kill the other process or use `--port 8081` |
| `connection refused` | PostgreSQL not running | Start PostgreSQL |
| `password authentication failed` | Bad credentials | Fix `DATABASE_URL` or PostgreSQL `pg_hba.conf` |
| `Failed to parse config` | Invalid TOML | Check `mentatd.toml` syntax |

### Issue: Extension Not Loaded

```sql
-- Check if extension is installed
SELECT * FROM pg_extension WHERE extname = 'pg_mentat';

-- If not installed:
CREATE EXTENSION pg_mentat;

-- Check extension version
SELECT extversion FROM pg_extension WHERE extname = 'pg_mentat';
```

If `CREATE EXTENSION` fails, the extension shared library is missing:
```bash
# Check extension files exist
ls $(pg_config --sharedir)/extension/pg_mentat*
ls $(pg_config --pkglibdir)/pg_mentat*

# Reinstall
cargo pgrx install --release
```

### Issue: Disk Space Running Out

The `mentat.datoms` table grows monotonically because retractions add new rows.

**Check disk usage:**
```sql
SELECT
  pg_size_pretty(pg_total_relation_size('mentat.datoms')) AS datoms_total,
  pg_size_pretty(pg_relation_size('mentat.datoms')) AS datoms_table,
  pg_size_pretty(
    pg_total_relation_size('mentat.datoms') -
    pg_relation_size('mentat.datoms')
  ) AS datoms_indexes;

-- Check WAL size
SELECT pg_size_pretty(sum(size))
FROM pg_ls_waldir();

-- Check temp files
SELECT pg_size_pretty(sum(temp_bytes))
FROM pg_stat_database;
```

**Solutions:**
1. **VACUUM** to reclaim dead tuple space (does not reduce datom table growth):
   ```sql
   VACUUM FULL mentat.datoms;  -- Locks table; use during maintenance window
   ```
2. **Reduce WAL retention:**
   ```ini
   max_wal_size = 1GB  # was 2GB
   ```
3. **Archive old data** to separate storage (requires custom tooling)
4. **Add disk capacity** -- the simplest solution for a growing database

### Issue: Replication Lag

If using PostgreSQL streaming replication for read replicas:

```sql
-- On primary: check replication status
SELECT
  client_addr,
  state,
  sent_lsn,
  write_lsn,
  replay_lsn,
  pg_wal_lsn_diff(sent_lsn, replay_lsn) AS replay_lag_bytes
FROM pg_stat_replication;

-- On replica: check lag
SELECT
  pg_last_wal_receive_lsn() AS received,
  pg_last_wal_replay_lsn() AS replayed,
  pg_wal_lsn_diff(pg_last_wal_receive_lsn(), pg_last_wal_replay_lsn()) AS lag_bytes;
```

**If lag is growing:**
1. Check replica I/O: disk may be bottleneck
2. Reduce transaction rate on primary
3. Increase `wal_sender_timeout` on primary
4. Check network between primary and replica

## Diagnostic Commands Reference

### mentatd

```bash
# Health check
curl -s http://localhost:8080/health

# Metrics
curl -s http://localhost:8080/metrics

# Debug logging (temporary)
RUST_LOG=debug ./target/release/mentatd

# Module-specific logging
RUST_LOG=mentatd::server=debug,mentatd::cache=trace ./target/release/mentatd

# Check if running
pidof mentatd
lsof -i :8080
```

### PostgreSQL

```sql
-- Active connections and queries
SELECT pid, state, query, query_start
FROM pg_stat_activity
WHERE datname = 'mentat'
ORDER BY query_start;

-- Lock contention
SELECT
  blocked.pid AS blocked_pid,
  blocked.query AS blocked_query,
  blocking.pid AS blocking_pid,
  blocking.query AS blocking_query
FROM pg_stat_activity blocked
JOIN pg_locks bl ON bl.pid = blocked.pid
JOIN pg_locks bll ON bll.locktype = bl.locktype
  AND bll.database IS NOT DISTINCT FROM bl.database
  AND bll.relation IS NOT DISTINCT FROM bl.relation
  AND bll.page IS NOT DISTINCT FROM bl.page
  AND bll.tuple IS NOT DISTINCT FROM bl.tuple
  AND bll.pid != bl.pid
JOIN pg_stat_activity blocking ON blocking.pid = bll.pid
WHERE NOT bl.granted;

-- Table statistics
SELECT * FROM pg_stat_user_tables WHERE schemaname = 'mentat';

-- Index statistics
SELECT * FROM pg_stat_user_indexes WHERE schemaname = 'mentat';

-- Database size
SELECT pg_size_pretty(pg_database_size('mentat'));

-- Configuration check
SHOW shared_buffers;
SHOW work_mem;
SHOW max_connections;
SHOW effective_cache_size;
```

### System (Linux)

```bash
# CPU and memory overview
top -p $(pidof mentatd),$(pidof postgres)

# Disk I/O
iostat -x 1

# Network connections
ss -tnp | grep 8080
ss -tnp | grep 5432

# Open file descriptors
ls /proc/$(pidof mentatd)/fd | wc -l

# Memory details
cat /proc/$(pidof mentatd)/status | grep -E 'Vm|Threads'
```

## Runbook: Emergency Response

### mentatd is down

1. Check process: `pidof mentatd`
2. Check logs: `journalctl -u mentatd -n 50`
3. Check port: `lsof -i :8080`
4. Restart: `systemctl restart mentatd`
5. Verify: `curl http://localhost:8080/health`

### PostgreSQL is down

1. Check status: `pg_isready`
2. Check logs: `tail -50 /var/log/postgresql/postgresql-*.log`
3. Restart: `systemctl restart postgresql`
4. Verify: `psql -c "SELECT 1"`
5. Restart mentatd (to reset connection pool): `systemctl restart mentatd`

### High latency spike

1. Check mentatd metrics: `curl -s localhost:8080/metrics | grep duration`
2. Check PostgreSQL for long queries:
   ```sql
   SELECT pid, query, extract(epoch from (now()-query_start))::int AS sec
   FROM pg_stat_activity WHERE state='active' ORDER BY query_start LIMIT 5;
   ```
3. Cancel long queries if needed: `SELECT pg_cancel_backend(<pid>);`
4. Check disk I/O: `iostat -x 1 5`
5. Check for lock contention (see diagnostic commands above)

### Out of disk space

1. Check usage: `df -h`
2. Check WAL: `SELECT pg_size_pretty(sum(size)) FROM pg_ls_waldir();`
3. Check temp files: `SELECT pg_size_pretty(sum(temp_bytes)) FROM pg_stat_database;`
4. Run checkpoint: `CHECKPOINT;` (flushes WAL)
5. VACUUM if needed: `VACUUM mentat.datoms;`
6. Clear old logs: rotate or truncate log files
7. Add disk capacity

## See Also

- [Performance Tuning Guide](./PERFORMANCE_TUNING.md) -- PostgreSQL and mentatd tuning
- [Monitoring Guide](./MONITORING.md) -- Metrics, alerts, and dashboards
- [Common Issues](../troubleshooting/COMMON_ISSUES.md) -- Schema, query, and transaction errors
- [Debugging Guide](../troubleshooting/DEBUGGING.md) -- Advanced debugging techniques
