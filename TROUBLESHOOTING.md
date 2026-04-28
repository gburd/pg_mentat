# Troubleshooting Guide

Operational troubleshooting guide for pg_mentat and mentatd in production. This document covers common issues, performance debugging, index maintenance, and query optimization.

---

## Table of Contents

1. [Quick Diagnostic Commands](#quick-diagnostic-commands)
2. [Extension Issues](#extension-issues)
3. [Query Performance](#query-performance)
4. [Transaction Issues](#transaction-issues)
5. [Connection and Pool Issues](#connection-and-pool-issues)
6. [Memory Issues](#memory-issues)
7. [Disk Space Issues](#disk-space-issues)
8. [mentatd Issues](#mentatd-issues)
9. [Cache Tuning](#cache-tuning)
10. [Index Maintenance](#index-maintenance)
11. [Emergency Runbook](#emergency-runbook)

---

## Quick Diagnostic Commands

### pg_mentat Extension

```sql
-- Verify extension is loaded
SELECT extversion FROM pg_extension WHERE extname = 'pg_mentat';

-- Storage statistics
SELECT mentat_storage_stats();

-- Query performance statistics
SELECT mentat_query_stats();

-- Current schema
SELECT mentat_schema();
```

### PostgreSQL

```sql
-- Active connections and queries
SELECT pid, state, query, now() - query_start AS duration
FROM pg_stat_activity
WHERE datname = current_database()
ORDER BY query_start;

-- Table sizes (mentat schema)
SELECT
    relname,
    pg_size_pretty(pg_total_relation_size(c.oid)) AS total,
    n_live_tup AS live_rows,
    n_dead_tup AS dead_rows
FROM pg_stat_user_tables s
JOIN pg_class c ON s.relid = c.oid
WHERE schemaname = 'mentat'
ORDER BY pg_total_relation_size(c.oid) DESC;

-- Database size
SELECT pg_size_pretty(pg_database_size(current_database()));
```

### mentatd

```bash
# Health check
curl -s http://localhost:8080/health

# Prometheus metrics
curl -s http://localhost:8080/metrics

# Debug logging
RUST_LOG=debug mentatd
```

### System (Linux)

```bash
# Process overview
ps aux | grep -E 'mentatd|postgres'

# Disk I/O
iostat -x 1 5

# Network connections
ss -tnp | grep -E '8080|5432'
```

---

## Extension Issues

### CREATE EXTENSION fails

**Symptom:** `CREATE EXTENSION pg_mentat;` returns an error.

**Diagnosis:**

```sql
-- Check if extension files are available
SELECT * FROM pg_available_extensions WHERE name = 'pg_mentat';
```

```bash
# Check extension files on disk
ls $(pg_config --sharedir)/extension/pg_mentat*
ls $(pg_config --pkglibdir)/pg_mentat*
```

**Common causes and fixes:**

| Error | Cause | Fix |
|-------|-------|-----|
| `could not open extension control file` | Extension not installed | Run `cargo pgrx install --release` |
| `incompatible library` | Built for different PG version | Rebuild with correct `--features pgNN` flag |
| `could not load library` | Missing shared library dependencies | Check `ldd $(pg_config --pkglibdir)/pg_mentat.so` for missing libs |
| `permission denied` | File permissions | Ensure the PostgreSQL user can read the extension files |

**Rebuild from source:**

```bash
cd pg_mentat
cargo pgrx install --release --pg-config=$(which pg_config)
```

### Extension functions return errors

**Symptom:** Functions like `mentat_transact()` or `mentat_query()` return unexpected errors.

**Diagnosis:**

```sql
-- Check extension version
SELECT extversion FROM pg_extension WHERE extname = 'pg_mentat';

-- Check if bootstrap data is present
SELECT count(*) FROM mentat.schema;
-- Should be > 0 (bootstrap creates ~80 attribute definitions)

-- Check partition boundaries
SELECT name, next_entid FROM mentat.partitions;
```

If bootstrap data is missing, the extension may need to be dropped and recreated:

```sql
DROP EXTENSION pg_mentat CASCADE;
CREATE EXTENSION pg_mentat;
```

---

## Query Performance

### Slow Datalog Queries

**Symptom:** `mentat_query()` calls take > 100ms for simple queries.

**Step 1: Get the generated SQL**

```sql
-- Use mentat_explain to see the query plan
SELECT mentat_explain('[:find ?name :where [?e :person/name ?name]]', '{}');

-- Or get the raw SQL
-- SELECT mentat_query_sql('[:find ?name :where [?e :person/name ?name]]', '{}');
```

**Step 2: Analyze the query plan**

```sql
EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT)
<paste the generated SQL here>;
```

**Step 3: Identify the problem**

| Plan observation | Likely cause | Fix |
|------------------|-------------|-----|
| `Seq Scan on datoms` | Missing or unused index | Run `ANALYZE mentat.datoms;` or add index via schema |
| `actual rows` >> `estimated rows` | Stale statistics | Run `ANALYZE` on affected tables |
| `Sort Method: external merge Disk` | Insufficient work_mem | Increase `work_mem` |
| `Nested Loop (actual loops=100000)` | Poor join order | Reorder Datalog patterns: most selective first |
| `Buffers: shared read=50000` | Cold buffer pool | Increase `shared_buffers` or pre-warm |

**Step 4: Reorder patterns for better performance**

The order of `:where` patterns matters. Place the most selective patterns first:

```clojure
;; Slow: broad scan first
[:find ?name
 :where
 [?e :person/name ?name]
 [?e :person/country "US"]]

;; Faster: selective pattern first
[:find ?name
 :where
 [?e :person/country "US"]
 [?e :person/name ?name]]
```

**Step 5: Update statistics**

```sql
ANALYZE mentat.datoms;
ANALYZE mentat.schema;
ANALYZE mentat.transactions;

-- For type-specific tables (Phase 3 storage)
ANALYZE mentat.datoms_ref_new;
ANALYZE mentat.datoms_long_new;
ANALYZE mentat.datoms_text_new;
-- ... etc for all type-specific tables
```

### UNION ALL Performance

The current storage design uses UNION ALL across 9 type-specific tables when the value type is unknown at query-planning time. For queries where the attribute type is known, pg_mentat can target a single table directly.

**Workaround for known-type queries:**

If you know the value type, use the type-specific SQL views:

```sql
-- Instead of a generic Datalog query for text values
SELECT * FROM mentat.text_values WHERE attribute = ':person/name';

-- For numeric values
SELECT * FROM mentat.numeric_values WHERE attribute = ':person/age';
```

---

## Transaction Issues

### Serialization Failures

**Symptom:** `mentat_transact()` fails with serialization or deadlock errors under concurrent writes.

**Cause:** pg_mentat uses serializable isolation. Concurrent transactions modifying the same entities will conflict.

**Fix:** Implement client-side retry logic:

```python
import time
import psycopg2

def transact_with_retry(conn, edn, max_retries=3):
    for attempt in range(max_retries):
        try:
            with conn.cursor() as cur:
                cur.execute("SELECT mentat_transact(%s)", [edn])
                return cur.fetchone()[0]
        except psycopg2.errors.SerializationFailure:
            conn.rollback()
            time.sleep(0.1 * (2 ** attempt))  # Exponential backoff
    raise Exception("Transaction failed after retries")
```

### Unique Constraint Violations

**Symptom:** Transaction fails with a constraint violation on a `:db.unique/identity` attribute.

**Diagnosis:**

```sql
-- Check for duplicate values on unique attributes
SELECT s.ident, d.a, count(*)
FROM mentat.datoms d
JOIN mentat.schema s ON s.entid = d.a
WHERE s.unique_constraint IS NOT NULL AND d.added = TRUE
GROUP BY s.ident, d.a
HAVING count(*) > 1;
```

**Note:** Unique identity upsert semantics are under active development (Task #4). Currently, transacting a value that matches an existing unique identity attribute may error instead of performing an upsert.

### Large Transactions Are Slow

**Cause:** Each datom in a transaction generates individual INSERT statements within a single PostgreSQL transaction.

**Fix:** Batch assertions into groups of 100-500 datoms per transaction. This balances throughput with lock duration:

```sql
-- Instead of one transaction with 10,000 datoms, split into 20 batches of 500
SELECT mentat_transact('[
  {:person/name "Alice" :person/age 30}
  {:person/name "Bob" :person/age 25}
  ... -- up to ~500 entities per batch
]');
```

---

## Connection and Pool Issues

### "Failed to get connection from pool"

**Symptom:** mentatd returns 503 errors. `mentatd_connection_pool_available` drops to 0.

**Diagnosis:**

```bash
curl -s http://localhost:8080/metrics | grep connection_pool
```

```sql
-- Check PostgreSQL connections
SELECT state, count(*)
FROM pg_stat_activity
WHERE datname = current_database()
GROUP BY state;

-- Find long-running queries holding connections
SELECT pid, state, query, now() - query_start AS duration
FROM pg_stat_activity
WHERE state = 'active'
ORDER BY query_start;
```

**Fixes:**

1. Increase `DATABASE_POOL_SIZE` (and PostgreSQL `max_connections` if needed)
2. Kill long-running queries:
   ```sql
   SELECT pg_cancel_backend(pid)
   FROM pg_stat_activity
   WHERE state = 'active'
     AND now() - query_start > interval '60 seconds';
   ```
3. Set `statement_timeout` to prevent runaway queries:
   ```sql
   ALTER ROLE mentat_app SET statement_timeout = '30s';
   ```

### "Too many connections"

**Symptom:** PostgreSQL refuses new connections.

```sql
SELECT count(*) AS current,
       setting::int AS maximum
FROM pg_stat_activity, pg_settings
WHERE pg_settings.name = 'max_connections'
GROUP BY setting;
```

**Fixes:**

1. Reduce mentatd `pool_size`
2. Increase PostgreSQL `max_connections` (requires restart)
3. Deploy PgBouncer for connection multiplexing

### Connection Errors After PostgreSQL Restart

**Cause:** mentatd's pooled connections become invalid after a PostgreSQL restart. The pool detects broken connections lazily.

**Fix:** Restart mentatd, or reduce `max_lifetime_secs` so connections recycle faster:

```toml
[database]
max_lifetime_secs = 900  # 15 minutes
```

---

## Memory Issues

### PostgreSQL Out of Memory

**Diagnosis:**

```sql
SHOW shared_buffers;
SHOW work_mem;
SHOW max_connections;
```

**Common causes:**

- `work_mem` too high with many concurrent queries: total memory = `work_mem * max_connections * operations_per_query`
- Too many connections: each backend uses ~10 MB base memory
- Large result sets loaded entirely into memory by the extension

**Fixes:**

1. Reduce `work_mem` (start at 32 MB, increase only if `EXPLAIN ANALYZE` shows disk sorts)
2. Reduce `max_connections` and use PgBouncer
3. Set `temp_file_limit` to catch runaway queries:
   ```sql
   ALTER SYSTEM SET temp_file_limit = '1GB';
   SELECT pg_reload_conf();
   ```

### mentatd High Memory Usage

**Diagnosis:**

```bash
ps -o rss,vsz,pid,command -p $(pidof mentatd)
```

mentatd memory consists of:
- Base process: ~50-100 MB
- Connection pool: ~5-10 MB per connection
- Query cache: capacity * average_result_size

**Fixes:**

1. Reduce cache `capacity`
2. Reduce `pool_size`
3. Set request `timeout` to prevent long-lived requests
4. Ensure queries don't return unbounded result sets

---

## Disk Space Issues

### Datom Tables Growing Continuously

**Cause:** pg_mentat is append-only by design. Retractions add new rows with `added = FALSE` rather than deleting existing rows. The datoms table grows monotonically.

**Diagnosis:**

```sql
-- Data vs index size
SELECT
    relname,
    pg_size_pretty(pg_relation_size(c.oid)) AS data,
    pg_size_pretty(pg_indexes_size(c.oid)) AS indexes,
    pg_size_pretty(pg_total_relation_size(c.oid)) AS total
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'mentat'
ORDER BY pg_total_relation_size(c.oid) DESC;

-- Dead tuple bloat
SELECT relname, n_dead_tup, n_live_tup,
       round(100.0 * n_dead_tup / GREATEST(n_live_tup + n_dead_tup, 1), 1) AS dead_pct
FROM pg_stat_user_tables
WHERE schemaname = 'mentat'
ORDER BY n_dead_tup DESC;

-- WAL size
SELECT pg_size_pretty(sum(size)) FROM pg_ls_waldir();
```

**Fixes:**

1. Run `VACUUM FULL` during a maintenance window (locks the table):
   ```sql
   VACUUM FULL mentat.datoms;
   ```
2. Tune autovacuum for aggressive dead-tuple reclamation (see PostgreSQL Tuning in `PRODUCTION_DEPLOYMENT.md`)
3. Add disk capacity -- this is often the simplest solution for an append-only system

### WAL Accumulation

**Cause:** WAL archiving is misconfigured or the archive destination is full.

**Fix:**

```sql
-- Force a checkpoint to flush WAL
CHECKPOINT;
```

Check that `archive_command` is working:

```bash
# Test archive command manually
ls /backup/wal/ | tail -5
```

---

## mentatd Issues

### mentatd Won't Start

**Diagnosis:**

```bash
# Run with debug logging
RUST_LOG=debug mentatd

# Check if the port is in use
ss -tlnp | grep 8080

# Check PostgreSQL connectivity
psql "$DATABASE_URL" -c "SELECT 1;"
```

**Common causes:**

| Error | Cause | Fix |
|-------|-------|-----|
| `Address already in use` | Port 8080 occupied | Change `MENTATD_PORT` or stop conflicting process |
| `connection refused` | PostgreSQL not running | Start PostgreSQL first |
| `password authentication failed` | Wrong credentials | Fix `DATABASE_URL` or `pg_hba.conf` |
| `Failed to parse config` | Invalid TOML | Check `mentatd.toml` syntax |
| Extension not loaded | `mentat` schema missing | Run `CREATE EXTENSION pg_mentat;` |

### mentatd High Latency

**Diagnosis:**

```bash
curl -s http://localhost:8080/metrics | grep duration
```

**Common causes and fixes:**

1. **PostgreSQL is the bottleneck** -- check `pg_stat_activity` for slow queries
2. **Connection pool saturated** -- increase `pool_size`
3. **Cache not effective** -- check `mentatd_cache_hit_rate`; tune capacity and TTL
4. **JIT compilation overhead** -- disable JIT: `SET jit = off;`
5. **Network latency** -- ensure mentatd is co-located with PostgreSQL

---

## Cache Tuning

### Low Hit Rate

**Symptom:** `mentatd_cache_hit_rate` < 0.5

```bash
curl -s http://localhost:8080/metrics | grep cache
```

**Causes and fixes:**

| Cause | Diagnosis | Fix |
|-------|-----------|-----|
| High query cardinality | Many distinct query+args combos | Increase `capacity` |
| Frequent writes | High transaction rate invalidates cache | Reduce `capacity` to save memory |
| Short TTL | Entries expire before reuse | Increase `ttl_secs` |
| Cache too small | Entries at capacity, LRU evicting useful entries | Increase `capacity` |

**Read-heavy configuration:**

```toml
[cache]
enabled = true
capacity = 10000
ttl_secs = 600
```

**Write-heavy configuration:**

```toml
[cache]
enabled = true
capacity = 200
ttl_secs = 60
```

### Stale Data After Transaction

**Symptom:** Queries return old data after a successful transaction.

mentatd invalidates the cache after every successful transaction. If stale data persists:

1. Restart mentatd to clear the in-memory cache
2. Reduce `ttl_secs` to limit the staleness window
3. Check for clock skew between mentatd instances

---

## Index Maintenance

### Monitoring Index Bloat

```sql
-- Index sizes and scan counts
SELECT
    indexrelname AS index_name,
    pg_size_pretty(pg_relation_size(indexrelid)) AS size,
    idx_scan AS scans
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
ORDER BY pg_relation_size(indexrelid) DESC;

-- Unused indexes (candidates for investigation, not removal)
SELECT indexrelname, idx_scan, pg_size_pretty(pg_relation_size(indexrelid))
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat' AND idx_scan = 0
ORDER BY pg_relation_size(indexrelid) DESC;
```

### Rebuilding Indexes

If indexes are bloated (size much larger than expected for the row count):

```sql
-- Online reindex (PostgreSQL 12+, minimal locking)
REINDEX TABLE CONCURRENTLY mentat.datoms;

-- Or individual indexes
REINDEX INDEX CONCURRENTLY mentat.idx_datoms_eavt;
```

### Index Bloat Monitoring View

Create a monitoring view for ongoing observation:

```sql
CREATE VIEW mentat.index_health AS
SELECT
    schemaname,
    tablename,
    indexname,
    pg_size_pretty(pg_relation_size(indexrelid)) AS size,
    idx_scan,
    idx_tup_read,
    idx_tup_fetch
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
ORDER BY pg_relation_size(indexrelid) DESC;
```

---

## Emergency Runbook

### mentatd is Down

1. Check process: `pidof mentatd`
2. Check logs: `journalctl -u mentatd -n 50`
3. Check port: `ss -tlnp | grep 8080`
4. Restart: `systemctl restart mentatd`
5. Verify: `curl http://localhost:8080/health`

### PostgreSQL is Down

1. Check status: `pg_isready`
2. Check logs: `tail -50 /var/log/postgresql/postgresql-*.log`
3. Restart: `systemctl restart postgresql`
4. Verify: `psql -c "SELECT 1"`
5. Restart mentatd to reset pool: `systemctl restart mentatd`

### High Latency Spike

1. Check mentatd metrics: `curl -s localhost:8080/metrics | grep duration`
2. Check for long queries:
   ```sql
   SELECT pid, query, extract(epoch from (now()-query_start))::int AS sec
   FROM pg_stat_activity WHERE state='active' ORDER BY query_start LIMIT 5;
   ```
3. Cancel long queries: `SELECT pg_cancel_backend(<pid>);`
4. Check disk I/O: `iostat -x 1 5`
5. Check for lock contention:
   ```sql
   SELECT * FROM pg_locks WHERE NOT granted;
   ```

### Out of Disk Space

1. Check usage: `df -h`
2. Check WAL: `SELECT pg_size_pretty(sum(size)) FROM pg_ls_waldir();`
3. Run checkpoint: `CHECKPOINT;`
4. VACUUM bloated tables: `VACUUM mentat.datoms;`
5. Clear old logs
6. Add disk capacity

### Data Corruption Suspected

1. Stop mentatd: `systemctl stop mentatd`
2. Check referential integrity:
   ```sql
   SELECT count(*) FROM mentat.datoms d
   WHERE NOT EXISTS (SELECT 1 FROM mentat.schema s WHERE s.entid = d.a);
   -- Should be 0
   ```
3. If corruption confirmed, restore from backup (see `PRODUCTION_DEPLOYMENT.md`)
4. Verify partition boundaries match sequence values after restore

---

## See Also

- [PRODUCTION_DEPLOYMENT.md](PRODUCTION_DEPLOYMENT.md) -- Deployment, tuning, backup/restore
- [CAPACITY_PLANNING.md](CAPACITY_PLANNING.md) -- Sizing guidelines and scaling strategies
- [MIGRATION_FROM_DATOMIC.md](MIGRATION_FROM_DATOMIC.md) -- Migration from Datomic
- [EXPERT_REVIEW.md](EXPERT_REVIEW.md) -- Detailed production readiness assessment
