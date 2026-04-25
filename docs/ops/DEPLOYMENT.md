# pg_mentat Deployment Guide

## Architecture Overview

pg_mentat consists of two components:

1. **pg_mentat** -- A PostgreSQL extension (shared library) that provides the Mentat Datalog
   database schema, types, functions, and query planner hooks inside PostgreSQL.
2. **mentatd** -- A standalone HTTP server that exposes a Datomic-compatible client API,
   translating Datomic wire-protocol requests into SQL executed against the pg_mentat schema.

Both components can be deployed independently. The extension is required; mentatd is optional
if you access the database directly via SQL.

## Prerequisites

| Dependency       | Version          | Notes                                    |
|------------------|------------------|------------------------------------------|
| PostgreSQL       | 13 -- 18         | pg16 is the default and best tested      |
| Rust toolchain   | >= 1.88          | 1.90.0 recommended (matches CI)          |
| cargo-pgrx       | ~0.17            | Must match the pgrx crate version        |
| LLVM/Clang       | 18.x             | Required by pgrx bindgen                 |
| OpenSSL          | >= 1.1           | Required for TLS in mentatd              |

## Installation

### From Source (Nix)

The repository includes a `flake.nix` for reproducible builds:

```bash
# Enter the development shell (installs all deps)
nix develop

# First-time setup: install and initialize cargo-pgrx
setup-pgrx

# Build the extension
build-extension

# Install to local PostgreSQL
install-extension
```

### From Source (Manual)

```bash
# Install cargo-pgrx
cargo install --locked cargo-pgrx --version '~0.17'

# Initialize pgrx with your PostgreSQL installation
cargo pgrx init --pg16=$(which pg_config)

# Build and install the extension
cd pg_mentat
cargo pgrx install --release --pg-config=$(which pg_config)
```

### From Source (mentatd)

```bash
cd mentatd
cargo build --release
# Binary is at target/release/mentatd
```

### Docker

```bash
# Build the image (includes PostgreSQL 16 + pg_mentat extension + demo data)
docker build -t pg_mentat .

# Run
docker run -d --name pg_mentat \
  -p 5432:5432 \
  -e POSTGRES_PASSWORD=secret \
  pg_mentat

# Connect
psql -h localhost -U postgres
```

For production, override the default trust authentication:

```bash
docker run -d --name pg_mentat \
  -p 5432:5432 \
  -e POSTGRES_PASSWORD=<strong-password> \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  -v pgdata:/var/lib/postgresql/data \
  pg_mentat
```

### Kubernetes (Helm)

```bash
# Install the chart
helm install pg-mentat ./helm/pg-mentat \
  --set postgresql.auth.password=<strong-password> \
  --set mentatd.config.logFormat=json

# Or with custom values
helm install pg-mentat ./helm/pg-mentat -f my-values.yaml
```

Key Helm values:

| Value                           | Default        | Description                       |
|---------------------------------|----------------|-----------------------------------|
| `mentatd.replicaCount`          | 2              | Number of mentatd pods            |
| `mentatd.pool.size`             | 20             | Connection pool size per pod      |
| `mentatd.cache.capacity`        | 5000           | Query cache entries               |
| `mentatd.cache.ttlSecs`         | 300            | Cache TTL in seconds              |
| `postgresql.auth.password`      | mentat         | **Change in production**          |
| `postgresql.persistence.size`   | 10Gi           | PV size for PostgreSQL data       |
| `autoscaling.enabled`           | true           | Enable HPA                        |
| `autoscaling.minReplicas`       | 2              | Minimum mentatd pods              |
| `autoscaling.maxReplicas`       | 10             | Maximum mentatd pods              |
| `podDisruptionBudget.enabled`   | true           | Ensure availability during updates|

### Kubernetes (Raw Manifests)

Raw Kubernetes manifests are in the `k8s/` directory:

```bash
kubectl apply -f k8s/namespace.yaml
kubectl apply -f k8s/secret.yaml      # Edit first: set credentials
kubectl apply -f k8s/configmap.yaml
kubectl apply -f k8s/statefulset.yaml  # PostgreSQL
kubectl apply -f k8s/deployment.yaml   # mentatd
kubectl apply -f k8s/service.yaml
kubectl apply -f k8s/hpa.yaml
kubectl apply -f k8s/pdb.yaml
```

## Enabling the Extension

After installation, enable the extension in your database:

```sql
-- Create the extension (creates mentat schema, tables, indexes, bootstrap data)
CREATE EXTENSION pg_mentat;

-- Verify
SELECT * FROM mentat.partitions;
```

The extension automatically creates:
- The `mentat` schema
- Core tables: `datoms`, `schema`, `transactions`, `partitions`, `idents`, `fulltext`
- All EAVT/AEVT/VAET/AVET indexes
- Bootstrap schema attributes (entids 10-92)
- Lock-free sequences for entity ID allocation

## Configuration

### PostgreSQL Settings

Add to `postgresql.conf` or set via `ALTER SYSTEM`:

```ini
# Required if using planner hooks (future feature)
# shared_preload_libraries = 'pg_mentat'

# Recommended for mentat workloads
shared_buffers = '256MB'              # 25% of RAM for dedicated servers
effective_cache_size = '768MB'        # 75% of RAM
work_mem = '64MB'                     # Per-sort/hash operation
maintenance_work_mem = '256MB'        # For VACUUM, CREATE INDEX

# WAL settings for write-heavy workloads
wal_level = replica                   # Required for PITR
max_wal_size = '2GB'
min_wal_size = '256MB'
checkpoint_completion_target = 0.9

# Connection limits
max_connections = 200                 # Must exceed mentatd pool_size
```

