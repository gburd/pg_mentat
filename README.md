# Project Mentat -> pg_mentat

## PostgreSQL Migration In Progress (~90% Complete)

**Branch:** `claude`
**Last updated:** 2026-03-07

This repository is migrating Mentat from SQLite to PostgreSQL. The original
SQLite implementation remains on the `master` branch.

### What's New

**pg_mentat** -- PostgreSQL extension (via pgrx) that brings Mentat's Datalog
query capabilities to PostgreSQL. Stores datoms in a proper relational schema
with four covering indexes (EAVT, AEVT, AVET, VAET), full-text search via
tsvector, and time-travel queries.

**mentatd** -- Standalone HTTP server that speaks the Datomic wire protocol,
translating client requests to PostgreSQL queries through the pg_mentat
extension.

**Architecture:**

```
Datomic Client -> mentatd (HTTP/EDN) -> PostgreSQL (pg_mentat extension) -> Datoms
```

### Migration Status

✅ **Complete:**
- Extension schema: tables, indexes, constraints, bootstrap data
- EDN custom type: text I/O, operators, type predicates
- Extension functions: mentat_transact, mentat_query, mentat_pull, mentat_entity, mentat_schema
- mentatd server: HTTP endpoints, connection pooling, EDN protocol
- Test migration: 38 pgrx tests migrated to src/lib.rs
- Nix flake: reproducible development environment
- Compilation: Clean build with 0 errors

⏳ **In Progress:**
- Test execution: Blocked by local environment, GitHub Actions workflow ready

⏸️ **Pending:**
- Test validation: Awaiting clean environment execution
- Bug fixes: Based on test results
- Missing types: ref, double, instant, uuid, bytes (5 of 9 EDN types)

See [IMPLEMENTATION_SUMMARY.md](IMPLEMENTATION_SUMMARY.md) and [PHASE_STATUS.md](PHASE_STATUS.md) for detailed status.

---

## Quick Start

### With Nix (Recommended)

The project provides a Nix flake with all dependencies pre-configured.

```bash
# Enter development shell
nix develop

# Install and initialize cargo-pgrx
setup-pgrx

# Run all 38 tests
test-pg16

# Build the extension
build-extension

# Install to local PostgreSQL
install-extension
```

See [NIX_SETUP.md](NIX_SETUP.md) for full details.

### Without Nix

```bash
# Install system dependencies (Fedora)
sudo dnf install -y postgresql-server-devel postgresql-private-devel \
    clang-devel llvm-devel openssl-devel

# Install Rust toolchain
rustup toolchain install 1.90.0
rustup default 1.90.0

# Install cargo-pgrx
cargo install --locked cargo-pgrx --version '~0.17'
cargo pgrx init --pg16=$(which pg_config)

# Build and test
cd pg_mentat
cargo pgrx test pg16
```

### Using the Extension

```sql
CREATE EXTENSION pg_mentat;

-- Initialize schema
SELECT mentat.initialize_schema();

-- Define attributes
SELECT mentat.mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Query
SELECT mentat.mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);

-- Schema introspection
SELECT mentat.mentat_schema();
```

---

## About Project Mentat

