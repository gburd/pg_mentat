# pg_mentat Extension Architecture Design

## Executive Summary

This document outlines the architecture for `pg_mentat`, a PostgreSQL extension that brings Mentat's Datalog query capabilities to PostgreSQL. The extension will be built using pgrx (PostgreSQL extension framework in Rust) and will implement a custom EDN (Extensible Data Notation) type to store and manipulate Mentat data within PostgreSQL.

**Key Design Goals:**
1. Native PostgreSQL integration with custom EDN type
2. Memory-safe implementation leveraging Rust and pgrx
3. Efficient serialization using CBOR for storage
4. SPI integration for query execution
5. Support for PostgreSQL 13-18

## 1. Technology Stack

### 1.1 pgrx Framework

**Recommended Version:** Latest stable (0.12.x series as of research date)

**Why pgrx:**
- Memory safety through Rust's ownership model
- Automatic panic-to-ERROR translation prevents PostgreSQL crashes
- Built-in support for custom types with serialization
- Cross-version compatibility (PG 13-18 from single codebase)
- Strong type system with NULL safety via `Option<T>`

**PostgreSQL Compatibility Matrix:**

| pgrx Version | PostgreSQL Versions | Platform Support |
|-------------|---------------------|------------------|
| 0.12.x      | 13, 14, 15, 16, 17, 18 | Linux (x86_64, aarch64), macOS (aarch64), Windows (x86_64, MSVC) |

**Critical Requirements:**
- UTF-8 database encoding (SQL_ASCII not supported)
- Latest stable Rust toolchain via rustup
- libclang 11+ for build
- No threading (PostgreSQL is single-threaded)

### 1.2 Custom Type Strategy

Based on pgrx examples and production extensions (pg_graphql, zombodb), we'll implement:

```rust
use pgrx::prelude::*;

#[derive(PostgresType, Serialize, Deserialize)]
#[inoutfuncs]
pub struct EdnValue {
    inner: edn::Value,
}
```

**Key Features:**
- `#[derive(PostgresType)]` - Automatic type registration
- `#[inoutfuncs]` - Generate text I/O functions for user interaction
- CBOR serialization for efficient binary storage (pgrx default)
- JSON representation for human-readable output

## 2. EDN Custom Type Implementation

### 2.1 Type Mapping

Mentat's EDN types map to PostgreSQL/pgrx as follows:

| EDN Type | Rust Type | PostgreSQL Representation | Notes |
|----------|-----------|--------------------------|--------|
| nil | `Value::Nil` | NULL or special marker | Use Option<T> at boundaries |
| boolean | `Value::Boolean(bool)` | BOOLEAN | Direct mapping |
| integer | `Value::Integer(i64)` | BIGINT | 64-bit signed |
| instant | `Value::Instant(DateTime<Utc>)` | TIMESTAMPTZ | UTC timezone |
| big-integer | `Value::BigInteger(BigInt)` | NUMERIC | Arbitrary precision |
| float | `Value::Float(OrderedFloat<f64>)` | DOUBLE PRECISION | IEEE 754 |
| string | `Value::Text(String)` | TEXT | UTF-8 encoded |
| uuid | `Value::Uuid(Uuid)` | UUID | Native PG UUID |
| keyword | `Value::Keyword` | Custom format | ":namespace/name" |
| symbol | `Value::PlainSymbol`, `Value::NamespacedSymbol` | Custom format | "symbol" or "ns/symbol" |
| vector | `Value::Vector(Vec<Value>)` | Array-like | Ordered collection |
| list | `Value::List(LinkedList<Value>)` | List-like | Ordered collection |
| set | `Value::Set(BTreeSet<Value>)` | Set-like | Unordered, unique |
| map | `Value::Map(BTreeMap<Value, Value>)` | JSON-like | Key-value pairs |
| bytes | `Value::Bytes(Bytes)` | BYTEA | Binary data |

### 2.2 Serialization Strategy

