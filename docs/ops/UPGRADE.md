# pg_mentat Upgrade Guide

## Version Compatibility Matrix

| pg_mentat Version | PostgreSQL | Rust  | pgrx  | mentatd |
|-------------------|------------|-------|-------|---------|
| 0.1.0             | 13-18      | 1.88+ | 0.17  | 0.1.0   |

## Pre-Upgrade Checklist

Before upgrading any component:

- [ ] Review the CHANGELOG for breaking changes
- [ ] Back up the database (see [BACKUP.md](BACKUP.md))
- [ ] Test the upgrade in a staging environment
- [ ] Verify disk space for the upgrade process
- [ ] Schedule a maintenance window (for extension upgrades)
- [ ] Notify users of planned downtime (if applicable)
- [ ] Confirm rollback procedure is documented and tested

## Upgrading pg_mentat Extension

### Minor Version Upgrade (e.g., 0.1.0 -> 0.1.1)

Minor upgrades may include bug fixes and non-breaking changes.

```bash
# 1. Build the new version
cd pg_mentat
git pull
cargo pgrx install --release --pg-config=$(which pg_config)

# 2. Apply the upgrade SQL (if migration script exists)
psql -d mentat -c "ALTER EXTENSION pg_mentat UPDATE TO '0.1.1';"

# 3. Verify
psql -d mentat -c "SELECT * FROM pg_extension WHERE extname = 'pg_mentat';"
```

If no migration script exists for the target version, a full reinstall may be required:

```bash
# 1. Back up data
pg_dump -Fc -f mentat_backup.dump --schema=mentat mentat

# 2. Drop and recreate
psql -d mentat -c "DROP EXTENSION pg_mentat CASCADE;"

# 3. Install new version
cargo pgrx install --release --pg-config=$(which pg_config)

# 4. Recreate extension
psql -d mentat -c "CREATE EXTENSION pg_mentat;"

# 5. Restore data
pg_restore -d mentat --data-only --schema=mentat mentat_backup.dump

# 6. Reset sequences (critical!)
psql -d mentat <<'SQL'
SELECT setval('mentat.partition_db_seq',
  GREATEST(100, (SELECT COALESCE(MAX(entid), 0) + 1 FROM mentat.schema WHERE entid < 10000)));
SELECT setval('mentat.partition_user_seq',
  GREATEST(10000, (SELECT COALESCE(MAX(e), 0) + 1 FROM mentat.datoms WHERE e >= 10000 AND e < 1000000)));
SELECT setval('mentat.partition_tx_seq',
  GREATEST(1000001, (SELECT COALESCE(MAX(tx), 0) + 1 FROM mentat.transactions)));
SQL

# 7. Analyze
psql -d mentat -c "ANALYZE;"
```

### Major Version Upgrade

Major upgrades may include schema changes. Always follow the specific migration guide
provided in the CHANGELOG for the target version.

## Upgrading mentatd

mentatd is a standalone binary with no persistent state. Upgrades are straightforward:

### Rolling Upgrade (Zero-Downtime)

If running multiple mentatd instances behind a load balancer:

```bash
# 1. Build new binary
cd mentatd
cargo build --release

# 2. Deploy to one instance at a time
# For each instance:
systemctl stop mentatd
cp target/release/mentatd /usr/local/bin/mentatd
systemctl start mentatd

# 3. Verify health before moving to next instance
curl http://instance:8080/health
```

### Docker/Kubernetes Rolling Update

```bash
# Docker
docker pull pg-mentat/mentatd:new-version
docker stop mentatd
docker rm mentatd
docker run -d --name mentatd ... pg-mentat/mentatd:new-version

# Kubernetes (Helm)
helm upgrade pg-mentat ./helm/pg-mentat --set mentatd.image.tag=new-version

# Kubernetes (raw manifests)
kubectl set image deployment/mentatd mentatd=pg-mentat/mentatd:new-version
```

The Helm chart is configured with rolling update strategy and pod disruption budget
(`minAvailable: 1`) to maintain availability during upgrades.

## Upgrading PostgreSQL

### Minor Version Upgrade (e.g., 16.2 -> 16.3)

PostgreSQL minor upgrades are in-place and do not require data migration:

