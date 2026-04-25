# pg_mentat Performance Tuning Guide

## Architecture

pg_mentat stores data as datoms (entity-attribute-value-transaction tuples) in a single
PostgreSQL table with type-specific value columns. Performance depends on:

- **Index selection**: Correct index usage for each query pattern
- **Connection pooling**: Efficient connection reuse in mentatd
- **Query caching**: Avoiding redundant query execution
- **PostgreSQL tuning**: Buffer pool, work memory, vacuum settings

## Index Strategy

The datoms table uses partial, type-specific indexes based on the Datomic EAVT/AEVT/VAET/AVET
index model.

### Core Indexes

| Index Name              | Columns                       | Filter                           | Use Case                    |
|-------------------------|-------------------------------|----------------------------------|-----------------------------|
| `idx_datoms_eavt`       | `(e, a, value_type_tag, tx)`  | `added = TRUE`                   | Entity lookups              |
| `idx_datoms_aevt`       | `(a, e, value_type_tag, tx)`  | `added = TRUE`                   | Attribute scans             |
| `idx_datoms_vaet`       | `(v_ref, a, e, tx)`           | `added = TRUE AND type_tag = 0`  | Reverse reference lookups   |
| `idx_datoms_tx`         | `(tx DESC)`                   | None                             | Transaction queries         |

### Type-Specific AVET Indexes

These indexes enable correct native-type range queries and value lookups:

| Index                      | Type Column  | Type Tag | Value Types       |
|----------------------------|-------------|----------|-------------------|
| `idx_datoms_avet_ref`      | `v_ref`     | 0        | Entity references |
| `idx_datoms_avet_long`     | `v_long`    | 2        | Integers          |
| `idx_datoms_avet_double`   | `v_double`  | 3        | Floats            |
| `idx_datoms_avet_instant`  | `v_instant` | 4        | Timestamps        |
| `idx_datoms_avet_text`     | `v_text`    | 7        | Strings           |
| `idx_datoms_avet_keyword`  | `v_keyword` | 8        | Keywords          |
| `idx_datoms_avet_uuid`     | `v_uuid`    | 10       | UUIDs             |

### Verifying Index Usage

```sql
-- Check if a query uses the expected index
EXPLAIN (ANALYZE, BUFFERS) SELECT e, v_text FROM mentat.datoms
WHERE a = 10 AND v_keyword = ':db/ident' AND added = TRUE;

-- Should show: Index Scan using idx_datoms_avet_keyword
```

If the planner chooses a sequential scan, check:
1. Statistics are up to date: `ANALYZE mentat.datoms;`
2. `random_page_cost` is appropriate (lower for SSDs: `1.1`)
3. The query predicate matches the index filter conditions

## Query Cache Tuning

mentatd includes an LRU query cache with entity-level dependency tracking.

### How It Works

- Query results are cached by `(query_string, args_json)` key.
- Entries with entity dependency tracking survive unrelated write transactions.
- Entries without tracking (untracked) are invalidated on every transaction.
- The cache uses LRU eviction when at capacity.
- Entries expire after the configured TTL.

### Configuration

| Parameter                 | Default | Recommendation                              |
|---------------------------|---------|---------------------------------------------|
| `MENTATD_CACHE_ENABLED`  | `true`  | Enable unless write-heavy workload          |
| `MENTATD_CACHE_CAPACITY` | `1000`  | Increase for read-heavy workloads (5000+)   |
| `MENTATD_CACHE_TTL`      | `300`   | Lower for frequently changing data (60-120) |

### Monitoring Cache Effectiveness

```bash
# Check hit rate
curl -s http://localhost:8080/metrics | grep mentatd_cache_hit_rate
# Target: > 0.6 for read-heavy workloads

# Check invalidation patterns
curl -s http://localhost:8080/metrics | grep mentatd_cache.*invalidations
# High full_invalidations suggests writes are clearing the whole cache
# High targeted_invalidations with stable entries count is healthy
```

### Improving Hit Rate

1. **Increase capacity** if the cache fills up frequently (entries == capacity).
2. **Increase TTL** if entries expire before reuse.
3. **Reduce write frequency** -- batch writes together to reduce invalidation events.
4. **Use entity-specific queries** -- queries that can be tracked to specific entities
   survive unrelated writes.

## Connection Pool Tuning

### Pool Configuration

| Parameter                | Default | Description                                  |
|--------------------------|---------|----------------------------------------------|
| `DATABASE_POOL_SIZE`     | `100`   | Maximum connections in the pool               |
| `DATABASE_MAX_LIFETIME`  | `1800`  | Connection max age in seconds (30 min)        |

The pool uses `deadpool-postgres` with:
- 30-second wait timeout for connection acquisition
- TCP keepalives every 60 seconds
- Pool metrics updated every 5 seconds

### Sizing Guidelines

```
pool_size = max_concurrent_queries + buffer

Where:
  max_concurrent_queries = peak_requests_per_second * avg_query_duration_seconds
  buffer = 10-20% headroom
```

Example: 200 RPS with 10ms average query time = 2 concurrent + 20% = ~5 connections
(but in practice, set higher to handle burst traffic and slow queries).

**Important**: Ensure PostgreSQL `max_connections` > sum of all pool sizes + superuser reserve:

```sql
SHOW max_connections;
-- Must exceed: (mentatd_pool_size * num_mentatd_instances) + 5 (superuser reserve)
```

## PostgreSQL Tuning

### Memory Settings

