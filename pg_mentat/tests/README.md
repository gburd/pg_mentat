# pg_mentat Test Suite

PostgreSQL test suite for the Mentat Datalog database, ported from the original SQLite-based implementation.

## Quick Start

### Run All Tests
```bash
cd pg_mentat
cargo pgrx test
```

### Run Specific Test File
```bash
cargo pgrx test test_query
cargo pgrx test test_fulltext
cargo pgrx test test_rules
cargo pgrx test test_timetravel
```

### Run Single Test
```bash
cargo pgrx test test_pg_scalar
```

## Test Files

| File | Tests | Description |
|------|-------|-------------|
| `test_common.rs` | - | Common test utilities and helpers |
| `test_query.rs` | 11 | Core datalog query tests (rel, scalar, tuple, coll) |
| `test_fulltext.rs` | 7 | Full-text search using PostgreSQL tsvector |
| `test_rules.rs` | 8 | Rules and recursive query tests |
| `test_timetravel.rs` | 8 | Temporal queries (as-of, since, history) |
| **Total** | **34** | **18% of original 187 tests** |

## Test Infrastructure

### Setup Functions

```rust
use crate::common::{setup_test_db, bootstrap_schema};

#[pg_test]
fn my_test() {
    setup_test_db().expect("Failed to setup test db");
    bootstrap_schema().expect("Failed to bootstrap schema");

    // Your test logic here
}
```

### Helper Functions

```rust
// Execute query
let result = query("[:find ?e :where [?e :db/ident ?i]]", "{}");

// Execute transaction
let tx_report = transact("[[:db/add \"e\" :attr \"value\"]]");

// Get entity
let entity_json = entity(123);

// Get schema
let schema_json = schema();
```

## Test Patterns

### Basic Query Test
```rust
#[pg_test]
fn test_simple_query() {
    setup_test_db().expect("Setup failed");
    bootstrap_schema().expect("Bootstrap failed");

    let result = Spi::get_one::<String>(
        "SELECT mentat.mentat_query('[:find ?x :where [?x :db/ident ?i]]', '{}'::jsonb)"
    ).expect("Query failed");

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    let results = json["results"].as_array().unwrap();

    assert!(results.len() > 0, "Expected results");
}
```

### Transaction Test
```rust
#[pg_test]
fn test_transaction() {
    setup_test_db().expect("Setup failed");
    bootstrap_schema().expect("Bootstrap failed");

    Spi::run("SELECT mentat.mentat_transact('[[:db/add \"e\" :person/name \"Alice\"]]')")
        .expect("Transaction failed");

    // Verify transaction
    let result = query("[:find ?name :where [?e :person/name ?name]]", "{}");
    // ... assertions
}
```

### Temporal Query Test
```rust
#[pg_test]
fn test_as_of() {
    setup_test_db().expect("Setup failed");
    bootstrap_schema().expect("Bootstrap failed");

    // Create data in tx1
    transact("[[:db/add \"e\" :attr \"val1\"]]").unwrap();
    let tx1 = get_current_tx();

    // Update in tx2
    transact("[[:db/add \"e\" :attr \"val2\"]]").unwrap();

    // Query as-of tx1
    let result = Spi::get_one::<String>(&format!(
        "SELECT mentat.mentat_query('[:find ?v :where [?e :attr ?v]]', '{{\"asOf\": {}}}'::jsonb)",
        tx1
    )).expect("Query failed");

    // Should see val1, not val2
}
```

## Key Differences from SQLite Tests

### 1. Connection
- **SQLite:** `new_connection("")` (in-memory)
- **PostgreSQL:** pgrx SPI (test transaction)

### 2. Results
- **SQLite:** Rust structs (`QueryResults` enum)
- **PostgreSQL:** JSON strings (parse with `serde_json`)

### 3. Full-Text Search
- **SQLite:** FTS4 with `MATCH` operator
- **PostgreSQL:** tsvector/tsquery with `@@` operator

### 4. Test Annotations
- **SQLite:** `#[test]`
- **PostgreSQL:** `#[pg_test]`

## Current Status

### Completed (34 tests)
- ✅ Basic query types (rel, scalar, tuple, coll)
- ✅ Query operators (limit, order, or, not)
- ✅ Full-text search (basic, multi-term, scoring, phrase)
- ✅ Rules and recursion
- ✅ Temporal queries (as-of, since, history)

### Pending (153 tests)
- ⏳ Additional core queries (13 tests)
- ⏳ Cache tests (6 tests)
- ⏳ Entity builder tests (3 tests)
- ⏳ Vocabulary tests (4 tests)
- ⏳ Pull API tests (1 test)
- ⏳ Transaction tests (~20 tests)
- ⏳ Aggregate tests (~10 tests)
- ⏳ Integration tests (~96 tests)

## Documentation

- **TEST_PORT_STATUS.md** - Detailed progress tracking
- **TEST_MIGRATION_GUIDE.md** - Migration patterns and best practices
- **README.md** (this file) - Quick reference

## Contributing

When adding new tests:

1. Follow the naming convention: `test_pg_<feature>`
2. Always call `setup_test_db()` and `bootstrap_schema()`
3. Parse JSON results with `serde_json`
4. Add assertions that validate behavior matches SQLite
5. Document any PostgreSQL-specific workarounds
6. Update TEST_PORT_STATUS.md progress

## Troubleshooting

### Tests Won't Compile
```bash
# Clean build
cargo clean
cargo pgrx test
```

### Tests Fail with "relation does not exist"
Ensure `setup_test_db()` is called at the start of your test.

### JSON Parsing Errors
```rust
// Use serde_json for all results
let json: serde_json::Value = serde_json::from_str(&result)
    .expect("Failed to parse JSON");
```

### Module Not Found
```rust
// Use explicit path for test_common.rs
#[path = "test_common.rs"]
mod common;
```

## Performance Expectations

Tests should complete within:
- Setup: <10ms per test
- Simple query: <5ms
- Complex query: <20ms
- FTS query: <30ms
- Transaction: <10ms

PostgreSQL has ~2-5ms overhead vs SQLite in-memory, which is acceptable for validation.

## References

- pgrx: https://github.com/pgcentralfoundation/pgrx
- PostgreSQL FTS: https://www.postgresql.org/docs/current/textsearch.html
- Original Mentat: https://github.com/mozilla/mentat
