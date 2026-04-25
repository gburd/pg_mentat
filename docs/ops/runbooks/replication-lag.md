# Runbook: Replication Lag

## Severity: P2 (lag > 5 minutes), P1 (lag > 30 minutes or replica disconnected)

## Trigger

Replication lag between the primary PostgreSQL instance and replicas exceeds acceptable
thresholds. This affects read-replica query freshness if mentatd routes reads to replicas.

## Symptoms

- Stale data returned from read replicas
- `pg_stat_replication` shows increasing `replay_lag`
- WAL accumulation on the primary
- Replica reports recovery conflicts

## Investigation Steps

### 1. Check Replication Status on Primary

```sql
-- Replication connections and lag
SELECT
    client_addr,
    state,
    sent_lsn,
    write_lsn,
    flush_lsn,
    replay_lsn,
    pg_size_pretty(pg_wal_lsn_diff(sent_lsn, replay_lsn)) AS replay_lag_bytes,
    write_lag,
    flush_lag,
    replay_lag
FROM pg_stat_replication;
```

### 2. Check Replica Status

```sql
-- On the replica
SELECT pg_is_in_recovery();           -- Should be true
SELECT pg_last_wal_receive_lsn();     -- Last received WAL position
SELECT pg_last_wal_replay_lsn();      -- Last replayed WAL position
SELECT pg_last_xact_replay_timestamp(); -- Timestamp of last replayed transaction
SELECT now() - pg_last_xact_replay_timestamp() AS lag;
```

### 3. Check for Recovery Conflicts

```sql
-- On the replica: check for conflicts canceling queries
SELECT datname, confl_tablespace, confl_lock, confl_snapshot, confl_bufferpin, confl_deadlock
FROM pg_stat_database_conflicts
WHERE datname = 'mentat';
```

### 4. Check Network and Disk I/O

```bash
# Network latency between primary and replica
ping replica-host

# Disk I/O on replica (is it keeping up with WAL replay?)
iostat -x 1 5
```

## Remediation

### If replica is disconnected:

```bash
# Restart replication on the replica
pg_ctl -D /var/lib/postgresql/16/main promote  # Only if failover
# Or restart the replica to reconnect
systemctl restart postgresql
```

### If lag is due to long-running queries on replica:

```sql
-- On the replica: increase max_standby_streaming_delay
ALTER SYSTEM SET max_standby_streaming_delay = '5min';
SELECT pg_reload_conf();

-- Or terminate conflicting queries
SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE state = 'active' AND query_start < now() - interval '2 minutes';
```

### If lag is due to heavy writes on primary:

- The replica replay speed may not keep up with write volume.
- Consider increasing `wal_buffers` on the primary.
- Ensure the replica has sufficient I/O capacity (SSD recommended).
- Reduce write batch sizes if possible.

### If lag is due to network bandwidth:

- Check network capacity between primary and replica.
- Enable WAL compression:
  ```sql
  ALTER SYSTEM SET wal_compression = on;
  SELECT pg_reload_conf();
  ```

## Prevention

- Monitor replication lag continuously
- Set `max_standby_streaming_delay` to balance query freshness vs. long queries
- Use SSDs on replicas for faster WAL replay
- Enable `hot_standby_feedback` to reduce recovery conflicts:
  ```sql
  ALTER SYSTEM SET hot_standby_feedback = on;  -- On replica
  ```
- Size replica hardware to match or exceed primary I/O capacity

## Escalation

If replication lag is persistent and growing:
- Evaluate whether the replica hardware is undersized
- Consider adding more replicas to distribute read load
- Review whether write volume has increased (new workload pattern)
- Plan for potential failover if the primary is at risk
