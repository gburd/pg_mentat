# pg_mentat Backup and Recovery

## Overview

pg_mentat stores all data in standard PostgreSQL tables within the `mentat` schema. Standard
PostgreSQL backup and recovery procedures apply. This guide covers strategies specific to
the mentat data model.

## Backup Strategy

### Logical Backup (pg_dump)

Best for: smaller databases, schema migrations, selective backups.

```bash
# Full backup of the mentat schema
pg_dump -Fc -f mentat_backup.dump \
  --schema=mentat \
  --no-owner \
  postgresql://mentat:password@localhost:5432/mentat

# Verify backup integrity
pg_restore --list mentat_backup.dump | head -20

# Data-only backup (no schema DDL)
pg_dump -Fc -f mentat_data.dump \
  --schema=mentat \
  --data-only \
  postgresql://mentat:password@localhost:5432/mentat
```

**Restore from logical backup:**

```bash
# Restore to a fresh database (schema + data)
createdb mentat_restored
psql -d mentat_restored -c "CREATE EXTENSION pg_mentat;"
pg_restore -d mentat_restored \
  --data-only \
  --schema=mentat \
  mentat_backup.dump

# Or restore to the same database (destructive)
pg_restore -d mentat --clean --if-exists --schema=mentat mentat_backup.dump
```

### Physical Backup (pg_basebackup)

Best for: large databases, point-in-time recovery (PITR), minimal downtime.

```bash
# Ensure WAL archiving is enabled in postgresql.conf:
#   wal_level = replica
#   archive_mode = on
#   archive_command = 'cp %p /backup/wal/%f'

# Take a base backup
pg_basebackup -D /backup/base/$(date +%Y%m%d) \
  -Ft -z -Xs -P \
  -h localhost -U replication_user
```

### Continuous Archiving (WAL Shipping)

For point-in-time recovery with minimal data loss:

1. **Configure WAL archiving** in `postgresql.conf`:

```ini
wal_level = replica
archive_mode = on
archive_command = 'test ! -f /backup/wal/%f && cp %p /backup/wal/%f'
archive_timeout = 300    # Force archive every 5 minutes at minimum
```

2. **Take periodic base backups**:

```bash
# Weekly base backup (cron job)
0 2 * * 0 pg_basebackup -D /backup/base/$(date +\%Y\%m\%d) -Ft -z -Xs -P
```

3. **Restore to a specific point in time**:

```bash
# Create recovery.conf or postgresql.auto.conf
restore_command = 'cp /backup/wal/%f %p'
recovery_target_time = '2026-04-24 14:30:00 UTC'
recovery_target_action = 'promote'
```

## Backup Schedule

| Backup Type      | Frequency | Retention | RPO          |
|------------------|-----------|-----------|--------------|
| pg_dump          | Daily     | 30 days   | 24 hours     |
| pg_basebackup    | Weekly    | 4 weeks   | WAL interval |
| WAL archiving    | Continuous| 7 days    | ~5 minutes   |

Adjust based on your data loss tolerance and compliance requirements.

## What to Back Up

### Critical Data

| Table                    | Contains                            | Priority |
|--------------------------|-------------------------------------|----------|
| `mentat.datoms`          | All facts (entity-attribute-value)  | Critical |
| `mentat.schema`          | Attribute definitions               | Critical |
| `mentat.transactions`    | Transaction metadata                | Critical |
| `mentat.partitions`      | Entity ID partition boundaries      | Critical |
| `mentat.idents`          | Keyword-to-entid cache              | High     |
| `mentat.fulltext`        | Full-text search data               | High     |

### Sequences

Sequences are critical for entity ID allocation. They are included in `pg_dump` by default:

| Sequence                       | Purpose                    |
|--------------------------------|----------------------------|
| `mentat.partition_db_seq`      | System entity IDs          |
| `mentat.partition_user_seq`    | User entity IDs            |
| `mentat.partition_tx_seq`      | Transaction IDs            |

If restoring data-only, reset sequences to values beyond the maximum existing IDs:

```sql
SELECT setval('mentat.partition_db_seq',
  GREATEST(100, (SELECT COALESCE(MAX(entid), 0) + 1 FROM mentat.schema WHERE entid < 10000)));
SELECT setval('mentat.partition_user_seq',
  GREATEST(10000, (SELECT COALESCE(MAX(e), 0) + 1 FROM mentat.datoms WHERE e >= 10000 AND e < 1000000)));
SELECT setval('mentat.partition_tx_seq',
  GREATEST(1000001, (SELECT COALESCE(MAX(tx), 0) + 1 FROM mentat.transactions)));
```

