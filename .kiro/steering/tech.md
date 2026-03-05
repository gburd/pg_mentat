# Technology Stack

## Core Technologies

- **Language**: Rust (edition 2021, toolchain 1.90.0)
- **Database**: SQLite with FTS4 full-text search
- **Build System**: Cargo with workspace configuration
- **Query Language**: Datalog for queries, EDN for data representation

## Key Dependencies

- `rusqlite` - SQLite bindings with bundled SQLite
- `chrono` - Date/time handling
- `uuid` - UUID generation and parsing
- `edn` - EDN (Extensible Data Notation) parsing
- `thiserror` - Error handling (migrated from failure crate)
- `lazy_static` - Static initialization

## Build Commands

### Basic Operations
```bash
# Build all crates
cargo build

# Run all tests
cargo test --all

# Build specific crate
cargo build -p mentat_query_algebrizer

# Test specific crate with debug output
cargo test -p mentat_query_algebrizer -- --nocapture
```

### Documentation
```bash
# Generate documentation
cargo doc

# Generate and open docs
cargo doc --open
```

### Development Tools
```bash
# Check for outdated dependencies
make outdated

# Fix code issues
make fix

# Check for available upgrades
cargo upgrades
```

## Features

- `bundled_sqlite3` (default) - Use bundled SQLite
- `sqlcipher` - SQLCipher encryption support
- `syncable` (default) - Enable sync capabilities

## Platform Support

- **Core**: Rust library
- **Android**: Java/Kotlin bindings via JNI
- **iOS**: Swift bindings
- **CLI**: Command-line interface tool
