# Monitoring Guide

Practical guide for monitoring pg_mentat and mentatd in production.

## Metrics Overview

mentatd exposes Prometheus-format metrics at the `/metrics` endpoint. These are registered in `mentatd/src/metrics.rs` and updated throughout the request lifecycle.

### Available Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `mentatd_requests_total` | Counter | Total HTTP requests received |
| `mentatd_errors_total` | Counter | Total errors (parse, database, internal) |
| `mentatd_query_total` | Counter | Total Datalog queries executed |
| `mentatd_query_duration_seconds` | Histogram | Query execution duration (includes cache misses only) |
| `mentatd_cache_hits_total` | Counter | Query cache hits |
| `mentatd_cache_misses_total` | Counter | Query cache misses |
| `mentatd_transactions_total` | Counter | Total transactions executed |
| `mentatd_connection_pool_size` | Gauge | Current connections in the pool |
| `mentatd_stream_queries_total` | Counter | Total streaming queries |
| `mentatd_stream_rows_sent_total` | Counter | Total rows sent via streaming |
| `mentatd_stream_duration_seconds` | Histogram | Streaming query duration |

### Histogram Buckets

Query duration histogram uses these buckets (in seconds):
```
0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0
```

Streaming duration histogram uses:
```
0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0
```

### Accessing Metrics

```bash
# Raw Prometheus text format
curl -s http://localhost:8080/metrics

# Example output:
# HELP mentatd_query_total Total number of queries executed
# TYPE mentatd_query_total counter
# mentatd_query_total 1542
# HELP mentatd_query_duration_seconds Query execution duration in seconds
# TYPE mentatd_query_duration_seconds histogram
# mentatd_query_duration_seconds_bucket{le="0.001"} 12
# mentatd_query_duration_seconds_bucket{le="0.005"} 89
# ...
```

## Key Metrics to Monitor

### 1. Error Rate

The most important metric. A rising error rate indicates database connectivity issues, malformed requests, or resource exhaustion.

```
error_rate = rate(mentatd_errors_total[5m]) / rate(mentatd_requests_total[5m])
```

**Healthy:** < 1%
**Warning:** 1-5%
**Critical:** > 5%

### 2. Cache Hit Ratio

Indicates how effectively the query cache is working.

```
cache_hit_ratio = rate(mentatd_cache_hits_total[5m]) / (rate(mentatd_cache_hits_total[5m]) + rate(mentatd_cache_misses_total[5m]))
```

**Healthy (read-heavy):** > 80%
**Healthy (mixed):** > 50%
**Investigate if:** < 30% -- either queries have high cardinality or writes invalidate the cache frequently

### 3. Query Latency (p50 / p95 / p99)

Track latency percentiles from the histogram.

```
histogram_quantile(0.5, rate(mentatd_query_duration_seconds_bucket[5m]))
histogram_quantile(0.95, rate(mentatd_query_duration_seconds_bucket[5m]))
histogram_quantile(0.99, rate(mentatd_query_duration_seconds_bucket[5m]))
```

**Healthy p50:** < 50ms
**Healthy p99:** < 500ms
**Investigate if:** p99 > 2s or p99/p50 ratio > 20x (indicates outlier queries)

### 4. Connection Pool Saturation

If the pool gauge equals the configured `pool_size` and latency is rising, the pool is saturated.

```
pool_utilization = mentatd_connection_pool_size / <configured_pool_size>
```

**Healthy:** < 70%
**Warning:** 70-90%
**Critical:** > 90%

### 5. Request Rate

Monitor for unexpected traffic spikes or drops.

```
request_rate = rate(mentatd_requests_total[5m])
```

### 6. Transaction Rate

Track write throughput. Each transaction invalidates the query cache.

```
tx_rate = rate(mentatd_transactions_total[5m])
```

If transaction rate is high and cache hit ratio is low, cache tuning may not help -- the workload is write-dominated.

## Prometheus Configuration

### Scrape Configuration

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'mentatd'
    scrape_interval: 15s
    static_configs:
      - targets: ['mentatd-host:8080']
        labels:
          environment: 'production'
          service: 'mentatd'
    metrics_path: '/metrics'

  - job_name: 'postgresql'
    scrape_interval: 15s
    static_configs:
      - targets: ['postgres-exporter:9187']
        labels:
          environment: 'production'
          service: 'postgresql'