**Storage Format:** CBOR (Compact Binary Object Representation)
- Default for pgrx `#[derive(PostgresType)]`
- Efficient binary encoding
- Self-describing format
- Handles recursive structures naturally

**Human Interface:** JSON
- Text input/output via inout functions
- Familiar syntax for users
- Standard PostgreSQL JSON compatibility where applicable

**Implementation Pattern:**

```rust
#[pg_extern]
fn edn_in(input: &str) -> EdnValue {
    // Parse EDN text format
    let value = edn::parse::value(input)
        .map_err(|e| error!("Invalid EDN: {}", e))?;
    EdnValue { inner: value }
}

#[pg_extern]
fn edn_out(value: EdnValue) -> String {
    // Convert to EDN text format
    value.inner.to_string()
}
```

### 2.3 Memory Management

**pgrx Memory Safety Model:**

1. **PgMemoryContexts** - Safe access to PostgreSQL's arena allocator
2. **PgBox<T>** - PostgreSQL-aware smart pointer
3. **Datum Lifetimes** - Explicit lifetime tracking with `Datum<'src>`
4. **Automatic Cleanup** - Drop semantics work even with `panic!` or `elog(ERROR)`

**Best Practices for EDN Type:**

```rust
use pgrx::prelude::*;

#[pg_extern]
fn edn_operation(value: EdnValue) -> EdnValue {
    // Allocate in current memory context
    PgMemoryContexts::CurrentMemoryContext.switch_to(|context| {
        // Work with value
        // Automatic cleanup on error/panic
        process_edn(value)
    })
}
```

**Critical Considerations:**
- All heap allocations must use PostgreSQL's allocator
- Never use `std::alloc` directly in extension code
- SPI query results tied to specific memory contexts
- Use `pg_guard` macro to ensure cleanup on errors

## 3. Extension Structure

### 3.1 Project Organization

```
pg_mentat/
├── Cargo.toml              # Dependencies and pgrx configuration
├── pg_mentat.control       # Extension metadata
├── sql/                    # SQL installation scripts
│   └── pg_mentat--0.1.0.sql
├── src/
│   ├── lib.rs             # Extension entry point
│   ├── types.rs           # EDN custom type definition
│   ├── operators.rs       # EDN operators (+, -, comparison, etc.)
│   ├── functions.rs       # EDN manipulation functions
│   ├── datalog.rs         # Datalog query execution
│   ├── storage.rs         # PostgreSQL storage integration
│   └── spi.rs             # Server Programming Interface wrappers
└── test/                  # Integration tests
    └── sql/
        └── basic_edn.sql
```

### 3.2 Core Components

**lib.rs - Extension Registration:**

```rust
use pgrx::prelude::*;

pgrx::pg_module_magic!();

#[pg_schema]
mod mentat {
    use pgrx::prelude::*;

    // Export all extension functionality
}

#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {
        // Initialize extension for testing
    }

    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![]
    }
}
```

**types.rs - EDN Type Definition:**

```rust
use pgrx::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PostgresType, Serialize, Deserialize)]
#[inoutfuncs]
pub struct EdnValue {
    #[serde(flatten)]
    inner: mentat_edn::Value,
}

// Implement IntoDatum and FromDatum for type conversion
unsafe impl SqlTranslatable for EdnValue {
    fn argument_sql() -> Result<SqlMapping, ArgumentError> {
        Ok(SqlMapping::As("EdnValue".to_string()))
    }

    fn return_sql() -> Result<Returns, ReturnsError> {
        Ok(Returns::One(SqlMapping::As("EdnValue".to_string())))
    }
}
```

## 4. Server Programming Interface (SPI) Integration

### 4.1 SPI Usage Patterns

**Query Execution:**

