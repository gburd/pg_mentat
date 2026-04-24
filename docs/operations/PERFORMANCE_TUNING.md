# Performance Tuning Guide

Practical guide for tuning pg_mentat and mentatd for production workloads.

## PostgreSQL Configuration

pg_mentat performance is dominated by PostgreSQL query execution. The `mentat.datoms` table is the primary data store, with EAVT-indexed rows and BYTEA-encoded values. Tuning PostgreSQL memory and planner settings has the largest impact on query latency.

### OLTP Workloads (Many Small Transactions)

Typical use case: web applications, microservices, frequent reads and writes with small transaction batches (< 100 datoms per transaction).

```ini
# postgresql.conf -- OLTP profile
# Assumes 16 GB RAM, SSD storage, 4-8 CPU cores

# Memory
shared_buffers = 4GB                # 25% of RAM; holds hot datoms pages
effective_cache_size = 12GB         # 75% of RAM; planner hint for OS cache
work_mem = 32MB                     # Per-sort/hash; mentat queries use joins
maintenance_work_mem = 512MB        # VACUUM, CREATE INDEX

# WAL -- favor latency over crash recovery time
wal_buffers = 64MB
wal_level = replica                 # Needed for replication; 'minimal' if standalone
synchronous_commit = on             # Set 'off' only if you accept data loss on crash
checkpoint_completion_target = 0.9
max_wal_size = 2GB
min_wal_size = 512MB

# Connections -- match mentatd pool_size + headroom
max_connections = 100

# Planner -- SSD settings
random_page_cost = 1.1              # SSD: nearly same cost as sequential
effective_io_concurrency = 200      # SSD: high concurrency
seq_page_cost = 1.0

# JIT -- disable for short queries
jit = off                           # JIT overhead > benefit for sub-100ms queries

# Parallelism -- keep off for small queries
max_parallel_workers_per_gather = 0
```

### OLAP Workloads (Complex Analytical Queries)

Typical use case: reporting, data analysis, large aggregation queries across many entities, infrequent writes.

```ini
# postgresql.conf -- OLAP profile
# Assumes 64 GB RAM, SSD/NVMe storage, 16+ CPU cores

# Memory -- larger allocations for complex joins
shared_buffers = 16GB               # 25% of RAM
effective_cache_size = 48GB         # 75% of RAM
work_mem = 256MB                    # Large sorts and hash joins
maintenance_work_mem = 2GB
hash_mem_multiplier = 2.0           # PG 13+: extra memory for hash operations

# WAL -- favor throughput
wal_buffers = 64MB
wal_level = minimal                 # If no replication needed
synchronous_commit = off            # Writes are infrequent
checkpoint_completion_target = 0.9
max_wal_size = 4GB

# Connections
max_connections = 50                # Fewer connections, each uses more memory

# Planner
random_page_cost = 1.1
effective_io_concurrency = 200
seq_page_cost = 1.0

# JIT -- enable for complex queries
jit = on
jit_above_cost = 100000

# Parallelism -- enable for large scans
max_parallel_workers_per_gather = 4
max_parallel_workers = 8
max_parallel_maintenance_workers = 4
parallel_tuple_cost = 0.01
parallel_setup_cost = 1000
min_parallel_table_scan_size = 8MB
```

### Mixed Workloads

For workloads that combine both patterns, start with the OLTP profile and selectively enable parallelism:

```ini
# Start from OLTP profile, then adjust:
work_mem = 64MB                     # Compromise
jit = off                           # Usually not worth it for mixed
max_parallel_workers_per_gather = 2 # Limited parallelism
```

### Key Parameters Explained

| Parameter | OLTP | OLAP | Why |
|-----------|------|------|-----|
| `shared_buffers` | 25% RAM | 25% RAM | Hot datoms pages stay cached |
| `work_mem` | 32MB | 256MB | Mentat queries generate multi-way joins |
| `random_page_cost` | 1.1 | 1.1 | SSD makes random I/O cheap |
| `jit` | off | on | Short queries lose time to JIT compilation |
| `max_parallel_workers_per_gather` | 0 | 4 | Parallel scans help large datom table scans |