### mentatd Configuration

mentatd can be configured via a TOML file or environment variables.

**TOML file** (`mentatd.toml`):

```toml
[server]
host = "0.0.0.0"
port = 8080
timeout = 30
# api_key = "your-secret-key-here"   # Uncomment to enable auth

[database]
connection_string = "postgresql://mentat:password@localhost:5432/mentat"
pool_size = 100
max_lifetime_secs = 1800

[logging]
level = "info"
format = "json"       # "compact", "pretty", or "json"

[cache]
enabled = true
capacity = 1000
ttl_secs = 300
```

**Environment variables** (override TOML and defaults):

| Variable                  | Default                              | Description                     |
|---------------------------|--------------------------------------|---------------------------------|
| `MENTATD_CONFIG`          | `mentatd.toml`                       | Path to config file             |
| `MENTATD_HOST`            | `127.0.0.1`                          | Bind address                    |
| `MENTATD_PORT`            | `8080`                               | Listen port                     |
| `MENTATD_TIMEOUT`         | `30`                                 | Request timeout (seconds)       |
| `MENTATD_API_KEY`         | *(none)*                             | Bearer token for auth           |
| `DATABASE_URL`            | `postgresql://localhost/mentat`       | PostgreSQL connection string    |
| `DATABASE_POOL_SIZE`      | `100`                                | Max pool connections            |
| `DATABASE_MAX_LIFETIME`   | `1800`                               | Connection max age (seconds)    |
| `RUST_LOG`                | `info`                               | Log level (trace/debug/info/warn/error) |
| `LOG_FORMAT`              | `compact`                            | Log format                      |
| `MENTATD_CACHE_ENABLED`   | `true`                               | Enable query cache              |
| `MENTATD_CACHE_CAPACITY`  | `1000`                               | Max cache entries               |
| `MENTATD_CACHE_TTL`       | `300`                                | Cache TTL (seconds)             |

### Running mentatd

```bash
# With environment variables
DATABASE_URL="postgresql://mentat:password@localhost:5432/mentat" \
  MENTATD_HOST="0.0.0.0" \
  mentatd

# With config file
MENTATD_CONFIG=/etc/mentatd/mentatd.toml mentatd

# With systemd (example unit)
# See Security section below for a full systemd unit
```

### Verifying the Deployment

```bash
# Health check
curl http://localhost:8080/health
# Expected: "mentatd ready"

# Metrics (Prometheus format)
curl http://localhost:8080/metrics

# Test a query (Datomic client wire protocol)
curl -X POST http://localhost:8080/ \
  -H "Content-Type: application/edn" \
  -d '{:op :query :query "[:find ?e :where [?e :db/ident]]"}'
```

## Security

### Authentication

mentatd supports Bearer token authentication. Set the `MENTATD_API_KEY` environment variable
or `server.api_key` in the TOML config:

```bash
MENTATD_API_KEY="your-secret-api-key" mentatd
```

Clients must include the key in every request:

```bash
curl -X POST http://localhost:8080/ \
  -H "Authorization: Bearer your-secret-api-key" \
  -H "Content-Type: application/edn" \
  -d '{:op :query ...}'
```

The `/health` and `/metrics` endpoints are **not** protected by authentication.

### TLS Configuration

mentatd does not natively terminate TLS. Use a reverse proxy (nginx, Caddy, HAProxy, or
a Kubernetes Ingress controller) to provide TLS termination:

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

### PostgreSQL Security

- Use `scram-sha-256` authentication (default in PostgreSQL 14+).
- Create a dedicated database user for mentatd with minimal privileges:

```sql
CREATE ROLE mentat_app LOGIN PASSWORD 'strong-password';
GRANT USAGE ON SCHEMA mentat TO mentat_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA mentat TO mentat_app;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA mentat TO mentat_app;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA mentat TO mentat_app;
```

- Restrict network access via `pg_hba.conf`:

```
# Only allow mentatd from the application subnet
hostssl mentat mentat_app 10.0.0.0/24 scram-sha-256
```

### systemd Hardening

Example systemd unit with security restrictions:

```ini
[Unit]
Description=mentatd - Datomic-compatible HTTP server for Mentat
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

### Request Size Limits

mentatd enforces a 16 MiB maximum request body size to prevent denial-of-service via
oversized payloads. This is not configurable; if you need larger transactions, break them
into smaller batches.

## Capacity Planning

### Connection Pool Sizing

The mentatd connection pool should be sized based on expected concurrency:

| Workload          | Recommended `pool_size` | Notes                              |
|-------------------|-------------------------|------------------------------------|
| Light (< 10 RPS)  | 10-20                   | Default is fine                    |
| Medium (10-100 RPS)| 50-100                  | Match to max concurrent queries    |
| Heavy (> 100 RPS) | 100-200                 | Ensure `max_connections` allows it |

**Important**: `pool_size` must be less than PostgreSQL's `max_connections` minus connections
reserved for superusers and other applications.

### Storage Estimates

The `datoms` table is the primary storage consumer. Each datom row is approximately
100-200 bytes depending on value types. Estimate:

```
storage = num_entities * avg_attributes_per_entity * 150 bytes * 2 (indexes)
```

For 1 million entities with 10 attributes each: ~3 GB data + ~3 GB indexes.
