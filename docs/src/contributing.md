# Contributing

## Development Environment

### Prerequisites

- Rust 1.88+ (stable)
- PostgreSQL 16 development headers (or 13-18)
- LLVM 18 / Clang (for pgrx bindgen)
- cargo-pgrx 0.17.0

### Nix (Recommended)

The project includes a Nix flake that provides the complete development environment:

```bash
nix develop
```

This gives you Rust 1.90, PostgreSQL 16, LLVM 18, cargo-pgrx, and all required build dependencies.

### Manual Setup

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable

# Install cargo-pgrx
cargo install cargo-pgrx --version 0.17.0

# Initialize pgrx (downloads and builds PostgreSQL)
cargo pgrx init --pg16 $(which pg_config)
```

## Building

```bash
# Debug build (fast compilation, assertions enabled)
cargo pgrx install --features pg16

# Release build (optimized, LTO)
cargo pgrx install --release --features pg16

# Build without installing
cargo build --features pg16
```

## Testing

### Unit Tests

The project uses pgrx's test infrastructure, which spins up a temporary PostgreSQL instance:

```bash
# Run all tests
cargo pgrx test pg16

# Run tests in a specific file
cargo pgrx test pg16 -- --test query_tests

# Run a single test
cargo pgrx test pg16 -- --test query_tests::test_basic_query
```

### Running Interactively

Start a PostgreSQL instance with the extension pre-loaded:

```bash
cargo pgrx run pg16
```

This drops you into a `psql` session where you can interactively test:

```sql
CREATE EXTENSION pg_mentat;
SELECT mentat_transact('[{:db/ident :test/attr :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]');
```

## Project Structure

```
pg_mentat/
  Cargo.toml              # Workspace root
  edn/                    # EDN parser (PEG grammar)
    src/lib.rs            # Parsing rules for EDN, queries, transactions
  core-traits/            # Shared type definitions
  core/                   # Core data structures
  pg_mentat/              # PostgreSQL extension (main crate)
    src/
      lib.rs              # Extension entry point, _PG_init, SQL schema DDL
      functions/          # SQL-callable functions
        query.rs          # Datalog-to-SQL compiler
        transact.rs       # Transaction processor
        pull.rs           # Pull API implementation
        schema.rs         # Schema introspection
        time_travel.rs    # As-of, since, history
        excision.rs       # Entity excision
        store_management.rs  # Multi-store management
        subscriptions.rs  # LISTEN/NOTIFY subscriptions
        stats.rs          # Performance statistics
        edn_functions.rs  # EDN helper functions
        bootstrap.rs      # Schema bootstrap
      planner/
        hooks.rs          # GUC registration, optimizer hints
      cache.rs            # Schema cache (LRU, generation-based invalidation)
      monitoring.rs       # Slow query logging, metrics
    pg_mentat.control     # Extension metadata
  mentatd/                # HTTP server
    src/
      main.rs             # Entry point
      server.rs           # Axum routes, request handling
      websocket.rs        # WebSocket support
      stream.rs           # Streaming query responses
      db_cache.rs         # Database snapshot cache
      session.rs          # Client session management
```

## Code Style

The project enforces strict Clippy lints:

- **No panics** -- `unwrap_used = "deny"`, `panic = "deny"`, `unimplemented = "deny"`
- **No debug output** -- `dbg_macro = "deny"`, `print_stdout = "deny"`
- **No TODOs** -- `todo = "deny"`
- **Pedantic warnings** -- `pedantic = "warn"` with minimal relaxations

Run lints locally:

```bash
cargo clippy --features pg16 -- -D warnings
```

Format code:

```bash
cargo fmt --all
```

## Architecture Decisions

### Why Narrow Tables?

Each value type gets its own table so PostgreSQL stores values in their native format. This means:
- No type tags or BYTEA serialization overhead
- Index comparisons use native operators (integer comparison for longs, text collation for strings)
- The query planner can estimate selectivity accurately
- Smaller indexes (no wasted space on a universal value column)

### Why SPI?

The query compiler generates SQL and executes it through PostgreSQL's SPI (Server Programming Interface) rather than implementing a custom executor. This means:
- Queries benefit from PostgreSQL's optimizer, statistics, and parallel execution
- No need to reimplement join algorithms, aggregation, or sorting
- Index usage decisions are delegated to the planner

### Why pgrx?

pgrx provides safe Rust bindings to PostgreSQL's C API. It handles:
- Memory context management (palloc/pfree)
- Error handling (longjmp safety)
- SPI lifecycle
- GUC registration
- Test infrastructure

## Submitting Changes

1. Fork the repository
2. Create a feature branch from `claude` (the main development branch)
3. Write tests for new functionality
4. Ensure all tests pass: `cargo pgrx test pg16`
5. Ensure Clippy is clean: `cargo clippy --features pg16 -- -D warnings`
6. Submit a pull request with a clear description

## License

pg_mentat is licensed under Apache-2.0. By contributing, you agree that your contributions will be licensed under the same terms.