## mentatd Configuration

### Connection Pool Sizing

The pool size determines how many concurrent PostgreSQL queries mentatd can execute. Undersizing causes request queuing; oversizing causes PostgreSQL contention.

```toml
# mentatd.toml
[database]
pool_size = 16          # Rule of thumb: CPU cores * 2
max_lifetime_secs = 1800  # Recycle connections every 30 minutes
```

**Guidelines:**
- Start with `pool_size = CPU_cores * 2`
- Never exceed PostgreSQL `max_connections - 10` (leave headroom for admin)
- Monitor `mentatd_connection_pool_size` metric; if it equals `pool_size` and latency is rising, increase the pool
- If CPU utilization is low but latency is high, the pool is too small
- If CPU utilization is high and latency is high, the pool is too large (contention)

Environment variable override:
```bash
export DATABASE_POOL_SIZE=16
```

### Query Cache Tuning

mentatd includes an LRU cache that stores serialized query results. The cache is keyed by `(query_string, args_json)` and is fully invalidated after every transaction.

```toml
# mentatd.toml
[cache]
enabled = true
capacity = 1000       # Number of cached query results
ttl_secs = 300        # Entries expire after 5 minutes
```

**Tuning by workload pattern:**

| Workload | capacity | ttl_secs | Rationale |
|----------|----------|----------|-----------|
| Read-heavy, few writes | 5000-10000 | 600 | High cache hit ratio; invalidations are rare |
| Balanced read/write | 1000-2000 | 300 | Default; reasonable hit ratio |
| Write-heavy | 100-500 | 60 | Frequent invalidations reduce cache benefit; save memory |
| Large result sets | 500 | 300 | Each entry stores the raw JSON string; reduce capacity to bound memory |

**Monitoring cache effectiveness:**
```
Cache hit ratio = mentatd_cache_hits_total / (mentatd_cache_hits_total + mentatd_cache_misses_total)
```

Target > 80% for read-heavy workloads. If the ratio is below 50%, either:
- Queries have high cardinality (many distinct query+args combinations) -- increase `capacity`
- Writes are frequent -- cache provides less benefit; consider reducing capacity to save memory

Environment variable overrides:
```bash
export MENTATD_CACHE_ENABLED=true
export MENTATD_CACHE_CAPACITY=5000
export MENTATD_CACHE_TTL=600
```

### Server Tuning

```toml
[server]
host = "0.0.0.0"       # Bind all interfaces in production
port = 8080
timeout = 30            # Request timeout in seconds
```

**Request timeout:** Set to the maximum acceptable query latency. If complex queries routinely take 10-20 seconds, set `timeout = 30`. If all queries should complete under 5 seconds, set `timeout = 10` to fail fast.

### Logging Overhead

Logging at `info` level has minimal overhead. At `debug` or `trace`, every request and cache lookup is logged, which can reduce throughput by 10-20%.

```toml
[logging]
level = "info"          # Production
format = "json"         # Structured for log aggregation
```

For temporary debugging:
```bash
RUST_LOG=mentatd::server=debug ./target/release/mentatd
```

### Response Format Selection

mentatd supports three serialization formats. The format is selected per-request via the `Accept` header.

| Format | Accept Header | Use Case | Relative Size |
|--------|--------------|----------|---------------|
| EDN | `application/edn` (default) | Human debugging, REPL | Baseline |
| Transit+JSON | `application/transit+json` | Web clients, JS apps | ~Same as EDN |
| Transit+MessagePack | `application/transit+msgpack` | Production, max throughput | 30-50% smaller |

For production clients, use Transit+MessagePack to reduce network transfer:
```bash
curl -X POST http://localhost:8080 \
  -H "Accept: application/transit+msgpack" \
  -H "Content-Type: application/edn" \
  -d '{:op :query :query "[:find ?e :where [?e :person/name]]"}'
```

## Indexing Strategies