```bash
# 1. Stop PostgreSQL
systemctl stop postgresql

# 2. Upgrade packages (distro-specific)
apt-get update && apt-get upgrade postgresql-16

# 3. Start PostgreSQL
systemctl start postgresql

# 4. Verify
psql -c "SELECT version();"
```

### Major Version Upgrade (e.g., 16 -> 17)

PostgreSQL major upgrades require pg_upgrade or dump/restore:

```bash
# 1. Back up everything
pg_dumpall -f full_backup.sql

# 2. Install new PostgreSQL version
apt-get install postgresql-17

# 3. Rebuild pg_mentat extension for the new version
cd pg_mentat
cargo pgrx install --release --pg-config=/usr/lib/postgresql/17/bin/pg_config --features pg17

# 4. Run pg_upgrade
pg_upgrade \
  -b /usr/lib/postgresql/16/bin \
  -B /usr/lib/postgresql/17/bin \
  -d /var/lib/postgresql/16/main \
  -D /var/lib/postgresql/17/main

# 5. Start PostgreSQL 17
systemctl start postgresql@17-main

# 6. Run post-upgrade analyze
/usr/lib/postgresql/17/bin/vacuumdb --all --analyze-in-stages

# 7. Verify extension
psql -d mentat -c "SELECT * FROM pg_extension WHERE extname = 'pg_mentat';"
psql -d mentat -c "SELECT count(*) FROM mentat.datoms;"
```

## Rollback Procedures

### Rolling Back pg_mentat Extension

If the upgrade introduced issues:

```bash
# If ALTER EXTENSION UPDATE was used and a downgrade path exists:
psql -d mentat -c "ALTER EXTENSION pg_mentat UPDATE TO '0.1.0';"

# If no downgrade path, restore from backup:
# 1. Stop mentatd
systemctl stop mentatd

# 2. Drop new extension
psql -d mentat -c "DROP EXTENSION pg_mentat CASCADE;"

# 3. Install old version
git checkout v0.1.0
cargo pgrx install --release --pg-config=$(which pg_config)

# 4. Recreate and restore
psql -d mentat -c "CREATE EXTENSION pg_mentat;"
pg_restore -d mentat --data-only --schema=mentat mentat_backup.dump

# 5. Reset sequences, analyze, restart mentatd
```

### Rolling Back mentatd

```bash
# Simply deploy the old binary
systemctl stop mentatd
cp /backup/mentatd-old /usr/local/bin/mentatd
systemctl start mentatd
```

### Rolling Back PostgreSQL Major Upgrade

This requires restoring from the full backup taken before the upgrade:

```bash
# 1. Stop PostgreSQL 17
systemctl stop postgresql@17-main

# 2. Restore PostgreSQL 16
systemctl start postgresql@16-main
psql -f full_backup.sql

# 3. Rebuild pg_mentat for PostgreSQL 16
cargo pgrx install --release --pg-config=/usr/lib/postgresql/16/bin/pg_config --features pg16
```

## Post-Upgrade Verification

After any upgrade, run these checks:

```sql
-- 1. Extension is loaded
SELECT extname, extversion FROM pg_extension WHERE extname = 'pg_mentat';

-- 2. Schema objects exist
SELECT count(*) FROM information_schema.tables WHERE table_schema = 'mentat';
-- Expected: 6 tables (datoms, schema, transactions, partitions, idents, fulltext)

-- 3. Data integrity
SELECT count(*) FROM mentat.datoms;
SELECT count(*) FROM mentat.schema;
SELECT count(*) FROM mentat.transactions;

-- 4. Sequences are correct
SELECT 'partition_db_seq' AS seq, last_value FROM mentat.partition_db_seq
UNION ALL
SELECT 'partition_user_seq', last_value FROM mentat.partition_user_seq
UNION ALL
SELECT 'partition_tx_seq', last_value FROM mentat.partition_tx_seq;

-- 5. Indexes exist
SELECT indexname FROM pg_indexes WHERE schemaname = 'mentat' ORDER BY indexname;

-- 6. Test a query
SELECT e, v_keyword FROM mentat.datoms
WHERE a = 10 AND value_type_tag = 8 AND added = TRUE
LIMIT 5;

-- 7. Test a transaction
SELECT mentat.allocate_entid('db.part/user');
```

```bash
# 8. mentatd health check
curl http://localhost:8080/health

# 9. mentatd metrics
curl -s http://localhost:8080/metrics | head -20
```
