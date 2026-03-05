# Project Structure

## Workspace Organization

Mentat uses a Cargo workspace with multiple crates for modularity, incremental builds, and feature separation.

## Core Crates

### Foundation Layer
- `core-traits/` - Core trait definitions and interfaces
- `core/` - Fundamental types, schema management, SQL mappings
- `edn/` - EDN parser and data structures

### Database Layer
- `db-traits/` - Database trait definitions
- `db/` - Core storage logic, transactions, schema bootstrap
- `sql-traits/` - SQL abstraction traits
- `sql/` - SQL query generation

### Query Processing Pipeline
- `query-algebrizer-traits/` - Query algebrizer interfaces
- `query-algebrizer/` - Datalog to SQL translation logic
- `query-projector-traits/` - Result projection interfaces  
- `query-projector/` - Query result formatting and projection
- `query-pull-traits/` - Pull query interfaces
- `query-pull/` - Pull query implementation
- `query-sql/` - SQL query representation

### High-Level APIs
- `src/` - Main library interface and public API
- `transaction/` - Transaction building and entity operations
- `public-traits/` - Public error types and traits

### Sync & Extensions
- `tolstoy-traits/` - Sync trait definitions
- `tolstoy/` - Sync implementation (named after Tolstoy for referential reasons)
- `ffi/` - Foreign function interface for mobile platforms

### Tools & SDKs
- `tools/cli/` - Command-line interface
- `sdks/android/` - Android/Java SDK
- `sdks/swift/` - iOS/Swift SDK

## Key Conventions

### File Organization
- Each crate has standard `src/lib.rs` entry point
- Error types in dedicated `errors.rs` files
- Tests in `tests/` directories or inline with `#[cfg(test)]`

### Naming Patterns
- Crate names use underscores: `mentat_core`, `query_algebrizer_traits`
- Folder names use hyphens: `query-algebrizer`, `core-traits`
- Traits crates separate from implementation crates

### Module Structure
- `lib.rs` - Main module with re-exports
- Logical grouping of related functionality
- Public API carefully curated through re-exports

## Development Files
- `fixtures/` - Test data and database fixtures
- `examples/` - Example EDN data files
- `docs/` - Documentation and API docs
- `scripts/` - Build and development scripts