### Default Indexes

pg_mentat creates these indexes on the `mentat.datoms` table:

| Index | Columns | Purpose |
|-------|---------|---------|
| EAVT (Primary Key) | `(e, a, v, tx)` | Entity lookups, pull API |
| AEVT | `(a, e, v, tx)` | Attribute scans, schema queries |
| AVET | `(a, v, e, tx)` | Value lookups, unique constraints |

These cover the common access patterns. The EAVT index serves entity-centric queries (`[:find ?a ?v :where [10001 ?a ?v]]`). The AEVT index serves attribute scans (`[:find ?e :where [?e :person/name]]`). The AVET index serves value lookups (`[:find ?e :where [?e :person/email "alice@example.com"]]`).

### When to Add Custom Indexes

Add `:db/index true` to an attribute when:

1. **Frequent value lookups** -- You query by a specific value of that attribute
2. **Range queries** -- You use predicates like `[(> ?age 25)]` on that attribute
3. **Join selectivity** -- The attribute appears early in query patterns and filters significantly

```sql
-- Mark attribute as indexed
SELECT mentat_transact($$
[{:db/id :person/email
  :db/index true}]
$$);
```

**Do NOT index when:**
- The attribute is only used for display (fetched after entity is found)
- The attribute has very low cardinality (e.g., boolean status flags)
- Write throughput is the bottleneck (each index adds write overhead)

### Checking Index Usage

```sql
-- See which indexes PostgreSQL actually uses
SELECT
  indexrelname AS index_name,
  idx_scan AS times_used,
  idx_tup_read AS rows_read,
  idx_tup_fetch AS rows_fetched
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
ORDER BY idx_scan DESC;

-- Find tables with high sequential scan counts (missing index candidates)
SELECT
  relname AS table_name,
  seq_scan,
  seq_tup_read,
  idx_scan,
  CASE WHEN seq_scan > 0
    THEN seq_tup_read / seq_scan
    ELSE 0
  END AS avg_rows_per_seq_scan
FROM pg_stat_user_tables
WHERE schemaname = 'mentat'
ORDER BY seq_tup_read DESC;
```

### PostgreSQL-Level Custom Indexes

For patterns that the built-in datom indexes do not cover efficiently, you can create PostgreSQL indexes directly:

```sql
-- Partial index: only current (non-retracted) datoms for a specific attribute
CREATE INDEX idx_person_name_current
ON mentat.datoms (v)
WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':person/name')
  AND added = true;

-- Covering index for a common join pattern
CREATE INDEX idx_datoms_attr_value
ON mentat.datoms (a, v)
WHERE added = true;
```

**Caution:** Custom PostgreSQL indexes are not managed by pg_mentat. You must maintain them yourself across schema changes and upgrades. Document any custom indexes you create.

### Maintaining Indexes

```sql
-- Rebuild indexes after bulk data load
REINDEX TABLE mentat.datoms;

-- Update statistics for the query planner
ANALYZE mentat.datoms;

-- Full vacuum + analyze (run during maintenance windows)
VACUUM ANALYZE mentat.datoms;
```

## Query Optimization Tips

### 1. Be Specific in Patterns

More specific patterns generate more efficient SQL with better index usage.

```clojure
;; SLOW: scans all datoms
[:find ?e ?a ?v
 :where [?e ?a ?v]]

;; FAST: uses AEVT index on :person/name
[:find ?e ?name
 :where [?e :person/name ?name]]

;; FASTER: uses EAVT index for known entity
[:find ?name
 :where [10001 :person/name ?name]]
```

### 2. Order Patterns by Selectivity

Place the most restrictive pattern first. The query engine joins patterns in order, so starting with a selective pattern reduces intermediate result sizes.

```clojure
;; SLOW: starts with broad scan, then filters
[:find ?name
 :where
 [?e :person/name ?name]
 [?e :person/status "active"]      ;; few active, but checked last
 [?e :person/age ?age]
 [(> ?age 65)]]

;; FASTER: start with most selective pattern
[:find ?name
 :where
 [?e :person/status "active"]      ;; fewest results, checked first
 [?e :person/age ?age]
 [(> ?age 65)]
 [?e :person/name ?name]]          ;; fetch name last
```