```

### PostgreSQL Exporter

Install [postgres_exporter](https://github.com/prometheus-community/postgres_exporter) to get PostgreSQL-level metrics:

```bash
# Run postgres_exporter
export DATA_SOURCE_NAME="postgresql://mentat@localhost/mentat?sslmode=disable"
./postgres_exporter --web.listen-address=:9187
```

Key PostgreSQL metrics to collect:
- `pg_stat_activity` -- Active connections, query states
- `pg_stat_user_tables` -- Table scan counts, row estimates
- `pg_stat_user_indexes` -- Index usage statistics
- `pg_stat_statements` -- Query-level performance (requires extension)

## Prometheus Alert Rules

```yaml
# mentatd_alerts.yml
groups:
  - name: mentatd
    rules:

      # High error rate
      - alert: MentatdHighErrorRate
        expr: |
          (rate(mentatd_errors_total[5m]) / rate(mentatd_requests_total[5m])) > 0.05
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "mentatd error rate above 5%"
          description: >
            Error rate is {{ $value | humanizePercentage }}
            over the last 5 minutes.

      # High query latency
      - alert: MentatdHighQueryLatency
        expr: |
          histogram_quantile(0.99, rate(mentatd_query_duration_seconds_bucket[5m])) > 2
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "mentatd p99 query latency above 2 seconds"
          description: >
            p99 query latency is {{ $value | humanizeDuration }}.

      # Connection pool exhaustion
      - alert: MentatdPoolExhaustion
        expr: |
          mentatd_connection_pool_size >= <configured_pool_size> * 0.9
        for: 2m
        labels:
          severity: warning
        annotations:
          summary: "mentatd connection pool near capacity"
          description: >
            Pool is at {{ $value }} connections.
            Consider increasing pool_size or optimizing queries.

      # Low cache hit ratio
      - alert: MentatdLowCacheHitRatio
        expr: |
          (rate(mentatd_cache_hits_total[15m]) /
           (rate(mentatd_cache_hits_total[15m]) + rate(mentatd_cache_misses_total[15m])))
          < 0.3
        for: 15m
        labels:
          severity: info
        annotations:
          summary: "mentatd cache hit ratio below 30%"
          description: >
            Cache hit ratio is {{ $value | humanizePercentage }}.
            Consider increasing cache capacity or reviewing query patterns.

      # No requests (possible outage)
      - alert: MentatdNoRequests
        expr: |
          rate(mentatd_requests_total[5m]) == 0
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "mentatd receiving no requests"
          description: >
            No requests received in the last 5 minutes.
            Check if the service is running and reachable.

      # High transaction rate (cache thrash)
      - alert: MentatdHighTransactionRate
        expr: |
          rate(mentatd_transactions_total[5m]) > 100
        for: 5m
        labels:
          severity: info
        annotations:
          summary: "mentatd high transaction rate"
          description: >
            Transaction rate is {{ $value }}/s.
            Query cache is being invalidated frequently.

  - name: postgresql
    rules:

      # PostgreSQL connection saturation
      - alert: PostgreSQLConnectionSaturation
        expr: |
          pg_stat_activity_count / pg_settings_max_connections > 0.8
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "PostgreSQL connections above 80% capacity"

      # High sequential scan ratio on datoms
      - alert: PostgreSQLHighSeqScan
        expr: |
          rate(pg_stat_user_tables_seq_scan{relname="datoms"}[5m]) > 100
        for: 10m
        labels:
          severity: info
        annotations:
          summary: "High sequential scan rate on mentat.datoms"
          description: >
            Consider adding indexes or reviewing query patterns.
            See the Performance Tuning guide.

      # Table bloat
      - alert: PostgreSQLTableBloat
        expr: |
          pg_stat_user_tables_n_dead_tup{relname="datoms"} > 1000000
        for: 30m
        labels:
          severity: warning
        annotations:
          summary: "mentat.datoms has over 1M dead tuples"
          description: >
            Run VACUUM ANALYZE mentat.datoms during maintenance window.