Project Mentat is a persistent, embedded knowledge base. It draws heavily on
[DataScript](https://github.com/tonsky/datascript) and
[Datomic](http://datomic.com).

This project was started by Mozilla, but
[is no longer being developed or actively maintained by them](https://mail.mozilla.org/pipermail/firefox-dev/2018-September/006780.html).
[Their repository](https://github.com/mozilla/mentat) was marked read-only.
[This fork](https://github.com/qpdb/mentat) is an attempt to revive and
continue that work. We owe the team at Mozilla our thanks for the inspiration
and the code.

[Original SQLite Documentation](https://docs.rs/mentat)

---

## Repository Structure

### New PostgreSQL Crates

#### `pg_mentat/`

PostgreSQL extension built with pgrx. Implements the storage layer and query
execution as PostgreSQL extension functions:

- Custom `EdnValue` PostgreSQL type with text I/O
- Extension functions: `mentat_transact()`, `mentat_query()`, `mentat_pull()`,
  `mentat_entity()`, `mentat_schema()`
- Schema with EAVT/AEVT/AVET/VAET indexes
- Full-text search via tsvector/GIN indexes
- 38 inline pgrx tests

#### `mentatd/`

HTTP server that speaks the Datomic wire protocol. Translates Datomic client
requests to PostgreSQL queries via pg_mentat. Uses axum for HTTP and
deadpool-postgres for connection pooling.

### Original Mentat Crates

| Crate | Purpose |
|-------|---------|
| `edn` | EDN parser using rust-peg |
| `core` / `core-traits` | Fundamental data structures (ValueType, TypedValue) |
| `db` / `db-traits` | Core storage logic (originally SQLite) |
| `query-algebrizer` | Translates parsed Datalog to algebraic query representation |
| `query-projector` | Projects query variables into output data structures |
| `query-sql` | Abstract SQL representation |
| `sql` / `sql-traits` | SQL text generation |
| `transaction` | Transaction processing |
| `tolstoy` | Sync protocol (work in progress) |
| `tools/cli` | Command-line interface |

---

## Testing

### pg_mentat Extension

38 pgrx tests defined inline in `pg_mentat/src/lib.rs`:

| Category | Count | Description |
|----------|-------|-------------|
| EDN Types | 5 | Boolean, integer, string, vector, map roundtrips |
| Queries | 11 | Rel, scalar, tuple, coll, inputs, multi-clause, not, or, order, limit |
| Time-Travel | 7 | As-of, since, history, retraction, temporal queries |
| Rules | 8 | Simple, recursive, multi-clause, predicates, negation, aggregation |
| Full-Text | 7 | Basic FTS, multi-term, scoring, special chars, phrase search |

```bash
# Run all tests (requires Nix or manual pgrx setup)
cd pg_mentat
cargo pgrx test pg16
```

### mentatd Server

- 12 unit tests (protocol-level, pass without PostgreSQL)
- 21 integration tests (require running PostgreSQL)

```bash
cd mentatd
cargo test          # unit tests
cargo test --all    # all tests (needs PostgreSQL)
```

### Original Mentat

415 core tests for the SQLite implementation:

```bash
cargo test --all
```

---

## Documentation

| Document | Description |
|----------|-------------|
| [CURRENT_STATUS.md](CURRENT_STATUS.md) | Detailed status with component breakdown |
| [NIX_SETUP.md](NIX_SETUP.md) | Nix development environment guide |
| [QUICK_START.md](QUICK_START.md) | Getting started for new developers |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to contribute |
| [TEST_MIGRATION_COMPLETE.md](TEST_MIGRATION_COMPLETE.md) | Test restructuring details |
| [HONEST_STATUS.md](HONEST_STATUS.md) | Validator audit findings |
| [pg_mentat/README.md](pg_mentat/README.md) | Extension-specific docs |

---

## Motivation

Mentat is a flexible relational (not key-value, not document-oriented) store
that makes it easy to describe, grow, and reuse your domain schema.

By abstracting away the storage schema and by exposing change listeners outside
the database (not via triggers), we hope to make domain schemas stable, and
allow both the data store itself and embedding applications to use better
architectures, meeting performance goals in a way that allows future evolution.

For more on the design philosophy, see the comparisons to
[DataScript](https://github.com/tonsky/datascript),
[Datomic](http://datomic.com), and
[SQLite](https://www.sqlite.org/) in the original project documentation.

---

## Database Dependencies

### PostgreSQL (Claude Branch)

pg_mentat requires PostgreSQL 14 or higher. It uses:

- BTREE and GIN indexes
- tsvector/tsquery for full-text search
- SERIALIZABLE isolation level for consistency
- Standard types (BIGINT, TEXT, BYTEA, TIMESTAMPTZ, BOOLEAN)
- Custom EdnValue type via pgrx

Development requires:

- cargo-pgrx 0.17+
- PostgreSQL development headers
- LLVM/Clang for bindgen
- Linux (recommended; macOS ARM64 has known pgrx linking issues)

### SQLite (Master Branch)

The original implementation uses SQLite 3.8.0+ with partial indices and FTS4.

---

## Contributing

Please note that this project is released with a Contributor Code of Conduct.
By participating in this project you agree to abide by its terms.

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on environment setup,
coding standards, testing requirements, and pull request process.

---

## License

Project Mentat is licensed under the Apache License v2.0. See the `LICENSE`
file for details.
