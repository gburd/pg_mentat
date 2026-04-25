# pg_mentat Monitoring Guide

## Overview

pg_mentat exposes metrics in Prometheus text exposition format at the `/metrics` endpoint
on the mentatd server. PostgreSQL's built-in statistics views provide additional insight
into database-level performance.

## Endpoints

| Endpoint    | Method | Auth Required | Description                    |
|-------------|--------|---------------|--------------------------------|
| `/health`   | GET    | No            | Returns `mentatd ready` if up  |
| `/metrics`  | GET    | No            | Prometheus metrics             |

## mentatd Prometheus Metrics

### Request Metrics

| Metric                                    | Type      | Description                                |
|-------------------------------------------|-----------|--------------------------------------------|
| `mentatd_requests_total`                  | Counter   | Total HTTP requests received               |
| `mentatd_errors_total`                    | Counter   | Total errors                               |
| `mentatd_operations_total{operation}`     | Counter   | Operations by type (query, transact, pull, datoms) |
| `mentatd_operation_duration_seconds{operation}` | Histogram | Duration by operation type           |

### Query Metrics

| Metric                                    | Type      | Description                                |
|-------------------------------------------|-----------|--------------------------------------------|
| `mentatd_query_total`                     | Counter   | Total queries executed                     |
| `mentatd_query_duration_seconds`          | Histogram | Query execution duration                   |

Histogram buckets: 1ms, 5ms, 10ms, 25ms, 50ms, 100ms, 250ms, 500ms, 1s, 2.5s, 5s, 10s

### Transaction Metrics

| Metric                                    | Type      | Description                                |
|-------------------------------------------|-----------|--------------------------------------------|
| `mentatd_transactions_total`              | Counter   | Total transactions executed                |
| `mentatd_transaction_duration_seconds`    | Histogram | Transaction execution duration             |

### Cache Metrics

| Metric                                              | Type    | Description                                    |
|-----------------------------------------------------|---------|------------------------------------------------|
| `mentatd_cache_hits_total`                          | Counter | Total query cache hits                         |
| `mentatd_cache_misses_total`                        | Counter | Total query cache misses                       |
| `mentatd_cache_entries`                             | Gauge   | Current number of cached entries               |
| `mentatd_cache_hit_rate`                            | Gauge   | Cache hit rate (0.0 -- 1.0)                    |
| `mentatd_cache_targeted_invalidations_total`        | Counter | Entity-level invalidations                     |
| `mentatd_cache_full_invalidations_total`            | Counter | Full cache clears                              |
| `mentatd_cache_tracked_entries`                     | Gauge   | Entries with entity dependency tracking         |
| `mentatd_cache_avg_dependency_count`                | Gauge   | Average entity deps per tracked entry           |

### Connection Pool Metrics

| Metric                                    | Type    | Description                                  |
|-------------------------------------------|---------|----------------------------------------------|
| `mentatd_connection_pool_size`            | Gauge   | Current total connections in pool             |
| `mentatd_connection_pool_available`       | Gauge   | Idle connections available                    |
| `mentatd_connection_pool_waiting`         | Gauge   | Estimated tasks waiting for a connection      |

Pool metrics are updated every 5 seconds by a background task.

### Streaming Metrics

| Metric                                    | Type      | Description                                |
|-------------------------------------------|-----------|--------------------------------------------|
| `mentatd_stream_queries_total`            | Counter   | Total streaming queries                    |
| `mentatd_stream_rows_sent_total`          | Counter   | Total rows sent via streaming              |
| `mentatd_stream_duration_seconds`         | Histogram | Streaming query duration                   |

## PostgreSQL Metrics

Use these views for database-level insight:

### Connection Monitoring

```sql
-- Active connections by state
SELECT state, count(*)
FROM pg_stat_activity
WHERE datname = 'mentat'
GROUP BY state;

-- Long-running queries (> 5 seconds)
SELECT pid, now() - query_start AS duration, query
FROM pg_stat_activity
WHERE datname = 'mentat'
  AND state = 'active'
  AND now() - query_start > interval '5 seconds'
ORDER BY duration DESC;
```

### Query Performance (pg_stat_statements)

Enable `pg_stat_statements` in `postgresql.conf`:

```ini
shared_preload_libraries = 'pg_stat_statements'
pg_stat_statements.track = all
```

```sql
-- Slowest queries by mean execution time
SELECT query, calls, mean_exec_time, total_exec_time
FROM pg_stat_statements
WHERE dbid = (SELECT oid FROM pg_database WHERE datname = 'mentat')
ORDER BY mean_exec_time DESC
LIMIT 20;

-- Most frequently called queries
SELECT query, calls, mean_exec_time
FROM pg_stat_statements
WHERE dbid = (SELECT oid FROM pg_database WHERE datname = 'mentat')
ORDER BY calls DESC
LIMIT 20;
```

### Table Statistics

