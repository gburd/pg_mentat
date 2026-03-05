# pg_mentat: pgrx Recommendations Summary

## Quick Reference

### Recommended Technology Stack

| Component | Version/Choice | Rationale |
|-----------|---------------|-----------|
| **pgrx** | 0.12.x (latest stable) | Production-ready, multi-version support |
| **PostgreSQL** | 13-18 (target 15+) | Widest compatibility, modern features in 15+ |
| **Rust** | Latest stable | Required by pgrx, no MSRV policy |
| **Serialization** | CBOR (internal), EDN (user-facing) | Efficient storage, familiar syntax |
| **Platform** | Linux (primary), macOS (dev) | Best pgrx support |

### PostgreSQL Compatibility Matrix

| PostgreSQL Version | pgrx Support | Recommended | Notes |
|-------------------|--------------|-------------|-------|
| 13 | ✅ Full | ⚠️ Minimum | Oldest supported, EOL Nov 2025 |
| 14 | ✅ Full | ✅ Yes | Stable, EOL Nov 2026 |
| 15 | ✅ Full | ✅ Yes | Stable, improved performance |
| 16 | ✅ Full | ✅ **Primary** | Current stable, best features |
| 17 | ✅ Full | ✅ Yes | Latest stable, enhanced ICU |
| 18 | ✅ Full | ⚠️ Beta | Development version |

**Primary Development Target:** PostgreSQL 16 (current LTS)

**Testing Matrix:** 14, 15, 16, 17 (skip 13 unless required, 18 in CI only)

## EDN Custom Type Implementation Plan

### Type Architecture

```rust
// Core type definition
#[derive(Debug, Clone, PostgresType, Serialize, Deserialize)]
#[inoutfuncs]  // Generates edn_in/edn_out functions
pub struct EdnValue {
    inner: mentat_edn::Value,
}

// Automatic CBOR serialization via pgrx
// Manual EDN text I/O via inoutfuncs
```

### Type System Mapping

**Simple Types** (direct mapping):
```sql
-- Boolean
SELECT edn_in('true');           -- => EdnValue::Boolean(true)

-- Integer
SELECT edn_in('42');             -- => EdnValue::Integer(42)

-- Float
SELECT edn_in('3.14');           -- => EdnValue::Float(3.14)

-- String
SELECT edn_in('"hello"');        -- => EdnValue::Text("hello")

-- UUID
SELECT edn_in('#uuid "550e8400-e29b-41d4-a716-446655440000"');

-- Instant
SELECT edn_in('#inst "2025-03-05T12:00:00Z"');
```

**Complex Types** (nested structures):
```sql
-- Vector
SELECT edn_in('[1 2 3 4 5]');

-- Map
SELECT edn_in('{:name "Alice" :age 30}');

-- Set
SELECT edn_in('#{:red :green :blue}');

-- Nested
SELECT edn_in('{:users [{:name "Alice"} {:name "Bob"}]}');
```

### Memory Management Strategy

**1. Use PostgreSQL's Memory Contexts**
```rust
use pgrx::prelude::*;

#[pg_extern]
fn edn_operation(value: EdnValue) -> EdnValue {
    // Switch to appropriate memory context
    PgMemoryContexts::For(value.inner()).switch_to(|_| {
        // All allocations within this block use PG allocator
        process_edn_safely(value)
    })
}
```

**2. Leverage PgBox for Owned Data**
```rust
use pgrx::PgBox;

fn allocate_edn_in_pg_heap(value: Value) -> PgBox<EdnValue> {
    PgBox::new(EdnValue { inner: value })
}
```

**3. Implement Drop Carefully**
```rust
impl Drop for EdnValue {
    fn drop(&mut self) {
        // Rust Drop runs even on panic/elog(ERROR)
        // No manual cleanup needed for PG-allocated memory
        // Only needed for external resources (files, sockets, etc.)
    }
}
```

**4. SPI Memory Context Management**
```rust
#[pg_extern]
fn query_with_edn() -> SetOfIterator<'static, EdnValue> {
    Spi::connect(|client| {
        // SPI context active here
        let results = client.select("SELECT ...", None, None)?;

        // Copy results to longer-lived context
        PgMemoryContexts::CurrentMemoryContext.switch_to(|_| {
            results.map(|row| {
                // Extract and copy to current context
                row.get::<EdnValue>(1).expect("column 1")
            }).collect()
        })
    })
}
```

### Serialization Details

**Internal Storage (CBOR):**
- Automatic via `#[derive(Serialize, Deserialize)]`
- Compact binary format
- Self-describing
- Handles all EDN types natively

**User Interface (EDN Text):**
```rust
#[pg_extern]
fn edn_in(input: &str) -> Result<EdnValue, Box<dyn std::error::Error>> {
    // Parse EDN text
    let value = mentat_edn::parse::value(input)?;

    // Validate constraints
    validate_edn_limits(&value)?;

    Ok(EdnValue { inner: value })
}

#[pg_extern]
fn edn_out(value: EdnValue) -> String {
    // Convert to EDN text format
    format!("{}", value.inner)
}
```

