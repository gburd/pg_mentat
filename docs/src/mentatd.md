# mentatd HTTP Server

`mentatd` is a standalone HTTP server that provides a Datomic client-compatible API on top of pg_mentat. It connects to PostgreSQL (with pg_mentat installed), translates HTTP requests into SQL function calls, and returns results in EDN or Transit+JSON format.

## Architecture

```
Client (Datomic SDK / HTTP) --> mentatd (Axum) --> PostgreSQL (pg_mentat extension)
```

mentatd is built with:
- **Axum** -- async HTTP framework
- **Tokio** -- async runtime
- **deadpool-postgres** -- connection pooling
- **tower-http** -- CORS, tracing, timeouts
- **Prometheus** -- metrics collection

## Running mentatd

### From Source

```bash
cd mentatd
cargo run -- --config config.toml
```

### With Docker Compose

```bash
cd docker
docker compose up -d
```

This starts PostgreSQL with pg_mentat, mentatd, Prometheus, and Grafana.

### Environment Variables

mentatd reads configuration from a TOML file. The path defaults to `config.toml` or can be specified with `--config`.

## Configuration

```toml
[server]
host = "0.0.0.0"
port = 8080
api_key = "your-secret-key"  # Optional; omit for no auth

[database]
host = "localhost"
port = 5432
dbname = "postgres"
user = "postgres"
password = "secret"
pool_size = 16

[logging]
level = "info"         # trace, debug, info, warn, error
format = "json"        # json or pretty
```

## API Endpoints

### Unified Endpoint

All operations can be dispatched through the root endpoint:

```
POST /
Content-Type: application/edn
```

The request body contains the operation type and parameters in EDN format.

### RESTful Aliases

mentatd also exposes Datomic-compatible route aliases:

| Endpoint | Operation |
|----------|-----------|
| `POST /api/query` | Execute a Datalog query |
| `POST /api/transact` | Execute a transaction |
| `POST /api/pull` | Pull entity data |
| `POST /api/list-dbs` | List available stores |
| `POST /api/create-db` | Create a new store |
| `POST /api/delete-db` | Delete a store |
| `POST /api/db-stats` | Get database statistics |
| `POST /api/datoms` | Retrieve raw datoms |
| `POST /stream/query` | Streaming query results |

### Public Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Health check (returns 200 if connected to PostgreSQL) |
| `GET /metrics` | Prometheus metrics |

### WebSocket

| Endpoint | Description |
|----------|-------------|
| `GET /ws` | WebSocket connection for real-time subscriptions |

## Request Format

### Content Types

mentatd accepts:
- `application/edn` -- EDN format (default)
- `application/transit+json` -- Transit JSON format

And returns results in the same format as the request, or as specified by the `Accept` header.

### Query Request

```edn
{:op :query
 :query "[:find ?name :where [?e :person/name ?name]]"
 :args {}
 :db-name "default"}
```

**HTTP example:**

```bash
curl -X POST http://localhost:8080/api/query \
  -H "Content-Type: application/edn" \
  -H "Authorization: Bearer your-secret-key" \
  -d '{:op :query
       :query "[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]"
       :args {}
       :db-name "default"}'
```

### Transact Request

```edn
{:op :transact
 :tx-data "[{:db/id \"t1\" :person/name \"Alice\" :person/age 30}]"
 :db-name "default"}
```

**HTTP example:**

```bash
curl -X POST http://localhost:8080/api/transact \
  -H "Content-Type: application/edn" \
  -H "Authorization: Bearer your-secret-key" \
  -d '{:op :transact
       :tx-data "[{:db/id \"t1\" :person/name \"Alice\"}]"
       :db-name "default"}'
```

### Pull Request

```edn
{:op :pull
 :pattern "[:person/name :person/age]"
 :eid 10001
 :db-name "default"}
```

### List Databases

```edn
{:op :list-dbs}
```

### Create Database

```edn
{:op :create-db
 :db-name "analytics"}
```

### Delete Database

```edn
{:op :delete-db
 :db-name "analytics"}
```

## Authentication

When `api_key` is configured in the server settings, all API endpoints (except `/health` and `/metrics`) require an `Authorization` header:

```
Authorization: Bearer your-secret-key
```

Requests without a valid key receive a `401 Unauthorized` response.

## CORS

mentatd enables CORS by default via `tower-http`, allowing cross-origin requests from browser-based clients. The default configuration permits all origins. Restrict this in production by configuring allowed origins in the TOML file.

## Streaming Queries

The `/stream/query` endpoint returns results as a stream of newline-delimited JSON objects, suitable for large result sets:

```bash
curl -X POST http://localhost:8080/stream/query \
  -H "Content-Type: application/edn" \
  -d '{:op :query
       :query "[:find ?e ?name :where [?e :person/name ?name]]"
       :db-name "default"}'
```

Results arrive incrementally rather than buffered into a single response.

## Metrics

mentatd exposes Prometheus metrics at `GET /metrics`:

- `mentatd_requests_total` -- total HTTP requests by operation and status
- `mentatd_request_duration_seconds` -- request latency histogram
- `mentatd_active_connections` -- current PostgreSQL pool connections in use
- `mentatd_pool_size` -- total pool capacity

### Grafana Dashboard

The Docker Compose setup includes a pre-configured Grafana dashboard for mentatd monitoring.

## Connection Pooling

mentatd uses `deadpool-postgres` for connection pooling. The `pool_size` configuration determines the maximum number of concurrent PostgreSQL connections. Each HTTP request acquires a connection from the pool for the duration of the operation.

Recommended pool sizing: 2-4x the number of CPU cores, or match your expected concurrent request volume.

## Production Deployment

### Reverse Proxy

Place mentatd behind nginx or a similar reverse proxy for TLS termination:

```nginx
server {
    listen 443 ssl;
    server_name mentat.example.com;

    ssl_certificate /etc/ssl/cert.pem;
    ssl_certificate_key /etc/ssl/key.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

### Health Checks

Use `/health` for load balancer health checks. It verifies PostgreSQL connectivity and returns:
- `200 OK` -- healthy
- `503 Service Unavailable` -- PostgreSQL connection failed

### Resource Limits

Configure connection pool size and PostgreSQL timeouts appropriately:

```toml
[database]
pool_size = 32  # Match expected concurrency

[server]
request_timeout_ms = 30000  # Match mentat.query_timeout_ms
```
