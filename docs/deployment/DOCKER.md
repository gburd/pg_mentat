# Docker Deployment

Run the full pg_mentat stack (PostgreSQL + mentatd HTTP server) with a single command.

## Prerequisites

- Docker Engine 20.10+ (or Podman with docker-compose compatibility)
- Docker Compose v2+

## Quick Start

From the repository root:

```bash
# Start everything (builds images on first run)
docker compose -f docker/docker-compose.yml up -d

# Verify both services are healthy
docker compose -f docker/docker-compose.yml ps

# Connect via psql
psql -h localhost -p 5432 -U postgres -d mentat

# Query the mentatd HTTP API
curl http://localhost:8080/health
```

## Architecture

```
                        +------------------+
  psql / SQL clients -->|  PostgreSQL 16   |
         port 5432      |  + pg_mentat ext |
                        +--------+---------+
                                 |
                        +--------+---------+
  HTTP / Datomic API -->|     mentatd      |
         port 8080      |  (Rust daemon)   |
                        +------------------+
```

- **postgres** -- PostgreSQL 16 with the `pg_mentat` Datalog extension pre-installed. On first start the `demo.sql` script creates the extension, bootstraps schema tables, and loads sample data.
- **mentatd** -- Datomic-compatible HTTP server that connects to PostgreSQL and exposes a REST/EDN API for queries and transactions.

## Configuration

Copy the example environment file and edit as needed:

```bash
cp docker/.env.example docker/.env
```

Key variables:

| Variable | Default | Description |
|---|---|---|
| `POSTGRES_USER` | `postgres` | PostgreSQL superuser |
| `POSTGRES_PASSWORD` | `postgres` | Superuser password |
| `POSTGRES_DB` | `mentat` | Database name |
| `PG_PORT` | `5432` | Published host port for PostgreSQL |
| `MENTATD_PORT` | `8080` | Published host port for mentatd |
| `DATABASE_POOL_SIZE` | `10` | mentatd connection pool size |
| `RUST_LOG` | `info` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |
| `LOG_FORMAT` | `compact` | Log format (`compact`, `pretty`, `json`) |
| `MENTATD_CACHE_ENABLED` | `true` | Enable query result cache |
| `MENTATD_CACHE_CAPACITY` | `1000` | Maximum cached query results |
| `MENTATD_CACHE_TTL` | `300` | Cache entry TTL in seconds |

Resource limits (`PG_CPU_LIMIT`, `PG_MEM_LIMIT`, `MENTATD_CPU_LIMIT`, `MENTATD_MEM_LIMIT`) can be tuned for your host.

## Building Images Individually

```bash
# pg_mentat PostgreSQL image only
docker build -f docker/Dockerfile.pg_mentat -t pg_mentat .

# mentatd daemon only
docker build -f docker/Dockerfile.mentatd -t mentatd .
```

## Managing the Stack

```bash
# View logs (follow)
docker compose -f docker/docker-compose.yml logs -f

# View logs for a single service
docker compose -f docker/docker-compose.yml logs -f mentatd

# Restart a service
docker compose -f docker/docker-compose.yml restart mentatd

# Stop everything (data persists in the pgdata volume)
docker compose -f docker/docker-compose.yml down

# Stop and remove all data (destructive)
docker compose -f docker/docker-compose.yml down -v
```

## Persistent Data

PostgreSQL data is stored in a named Docker volume (`pgdata`). Stopping or recreating containers does not delete data. To wipe and start fresh:

```bash
docker compose -f docker/docker-compose.yml down -v
docker compose -f docker/docker-compose.yml up -d
```

## Health Checks

Both services include health checks:

- **postgres** -- `pg_isready` every 10 seconds with a 30-second start-up grace period.
- **mentatd** -- HTTP `GET /health` every 10 seconds. The service only starts after PostgreSQL passes its health check.

Check status:

```bash
docker inspect --format='{{.State.Health.Status}}' pg_mentat_postgres
docker inspect --format='{{.State.Health.Status}}' pg_mentat_mentatd
```

## Using with an Existing PostgreSQL Instance

If you already have PostgreSQL running and only need the mentatd daemon:

```bash
docker build -f docker/Dockerfile.mentatd -t mentatd .

docker run -d --name mentatd \
  -p 8080:8080 \
  -e DATABASE_URL="postgresql://user:pass@host:5432/mentat" \
  -e MENTATD_HOST=0.0.0.0 \
  mentatd
```

## Troubleshooting

**Build fails during `cargo pgrx install`**

The pg_mentat extension build requires significant memory. Ensure Docker has at least 4 GB of memory available (check Docker Desktop settings on macOS/Windows).

**mentatd cannot connect to PostgreSQL**

Ensure the postgres service is healthy before mentatd starts. The compose file uses `depends_on: condition: service_healthy` to enforce this. If you see connection errors, check that `POSTGRES_PASSWORD` matches between the two services.

**Port conflicts**

If ports 5432 or 8080 are already in use on the host, set `PG_PORT` and `MENTATD_PORT` in your `.env` file.