### 3. Use Aggregates Instead of Fetching All

```clojure
;; SLOW: fetches all entities, counts in application
[:find ?e
 :where [?e :person/name]]
;; then: len(results)

;; FAST: PostgreSQL COUNT
[:find (count ?e) .
 :where [?e :person/name]]
```

### 4. Use Pull for Entity Retrieval

When you need multiple attributes of the same entity, use `mentat_pull` instead of a query with many find variables.

```sql
-- SLOW: multiple round trips or wide query
SELECT mentat_query(
  '[:find ?name ?age ?email
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    [?e :person/email ?email]
    [(= ?e 10001)]]',
  '{}'::jsonb
);

-- FAST: single pull
SELECT mentat_pull('[:person/name :person/age :person/email]', 10001);
```

### 5. Batch Transactions

Group related assertions into a single transaction to reduce overhead.

```sql
-- SLOW: 3 transactions, 3 cache invalidations, 3 round trips
SELECT mentat_transact('[{:person/name "Alice"}]');
SELECT mentat_transact('[{:person/name "Bob"}]');
SELECT mentat_transact('[{:person/name "Carol"}]');

-- FAST: 1 transaction, 1 cache invalidation, 1 round trip
SELECT mentat_transact($$
[{:person/name "Alice"}
 {:person/name "Bob"}
 {:person/name "Carol"}]
$$);
```

Each transaction invalidates the entire query cache, so batching reduces cache churn.

### 6. Limit History Queries

History queries scan all datom rows including retractions. Always constrain by entity or time range.

```clojure
;; SLOW: full history scan
[:find ?e ?a ?v ?tx ?added
 :where [?e ?a ?v ?tx ?added]]

;; FAST: history of one entity
[:find ?a ?v ?tx ?added
 :where
 [?e ?a ?v ?tx ?added]
 [(= ?e 10001)]]

;; FAST: history within time range
[:find ?e ?a ?v ?tx ?added
 :where
 [?e ?a ?v ?tx ?added]
 [(>= ?tx 1000010)]
 [(<= ?tx 1000020)]]
```

### 7. Analyze Slow Queries

Enable `pg_stat_statements` to find slow queries:

```sql
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

-- Top 10 slowest mentat queries by mean execution time
SELECT
  query,
  calls,
  round(mean_exec_time::numeric, 2) AS avg_ms,
  round(total_exec_time::numeric, 2) AS total_ms,
  rows
FROM pg_stat_statements
WHERE query LIKE '%mentat%'
ORDER BY mean_exec_time DESC
LIMIT 10;
```

Use `EXPLAIN ANALYZE` for specific queries:

```sql
EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT)
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);
```

Look for:
- **Seq Scan** on `mentat.datoms` -- missing index
- **High actual rows vs. estimated rows** -- stale statistics, run `ANALYZE`
- **Sort operations using disk** -- increase `work_mem`
- **Nested Loop with high loop count** -- reorder query patterns

## Operating System Tuning

### Linux Kernel Parameters

```bash
# /etc/sysctl.conf

# Shared memory -- must accommodate shared_buffers
kernel.shmmax = 17179869184          # 16 GB
kernel.shmall = 4194304

# Virtual memory
vm.swappiness = 1                    # Minimize swapping
vm.dirty_ratio = 10
vm.dirty_background_ratio = 3
vm.overcommit_memory = 2             # Strict overcommit
vm.overcommit_ratio = 90

# Network
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
```

Apply with:
```bash
sudo sysctl -p
```

### File Descriptors

mentatd and PostgreSQL both need sufficient file descriptors:

```bash
# /etc/security/limits.conf
postgres    soft    nofile    65536
postgres    hard    nofile    65536
mentat      soft    nofile    65536
mentat      hard    nofile    65536
```

