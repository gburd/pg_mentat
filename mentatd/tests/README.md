# mentatd Integration Tests

This directory contains integration tests for the mentatd server that verify Datomic protocol compatibility.

## Test Structure

- `integration_test.rs` - Main integration test suite
- `helpers.rs` - Test server and client utilities
- `datomic_client/` - Scripts for testing with actual Datomic client (optional)

## Running Tests

### Prerequisites

1. PostgreSQL must be running and accessible
2. Set the `DATABASE_URL` environment variable (or tests will use default: `postgresql://localhost/mentat`)

```bash
export DATABASE_URL="postgresql://localhost:5432/mentat"
```

### Run All Integration Tests

```bash
cargo test -p mentatd
```

### Run Specific Test

```bash
cargo test -p mentatd test_connect_operation
```

### Run with Logging

```bash
RUST_LOG=debug cargo test -p mentatd -- --nocapture
```

## Test Coverage

The integration test suite covers:

### Protocol Operations

1. **Health Check** - `/health` endpoint verification
2. **Connect** - Database connection with connection-id generation
3. **List Databases** - Enumerate available databases
4. **Query (q)** - Datomic query execution with args, timeout, limit, offset
5. **Transact** - Transaction submission and response
6. **Db** - Database handle operations
7. **Create/Delete Database** - Database lifecycle management

### Error Handling

- Invalid operation names
- Missing required fields
- Invalid EDN format
- Invalid UUIDs
- Nonexistent databases
- Invalid database names
- Empty and whitespace-only requests

### Protocol Compliance

- EDN request/response format validation
- Content-Type header verification (`application/edn`)
- Cognitect anomalies error format
- Datomic namespace support (`:datomic.catalog/list-dbs`)

### Concurrency

- Multiple concurrent requests
- Connection pooling verification

## Test Server

The `TestServer` helper:
- Starts mentatd on a random available port
- Uses actual PostgreSQL connection pool
- Graceful shutdown on test completion
- Isolated per-test instance

## CI Integration

These tests are designed to run in CI environments:
- Fast startup and teardown
- No external dependencies beyond PostgreSQL
- Deterministic and repeatable
- Clear error messages

## Datomic Client Testing

For testing with an actual Datomic client, see `datomic_client/README.md`.

## Troubleshooting

### Tests fail with "connection refused"

Ensure PostgreSQL is running:
```bash
psql $DATABASE_URL -c "SELECT version();"
```

### Tests timeout

Increase timeout or check PostgreSQL connection:
```bash
RUST_LOG=debug cargo test -p mentatd -- --nocapture
```

### Database creation tests fail

Ensure your PostgreSQL user has `CREATEDB` privilege:
```sql
ALTER USER your_user CREATEDB;
```
