# Runbook: Connection Pool Exhaustion

## Severity: P1

## Trigger

Alert `MentatdPoolExhausted` fires when:
- `mentatd_connection_pool_available < 2`

Or requests start failing with "Database unavailable" anomaly responses.

## Symptoms

- `mentatd_connection_pool_available` drops to 0
- `mentatd_connection_pool_waiting` is > 0
- Requests timing out (30-second wait timeout on pool acquisition)
- HTTP 503 or anomaly responses with `db.error/unavailable`

## Investigation Steps

### 1. Confirm Pool Status

```bash
curl -s http://localhost:8080/metrics | grep connection_pool
# mentatd_connection_pool_size       -- total connections
# mentatd_connection_pool_available  -- idle connections
# mentatd_connection_pool_waiting    -- estimated waiting requests
```

### 2. Check PostgreSQL Connection State

```sql
-- Total connections from mentatd
SELECT count(*), state
FROM pg_stat_activity
WHERE datname = 'mentat' AND application_name != ''
GROUP BY state;

-- Long-running queries consuming connections
SELECT pid, now() - query_start AS duration, state, left(query, 100) AS query
FROM pg_stat_activity
WHERE datname = 'mentat'
  AND state IN ('active', 'idle in transaction')
ORDER BY duration DESC;

-- Check if max_connections is reached
SELECT count(*) AS current, setting::int AS max
FROM pg_stat_activity, pg_settings
WHERE pg_settings.name = 'max_connections'
GROUP BY setting;
```

### 3. Check for Lock Contention

```sql
-- Queries waiting for locks
SELECT
    blocked.pid AS blocked_pid,
    blocked.query AS blocked_query,
    now() - blocked.query_start AS blocked_duration,
    blocking.pid AS blocking_pid,
    blocking.query AS blocking_query
FROM pg_stat_activity blocked
JOIN pg_locks bl ON bl.pid = blocked.pid AND NOT bl.granted
JOIN pg_locks l ON l.locktype = bl.locktype
    AND l.database IS NOT DISTINCT FROM bl.database
    AND l.relation IS NOT DISTINCT FROM bl.relation
    AND l.pid != bl.pid
    AND l.granted
JOIN pg_stat_activity blocking ON blocking.pid = l.pid;
```

### 4. Check Request Rate

```bash
# Is there a traffic spike?
curl -s http://localhost:8080/metrics | grep mentatd_requests_total
```

## Remediation

### Immediate: Free Up Connections

```sql
-- Terminate idle-in-transaction connections (they hold locks but do nothing)
SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE datname = 'mentat'
  AND state = 'idle in transaction'
  AND now() - state_change > interval '2 minutes';

-- Terminate queries running longer than 60 seconds
SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE datname = 'mentat'
  AND state = 'active'
  AND now() - query_start > interval '60 seconds';
```

### If pool size is too small:

1. Increase `DATABASE_POOL_SIZE` in mentatd configuration
2. Ensure PostgreSQL `max_connections` can accommodate the new pool size
3. Restart mentatd:

```bash
# Update environment
export DATABASE_POOL_SIZE=200

# Restart
systemctl restart mentatd
```

### If PostgreSQL max_connections is the limit:

```sql
-- Increase max_connections (requires restart)
ALTER SYSTEM SET max_connections = 300;
-- Restart PostgreSQL
```

### If there is a traffic spike:

- Scale up mentatd instances (if using Kubernetes, HPA should handle this)
- Enable or increase query cache to reduce database load:
  ```
  MENTATD_CACHE_ENABLED=true
  MENTATD_CACHE_CAPACITY=10000
  ```
- Consider rate limiting at the load balancer level

### If connections are leaking:

If the pool size keeps growing but connections are never returned, there may be a
connection leak. Check mentatd logs for connection errors and restart mentatd:

```bash
systemctl restart mentatd
```

## Prevention

- Set pool size based on capacity planning (see [PERFORMANCE.md](../PERFORMANCE.md))
- Monitor `mentatd_connection_pool_available` with alerting threshold at 10% of pool size
- Configure PostgreSQL `idle_in_transaction_session_timeout`:
  ```sql
  ALTER SYSTEM SET idle_in_transaction_session_timeout = '5min';
  SELECT pg_reload_conf();
  ```
- Configure `statement_timeout` to prevent runaway queries:
  ```sql
  ALTER SYSTEM SET statement_timeout = '30s';
  SELECT pg_reload_conf();
  ```
- Use connection pool metrics in Kubernetes HPA to scale mentatd pods

## Escalation

If pool exhaustion persists after remediation:
- Investigate whether a specific query pattern is causing the issue
- Review application-side connection handling
- Consider adding PgBouncer as a connection pooler between mentatd and PostgreSQL