```sql
-- Table sizes and row counts
SELECT
    relname AS table_name,
    pg_size_pretty(pg_total_relation_size(c.oid)) AS total_size,
    pg_size_pretty(pg_relation_size(c.oid)) AS data_size,
    pg_size_pretty(pg_indexes_size(c.oid)) AS index_size,
    n_live_tup AS live_rows,
    n_dead_tup AS dead_rows,
    last_vacuum,
    last_autovacuum,
    last_analyze
FROM pg_stat_user_tables s
JOIN pg_class c ON s.relid = c.oid
WHERE schemaname = 'mentat'
ORDER BY pg_total_relation_size(c.oid) DESC;
```

### Index Usage

```sql
-- Index hit rate (should be > 99% in production)
SELECT
    indexrelname AS index_name,
    idx_scan AS scans,
    idx_tup_read AS tuples_read,
    idx_tup_fetch AS tuples_fetched,
    pg_size_pretty(pg_relation_size(indexrelid)) AS size
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
ORDER BY idx_scan DESC;

-- Unused indexes (candidates for removal)
SELECT indexrelname, idx_scan, pg_size_pretty(pg_relation_size(indexrelid))
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
  AND idx_scan = 0
ORDER BY pg_relation_size(indexrelid) DESC;
```

### Transaction Rate

```sql
-- Transaction rate (commits and rollbacks)
SELECT
    xact_commit AS commits,
    xact_rollback AS rollbacks,
    tup_inserted AS rows_inserted,
    tup_updated AS rows_updated,
    tup_deleted AS rows_deleted
FROM pg_stat_database
WHERE datname = 'mentat';
```

## Alerting Rules

### Prometheus Alert Examples

```yaml
groups:
  - name: pg_mentat
    rules:
      # High error rate
      - alert: MentatdHighErrorRate
        expr: rate(mentatd_errors_total[5m]) / rate(mentatd_requests_total[5m]) > 0.01
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "mentatd error rate above 1%"

      # High query latency (p95 > 100ms)
      - alert: MentatdHighQueryLatency
        expr: histogram_quantile(0.95, rate(mentatd_query_duration_seconds_bucket[5m])) > 0.1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "mentatd p95 query latency above 100ms"

      # Connection pool exhaustion
      - alert: MentatdPoolExhausted
        expr: mentatd_connection_pool_available < 2
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "mentatd connection pool nearly exhausted"

      # Low cache hit rate
      - alert: MentatdLowCacheHitRate
        expr: mentatd_cache_hit_rate < 0.5
        for: 15m
        labels:
          severity: warning
        annotations:
          summary: "mentatd cache hit rate below 50%"

      # mentatd down
      - alert: MentatdDown
        expr: up{job="mentatd"} == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "mentatd instance is down"

      # High transaction latency
      - alert: MentatdHighTransactionLatency
        expr: histogram_quantile(0.95, rate(mentatd_transaction_duration_seconds_bucket[5m])) > 0.5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "mentatd p95 transaction latency above 500ms"
```

### Prometheus Scrape Config

```yaml
scrape_configs:
  - job_name: mentatd
    static_configs:
      - targets:
          - mentatd-host:8080
    metrics_path: /metrics
    scrape_interval: 15s
```

## Grafana Dashboard

### Key Panels

1. **Request Rate** -- `rate(mentatd_requests_total[5m])`
2. **Error Rate** -- `rate(mentatd_errors_total[5m]) / rate(mentatd_requests_total[5m])`
3. **Query Latency (p50/p95/p99)** -- `histogram_quantile(0.95, rate(mentatd_query_duration_seconds_bucket[5m]))`
4. **Transaction Latency** -- `histogram_quantile(0.95, rate(mentatd_transaction_duration_seconds_bucket[5m]))`
5. **Cache Hit Rate** -- `mentatd_cache_hit_rate`
6. **Cache Size** -- `mentatd_cache_entries`
7. **Connection Pool** -- `mentatd_connection_pool_size`, `mentatd_connection_pool_available`, `mentatd_connection_pool_waiting`
8. **Operations by Type** -- `rate(mentatd_operations_total[5m])` grouped by `operation`
9. **Streaming Queries** -- `rate(mentatd_stream_queries_total[5m])`

### Recommended Thresholds

| Metric                       | Warning         | Critical        |
|------------------------------|-----------------|-----------------|
| Error rate                   | > 1%            | > 5%            |
| Query p95 latency            | > 100ms         | > 500ms         |
| Transaction p95 latency      | > 500ms         | > 2s            |
| Cache hit rate               | < 50%           | < 20%           |
| Pool available connections   | < 10%           | < 2 connections |
| PostgreSQL dead tuples ratio | > 10%           | > 20%           |

## Log Analysis

mentatd supports three log formats configured via `LOG_FORMAT` or `logging.format`:

- **compact** (default) -- Single-line logs, human-readable
- **json** -- Structured JSON logs (recommended for production)
- **pretty** -- Multi-line formatted logs (for development)

### Key Log Patterns to Watch

```bash
# Errors (JSON format)
jq 'select(.level == "ERROR")' /var/log/mentatd.log

# Slow database queries (look for pool timeout warnings)
jq 'select(.message | contains("timeout"))' /var/log/mentatd.log

# Connection failures
jq 'select(.message | contains("Failed to get database connection"))' /var/log/mentatd.log
```