```rust
use pgrx::prelude::*;

#[pg_extern]
fn execute_datalog_query(query: &str) -> SetOfIterator<'static, EdnValue> {
    SetOfIterator::new(
        Spi::connect(|client| {
            // Parse Datalog query
            // Translate to SQL
            // Execute via SPI
            let results = client.select(
                "SELECT * FROM mentat_datoms WHERE ...",
                None,
                None,
            )?;

            // Convert results to EDN
            results.map(|row| {
                row.get::<EdnValue>(1)
                    .expect("Failed to get EDN value")
            }).collect()
        })
    )
}
```

**Key SPI Considerations:**
- SPI cursors for large result sets
- Transaction management via SPI_connect/SPI_finish
- Memory context switches during SPI calls
- Use `Spi::get_one::<T>()` for single values
- Use `Spi::connect()` for transaction safety

### 4.2 Storage Schema Integration

The extension will integrate with Mentat's existing storage schema:

```rust
#[pg_extern]
fn initialize_mentat_schema() -> Result<(), Box<dyn std::error::Error>> {
    Spi::run(r#"
        CREATE TABLE IF NOT EXISTS mentat_datoms (
            e BIGINT NOT NULL,
            a BIGINT NOT NULL,
            v EdnValue NOT NULL,
            tx BIGINT NOT NULL,
            added BOOLEAN NOT NULL DEFAULT TRUE
        );

        CREATE INDEX IF NOT EXISTS idx_mentat_eavt
            ON mentat_datoms (e, a, v, tx);
        CREATE INDEX IF NOT EXISTS idx_mentat_aevt
            ON mentat_datoms (a, e, v, tx);
        CREATE INDEX IF NOT EXISTS idx_mentat_avet
            ON mentat_datoms (a, v, e, tx);
        CREATE INDEX IF NOT EXISTS idx_mentat_vaet
            ON mentat_datoms (v, a, e, tx);
    "#)?;
    Ok(())
}
```

## 5. Implementation Roadmap

### 5.1 Phase 1: Foundation (Milestone 1)

**Objectives:**
- Set up pgrx project structure
- Implement basic EDN type with CBOR serialization
- Create input/output functions
- Write unit tests for type conversion

**Deliverables:**
- `cargo pgrx new pg_mentat`
- EdnValue type with all variants supported
- Text I/O functions (edn_in, edn_out)
- CBOR binary I/O functions (automatic via pgrx)

**Acceptance Criteria:**
- Can create tables with EdnValue columns
- Can INSERT/SELECT EDN values
- All EDN data types round-trip correctly
- Memory leaks checked with valgrind

### 5.2 Phase 2: Operators & Functions (Milestone 2)

**Objectives:**
- Implement EDN manipulation functions
- Add comparison operators
- Create indexing support (GIN/GIST)
- Build aggregate functions

**Deliverables:**
- `edn_get(value, key)` - Extract from maps
- `edn_contains(collection, element)` - Set/vector membership
- `edn_merge(map1, map2)` - Map merging
- Comparison operators (=, <>, <, >, <=, >=)
- B-tree operator class for indexing

**Acceptance Criteria:**
- WHERE clauses work with EDN values
- Can create indexes on EDN columns
- Performance benchmarks meet targets

### 5.3 Phase 3: Datalog Integration (Milestone 3)

**Objectives:**
- Integrate Mentat's query engine
- Implement datalog() function
- Add transaction log support
- Create time-travel queries

**Deliverables:**
- `datalog(query_string)` - Execute Datalog queries
- `transact(transaction_data)` - Add facts to database
- `as_of(tx_id)` - Query historical state
- `since(tx_id)` - Query changes since transaction

**Acceptance Criteria:**
- Can execute Datalog queries from SQL
- Transactions maintain ACID properties
- Time-travel queries work correctly
- Integration tests pass

### 5.4 Phase 4: Optimization (Milestone 4)

**Objectives:**
- Query planner hooks for optimization
- Custom index types (if needed)
- Parallel query support
- Performance tuning

**Deliverables:**
- Planner hook for Datalog optimization
- Cost estimation functions
- Parallel scan support
- Performance benchmarks