```ini
# shared_buffers: 25% of total RAM (dedicated server) or 10% (shared)
shared_buffers = '2GB'

# effective_cache_size: 75% of total RAM (tells planner about OS page cache)
effective_cache_size = '6GB'

# work_mem: Memory per sort/hash operation
# Start at 64MB; increase if EXPLAIN shows disk sorts
work_mem = '64MB'

# maintenance_work_mem: Memory for VACUUM, CREATE INDEX
maintenance_work_mem = '512MB'
```

### Write Performance

```ini
# WAL settings for write-heavy workloads
wal_buffers = '64MB'
max_wal_size = '4GB'
min_wal_size = '1GB'
checkpoint_completion_target = 0.9
checkpoint_timeout = '15min'

# Disable fsync ONLY in non-production environments for speed
# fsync = off  # NEVER in production
```

### Disk I/O

```ini
# For SSDs (most modern deployments)
random_page_cost = 1.1          # Default is 4.0, too high for SSDs
effective_io_concurrency = 200  # Default is 1, increase for SSDs
```

### Autovacuum

The datoms table is already configured with aggressive autovacuum:

```sql
-- Current settings
SELECT relname, reloptions
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'mentat' AND c.relname = 'datoms';
-- Should show: autovacuum_vacuum_scale_factor=0.05, autovacuum_analyze_scale_factor=0.02
```

For very high write rates (> 1000 TPS), further tune:

```sql
ALTER TABLE mentat.datoms SET (
    autovacuum_vacuum_scale_factor = 0.01,   -- Vacuum at 1% dead tuples
    autovacuum_vacuum_cost_delay = 2,        -- Less delay between vacuum pages
    autovacuum_vacuum_cost_limit = 1000      -- Higher budget per cycle
);
```

### Parallelism

```ini
# Enable parallel query for large scans
max_parallel_workers_per_gather = 4
max_parallel_workers = 8
parallel_tuple_cost = 0.001
parallel_setup_cost = 100
min_parallel_table_scan_size = '8MB'
```

## Write Optimization

### Batch Transactions

Group multiple assertions into a single transaction to reduce overhead:

```edn
;; Instead of separate transactions:
;; {:op :transact :data "[[:db/add 1 :name \"Alice\"]]"}
;; {:op :transact :data "[[:db/add 2 :name \"Bob\"]]"}

;; Use a single batch:
{:op :transact
 :data "[[:db/add 1 :name \"Alice\"]
         [:db/add 2 :name \"Bob\"]]"}
```

### Entity ID Allocation

pg_mentat uses PostgreSQL sequences with `CACHE 100` for lock-free entity ID allocation.
Each backend connection pre-allocates 100 IDs, eliminating row-level lock contention.
This is a significant improvement over the original SQLite-based `UPDATE ... SET next_entid`
pattern.

### Bulk Imports

For initial data loading:

```sql
-- Temporarily disable indexes during bulk load
-- (recreate afterward)
BEGIN;
SET LOCAL synchronous_commit = off;

-- Insert datoms in large batches
INSERT INTO mentat.datoms (e, a, value_type_tag, v_text, tx, added) VALUES
  (10001, 10, 8, ':user/name', 1000002, true),
  (10001, 19, 7, 'Alice Smith', 1000002, true),
  -- ... thousands more rows ...
;

COMMIT;

-- Rebuild statistics
ANALYZE mentat.datoms;
```

## Benchmarking

### Query Latency

```sql
-- Measure EAVT lookup performance
EXPLAIN (ANALYZE, BUFFERS, TIMING)
SELECT a, v_text, v_long, v_ref, value_type_tag
FROM mentat.datoms
WHERE e = 10001 AND added = TRUE;

-- Measure AVET lookup performance
EXPLAIN (ANALYZE, BUFFERS, TIMING)
SELECT e FROM mentat.datoms
WHERE a = 10 AND v_keyword = ':user/name' AND added = TRUE;
```

### mentatd Load Testing

```bash
# Use wrk, hey, or k6 for HTTP load testing
# Example with hey:
hey -n 10000 -c 50 -m POST \
  -H "Content-Type: application/edn" \
  -d '{:op :query :query "[:find ?e :where [?e :db/ident]]"}' \
  http://localhost:8080/
```

### Performance Targets

| Operation            | Target (p95) | Notes                          |
|----------------------|--------------|--------------------------------|
| Simple entity lookup | < 5ms        | EAVT index scan                |
| Attribute value scan | < 10ms       | AVET index scan                |
| Datalog query        | < 50ms       | Depends on complexity          |
| Transaction (small)  | < 20ms       | 1-10 assertions                |
| Transaction (batch)  | < 200ms      | 100-1000 assertions            |
| Pull API             | < 50ms       | Single entity with attributes  |

## Capacity Planning

### Storage Growth

```
Row size estimate:
  - Fixed columns (e, a, value_type_tag, tx, added): ~33 bytes
  - Value column: 8-100+ bytes depending on type
  - Index overhead: ~2x data size (with all indexes)
  - Dead tuple overhead: ~5-20% depending on write rate

Total per datom: ~200-400 bytes (data + indexes)
```

### Sizing Table

| Entities | Avg Attrs/Entity | Datoms    | Data Size | With Indexes |
|----------|------------------|-----------|-----------|--------------|
| 100K     | 10               | 1M        | ~200 MB   | ~600 MB      |
| 1M       | 10               | 10M       | ~2 GB     | ~6 GB        |
| 10M      | 10               | 100M      | ~20 GB    | ~60 GB       |
| 100M     | 10               | 1B        | ~200 GB   | ~600 GB      |

For datasets beyond 100M datoms, consider table partitioning (see Task #7).
