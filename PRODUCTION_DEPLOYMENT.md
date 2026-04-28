# Production Deployment Guide

This guide covers deploying pg_mentat in production environments: system requirements, PostgreSQL tuning, high-availability configurations, backup/restore procedures, and monitoring setup.

---

## Table of Contents

1. [System Requirements](#system-requirements)
2. [Installation](#installation)
3. [PostgreSQL Tuning](#postgresql-tuning)
4. [mentatd Configuration](#mentatd-configuration)
5. [High Availability](#high-availability)
6. [Backup and Restore](#backup-and-restore)
7. [Monitoring](#monitoring)
8. [Security Hardening](#security-hardening)
9. [Upgrade Procedures](#upgrade-procedures)
10. [Pre-Production Checklist](#pre-production-checklist)

---

## System Requirements

### Hardware

| Component | Minimum | Recommended (< 10M datoms) | Large Scale (10M-100M datoms) |
|-----------|---------|----------------------------|-------------------------------|
| CPU       | 2 cores | 4 cores                    | 8+ cores                      |
| RAM       | 2 GB    | 8 GB                       | 32+ GB                        |
| Disk      | 10 GB SSD | 50 GB NVMe SSD           | 500+ GB NVMe SSD              |
| Network   | 1 Gbps  | 1 Gbps                     | 10 Gbps                       |

SSDs are required for production. Spinning disks will cause unacceptable latency for index-heavy workloads.

### Software

| Dependency     | Supported Versions | Recommended | Notes                                     |
|----------------|--------------------|-------------|--------------------------------------------|
| PostgreSQL     | 13 -- 18           | 16          | Best tested; used in CI and Docker images  |
| Rust toolchain | >= 1.88            | 1.90.0      | Required only at build time                |
| cargo-pgrx     | ~0.17              | ~0.17       | Must match pgrx crate version              |
| LLVM/Clang     | 15 -- 18           | 18          | Required by pgrx bindgen at build time     |
| OpenSSL        | >= 1.1             | 3.x         | Required for TLS in mentatd                |

### Operating System

Tested on:
- Debian 12 / Ubuntu 22.04+
- Fedora 38+
- NixOS (with provided `flake.nix`)
- Docker (Debian Bookworm base)

---

## Installation

### Option 1: Docker (Recommended for Quick Start)

```bash
docker build -t pg_mentat .

docker run -d --name pg_mentat \
  -p 5432:5432 \
  -e POSTGRES_PASSWORD=<strong-password> \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  -v pgdata:/var/lib/postgresql/data \
  pg_mentat
```

The container initializes with demo data on first start. For production, remove or replace the demo initialization script in `/docker-entrypoint-initdb.d/`.

### Option 2: From Source (Nix)

```bash
nix develop
cd pg_mentat
cargo pgrx install --release --pg-config=$(which pg_config)
```

### Option 3: From Source (Manual)

```bash
cargo install --locked cargo-pgrx --version '~0.17'
cargo pgrx init --pg16=$(which pg_config)
cd pg_mentat
cargo pgrx install --release --pg-config=$(which pg_config)
```

### Option 4: Kubernetes (Helm)

```bash
helm install pg-mentat ./helm/pg-mentat \
  --set postgresql.auth.password=<strong-password> \
  --set mentatd.config.logFormat=json
```

See `helm/` for chart values and `k8s/` for raw manifests.

### Enabling the Extension

After installation, enable the extension in your target database:

```sql
CREATE EXTENSION pg_mentat;

-- Verify installation
SELECT * FROM mentat.partitions;
SELECT mentat_schema();
```

The extension creates the `mentat` schema with all required tables, indexes, sequences, and bootstrap data.

---

## PostgreSQL Tuning

### Memory Configuration

Set these in `postgresql.conf` or via `ALTER SYSTEM`:

```ini
# Shared memory - 25% of total RAM for a dedicated server
shared_buffers = '2GB'

# Planner cache size hint - 75% of total RAM
effective_cache_size = '6GB'

# Per-operation sort/hash memory
# Start at 64 MB; increase if EXPLAIN ANALYZE shows disk-based sorts
work_mem = '64MB'

# VACUUM and CREATE INDEX memory
maintenance_work_mem = '512MB'
```

### WAL and Checkpoint Settings

```ini
wal_level = replica                    # Required for PITR and replication
max_wal_size = '4GB'
min_wal_size = '1GB'
wal_buffers = '64MB'
checkpoint_completion_target = 0.9
checkpoint_timeout = '15min'
```

### Disk I/O (SSD-Optimized)

```ini
random_page_cost = 1.1                 # Default 4.0 is for spinning disks
effective_io_concurrency = 200         # Default 1; increase for NVMe
```

### Connection Settings

```ini
max_connections = 200                  # Must exceed total pool sizes
```

Ensure `max_connections` exceeds the sum of all mentatd pool sizes plus a reserve for superusers and maintenance:

```
max_connections >= (mentatd_pool_size * num_mentatd_instances) + 10
```

### Parallelism

```ini
max_parallel_workers_per_gather = 4
max_parallel_workers = 8
parallel_tuple_cost = 0.001
parallel_setup_cost = 100
min_parallel_table_scan_size = '8MB'
```

### Autovacuum Tuning

pg_mentat datom tables are configured with aggressive autovacuum by default. For high write rates (> 1000 TPS), tighten further:

```sql
ALTER TABLE mentat.datoms SET (
    autovacuum_vacuum_scale_factor = 0.01,
    autovacuum_vacuum_cost_delay = 2,
    autovacuum_vacuum_cost_limit = 1000
);
```

### JIT Compilation

For short OLTP-style queries, JIT compilation adds overhead:

```ini
jit = off
```

If you have a mix of short and long-running analytical queries, leave JIT enabled and set a threshold:

```ini
jit_above_cost = 500000
```

### Recommended `pg_stat_statements`

Enable for production query analysis:

```ini
shared_preload_libraries = 'pg_stat_statements'
pg_stat_statements.track = all
```

---

## mentatd Configuration

mentatd is the optional Datomic-compatible HTTP daemon. It is only needed if you require the Datomic wire protocol. Direct PostgreSQL access is preferred for new applications.

### Configuration File

Create `mentatd.toml`:

```toml
[server]
host = "0.0.0.0"
port = 8080
timeout = 30
# api_key = "your-secret-key-here"    # Uncomment to enable auth

[database]
connection_string = "postgresql://mentat_app:password@localhost:5432/mentat"
pool_size = 100
max_lifetime_secs = 1800

[logging]
level = "info"
format = "json"

[cache]
enabled = true
capacity = 5000
ttl_secs = 300
```

### Environment Variables

Environment variables override TOML values:

| Variable                | Default                         | Description                        |
|-------------------------|---------------------------------|------------------------------------|
| `MENTATD_CONFIG`        | `mentatd.toml`                  | Path to config file                |
| `MENTATD_HOST`          | `127.0.0.1`                     | Bind address                       |
| `MENTATD_PORT`          | `8080`                          | Listen port                        |
| `MENTATD_TIMEOUT`       | `30`                            | Request timeout (seconds)          |
| `MENTATD_API_KEY`       | *(none)*                        | Bearer token for auth              |
| `DATABASE_URL`          | `postgresql://localhost/mentat`  | PostgreSQL connection string       |
| `DATABASE_POOL_SIZE`    | `100`                           | Max pool connections               |
| `DATABASE_MAX_LIFETIME` | `1800`                          | Connection max age (seconds)       |
| `RUST_LOG`              | `info`                          | Log level                          |
| `LOG_FORMAT`            | `compact`                       | Log format: compact, json, pretty  |
| `MENTATD_CACHE_ENABLED` | `true`                          | Enable query cache                 |
| `MENTATD_CACHE_CAPACITY`| `1000`                          | Max cache entries                  |
| `MENTATD_CACHE_TTL`     | `300`                           | Cache TTL (seconds)                |

### Connection Pool Sizing

| Workload             | Recommended `pool_size` | Notes                             |
|----------------------|-------------------------|-----------------------------------|
| Light (< 10 RPS)    | 10-20                   | Default is fine                   |
| Medium (10-100 RPS)  | 50-100                  | Match to max concurrent queries   |
| Heavy (> 100 RPS)   | 100-200                 | Ensure max_connections allows it  |

### systemd Service Unit

```ini
[Unit]
Description=mentatd - Datomic-compatible HTTP server for pg_mentat
After=postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=mentatd
Group=mentatd
ExecStart=/usr/local/bin/mentatd
EnvironmentFile=/etc/mentatd/env
Restart=on-failure
RestartSec=5

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictSUIDSGID=true
RestrictNamespaces=true

[Install]
WantedBy=multi-user.target
```

---

## High Availability

### PostgreSQL Streaming Replication

Set up a primary + one or more replicas for failover and read scaling.

**Primary (`postgresql.conf`):**

```ini
wal_level = replica
max_wal_senders = 5
wal_keep_size = '1GB'
synchronous_standby_names = ''        # Set for synchronous replication
```

**Replica setup:**

```bash
pg_basebackup -D /var/lib/postgresql/16/replica \
  -h primary-host -U replication_user -Xs -P

# Configure replica
cat >> /var/lib/postgresql/16/replica/postgresql.auto.conf <<EOF
primary_conninfo = 'host=primary-host user=replication_user password=...'
EOF

touch /var/lib/postgresql/16/replica/standby.signal
```

**Read replica routing:**

mentatd does not natively support read/write splitting. Use PgBouncer or HAProxy to route read-only traffic to replicas:

```ini
# HAProxy example
frontend pg_frontend
    bind *:5432
    default_backend pg_primary

backend pg_primary
    server primary 10.0.0.1:5432 check

backend pg_replicas
    balance roundrobin
    server replica1 10.0.0.2:5432 check
    server replica2 10.0.0.3:5432 check
```

### mentatd High Availability

Deploy multiple mentatd instances behind a load balancer. mentatd is stateless (the query cache is per-instance and in-memory), so any instance can handle any request.

```yaml
# Kubernetes HPA example
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: mentatd
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: mentatd
  minReplicas: 2
  maxReplicas: 10
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
```

### Failover Considerations

- pg_mentat extension state is stored entirely in PostgreSQL tables. No external state management is required.
- After a PostgreSQL failover, restart mentatd instances to reset their connection pools.
- The in-process schema cache inside the extension is per-backend; it refreshes automatically on reconnection.

---

## Backup and Restore

### Logical Backup (pg_dump)

Best for: smaller databases (< 100M datoms), schema migrations, selective backups.

```bash
# Full backup of the mentat schema
pg_dump -Fc -f mentat_backup.dump \
  --schema=mentat \
  --no-owner \
  postgresql://mentat_app:password@localhost:5432/mentat

# Verify
pg_restore --list mentat_backup.dump | head -20
```

**Restore:**

```bash
# To a fresh database
createdb mentat_restored
psql -d mentat_restored -c "CREATE EXTENSION pg_mentat;"
pg_restore -d mentat_restored --data-only --schema=mentat mentat_backup.dump

# Reset sequences after data-only restore
psql -d mentat_restored <<'SQL'
SELECT setval('mentat.partition_db_seq',
  GREATEST(100, (SELECT COALESCE(MAX(entid), 0) + 1
                 FROM mentat.schema WHERE entid < 10000)));
SELECT setval('mentat.partition_user_seq',
  GREATEST(10000, (SELECT COALESCE(MAX(e), 0) + 1
                   FROM mentat.datoms WHERE e >= 10000 AND e < 1000000)));
SELECT setval('mentat.partition_tx_seq',
  GREATEST(1000001, (SELECT COALESCE(MAX(tx), 0) + 1
                     FROM mentat.transactions)));
SQL

# Rebuild statistics
psql -d mentat_restored -c "ANALYZE;"
```

### Physical Backup (pg_basebackup)

Best for: large databases, point-in-time recovery, minimal downtime.

```bash
# Prerequisite: WAL archiving enabled
# postgresql.conf:
#   wal_level = replica
#   archive_mode = on
#   archive_command = 'test ! -f /backup/wal/%f && cp %p /backup/wal/%f'

pg_basebackup -D /backup/base/$(date +%Y%m%d) \
  -Ft -z -Xs -P \
  -h localhost -U replication_user
```

### Point-in-Time Recovery (PITR)

```bash
# 1. Stop PostgreSQL
systemctl stop postgresql

# 2. Move damaged data directory
mv /var/lib/postgresql/16/main /var/lib/postgresql/16/main.damaged

# 3. Restore base backup
tar xzf /backup/base/20260420/base.tar.gz -C /var/lib/postgresql/16/main

# 4. Configure recovery target
cat >> /var/lib/postgresql/16/main/postgresql.auto.conf <<EOF
restore_command = 'cp /backup/wal/%f %p'
recovery_target_time = '2026-04-28 14:30:00 UTC'
recovery_target_action = 'promote'
EOF

# 5. Signal recovery
touch /var/lib/postgresql/16/main/recovery.signal

# 6. Start PostgreSQL
systemctl start postgresql

# 7. Verify and restart mentatd
psql -d mentat -c "SELECT count(*) FROM mentat.datoms;"
systemctl start mentatd
```

### Backup Schedule

| Method           | Frequency  | Retention | RPO           |
|------------------|------------|-----------|---------------|
| pg_dump          | Daily      | 30 days   | Up to 24h     |
| pg_basebackup    | Weekly     | 4 weeks   | WAL interval  |
| WAL archiving    | Continuous | 7 days    | ~5 minutes    |

### Multi-Store Backups

If using multiple stores (each in its own `mentat_<name>` schema), include all schemas:

```bash
pg_dump -Fc -f full_backup.dump \
  --schema='mentat*' \
  postgresql://localhost/mentat
```

---

## Monitoring

### Health Check

```bash
# mentatd health endpoint (no auth required)
curl http://localhost:8080/health
# Expected: "mentatd ready"
```

### Extension-Level Stats

```sql
-- Query performance statistics
SELECT mentat_query_stats();

-- Storage usage statistics
SELECT mentat_storage_stats();
```

### Prometheus Metrics (mentatd)

Scrape the `/metrics` endpoint:

```yaml
scrape_configs:
  - job_name: mentatd
    static_configs:
      - targets: ['mentatd-host:8080']
    metrics_path: /metrics
    scrape_interval: 15s
```

Key metrics:

| Metric                                    | Type      | Description                          |
|-------------------------------------------|-----------|--------------------------------------|
| `mentatd_requests_total`                  | Counter   | Total HTTP requests                  |
| `mentatd_errors_total`                    | Counter   | Total errors                         |
| `mentatd_query_duration_seconds`          | Histogram | Query execution duration             |
| `mentatd_transaction_duration_seconds`    | Histogram | Transaction execution duration       |
| `mentatd_cache_hit_rate`                  | Gauge     | Cache hit rate (0.0 -- 1.0)          |
| `mentatd_connection_pool_available`       | Gauge     | Idle connections available           |

### PostgreSQL Monitoring Queries

```sql
-- Table sizes
SELECT
    relname AS table_name,
    pg_size_pretty(pg_total_relation_size(c.oid)) AS total_size,
    n_live_tup AS live_rows,
    n_dead_tup AS dead_rows,
    last_autovacuum
FROM pg_stat_user_tables s
JOIN pg_class c ON s.relid = c.oid
WHERE schemaname = 'mentat'
ORDER BY pg_total_relation_size(c.oid) DESC;

-- Index hit rate (target: > 99%)
SELECT
    indexrelname,
    idx_scan,
    pg_size_pretty(pg_relation_size(indexrelid)) AS size
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
ORDER BY idx_scan DESC;

-- Long-running queries
SELECT pid, now() - query_start AS duration, query
FROM pg_stat_activity
WHERE datname = current_database()
  AND state = 'active'
  AND now() - query_start > interval '5 seconds'
ORDER BY duration DESC;
```

### Alerting Rules (Prometheus)

```yaml
groups:
  - name: pg_mentat
    rules:
      - alert: MentatdHighErrorRate
        expr: rate(mentatd_errors_total[5m]) / rate(mentatd_requests_total[5m]) > 0.01
        for: 5m
        labels:
          severity: warning

      - alert: MentatdHighQueryLatency
        expr: histogram_quantile(0.95, rate(mentatd_query_duration_seconds_bucket[5m])) > 0.1
        for: 5m
        labels:
          severity: warning

      - alert: MentatdPoolExhausted
        expr: mentatd_connection_pool_available < 2
        for: 1m
        labels:
          severity: critical

      - alert: MentatdDown
        expr: up{job="mentatd"} == 0
        for: 1m
        labels:
          severity: critical
```

---

## Security Hardening

### PostgreSQL Roles

Create least-privilege roles:

```sql
-- Read-only role
CREATE ROLE mentat_reader LOGIN PASSWORD 'strong-password';
GRANT USAGE ON SCHEMA mentat TO mentat_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA mentat TO mentat_reader;
GRANT EXECUTE ON FUNCTION mentat_query TO mentat_reader;
GRANT EXECUTE ON FUNCTION mentat_pull TO mentat_reader;
GRANT EXECUTE ON FUNCTION mentat_entity TO mentat_reader;
GRANT EXECUTE ON FUNCTION mentat_schema TO mentat_reader;

-- Read-write role (for applications)
CREATE ROLE mentat_app LOGIN PASSWORD 'strong-password';
GRANT USAGE ON SCHEMA mentat TO mentat_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA mentat TO mentat_app;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA mentat TO mentat_app;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA mentat TO mentat_app;

-- Admin role (for schema changes)
CREATE ROLE mentat_admin LOGIN PASSWORD 'strong-password';
GRANT ALL ON SCHEMA mentat TO mentat_admin;
```

### Network Security

Restrict access in `pg_hba.conf`:

```
# Only allow application connections from known subnets
hostssl mentat mentat_app  10.0.0.0/24  scram-sha-256
hostssl mentat mentat_reader 10.0.0.0/24 scram-sha-256

# Block all other connections
host    all    all          0.0.0.0/0   reject
```

### mentatd Authentication

Enable Bearer token authentication:

```bash
MENTATD_API_KEY="your-secret-api-key" mentatd
```

### TLS

mentatd does not terminate TLS. Use a reverse proxy:

```nginx
server {
    listen 443 ssl;
    server_name mentatd.example.com;
    ssl_certificate     /etc/ssl/mentatd.crt;
    ssl_certificate_key /etc/ssl/mentatd.key;
    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

### Statement Timeout

Prevent runaway queries from consuming resources:

```sql
ALTER ROLE mentat_app SET statement_timeout = '30s';
```

### Request Size Limit

mentatd enforces a 16 MiB maximum request body. For larger transactions, split into smaller batches.

---

## Upgrade Procedures

### Extension Upgrade

1. Build the new version:
   ```bash
   cd pg_mentat
   cargo pgrx install --release --pg-config=$(which pg_config)
   ```

2. Upgrade in PostgreSQL:
   ```sql
   ALTER EXTENSION pg_mentat UPDATE;
   ```

3. Verify:
   ```sql
   SELECT extversion FROM pg_extension WHERE extname = 'pg_mentat';
   SELECT mentat_schema();
   ```

### mentatd Upgrade

mentatd is stateless. Rolling upgrades work with zero downtime when running multiple instances behind a load balancer:

1. Deploy the new binary to one instance.
2. Restart that instance.
3. Verify health: `curl http://instance:8080/health`
4. Repeat for remaining instances.

### PostgreSQL Major Version Upgrade

pg_mentat must be recompiled for the target PostgreSQL major version:

1. Build the extension against the new version:
   ```bash
   cargo pgrx install --release --features pg17 --pg-config=/usr/lib/postgresql/17/bin/pg_config
   ```

2. Perform the PostgreSQL upgrade using `pg_upgrade`:
   ```bash
   pg_upgrade \
     --old-datadir=/var/lib/postgresql/16/main \
     --new-datadir=/var/lib/postgresql/17/main \
     --old-bindir=/usr/lib/postgresql/16/bin \
     --new-bindir=/usr/lib/postgresql/17/bin
   ```

3. Re-enable the extension if needed:
   ```sql
   DROP EXTENSION pg_mentat;
   CREATE EXTENSION pg_mentat;
   -- Restore data from backup if needed
   ```

---

## Pre-Production Checklist

### Infrastructure

- [ ] PostgreSQL version 13-18 installed on SSD-backed storage
- [ ] `postgresql.conf` tuned per the guidelines above
- [ ] WAL archiving configured for PITR
- [ ] Streaming replication configured (if HA required)
- [ ] Backup schedule established and tested
- [ ] Monitoring and alerting configured

### pg_mentat Extension

- [ ] Extension installed and `CREATE EXTENSION pg_mentat` succeeds
- [ ] `mentat_schema()` returns expected bootstrap attributes
- [ ] Test transaction succeeds: `SELECT mentat_transact('[{:db/ident :test/attr :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]');`
- [ ] Test query succeeds: `SELECT mentat_query('[:find ?e :where [?e :db/ident]]', '{}');`

### Security

- [ ] Dedicated PostgreSQL roles created with least-privilege access
- [ ] `pg_hba.conf` restricts connections to known subnets
- [ ] scram-sha-256 authentication enabled
- [ ] TLS configured for all external connections
- [ ] Statement timeout set on application roles
- [ ] mentatd API key configured (if using mentatd)

### mentatd (If Used)

- [ ] Connection pool sized appropriately
- [ ] systemd service unit installed with security hardening
- [ ] Health check responding at `/health`
- [ ] Prometheus metrics accessible at `/metrics`
- [ ] Log format set to `json` for structured log ingestion

### Backup Verification

- [ ] Logical backup (pg_dump) tested with successful restore
- [ ] Physical backup (pg_basebackup) tested
- [ ] PITR tested to a specific point in time
- [ ] Sequence values correct after restore

### Known Limitations

Before deploying, understand these current limitations:

- **Dataset size**: Tested up to 10M datoms. Beyond that, the UNION ALL query strategy may degrade performance. Schema-aware query translation (Task #1) will address this.
- **Concurrent writes**: No automatic retry for serialization failures. Clients should implement retry logic with exponential backoff.
- **Multi-store isolation**: Store-level row security (RLS) is not yet implemented.
- **Incomplete Datomic compatibility**: Some features are not yet implemented: `with` (speculative transactions), transaction functions, full-text search in Datalog. See `EXPERT_REVIEW.md` for a detailed assessment.