**JSONB Compatibility Layer:**
```rust
#[pg_extern]
fn edn_to_jsonb(value: EdnValue) -> pgrx::JsonB {
    // Convert EDN to JSON for PostgreSQL JSONB
    let json_value = edn_to_json(&value.inner);
    pgrx::JsonB(json_value)
}

#[pg_extern]
fn jsonb_to_edn(json: pgrx::JsonB) -> EdnValue {
    // Convert JSONB to EDN
    let edn_value = json_to_edn(&json.0);
    EdnValue { inner: edn_value }
}
```

## SPI Usage Patterns

### Pattern 1: Simple Query Execution
```rust
#[pg_extern]
fn execute_simple_query(sql: &str) -> Result<i64, Box<dyn std::error::Error>> {
    Spi::connect(|client| {
        let result = client.select(sql, None, None)?;
        Ok(result.len() as i64)
    })
}
```

### Pattern 2: Parameterized Queries
```rust
#[pg_extern]
fn find_by_id(entity_id: i64) -> Option<EdnValue> {
    Spi::connect(|client| {
        client.select(
            "SELECT data FROM mentat_entities WHERE e = $1",
            None,
            Some(vec![(PgBuiltInOids::INT8OID.oid(), entity_id.into_datum())]),
        )?.first()
         .get::<EdnValue>(1)
    })
}
```

### Pattern 3: Set-Returning Functions
```rust
#[pg_extern]
fn query_datoms(pattern: &str) -> SetOfIterator<'static, (i64, i64, EdnValue, i64)> {
    SetOfIterator::new(
        Spi::connect(|client| {
            let results = client.select(
                "SELECT e, a, v, tx FROM mentat_datoms WHERE ...",
                None,
                None,
            )?;

            results.map(|row| {
                (
                    row.get::<i64>(1)?,
                    row.get::<i64>(2)?,
                    row.get::<EdnValue>(3)?,
                    row.get::<i64>(4)?,
                )
            }).collect::<Result<Vec<_>, _>>()
        }).expect("SPI query failed").into_iter()
    )
}
```

### Pattern 4: Transaction Management
```rust
#[pg_extern]
fn atomic_transaction(ops: Vec<EdnValue>) -> Result<i64, Box<dyn std::error::Error>> {
    Spi::connect(|mut client| {
        // Start transaction
        client.select("BEGIN", None, None)?;

        let mut tx_id = 0i64;
        for op in ops {
            // Execute each operation
            let result = client.select(
                "INSERT INTO mentat_datoms (e, a, v, tx) VALUES ($1, $2, $3, $4) RETURNING tx",
                None,
                Some(extract_params(&op)),
            )?.first().get::<i64>(1)?;

            tx_id = result;
        }

        // Commit transaction
        client.select("COMMIT", None, None)?;

        Ok(tx_id)
    })
}
```

## Extension Structure Recommendations

### Directory Layout
```
pg_mentat/
├── Cargo.toml                    # pgrx = "0.12", serde, mentat-edn
├── pg_mentat.control             # Extension metadata
├── README.md
├── sql/
│   ├── pg_mentat--0.1.0.sql     # Initial schema
│   └── pg_mentat--0.1.0--0.2.0.sql  # Upgrade path
├── src/
│   ├── lib.rs                    # pg_module_magic!(), module structure
│   ├── types/
│   │   ├── mod.rs               # Type exports
│   │   ├── edn.rs               # EdnValue definition
│   │   └── conversions.rs       # Type conversions
│   ├── operators/
│   │   ├── mod.rs               # Operator exports
│   │   ├── comparison.rs        # =, <>, <, >, <=, >=
│   │   └── containment.rs       # @>, <@, ?, ?|, ?&
│   ├── functions/
│   │   ├── mod.rs               # Function exports
│   │   ├── accessors.rs         # edn_get, edn_keys, edn_values
│   │   ├── constructors.rs      # edn_vector, edn_map, edn_set
│   │   └── aggregates.rs        # edn_collect, edn_merge
│   ├── datalog/
│   │   ├── mod.rs               # Datalog integration
│   │   ├── query.rs             # Query execution
│   │   └── transaction.rs       # Transaction handling
│   ├── storage/
│   │   ├── mod.rs               # Storage layer
│   │   ├── schema.rs            # Schema initialization
│   │   └── indexes.rs           # Index management
│   └── spi/
│       ├── mod.rs               # SPI wrappers
│       └── helpers.rs           # Common SPI patterns
└── test/
    ├── expected/                 # Expected test output
    │   └── basic.out
    └── sql/                      # Test SQL scripts
        └── basic.sql
```

### Cargo.toml Configuration
```toml
[package]
name = "pg_mentat"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[features]
default = ["pg16"]
pg13 = ["pgrx/pg13"]
pg14 = ["pgrx/pg14"]
pg15 = ["pgrx/pg15"]
pg16 = ["pgrx/pg16"]
pg17 = ["pgrx/pg17"]
pg18 = ["pgrx/pg18"]
pg_test = []

[dependencies]
pgrx = "0.12"
serde = { version = "1.0", features = ["derive"] }
serde_cbor = "0.11"

# Mentat dependencies (from workspace)
mentat-edn = { path = "../edn" }
mentat-core = { path = "../core" }
mentat-query = { path = "../query-algebrizer" }
mentat-db = { path = "../db" }

[dev-dependencies]
pgrx-tests = "0.12"

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
```

