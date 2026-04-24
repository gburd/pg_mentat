# Backup and Restore Guide

Procedures for backing up and restoring pg_mentat databases, including
the mentat schema (datoms, transactions, schema cache) and the pg_mentat
extension itself.

## Architecture overview

pg_mentat stores all data in standard PostgreSQL tables within the
`mentat` schema.  This means standard PostgreSQL backup tools
(`pg_dump`, `pg_basebackup`, WAL archiving) work without modification.

Key tables:
- `mentat.datoms` -- all entity-attribute-value-transaction datoms
- `mentat.transactions` -- transaction log
- `mentat.schema` -- schema attribute cache
- `mentat.ident_cache` -- keyword-to-entid mapping

## Backup methods

### Method 1: Logical backup (pg_dump)

Best for: small-to-medium databases, cross-version migration, selective
restore.

```bash
# Full database dump (recommended for most cases)
pg_dump -Fc -d mentat -f mentat_backup.dump

# Schema-only dump (for replicating structure)
pg_dump -Fc -d mentat --schema=mentat -f mentat_schema.dump

# Datoms-only dump (data without extension objects)
pg_dump -Fc -d mentat --schema=mentat --data-only -f mentat_data.dump
```

### Method 2: Physical backup (pg_basebackup)

Best for: large databases, point-in-time recovery, streaming replication.

```bash
# Full cluster backup
pg_basebackup -D /backup/pg_mentat_$(date +%Y%m%d) \
    -Ft -z -P \
    -h localhost -U postgres

# With WAL archiving for PITR
pg_basebackup -D /backup/pg_mentat_$(date +%Y%m%d) \
    -Ft -z -P --wal-method=stream \
    -h localhost -U postgres
```

### Method 3: Docker volume backup

For Docker Compose deployments using the standard `docker-compose.yml`:

```bash
# Stop services to ensure consistency
docker compose -f docker/docker-compose.yml stop postgres

# Backup the data volume
docker run --rm \
    -v pg_mentat_pgdata:/data:ro \
    -v $(pwd)/backups:/backup \
    alpine tar czf /backup/pgdata_$(date +%Y%m%d_%H%M%S).tar.gz -C /data .

# Restart services
docker compose -f docker/docker-compose.yml start postgres
```

### Method 4: Kubernetes backup

For Kubernetes deployments with the pg-mentat StatefulSet:

```bash
# Option A: pg_dump via kubectl exec
kubectl exec -n pg-mentat pg-mentat-postgres-0 -- \
    pg_dump -Fc -U mentat -d mentat > mentat_backup.dump

# Option B: Volume snapshot (cloud provider specific)
# AWS EBS:
kubectl apply -f - <<EOF
apiVersion: snapshot.storage.k8s.io/v1
kind: VolumeSnapshot
metadata:
  name: pg-mentat-snap-$(date +%Y%m%d)
  namespace: pg-mentat
spec:
  volumeSnapshotClassName: csi-aws-vsc
  source:
    persistentVolumeClaimName: data-pg-mentat-postgres-0
EOF
```

## Restore procedures

### Restore from pg_dump

```bash
# 1. Create a fresh database with pg_mentat extension
createdb -U postgres mentat_restored
psql -U postgres -d mentat_restored -c "CREATE EXTENSION pg_mentat;"

# 2. Restore data
pg_restore -d mentat_restored -U postgres mentat_backup.dump

# 3. Verify
psql -U postgres -d mentat_restored -c "
    SELECT count(*) AS datom_count FROM mentat.datoms;
    SELECT count(*) AS schema_count FROM mentat.schema;
"
```

### Restore from pg_basebackup

```bash
# 1. Stop PostgreSQL
systemctl stop postgresql

# 2. Replace data directory
rm -rf /var/lib/postgresql/16/main
tar xzf /backup/pg_mentat_20260424/base.tar.gz -C /var/lib/postgresql/16/main

# 3. If using PITR, configure recovery
cat > /var/lib/postgresql/16/main/recovery.signal <<EOF
EOF
# Add to postgresql.conf:
#   restore_command = 'cp /backup/wal/%f %p'
#   recovery_target_time = '2026-04-24 12:00:00'

# 4. Start PostgreSQL
systemctl start postgresql
```

### Restore Docker volume

```bash
# 1. Stop services
docker compose -f docker/docker-compose.yml down

# 2. Remove old volume
docker volume rm pg_mentat_pgdata

# 3. Create new volume and restore
docker volume create pg_mentat_pgdata
docker run --rm \
    -v pg_mentat_pgdata:/data \
    -v $(pwd)/backups:/backup:ro \
    alpine tar xzf /backup/pgdata_20260424_120000.tar.gz -C /data

# 4. Start services
docker compose -f docker/docker-compose.yml up -d
```

### Restore in Kubernetes

