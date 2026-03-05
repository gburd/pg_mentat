# mentatd

Datomic-compatible HTTP server for Mentat backed by PostgreSQL.

## Overview

`mentatd` implements the Datomic wire protocol as an HTTP server, translating Datomic client requests into PostgreSQL queries via the `pg_mentat` extension. This allows existing Datomic clients to connect to a PostgreSQL-backed Mentat database.

## Architecture

```
Datomic Client → mentatd (HTTP/EDN) → PostgreSQL (pg_mentat extension)
```

## Configuration

Copy `mentatd.toml.example` to `mentatd.toml` and edit:

```toml
[server]
host = "127.0.0.1"
port = 8080
timeout = 30

[database]
connection_string = "postgresql://postgres:postgres@localhost:5432/mentat"
pool_size = 10
max_lifetime_secs = 1800

[logging]
level = "info"
format = "compact"
```

Alternatively, use environment variables:

- `MENTATD_HOST` - Server bind address (default: 127.0.0.1)
- `MENTATD_PORT` - Server port (default: 8080)
- `MENTATD_TIMEOUT` - Request timeout in seconds (default: 30)
- `DATABASE_URL` - PostgreSQL connection string
- `DATABASE_POOL_SIZE` - Connection pool size (default: 10)
- `DATABASE_MAX_LIFETIME` - Max connection lifetime in seconds (default: 1800)
- `RUST_LOG` - Log level (default: info)
- `LOG_FORMAT` - Log format: compact, pretty, or json (default: compact)

## Running

```bash
cargo build -p mentatd
cargo run -p mentatd
```

The server will start on http://127.0.0.1:8080 by default.

Health check:
```bash
curl http://127.0.0.1:8080/health
```

## Supported Operations

### Phase 1 (Implemented)

- `health` - Health check
- `list-dbs` - List available databases
- `create-db` - Create new database
- `delete-db` - Delete database
- `connect` - Connect to database
- `db` - Get database value
- `q` - Query (basic implementation)
- `transact` - Transaction (basic implementation)

### Phase 2 (Planned)

- `pull` - Pull entity data
- `datoms` - Index access
- `with` - Speculative transactions
- `as-of` - Time travel
- `since` - Time travel
- `tx-range` - Transaction log

## Protocol

Requests use EDN format:

```edn
{:op :connect
 :args {:db-name "my-database"}}
```

Responses:

```edn
{:result {:connection-id "550e8400-e29b-41d4-a716-446655440000"
          :db-name "my-database"
          :status "connected"}}
```

Errors:

```edn
{:error {:cognitect.anomalies/category :cognitect.anomalies/not-found
         :cognitect.anomalies/message "Database 'missing' not found"
         :db/error :db.error/not-found}}
```

## Development

Build:
```bash
cargo build -p mentatd
```

Test:
```bash
# Set PostgreSQL connection
export DATABASE_URL="postgresql://localhost:5432/mentat"

# Run integration tests
cargo test -p mentatd

# Run with logging
RUST_LOG=debug cargo test -p mentatd -- --nocapture
```

See [tests/README.md](tests/README.md) for detailed test documentation.

Lint:
```bash
cargo clippy -p mentatd
```

## Testing

The test suite includes:

### Integration Tests (Rust)
- 22 comprehensive tests covering all protocol operations
- HTTP server end-to-end testing
- EDN protocol validation
- Error handling verification
- Concurrency testing

### Protocol Compatibility Tests
- Shell script tests simulating Datomic client requests
- 15 protocol operation tests
- No external dependencies (uses curl)

### Datomic Client Tests (Optional)
- Clojure tests using actual Datomic Peer API
- Requires Datomic Free/Pro JAR
- Tests schema, queries, transactions, time-travel features

See [tests/TEST_RESULTS.md](tests/TEST_RESULTS.md) for complete test documentation and results.

## See Also

- [Datomic Protocol Specification](/docs/architecture/datomic_protocol.md)
- [pg_mentat Extension](/pg_mentat/README.md)