### I/O Scheduler

For SSD/NVMe storage:
```bash
# Check current scheduler
cat /sys/block/sda/queue/scheduler

# Set to none (NVMe) or mq-deadline (SATA SSD)
echo none > /sys/block/nvme0n1/queue/scheduler
```

## Performance Benchmarks

### Running Benchmarks

mentatd includes criterion-based microbenchmarks:

```bash
# Serialization benchmarks (no database needed)
cargo bench -p mentatd --bench serialization

# Cache benchmarks (no database needed)
cargo bench -p mentatd --bench cache

# Load tests (requires running mentatd + PostgreSQL)
cargo run -p mentatd --release
./mentatd/benches/load_test.sh localhost 8080
```

Reports are generated in `target/criterion/` with HTML charts.

### Performance Targets

Approximate targets for a 4-core server with SSD and ~1M datoms:

| Operation | Target p50 | Target p99 | Notes |
|-----------|-----------|-----------|-------|
| Health check | < 1ms | < 5ms | No database access |
| List databases | < 10ms | < 50ms | Simple pg_database query |
| Connect | < 10ms | < 50ms | Database existence check |
| Simple query (1-2 patterns) | < 50ms | < 200ms | Depends on result size |
| Complex query (5+ patterns) | < 500ms | < 2s | Depends on join complexity |
| Transaction (10 datoms) | < 50ms | < 200ms | Single PG function call |
| Transaction (100 datoms) | < 200ms | < 1s | Batch insert |
| Pull (single entity) | < 20ms | < 100ms | Index lookup |
| Cache hit | < 1ms | < 5ms | Mutex lock + LRU lookup |

If your measurements exceed these by more than 2x, check PostgreSQL configuration and index usage.

## Capacity Planning

### Disk Space

The `mentat.datoms` table stores all assertions and retractions. Since data is immutable (retractions add new rows with `added = false`), the table grows monotonically.

Rough sizing per datom row:
- Fixed overhead: ~60 bytes (e, a, tx, added, value_type_tag, tuple header)
- Value: varies by type (8 bytes for long/ref/double, variable for string/keyword)
- Index overhead: ~3x the table size (EAVT, AEVT, AVET indexes)

Estimate: **~250-300 bytes per datom** including indexes.

| Datom Count | Estimated Disk |
|-------------|---------------|
| 100K | ~30 MB |
| 1M | ~300 MB |
| 10M | ~3 GB |
| 100M | ~30 GB |
| 1B | ~300 GB |

Monitor actual usage:
```sql
SELECT
  pg_size_pretty(pg_total_relation_size('mentat.datoms')) AS total,
  pg_size_pretty(pg_relation_size('mentat.datoms')) AS table_only,
  pg_size_pretty(
    pg_total_relation_size('mentat.datoms') -
    pg_relation_size('mentat.datoms')
  ) AS indexes
;
```

### Memory

- **PostgreSQL `shared_buffers`**: 25% of system RAM
- **mentatd process**: ~50-200 MB base + cache memory
- **Cache memory**: approximately `capacity * avg_result_size_bytes`
  - For 1000 cached queries with 1 KB average result: ~1 MB
  - For 10000 cached queries with 10 KB average result: ~100 MB

### Connections

Each PostgreSQL connection uses ~5-10 MB of memory. Plan accordingly:

```
PostgreSQL memory = shared_buffers + (max_connections * 10 MB) + OS overhead
```

For a system with 16 GB RAM:
- `shared_buffers` = 4 GB
- `max_connections` = 100 -> 1 GB
- OS + filesystem cache -> ~11 GB
- Total: ~16 GB

## See Also

- [Monitoring Guide](./MONITORING.md) -- Metrics, alerts, and dashboards
- [Troubleshooting Guide](./TROUBLESHOOTING.md) -- Common issues and debugging
- [mentatd Configuration](../configuration/mentatd_config.md) -- Full configuration reference
- [Debugging Guide](../troubleshooting/DEBUGGING.md) -- Advanced debugging techniques