```

Replace `<configured_pool_size>` with your actual pool size value.

## Grafana Dashboard Recommendations

### Dashboard 1: mentatd Overview

**Panels:**

1. **Request Rate** (Graph)
   - Query: `rate(mentatd_requests_total[5m])`
   - Shows traffic patterns and anomalies

2. **Error Rate** (Graph + Stat)
   - Query: `rate(mentatd_errors_total[5m]) / rate(mentatd_requests_total[5m])`
   - Red threshold at 5%

3. **Query Latency Heatmap** (Heatmap)
   - Query: `rate(mentatd_query_duration_seconds_bucket[5m])`
   - Shows latency distribution over time

4. **Query Latency Percentiles** (Graph)
   - p50: `histogram_quantile(0.5, rate(mentatd_query_duration_seconds_bucket[5m]))`
   - p95: `histogram_quantile(0.95, rate(mentatd_query_duration_seconds_bucket[5m]))`
   - p99: `histogram_quantile(0.99, rate(mentatd_query_duration_seconds_bucket[5m]))`

5. **Cache Hit Ratio** (Gauge)
   - Query: `rate(mentatd_cache_hits_total[5m]) / (rate(mentatd_cache_hits_total[5m]) + rate(mentatd_cache_misses_total[5m]))`
   - Green > 80%, yellow 50-80%, red < 50%

6. **Connection Pool** (Graph)
   - Query: `mentatd_connection_pool_size`
   - Add threshold line at configured `pool_size`

7. **Transaction Rate** (Graph)
   - Query: `rate(mentatd_transactions_total[5m])`

8. **Streaming Queries** (Graph)
   - Query: `rate(mentatd_stream_queries_total[5m])`
   - Query: `rate(mentatd_stream_rows_sent_total[5m])`

### Dashboard 2: PostgreSQL for mentat

**Panels:**

1. **Active Connections** (Stat)
   ```sql
   SELECT count(*) FROM pg_stat_activity
   WHERE application_name LIKE '%mentatd%'
     AND state = 'active';
   ```

2. **Datoms Table Size** (Stat)
   ```sql
   SELECT pg_total_relation_size('mentat.datoms');
   ```

3. **Index Usage** (Table)
   ```sql
   SELECT indexrelname, idx_scan, idx_tup_read
   FROM pg_stat_user_indexes
   WHERE schemaname = 'mentat'
   ORDER BY idx_scan DESC;
   ```

4. **Sequential vs. Index Scans** (Graph)
   - PostgreSQL exporter metrics: `pg_stat_user_tables_seq_scan`, `pg_stat_user_tables_idx_scan`

5. **Dead Tuples** (Graph)
   - Query: `pg_stat_user_tables_n_dead_tup{relname="datoms"}`
   - Rising trends indicate need for VACUUM

6. **Slow Queries** (Table, requires pg_stat_statements)
   ```sql
   SELECT query, calls, mean_exec_time, total_exec_time
   FROM pg_stat_statements
   WHERE query LIKE '%mentat%'
   ORDER BY mean_exec_time DESC
   LIMIT 10;
   ```

### Grafana Dashboard JSON

A minimal dashboard definition for import:

```json
{
  "dashboard": {
    "title": "mentatd Overview",
    "panels": [
      {
        "title": "Request Rate",
        "type": "timeseries",
        "targets": [{"expr": "rate(mentatd_requests_total[5m])"}],
        "gridPos": {"h": 8, "w": 12, "x": 0, "y": 0}
      },
      {
        "title": "Error Rate",
        "type": "timeseries",
        "targets": [{"expr": "rate(mentatd_errors_total[5m]) / rate(mentatd_requests_total[5m])"}],
        "gridPos": {"h": 8, "w": 12, "x": 12, "y": 0}
      },
      {
        "title": "Query Latency (p50 / p95 / p99)",
        "type": "timeseries",
        "targets": [
          {"expr": "histogram_quantile(0.5, rate(mentatd_query_duration_seconds_bucket[5m]))", "legendFormat": "p50"},
          {"expr": "histogram_quantile(0.95, rate(mentatd_query_duration_seconds_bucket[5m]))", "legendFormat": "p95"},
          {"expr": "histogram_quantile(0.99, rate(mentatd_query_duration_seconds_bucket[5m]))", "legendFormat": "p99"}
        ],
        "gridPos": {"h": 8, "w": 12, "x": 0, "y": 8}
      },
      {
        "title": "Cache Hit Ratio",
        "type": "gauge",
        "targets": [{"expr": "rate(mentatd_cache_hits_total[5m]) / (rate(mentatd_cache_hits_total[5m]) + rate(mentatd_cache_misses_total[5m]))"}],
        "gridPos": {"h": 8, "w": 6, "x": 12, "y": 8}
      },
      {
        "title": "Connection Pool Size",
        "type": "stat",
        "targets": [{"expr": "mentatd_connection_pool_size"}],
        "gridPos": {"h": 8, "w": 6, "x": 18, "y": 8}
      }
    ]
  }
}
```

## Query Performance Analysis

### Identifying Slow Queries

**Step 1:** Enable `pg_stat_statements`:

```sql
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