### mentatd Configuration

Back up the mentatd configuration file and any secrets:

```bash
cp /etc/mentatd/mentatd.toml /backup/config/
# Do NOT back up API keys in plaintext; use secret management
```

## Disaster Recovery

### Recovery Time Objective (RTO)

| Scenario                | Target RTO | Method                      |
|-------------------------|------------|-----------------------------|
| Data corruption         | 1 hour     | Restore from pg_dump        |
| Disk failure            | 30 minutes | Restore from pg_basebackup  |
| Full server loss        | 2 hours    | PITR from base + WAL        |
| Human error (bad write) | 15 minutes | PITR to before the error    |

### Recovery Point Objective (RPO)

| Configuration             | RPO          |
|---------------------------|--------------|
| Daily pg_dump only        | Up to 24h    |
| WAL archiving (5 min)     | Up to 5 min  |
| Synchronous replication   | 0 (no loss)  |

### Recovery Procedures

**Scenario 1: Restore from pg_dump (data corruption)**

```bash
# 1. Stop mentatd
systemctl stop mentatd

# 2. Drop and recreate the schema
psql -d mentat -c "DROP EXTENSION pg_mentat CASCADE;"
psql -d mentat -c "CREATE EXTENSION pg_mentat;"

# 3. Restore data
pg_restore -d mentat --data-only --schema=mentat mentat_backup.dump

# 4. Reset sequences (see above)

# 5. Analyze tables for optimal query plans
psql -d mentat -c "ANALYZE;"

# 6. Restart mentatd
systemctl start mentatd
```

**Scenario 2: Point-in-time recovery (accidental data loss)**

```bash
# 1. Stop PostgreSQL
systemctl stop postgresql

# 2. Move current data directory
mv /var/lib/postgresql/16/main /var/lib/postgresql/16/main.damaged

# 3. Restore base backup
tar xzf /backup/base/20260420/base.tar.gz -C /var/lib/postgresql/16/main

# 4. Configure recovery
cat >> /var/lib/postgresql/16/main/postgresql.auto.conf <<EOF
restore_command = 'cp /backup/wal/%f %p'
recovery_target_time = '2026-04-24 14:30:00 UTC'
recovery_target_action = 'promote'
EOF

# 5. Create recovery signal
touch /var/lib/postgresql/16/main/recovery.signal

# 6. Start PostgreSQL (recovery mode)
systemctl start postgresql

# 7. Verify recovery, then restart mentatd
psql -d mentat -c "SELECT count(*) FROM mentat.datoms;"
systemctl start mentatd
```

## Testing Recovery

Recovery procedures should be tested regularly:

1. **Monthly**: Restore a pg_dump backup to a test environment and verify data integrity.
2. **Quarterly**: Perform a full PITR test using base backup + WAL archives.
3. **Verification queries** after every recovery:

```sql
-- Verify partition boundaries match sequence values
SELECT name, next_entid FROM mentat.partitions;
SELECT last_value FROM mentat.partition_db_seq;
SELECT last_value FROM mentat.partition_user_seq;
SELECT last_value FROM mentat.partition_tx_seq;

-- Verify referential integrity
SELECT count(*) FROM mentat.datoms d
WHERE NOT EXISTS (SELECT 1 FROM mentat.schema s WHERE s.entid = d.a);
-- Should be 0

-- Verify no duplicate values for unique attributes
SELECT s.ident, d.a, d.v_text, count(*)
FROM mentat.datoms d
JOIN mentat.schema s ON s.entid = d.a
WHERE s.unique_constraint IS NOT NULL AND d.added = TRUE
GROUP BY s.ident, d.a, d.v_text
HAVING count(*) > 1;
-- Should return no rows
```

## Replication

For high availability, set up PostgreSQL streaming replication:

```ini
# Primary (postgresql.conf)
wal_level = replica
max_wal_senders = 5
wal_keep_size = '1GB'

# Replica
primary_conninfo = 'host=primary-host user=replication_user password=...'
```

mentatd can point read-only workloads to the replica by using separate connection strings
for read and write operations (not yet implemented; use PgBouncer or HAProxy to route
read-only traffic).
