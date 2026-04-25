# pg_mentat Operations Documentation

This directory contains operations documentation for deploying, monitoring, and
maintaining pg_mentat in production.

## Guides

| Document                                     | Description                                      |
|----------------------------------------------|--------------------------------------------------|
| [DEPLOYMENT.md](DEPLOYMENT.md)               | Installation, configuration, security            |
| [MONITORING.md](MONITORING.md)               | Prometheus metrics, dashboards, alerting          |
| [TROUBLESHOOTING.md](TROUBLESHOOTING.md)     | Common issues and solutions                      |
| [BACKUP.md](BACKUP.md)                       | Backup strategy, disaster recovery               |
| [PERFORMANCE.md](PERFORMANCE.md)             | Query optimization, tuning, capacity planning    |
| [UPGRADE.md](UPGRADE.md)                     | Upgrade procedures, rollback                     |

## Runbooks

Incident response procedures for common production issues:

| Runbook                                                         | Severity | Trigger                       |
|-----------------------------------------------------------------|----------|-------------------------------|
| [High Latency](runbooks/high-latency.md)                       | P2       | p95 query latency > 100ms    |
| [High Error Rate](runbooks/high-error-rate.md)                  | P1/P2    | Error rate > 1%               |
| [Connection Pool Full](runbooks/connection-pool-full.md)        | P1       | Pool available < 2            |
| [Disk Full](runbooks/disk-full.md)                              | P1       | Disk usage > 90%              |
| [Replication Lag](runbooks/replication-lag.md)                  | P1/P2    | Replica lag > 5 min           |
| [Security Incident](runbooks/security-incident.md)              | P0       | Unauthorized access detected  |

## Quick Reference

### Health Check

```bash
curl http://localhost:8080/health
```

### Metrics

```bash
curl http://localhost:8080/metrics
```

### Key Diagnostic Queries

```sql
-- Connection count
SELECT count(*), state FROM pg_stat_activity WHERE datname = 'mentat' GROUP BY state;

-- Table sizes
SELECT relname, pg_size_pretty(pg_total_relation_size(c.oid))
FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'mentat' ORDER BY pg_total_relation_size(c.oid) DESC;

-- Dead tuple ratio
SELECT relname, n_dead_tup, n_live_tup,
       round(100.0 * n_dead_tup / GREATEST(n_live_tup + n_dead_tup, 1), 1) AS dead_pct
FROM pg_stat_user_tables WHERE schemaname = 'mentat';
```