-- Reset statistics to start fresh
SELECT pg_stat_statements_reset();
```

**Step 2:** Run your workload, then query:

```sql
-- Top 10 slowest queries by average execution time
SELECT
  left(query, 100) AS query_preview,
  calls,
  round(mean_exec_time::numeric, 2) AS avg_ms,
  round(max_exec_time::numeric, 2) AS max_ms,
  round(total_exec_time::numeric, 2) AS total_ms,
  rows
FROM pg_stat_statements
WHERE query LIKE '%mentat%'
ORDER BY mean_exec_time DESC
LIMIT 10;

-- Queries consuming the most total time
SELECT
  left(query, 100) AS query_preview,
  calls,
  round(total_exec_time::numeric, 2) AS total_ms,
  round((total_exec_time / sum(total_exec_time) OVER ()) * 100, 1) AS pct_total
FROM pg_stat_statements
WHERE query LIKE '%mentat%'
ORDER BY total_exec_time DESC
LIMIT 10;
```

**Step 3:** For a specific slow query, use `EXPLAIN ANALYZE`:

```sql
EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT)
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);
```

### Interpreting Query Plans

Things to look for in `EXPLAIN ANALYZE` output:

| Pattern | Meaning | Action |
|---------|---------|--------|
| `Seq Scan on datoms` | Full table scan | Add index or make query more specific |
| `actual rows=100000, estimated rows=100` | Bad row estimates | Run `ANALYZE mentat.datoms` |
| `Sort Method: external merge` | Sort spills to disk | Increase `work_mem` |
| `Nested Loop (actual loops=10000)` | Expensive join | Reorder query patterns for better selectivity |
| `Buffers: shared read=5000` | Cold cache | Increase `shared_buffers` or pre-warm |

### PostgreSQL Table Statistics

```sql
-- Table-level statistics
SELECT
  relname,
  n_live_tup AS live_rows,
  n_dead_tup AS dead_rows,
  last_vacuum,
  last_autovacuum,
  last_analyze,
  last_autoanalyze
FROM pg_stat_user_tables
WHERE schemaname = 'mentat';

-- Index statistics
SELECT
  indexrelname AS index_name,
  idx_scan AS scans,
  idx_tup_read AS rows_read,
  idx_tup_fetch AS rows_fetched,
  pg_size_pretty(pg_relation_size(indexrelid)) AS size
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
ORDER BY idx_scan DESC;

-- Table and index sizes
SELECT
  tablename,
  pg_size_pretty(pg_total_relation_size('mentat.' || tablename)) AS total,
  pg_size_pretty(pg_relation_size('mentat.' || tablename)) AS table_only
FROM pg_tables
WHERE schemaname = 'mentat'
ORDER BY pg_total_relation_size('mentat.' || tablename) DESC;
```

## Health Check Monitoring

mentatd exposes a health endpoint:

```bash
# Basic health check
curl -s http://localhost:8080/health
# Returns: "mentatd ready"
```

For load balancer health checks, configure:
- **Path:** `/health`
- **Expected status:** 200
- **Expected body contains:** `ready`
- **Interval:** 10s
- **Timeout:** 5s
- **Unhealthy threshold:** 3 consecutive failures

### Kubernetes Probes

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 8080
  initialDelaySeconds: 10
  periodSeconds: 15
  timeoutSeconds: 5
  failureThreshold: 3

readinessProbe:
  httpGet:
    path: /health
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 10
  timeoutSeconds: 5
  failureThreshold: 2
```

## Log Monitoring

### Structured Logging

Configure JSON logging for log aggregation:

```toml
[logging]
level = "info"
format = "json"
```

### Key Log Patterns to Monitor

| Pattern | Meaning | Action |
|---------|---------|--------|
| `"Operation failed"` | Database or internal error | Check database connectivity |
| `"Parse failed"` | Malformed request | Client-side issue |
| `"Cache miss"` | Query not in cache | Normal if cache is warming up |
| `"Query cache invalidated"` | Transaction completed | Normal after writes |
| `"Connection pool"` errors | Pool exhaustion | Increase pool_size |

### Log aggregation query examples (for Loki/ELK):

```
# Errors per minute
{job="mentatd"} |= "error" | rate()[1m]

# Slow operations (logged at debug level)
{job="mentatd"} |= "duration" | json | duration > 1s
```

## See Also

- [Performance Tuning Guide](./PERFORMANCE_TUNING.md) -- PostgreSQL and mentatd tuning
- [Troubleshooting Guide](./TROUBLESHOOTING.md) -- Common issues and solutions
- [mentatd Configuration](../configuration/mentatd_config.md) -- Metrics and logging configuration