```bash
# 1. Scale down
kubectl scale -n pg-mentat statefulset pg-mentat-postgres --replicas=0

# 2. Delete the PVC (data will be lost)
kubectl delete -n pg-mentat pvc data-pg-mentat-postgres-0

# 3A: Restore from volume snapshot
kubectl apply -f - <<EOF
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: data-pg-mentat-postgres-0
  namespace: pg-mentat
spec:
  dataSource:
    name: pg-mentat-snap-20260424
    kind: VolumeSnapshot
    apiGroup: snapshot.storage.k8s.io
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 10Gi
EOF

# 3B: Restore from pg_dump (after scaling up)
kubectl scale -n pg-mentat statefulset pg-mentat-postgres --replicas=1
# Wait for pod to be ready
kubectl wait -n pg-mentat pod/pg-mentat-postgres-0 --for=condition=Ready --timeout=120s
# Restore
kubectl exec -i -n pg-mentat pg-mentat-postgres-0 -- \
    pg_restore -U mentat -d mentat < mentat_backup.dump

# 4. Scale up mentatd
kubectl scale -n pg-mentat deployment mentatd --replicas=2
```

## Verification after restore

Run these checks after any restore operation:

```bash
# 1. Extension is loaded
psql -d mentat -c "SELECT extname, extversion FROM pg_extension WHERE extname = 'pg_mentat';"

# 2. Schema exists
psql -d mentat -c "SELECT count(*) FROM mentat.datoms;"
psql -d mentat -c "SELECT count(*) FROM mentat.schema;"

# 3. Typed columns are present (post-migration)
psql -d mentat -c "
    SELECT column_name, data_type
    FROM information_schema.columns
    WHERE table_schema = 'mentat' AND table_name = 'datoms'
    ORDER BY ordinal_position;
"

# 4. Basic query works
psql -d mentat -c "
    SELECT mentat.mentat_query(
        '[:find ?e ?ident :where [?e :db/ident ?ident]]',
        '{}'::jsonb
    );
"

# 5. Range query works (BYTEA fix validation)
psql -d mentat -c "
    SELECT mentat.mentat_query(
        '[:find ?e :where [?e :db/ident ?ident]]',
        '{}'::jsonb
    );
"
```

## Automated backup schedule

### Cron-based (Linux)

```bash
# /etc/cron.d/pg_mentat_backup
# Daily logical backup at 2 AM, retain 30 days
0 2 * * * postgres pg_dump -Fc -d mentat -f /backup/mentat_$(date +\%Y\%m\%d).dump && find /backup -name 'mentat_*.dump' -mtime +30 -delete
```

### Kubernetes CronJob

```yaml
apiVersion: batch/v1
kind: CronJob
metadata:
  name: pg-mentat-backup
  namespace: pg-mentat
spec:
  schedule: "0 2 * * *"
  jobTemplate:
    spec:
      template:
        spec:
          containers:
            - name: backup
              image: postgres:16-bookworm
              command:
                - sh
                - -c
                - |
                  pg_dump -Fc -h pg-mentat-postgres -U mentat -d mentat \
                    -f /backup/mentat_$(date +%Y%m%d).dump
                  find /backup -name 'mentat_*.dump' -mtime +30 -delete
              env:
                - name: PGPASSWORD
                  valueFrom:
                    secretKeyRef:
                      name: pg-mentat-secrets
                      key: postgres-password
              volumeMounts:
                - name: backup
                  mountPath: /backup
          restartPolicy: OnFailure
          volumes:
            - name: backup
              persistentVolumeClaim:
                claimName: pg-mentat-backup
```

## Disaster recovery

### Recovery Time Objectives

| Scenario                    | Method            | Expected RTO |
|-----------------------------|-------------------|-------------|
| mentatd crash               | Pod restart       | < 30s       |
| PostgreSQL crash            | Pod restart       | < 60s       |
| Data corruption             | pg_dump restore   | Minutes     |
| Volume loss                 | Volume snapshot   | Minutes     |
| Full cluster loss           | pg_basebackup     | 10-30 min   |
| Cross-region failover       | WAL streaming     | Minutes     |

### Recovery procedure for full cluster loss

1. Provision new PostgreSQL instance
2. Install pg_mentat extension
3. Restore from most recent backup (pg_dump or pg_basebackup)
4. If using the BYTEA-to-typed-columns migration, verify it has been applied:
   ```sql
   SELECT column_name FROM information_schema.columns
   WHERE table_schema = 'mentat' AND table_name = 'datoms'
   AND column_name = 'v' AND data_type = 'bytea';
   -- If this returns a row, run the migration:
   -- \i pg_mentat/migrations/001_bytea_to_typed_columns.sql
   ```
5. Start mentatd and verify connectivity
6. Run verification queries (see "Verification after restore" above)
