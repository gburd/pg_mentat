# Capacity Planning Guide

This guide provides sizing guidelines, storage estimates, throughput calculations, and scaling strategies for pg_mentat deployments.

---

## Table of Contents

1. [Data Model Size Fundamentals](#data-model-size-fundamentals)
2. [Storage Estimates](#storage-estimates)
3. [Memory Sizing](#memory-sizing)
4. [Throughput Calculations](#throughput-calculations)
5. [Connection Sizing](#connection-sizing)
6. [Scaling Strategies](#scaling-strategies)
7. [Hardware Recommendations](#hardware-recommendations)
8. [Growth Projections](#growth-projections)
9. [Sizing Worksheets](#sizing-worksheets)

---

## Data Model Size Fundamentals

### What Is a Datom?

Every fact in pg_mentat is stored as a datom: a tuple of (Entity, Attribute, Value, Transaction, Added). Each datom corresponds to one row in a type-specific table.

**Example:** Storing a person with 5 attributes creates 5 datoms:

```edn
{:person/name "Alice"
 :person/email "alice@example.com"
 :person/age 30
 :person/active true
 :person/manager 10042}
```

This produces:
- 1 datom in `datoms_text_new` (name)
- 1 datom in `datoms_text_new` (email)
- 1 datom in `datoms_long_new` (age)
- 1 datom in `datoms_boolean_new` (active)
- 1 datom in `datoms_ref_new` (manager)

### Datom Count Formula

```
total_datoms = num_entities * avg_attributes_per_entity * history_factor
```

Where:
- `num_entities` = number of distinct entities
- `avg_attributes_per_entity` = average attributes per entity (typically 5-20)
- `history_factor` = 1.0 for no updates, higher for frequently updated data
  - Infrequent updates: 1.1-1.2
  - Moderate updates: 1.5-2.0
  - Frequent updates: 2.0-5.0 (each update adds a retraction + assertion = 2 datoms)

### Cardinality-Many Multiplier

Attributes with `:db.cardinality/many` store one datom per value. If a person has 10 friends, that is 10 datoms for `:person/friends`:

```
total_datoms += entities_with_many * avg_many_values_per_entity
```

---

## Storage Estimates

### Per-Datom Storage Cost

With the Phase 3 type-specific table design:

| Value Type | Row Size (data) | Index Overhead | Total Per Datom |
|------------|-----------------|----------------|-----------------|
| ref        | ~40 bytes       | ~120 bytes     | ~160 bytes      |
| long       | ~40 bytes       | ~100 bytes     | ~140 bytes      |
| double     | ~40 bytes       | ~100 bytes     | ~140 bytes      |
| boolean    | ~33 bytes       | ~100 bytes     | ~133 bytes      |
| instant    | ~40 bytes       | ~100 bytes     | ~140 bytes      |
| keyword    | ~60 bytes avg   | ~120 bytes     | ~180 bytes      |
| uuid       | ~48 bytes       | ~110 bytes     | ~158 bytes      |
| text       | ~80+ bytes avg  | ~130 bytes     | ~210+ bytes     |
| bytes      | variable        | ~130 bytes     | variable        |

**Average across typical workloads: ~170 bytes per datom (data + indexes)**

### Storage Sizing Table

| Entities | Attrs/Entity | Datoms | Data Size | Index Size | Total Size |
|----------|-------------|--------|-----------|------------|------------|
| 10K      | 10          | 100K   | ~10 MB    | ~15 MB     | ~25 MB     |
| 100K     | 10          | 1M     | ~100 MB   | ~150 MB    | ~250 MB    |
| 1M       | 10          | 10M    | ~1 GB     | ~1.5 GB    | ~2.5 GB    |
| 5M       | 10          | 50M    | ~5 GB     | ~7.5 GB    | ~12.5 GB   |
| 10M      | 10          | 100M   | ~10 GB    | ~15 GB     | ~25 GB     |
| 50M      | 10          | 500M   | ~50 GB    | ~75 GB     | ~125 GB    |
| 100M     | 10          | 1B     | ~100 GB   | ~150 GB    | ~250 GB    |

### Additional Storage

Beyond datom tables, account for:

| Component | Typical Size | Notes |
|-----------|-------------|-------|
| `schema` table | < 1 MB | Schema attributes |
| `transactions` table | ~50 bytes/tx | Transaction metadata |
| `idents` table | < 1 MB | Keyword cache |
| `fulltext` table | Variable | Only if full-text attributes exist |
| `stores` metadata | < 1 MB | Store registry |
| WAL (write-ahead log) | 1-4 GB | Configurable via `max_wal_size` |
| Temporary files | 0-2 GB | Sort spills; configurable via `temp_file_limit` |

### Storage Formula

```
total_storage = datom_storage + wal_size + temp_files + overhead

Where:
  datom_storage = total_datoms * 170 bytes (average)
  wal_size = max_wal_size setting (default 2 GB)
  temp_files = work_mem overflow during complex queries
  overhead = ~20% for filesystem, TOAST, dead tuples
```

**Quick estimate:**

```
total_disk_needed = total_datoms * 200 bytes * 1.3 (safety margin)
```

---

## Memory Sizing

### PostgreSQL Memory

```
total_pg_memory = shared_buffers + (max_connections * per_connection_mem)

Where:
  shared_buffers = 25% of system RAM (dedicated server)
                 = 10% of system RAM (shared server)
  per_connection_mem = work_mem * avg_sorts_per_query + overhead
                    ≈ 64 MB * 2 + 10 MB = ~138 MB worst case
```

**Memory sizing table:**

| System RAM | shared_buffers | max_connections | work_mem | maintenance_work_mem |
|------------|---------------|-----------------|----------|---------------------|
| 4 GB       | 1 GB          | 50              | 32 MB    | 256 MB              |
| 8 GB       | 2 GB          | 100             | 64 MB    | 512 MB              |
| 16 GB      | 4 GB          | 200             | 64 MB    | 1 GB                |
| 32 GB      | 8 GB          | 300             | 128 MB   | 2 GB                |
| 64 GB      | 16 GB         | 500             | 256 MB   | 4 GB                |

### mentatd Memory

```
mentatd_memory = base + pool_memory + cache_memory

Where:
  base = ~100 MB (Tokio runtime, HTTP server)
  pool_memory = pool_size * ~10 MB
  cache_memory = cache_capacity * avg_result_size
```

| pool_size | cache_capacity | avg_result_size | Estimated mentatd Memory |
|-----------|----------------|-----------------|--------------------------|
| 20        | 1,000          | 5 KB            | 100 + 200 + 5 = ~305 MB  |
| 50        | 5,000          | 5 KB            | 100 + 500 + 25 = ~625 MB |
| 100       | 10,000         | 10 KB           | 100 + 1000 + 100 = ~1.2 GB |

### Buffer Pool Hit Rate Target

For good performance, the PostgreSQL buffer pool should hold the **working set** -- the portion of data frequently accessed. Aim for a buffer pool hit rate > 99%:

```sql
SELECT
  sum(heap_blks_hit) * 100.0 / NULLIF(sum(heap_blks_hit) + sum(heap_blks_read), 0) AS hit_rate
FROM pg_statio_user_tables
WHERE schemaname = 'mentat';
-- Target: > 99%
```

If the hit rate is below 95%, increase `shared_buffers` or add RAM.

**Rule of thumb:** `shared_buffers` should be at least 2x the "hot" data size (frequently queried entities and their indexes).

---

## Throughput Calculations

### Read Throughput

| Query Type | Expected Latency (p95) | Single-Core Throughput | Notes |
|------------|----------------------|----------------------|-------|
| Entity lookup (EAVT) | 2-10 ms | 100-500 QPS | Index scan |
| Attribute scan (AEVT) | 5-30 ms | 30-200 QPS | Depends on selectivity |
| Two-pattern join | 10-50 ms | 20-100 QPS | |
| Complex query (5+ patterns) | 50-500 ms | 2-20 QPS | |
| Pull (single entity) | 5-20 ms | 50-200 QPS | |
| Aggregate query | 20-200 ms | 5-50 QPS | |
| Recursive rule | 50-2000 ms | 0.5-20 QPS | Depends on graph depth |

**Multi-core scaling:** PostgreSQL can run queries in parallel across cores. With 4 cores, expect ~3x throughput for parallelizable queries.

**Formula:**

```
max_read_qps = num_cores * single_core_qps * parallelism_efficiency

Where:
  parallelism_efficiency = 0.7-0.8 (accounting for contention)
```

### Write Throughput

| Operation | Latency (p95) | Throughput | Notes |
|-----------|--------------|------------|-------|
| Single assertion | 5-15 ms | 65-200 TPS | |
| 10-datom transaction | 10-50 ms | 20-100 TPS | |
| 100-datom batch | 50-200 ms | 5-20 TPS | |
| 1000-datom batch | 200-1000 ms | 1-5 TPS | |
| Schema change | 20-100 ms | N/A | Infrequent |

**Datom write rate:**

```
datoms_per_second = batch_size * transactions_per_second

Example:
  batch_size = 100 datoms
  tps = 20
  datoms/sec = 2,000
```

**Target (with Phase 3 type-specific tables):** 8,000-10,000 datoms/sec sustained.

### Mixed Workload Sizing

For a mixed read/write workload:

```
cpu_utilization = (read_qps / max_read_qps) + (write_tps / max_write_tps)
```

Keep `cpu_utilization < 0.7` for headroom.

**Example:**

- Target: 200 read QPS + 50 write TPS
- Single-core capacity: 100 read QPS + 20 write TPS
- Required cores: (200/100) + (50/20) = 4.5 -> 6 cores (with headroom)

---

## Connection Sizing

### Formula

```
required_connections = peak_concurrent_requests * avg_request_hold_time / 1000 + buffer

Where:
  peak_concurrent_requests = peak_rps
  avg_request_hold_time = query_time_ms + network_overhead_ms
  buffer = 20% headroom
```

**Example:**

- Peak: 500 RPS
- Avg query time: 20 ms
- Hold time: 25 ms (with overhead)
- Connections needed: 500 * 0.025 = 12.5
- With buffer: 15 connections

In practice, set the pool higher to handle burst traffic:

| Peak RPS | Avg Query Time | Recommended Pool Size |
|----------|---------------|-----------------------|
| 50       | 20 ms         | 10-20                 |
| 200      | 20 ms         | 20-40                 |
| 500      | 20 ms         | 30-60                 |
| 1000     | 20 ms         | 50-100                |
| 5000     | 20 ms         | 200+                  |

### PostgreSQL max_connections

```
max_connections = sum(all_pool_sizes) + superuser_reserve + monitoring_connections

Where:
  superuser_reserve = 5
  monitoring_connections = 2-5
```

---

## Scaling Strategies

### Vertical Scaling (Scale Up)

The simplest approach. Increase resources on a single server.

| Bottleneck | Symptom | Solution |
|-----------|---------|---------|
| CPU | High CPU utilization during queries | More cores, faster CPUs |
| Memory | Low buffer hit rate, swap usage | More RAM, increase shared_buffers |
| Disk I/O | High `iowait`, slow queries with `Buffers: shared read` | NVMe SSD, RAID, more IOPS |
| Connections | Pool exhaustion | Increase max_connections, add PgBouncer |

**Practical limits:** A single PostgreSQL instance can handle:
- ~10M datoms with < 100ms query latency
- ~100M datoms with appropriate tuning
- Beyond 100M datoms, consider horizontal scaling

### Read Replicas (Scale Reads)

For read-heavy workloads, add PostgreSQL streaming replicas:

```
Write Path:  App -> Primary PostgreSQL
Read Path:   App -> Read Replica(s) -> PostgreSQL replicas
```

- pg_mentat extension must be installed on replicas
- Queries are read-only on replicas (no transactions)
- Replication lag is typically < 1 second

**Setup:**

```ini
# Primary
wal_level = replica
max_wal_senders = 5

# Replica
primary_conninfo = 'host=primary user=replication_user'
hot_standby = on
```

### Connection Pooling (PgBouncer)

For connection-heavy workloads, deploy PgBouncer between mentatd and PostgreSQL:

```
mentatd -> PgBouncer -> PostgreSQL
```

PgBouncer can multiplex many client connections onto fewer PostgreSQL connections:

```ini
# pgbouncer.ini
[databases]
mentat = host=localhost port=5432 dbname=mentat

[pgbouncer]
pool_mode = transaction
max_client_conn = 1000
default_pool_size = 50
```

### Multi-Store Partitioning

pg_mentat supports multiple isolated stores, each in its own PostgreSQL schema:

```sql
SELECT mentat_create_store('analytics', 'Analytics data');
SELECT mentat_create_store('users', 'User data');
```

This provides:
- Logical isolation between datasets
- Independent schema caches per store
- Per-store backup and maintenance

### Table Partitioning (Future)

For datasets beyond 100M datoms, PostgreSQL native partitioning can improve query performance:

```sql
-- Future: Partition by store_id
CREATE TABLE datoms_ref (
    store_id INT NOT NULL,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BIGINT NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL
) PARTITION BY HASH (store_id);
```

This enables:
- Partition pruning at query time
- Parallel scans across partitions
- Per-partition VACUUM and maintenance

---

## Hardware Recommendations

### Small Deployment (< 1M datoms, < 50 QPS)

- **Server**: 4 cores, 8 GB RAM, 50 GB NVMe SSD
- **PostgreSQL**: Single instance
- **mentatd**: Optional, single instance
- **Estimated cost**: ~$50/month (cloud VM)

### Medium Deployment (1M-10M datoms, 50-500 QPS)

- **Server**: 8 cores, 32 GB RAM, 200 GB NVMe SSD
- **PostgreSQL**: Single instance with streaming replica
- **mentatd**: 2 instances behind load balancer (if used)
- **Estimated cost**: ~$200-400/month (cloud VM)

### Large Deployment (10M-100M datoms, 500+ QPS)

- **Server**: 16+ cores, 64+ GB RAM, 1 TB NVMe SSD
- **PostgreSQL**: Primary + 2 read replicas
- **mentatd**: 2-4 instances with autoscaling (if used)
- **PgBouncer**: For connection multiplexing
- **Estimated cost**: ~$800-2000/month (cloud VM)

### Storage IOPS Requirements

| Dataset Size | Estimated IOPS (read-heavy) | Estimated IOPS (write-heavy) |
|-------------|---------------------------|------------------------------|
| 1M datoms   | 500-1,000                 | 1,000-3,000                  |
| 10M datoms  | 2,000-5,000               | 5,000-10,000                 |
| 100M datoms | 10,000-30,000             | 20,000-50,000                |

Provision NVMe SSDs that can deliver the required IOPS with low latency (< 1ms p99).

---

## Growth Projections

### Estimating Future Storage

```
storage_in_n_months = current_storage + (monthly_growth * n)

Where:
  monthly_growth = new_datoms_per_month * 200 bytes
  new_datoms_per_month = new_entities * attrs_per_entity
                       + updated_entities * 2 (retract + assert per update)
```

**Example:**

- Current: 5M datoms (1.25 GB)
- New entities/month: 100K at 10 attrs = 1M new datoms
- Updates/month: 200K entities, 2 attrs each = 800K new datoms (retract + assert)
- Monthly growth: 1.8M datoms * 200 bytes = 360 MB/month
- 1-year projection: 1.25 GB + (360 MB * 12) = ~5.6 GB

### Monitoring Growth

```sql
-- Datom count by table
SELECT
    'ref' AS type, count(*) FROM mentat.datoms_ref_new
UNION ALL SELECT
    'long', count(*) FROM mentat.datoms_long_new
UNION ALL SELECT
    'text', count(*) FROM mentat.datoms_text_new
UNION ALL SELECT
    'boolean', count(*) FROM mentat.datoms_boolean_new
UNION ALL SELECT
    'double', count(*) FROM mentat.datoms_double_new
UNION ALL SELECT
    'instant', count(*) FROM mentat.datoms_instant_new
UNION ALL SELECT
    'keyword', count(*) FROM mentat.datoms_keyword_new
UNION ALL SELECT
    'uuid', count(*) FROM mentat.datoms_uuid_new
UNION ALL SELECT
    'bytes', count(*) FROM mentat.datoms_bytes_new;

-- Transaction rate (growth velocity)
SELECT
    date_trunc('day', tx_instant) AS day,
    count(*) AS transactions
FROM mentat.transactions
GROUP BY 1
ORDER BY 1 DESC
LIMIT 30;
```

### Capacity Alerts

Set alerts for:

| Metric | Warning | Critical |
|--------|---------|----------|
| Disk usage | 70% | 85% |
| Datom count | 80% of tested threshold | 90% |
| Connection pool utilization | 80% | 95% |
| Query p95 latency | 2x baseline | 5x baseline |

---

## Sizing Worksheets

### Worksheet 1: Storage

Fill in your values:

```
Entities:               ___________
Avg attributes/entity:  ___________
Cardinality-many values: ___________
History factor:         ___________ (1.0 = no updates, 2.0 = every entity updated once)

Total datoms = (entities * attrs) + many_values * history_factor
             = ___________ datoms

Storage = total_datoms * 200 bytes * 1.3 (margin)
        = ___________ bytes
        = ___________ GB
```

### Worksheet 2: Memory

```
System RAM:             ___________ GB

shared_buffers = RAM * 0.25 = ___________ GB
effective_cache_size = RAM * 0.75 = ___________ GB
work_mem = 64 MB (default, adjust based on EXPLAIN)
max_connections = ___________

Memory check:
  shared_buffers + (max_connections * 138 MB) < system RAM * 0.9
  ___________ + ___________ = ___________ < ___________
```

### Worksheet 3: Throughput

```
Peak read QPS:          ___________
Peak write TPS:         ___________
Avg query latency:      ___________ ms

Required CPU cores:
  read_cores = read_qps / (single_core_qps * 0.75)
             = ___________ / ___________ = ___________
  write_cores = write_tps / (single_core_tps * 0.75)
              = ___________ / ___________ = ___________
  total_cores = read_cores + write_cores = ___________
```

### Worksheet 4: Connections

```
Peak concurrent requests: ___________
Avg request duration:     ___________ ms

Pool size = peak_requests * (duration_ms / 1000) * 1.2
          = ___________ * ___________ * 1.2
          = ___________

max_connections = pool_size * num_mentatd_instances + 10
               = ___________ * ___________ + 10
               = ___________
```

---

## See Also

- [PRODUCTION_DEPLOYMENT.md](PRODUCTION_DEPLOYMENT.md) -- Installation, tuning, backup, monitoring
- [TROUBLESHOOTING.md](TROUBLESHOOTING.md) -- Common issues and debugging
- [EXPERT_REVIEW.md](EXPERT_REVIEW.md) -- Detailed architecture review with scaling analysis
- [STORAGE_REDESIGN_PLAN.md](STORAGE_REDESIGN_PLAN.md) -- Phase 3 type-specific table design