### pg_mentat.control
```ini
# pg_mentat extension
comment = 'Mentat Datalog database for PostgreSQL'
default_version = '0.1.0'
module_pathname = '$libdir/pg_mentat'
relocatable = true
requires = ''
superuser = false
schema = mentat
```

## Implementation Priority

### Phase 1: Core Type (Week 1-2)
- [x] Research complete (this document)
- [ ] Set up pgrx project: `cargo pgrx new pg_mentat`
- [ ] Implement EdnValue type with all variants
- [ ] Test serialization round-trips
- [ ] Verify memory safety with valgrind

### Phase 2: Basic Operations (Week 3-4)
- [ ] Comparison operators (=, <>, <, >)
- [ ] Access functions (edn_get, edn_keys)
- [ ] Constructor functions (edn_vector, edn_map)
- [ ] SQL tests for all operations

### Phase 3: Storage Integration (Week 5-6)
- [ ] Initialize mentat_datoms schema
- [ ] Implement EAVT/AEVT/AVET/VAET indexes
- [ ] SPI integration for queries
- [ ] Transaction support

### Phase 4: Datalog Engine (Week 7-8)
- [ ] Integrate mentat-query
- [ ] Implement datalog() function
- [ ] Add transact() function
- [ ] Time-travel queries (as_of, since)

### Phase 5: Optimization (Week 9-10)
- [ ] Query planner hooks
- [ ] Cost estimation
- [ ] GIN index for containment
- [ ] Performance benchmarks

## Best Practices Checklist

### Memory Safety
- ✅ Use PgMemoryContexts for all allocations
- ✅ Never use std::alloc directly
- ✅ Copy SPI results to appropriate context
- ✅ Let pgrx handle Drop semantics
- ✅ Use `#[pg_guard]` for panic safety

### Type System
- ✅ Implement IntoDatum/FromDatum correctly
- ✅ Use Option<T> for nullable values
- ✅ Provide CBOR for storage, EDN for I/O
- ✅ Validate input sizes and nesting depth
- ✅ Handle all EDN type variants

### SPI Usage
- ✅ Always use parameterized queries
- ✅ Never concatenate user input into SQL
- ✅ Use Spi::connect() for transaction safety
- ✅ Copy results out of SPI memory context
- ✅ Set resource limits (timeout, work_mem)

### Testing
- ✅ Unit tests with `#[pg_test]`
- ✅ Integration tests in test/sql/
- ✅ Run `cargo careful test` for UB checks
- ✅ Test all PostgreSQL versions (14-17)
- ✅ Memory leak detection with valgrind

### Performance
- ✅ Use CBOR for compact storage
- ✅ Implement B-tree operators for indexing
- ✅ Provide GIN operators for containment
- ✅ Profile with pg_stat_statements
- ✅ Benchmark against native Mentat

### Security
- ✅ Validate EDN input sizes
- ✅ Limit nesting depth (100 levels)
- ✅ Limit collection sizes (1M elements)
- ✅ Set statement timeout (30s default)
- ✅ Enforce work_mem limits

## Common Pitfalls to Avoid

### ❌ Don't Do This
```rust
// BAD: Direct heap allocation
let data = Box::new(value);  // Wrong allocator!

// BAD: String concatenation SQL
let sql = format!("SELECT * FROM t WHERE id = {}", user_input);

// BAD: Ignoring memory context
let result = Spi::get_one::<EdnValue>("SELECT ...")?;
return result;  // May be freed!

// BAD: Unsafe without justification
unsafe {
    pg_sys::SomeFunction();  // Document why unsafe!
}
```

### ✅ Do This Instead
```rust
// GOOD: PostgreSQL allocator
let data = PgBox::new(value);

// GOOD: Parameterized query
client.select(
    "SELECT * FROM t WHERE id = $1",
    None,
    Some(vec![(PgBuiltInOids::INT8OID.oid(), user_input.into_datum())]),
)?;

// GOOD: Copy to safe context
let result = PgMemoryContexts::CurrentMemoryContext.switch_to(|_| {
    Spi::get_one::<EdnValue>("SELECT ...")?
})?;

// GOOD: Documented unsafe with safety invariants
// SAFETY: ptr is valid and aligned, lifetime constrained by 'a
unsafe {
    pg_sys::SomeFunction(ptr);
}
```

## Conclusion

This implementation plan provides a clear path from research to production. The key decisions are:

1. **pgrx 0.12.x** - Latest stable framework
2. **PostgreSQL 16** - Primary development target
3. **CBOR + EDN** - Efficient storage, familiar interface
4. **Memory safety first** - Use pgrx abstractions
5. **Incremental delivery** - Five clear phases

Next step: Begin Phase 1 implementation with `cargo pgrx new pg_mentat`.
