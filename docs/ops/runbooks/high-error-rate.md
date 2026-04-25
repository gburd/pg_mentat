# Runbook: High Error Rate

## Severity: P1 (> 5% error rate), P2 (> 1% error rate)

## Trigger

Alert `MentatdHighErrorRate` fires when:
- `rate(mentatd_errors_total[5m]) / rate(mentatd_requests_total[5m]) > 0.01`

## Symptoms

- `mentatd_errors_total` counter increasing rapidly
- Clients receiving HTTP 500 or anomaly responses
- Logs showing repeated error messages

## Investigation Steps

### 1. Identify Error Type

```bash
# Check error count
curl -s http://localhost:8080/metrics | grep mentatd_errors_total

# Check mentatd logs for error details
# JSON log format:
journalctl -u mentatd --since "5 minutes ago" | jq 'select(.level == "ERROR")'

# Compact log format:
journalctl -u mentatd --since "5 minutes ago" | grep -i error
```

### 2. Check Database Connectivity

```bash
# Health check
curl http://localhost:8080/health
# If not "mentatd ready", database is unreachable

# Test direct database connection
psql "postgresql://mentat:password@localhost:5432/mentat" -c "SELECT 1;"
```

### 3. Check PostgreSQL Status

```sql
-- Is PostgreSQL accepting connections?
SELECT count(*) FROM pg_stat_activity;

-- Check for connection limit
SHOW max_connections;
SELECT count(*) FROM pg_stat_activity;

-- Check for disk space issues
SELECT pg_size_pretty(pg_database_size('mentat'));

-- Check for crashed or inconsistent state
SELECT datname, xact_commit, xact_rollback FROM pg_stat_database WHERE datname = 'mentat';
```

### 4. Check Connection Pool

```bash
curl -s http://localhost:8080/metrics | grep connection_pool
# If pool_available = 0, all connections are in use or broken
```

### 5. Check for Schema Issues

```sql
-- Verify extension is loaded
SELECT * FROM pg_extension WHERE extname = 'pg_mentat';

-- Verify schema objects exist
SELECT count(*) FROM information_schema.tables WHERE table_schema = 'mentat';

-- Check for corrupt indexes
REINDEX SCHEMA mentat;
```

## Remediation

### If database is unreachable:

```bash
# Check PostgreSQL is running
systemctl status postgresql

# Restart if necessary
systemctl restart postgresql

# Restart mentatd to reconnect
systemctl restart mentatd
```

### If connection pool is exhausted:

See [connection-pool-full.md](connection-pool-full.md).

### If disk is full:

See [disk-full.md](disk-full.md).

### If errors are parse/protocol errors:

This indicates malformed client requests. Check:
- Client library version compatibility
- Request payload format (EDN, Transit JSON, Transit MessagePack)
- Request body size (max 16 MiB)

Parse errors are client-side issues and do not require server-side remediation.

### If errors are serialization/transaction conflicts:

```sql
-- Check for lock contention
SELECT * FROM pg_locks WHERE NOT granted;

-- Check for long-running transactions holding locks
SELECT pid, now() - xact_start AS duration, query
FROM pg_stat_activity
WHERE state = 'idle in transaction'
ORDER BY duration DESC;
```

Terminate long-running idle transactions:
```sql
SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE state = 'idle in transaction'
  AND now() - xact_start > interval '5 minutes';
```

### If the extension is missing or corrupt:

```bash
# Reinstall the extension
cd /path/to/pg_mentat/pg_mentat
cargo pgrx install --release --pg-config=$(which pg_config)

# Reconnect and verify
psql -d mentat -c "SELECT * FROM mentat.partitions;"
```

## Prevention

- Monitor `mentatd_errors_total` rate with alerting
- Set up health check monitoring (external probe hitting `/health`)
- Configure PostgreSQL `log_min_error_statement = error` for detailed error logs
- Implement client-side retry logic with exponential backoff
- Review connection pool sizing relative to `max_connections`

## Escalation

If error rate does not decrease after remediation:
- Check for infrastructure issues (network, DNS, storage)
- Review recent deployments or configuration changes
- Escalate to development team with error log samples