**Acceptance Criteria:**
- Query plans are optimal
- Parallel queries work correctly
- Performance meets or exceeds Mentat standalone

## 6. Testing Strategy

### 6.1 Unit Tests

Use pgrx's `#[pg_test]` macro for integration testing:

```rust
#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;

    #[pg_test]
    fn test_edn_roundtrip() {
        let result = Spi::get_one::<String>(
            "SELECT edn_out(edn_in('{:name \"Alice\" :age 30}'))"
        ).expect("Failed to execute query");

        assert!(result.contains("Alice"));
    }
}
```

### 6.2 SQL Integration Tests

Create `.sql` test files in `test/sql/`:

```sql
-- Test EDN type creation
CREATE TABLE edn_test (id SERIAL, data EdnValue);

-- Test insertion
INSERT INTO edn_test (data) VALUES
    (edn_in('[1 2 3 4 5]')),
    (edn_in('{:user/name "Bob" :user/email "bob@example.com"}'));

-- Test queries
SELECT * FROM edn_test WHERE data @> edn_in(':user/name');
```

### 6.3 Memory Safety Testing

Run tests with `cargo careful`:

```bash
cargo install cargo-careful
cargo careful test
```

This enables stdlib debug assertions and UB checks.

### 6.4 Performance Benchmarks

Create benchmark suite comparing:
- Native Mentat (in-process)
- pg_mentat (PostgreSQL extension)
- PostgreSQL JSONB (baseline)

Metrics:
- Query execution time
- Memory usage
- Storage overhead
- Indexing performance

## 7. Security Considerations

### 7.1 Input Validation

**EDN Parser Hardening:**
- Limit nesting depth (prevent stack overflow)
- Limit collection sizes (prevent memory exhaustion)
- Validate UTF-8 encoding strictly
- Reject malformed EDN early

```rust
const MAX_EDN_NESTING: usize = 100;
const MAX_COLLECTION_SIZE: usize = 1_000_000;

#[pg_extern]
fn edn_in(input: &str) -> Result<EdnValue, Box<dyn std::error::Error>> {
    // Validate size
    if input.len() > 10 * 1024 * 1024 {  // 10MB limit
        error!("EDN input too large");
    }

    // Parse with limits
    let value = parse_with_limits(input, MAX_EDN_NESTING)?;
    validate_size(&value, MAX_COLLECTION_SIZE)?;

    Ok(EdnValue { inner: value })
}
```

### 7.2 SQL Injection Prevention

Always use parameterized queries via SPI:

```rust
// GOOD: Parameterized
let result = client.select(
    "SELECT * FROM mentat_datoms WHERE e = $1",
    Some(vec![entity_id.into_datum()]),
    None,
)?;

// BAD: String concatenation
let sql = format!("SELECT * FROM mentat_datoms WHERE e = {}", entity_id);
let result = client.select(&sql, None, None)?;
```

### 7.3 Resource Limits

Implement safeguards against resource exhaustion:

```rust
#[pg_extern]
fn datalog(query: &str) -> SetOfIterator<'static, EdnValue> {
    // Set statement timeout
    Spi::run("SET LOCAL statement_timeout = '30s'")?;

    // Set work memory limit
    Spi::run("SET LOCAL work_mem = '256MB'")?;

    // Execute query
    execute_datalog_internal(query)
}
```

## 8. Deployment & Operations

### 8.1 Installation

```bash
# Build extension
cargo pgrx package

# Install system-wide
sudo cp target/release/pg_mentat-pg16/usr/share/postgresql/16/extension/* \
    /usr/share/postgresql/16/extension/

# Load in database
CREATE EXTENSION pg_mentat;
```

### 8.2 Configuration

**postgresql.conf:**

```ini
# Add to shared_preload_libraries if using hooks
shared_preload_libraries = 'pg_mentat'

# Set memory limits
mentat.max_query_mem = '512MB'
mentat.max_collection_size = 1000000
```

