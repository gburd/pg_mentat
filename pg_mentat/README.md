# pg_mentat

PostgreSQL extension that brings Mentat's Datalog capabilities and EDN data type to PostgreSQL.

## Overview

`pg_mentat` is a PostgreSQL extension built with pgrx that provides:

- **EDN Custom Type**: Native PostgreSQL type for Extensible Data Notation (EDN)
- **Datalog Queries**: Execute Datalog queries directly in PostgreSQL
- **Temporal Database**: Time-travel queries with transaction history
- **Storage Integration**: Native PostgreSQL storage for Mentat data

## Features

### EDN Type

The extension provides a custom `EdnValue` type that supports all EDN data types:

- Primitives: `nil`, `boolean`, `integer`, `float`, `string`
- Collections: `vector`, `list`, `set`, `map`
- Special types: `keyword`, `symbol`, `uuid`, `instant`, `bytes`, `bigint`

### Basic Usage

```sql
-- Create extension
CREATE EXTENSION pg_mentat;

-- Use EDN values
SELECT mentat.edn_in('42');
SELECT mentat.edn_in('{:name "Alice" :age 30}');

-- Create tables with EDN columns
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    data mentat.EdnValue
);

INSERT INTO users (data) VALUES
    (mentat.edn_in('{:name "Alice" :email "alice@example.com"}'));

-- Query EDN data
SELECT id, mentat.edn_out(data) FROM users;
```

### EDN Functions

- `edn_in(text)` - Parse EDN text to EdnValue
- `edn_out(EdnValue)` - Convert EdnValue to EDN text
- `edn_get(map, key)` - Get value from map by key
- `edn_nth(vector, index)` - Get element from vector by index
- `edn_count(collection)` - Get collection size
- `edn_contains(collection, element)` - Check if element exists
- `edn_keys(map)` - Extract map keys as vector
- `edn_values(map)` - Extract map values as vector

### Type Predicates

- `edn_is_nil(value)` - Check if value is nil
- `edn_is_boolean(value)` - Check if value is boolean
- `edn_is_integer(value)` - Check if value is integer
- `edn_is_float(value)` - Check if value is float
- `edn_is_text(value)` - Check if value is text
- `edn_is_keyword(value)` - Check if value is keyword
- `edn_is_vector(value)` - Check if value is vector
- `edn_is_list(value)` - Check if value is list
- `edn_is_set(value)` - Check if value is set
- `edn_is_map(value)` - Check if value is map

## Building

### Prerequisites

- PostgreSQL 13-18
- Rust (latest stable via rustup)
- pgrx CLI: `cargo install cargo-pgrx --locked`

### Build and Install

**IMPORTANT**: Before building, you must initialize pgrx:

```bash
# Initialize pgrx with your PostgreSQL installation
# This only needs to be done once per system
cargo pgrx init --pg16=/path/to/pg_config

# Example on macOS with Homebrew:
cargo pgrx init --pg16=/opt/homebrew/bin/pg_config
```

After initialization:

```bash
# Build the extension
cargo pgrx package

# Install in PostgreSQL
cargo pgrx install

# Run tests
cargo pgrx test pg16
```

**Note**: If you see `$PGRX_HOME does not exist` error, run `cargo pgrx init` first.

## Architecture

The extension is organized as follows:

- `src/lib.rs` - Extension entry point and schema initialization
- `src/types/edn.rs` - EdnValue type implementation
- `src/operators.rs` - EDN operators and functions
- `sql/bootstrap.sql` - SQL initialization scripts
- `test/sql/` - Integration tests

### Storage Format

Currently, EdnValue uses EDN text format for storage. Future versions will use CBOR (Compact Binary Object Representation) for efficient binary storage while maintaining EDN text for I/O functions.

## Development Status

**Phase 1: Foundation (Current)**
- ✅ EDN type with text I/O
- ✅ Basic type predicates
- ✅ Collection accessors
- ✅ Schema initialization
- ⚠️ CBOR serialization (planned)

**Phase 2: Datalog Integration (Planned)**
- ⏳ Query execution
- ⏳ Transaction support
- ⏳ Time-travel queries

**Phase 3: Optimization (Planned)**
- ⏳ Query planner hooks
- ⏳ Index support (GIN/GIST)
- ⏳ Performance tuning

## Testing

```bash
# Run unit tests
cargo test

# Run pgrx integration tests
cargo pgrx test pg16

# Run SQL tests
psql -d postgres -f test/sql/basic.sql
```

## License

Apache-2.0

## References

- [pgrx Documentation](https://docs.rs/pgrx/latest/pgrx/)
- [EDN Format Specification](https://github.com/edn-format/edn)
- [Mentat Project](https://github.com/mozilla/mentat)