### 8.3 Monitoring

Key metrics to track:
- EDN serialization/deserialization time
- SPI query execution time
- Memory context usage
- Custom type storage overhead

Use PostgreSQL's `pg_stat_statements` for query analysis.

### 8.4 Upgrades

Follow pgrx extension upgrade patterns:

```sql
-- Upgrade from 0.1.0 to 0.2.0
ALTER EXTENSION pg_mentat UPDATE TO '0.2.0';
```

**Note:** Extensions require new connections to see updates from `ALTER EXTENSION`.

## 9. Performance Optimization

### 9.1 CBOR vs JSONB

CBOR advantages:
- More compact binary representation
- Faster serialization/deserialization
- Native support for more types (bytes, timestamps)

JSONB advantages:
- Built-in GIN indexing
- Native PostgreSQL functions
- Better tooling support

**Recommendation:** Use CBOR for storage, provide JSONB conversion functions for compatibility.

### 9.2 Indexing Strategies

**B-tree indexes** for scalar comparisons:
```sql
CREATE INDEX idx_edn_scalar ON table_name (data)
    WHERE edn_is_scalar(data);
```

**GIN indexes** for containment queries:
```sql
CREATE INDEX idx_edn_gin ON table_name USING gin (data);
```

**Custom operator classes** for EDN-specific optimizations.

### 9.3 Query Planner Integration

Implement cost estimation functions:

```rust
#[pg_extern]
fn edn_contains_selectivity(
    internal: Internal,
    oid: pg_sys::Oid,
    args: Internal,
    var_relid: pg_sys::Oid,
) -> f64 {
    // Estimate selectivity for containment operator
    // Return value between 0.0 and 1.0
    0.1  // 10% selectivity estimate
}
```

## 10. References & Resources

### 10.1 pgrx Documentation

- **Official Docs:** https://docs.rs/pgrx/latest/pgrx/
- **GitHub Repository:** https://github.com/pgcentralfoundation/pgrx
- **Examples:** https://github.com/pgcentralfoundation/pgrx/tree/master/pgrx-examples

### 10.2 Example Extensions

- **zombodb:** Full-text search with Elasticsearch - https://github.com/zombodb/zombodb
- **pg_graphql:** GraphQL interface - https://github.com/supabase/pg_graphql
- **pg_analytics:** DuckDB integration (archived) - https://github.com/paradedb/pg_analytics

### 10.3 PostgreSQL Extension Development

- **Extension Building:** https://www.postgresql.org/docs/current/extend-extensions.html
- **C Language Functions:** https://www.postgresql.org/docs/current/xfunc-c.html
- **Index Access Methods:** https://www.postgresql.org/docs/current/indexam.html
- **SPI Documentation:** https://www.postgresql.org/docs/current/spi.html

### 10.4 EDN Specification

- **EDN Format:** https://github.com/edn-format/edn
- **Mentat EDN Implementation:** /Users/gregburd/src/mentat/edn/

## 11. Conclusion

This architecture provides a solid foundation for building `pg_mentat` as a production-ready PostgreSQL extension. The combination of pgrx's safety guarantees, PostgreSQL's robustness, and Mentat's Datalog capabilities creates a powerful platform for temporal database applications.

**Key Strengths:**
- Memory safety through Rust and pgrx
- Native PostgreSQL integration
- Efficient CBOR serialization
- Comprehensive type system
- Well-defined implementation phases

**Next Steps:**
1. Review and approve this architecture document
2. Set up development environment with pgrx
3. Begin Phase 1 implementation (EDN type foundation)
4. Establish CI/CD pipeline with testing
5. Create prototype demonstrating basic functionality

**Success Criteria:**
- All EDN types supported with correct semantics
- Memory-safe implementation with no leaks
- Performance competitive with native Mentat
- Full integration with PostgreSQL query planner
- Comprehensive test coverage (>80%)
- Clear documentation for users and developers
