# Test Migration Guide: SQLite → PostgreSQL

## Purpose

This guide documents the process of porting Mentat's test suite from SQLite to PostgreSQL's pg_mentat extension using pgrx.

## Architecture Changes

### SQLite Architecture
```
Test → new_connection("") → In-memory SQLite → Mentat API
```

### PostgreSQL Architecture
```
Test → pgrx SPI → PostgreSQL (test transaction) → pg_mentat extension → Mentat API
```

## Key Migrations

### 1. Test Setup

**Before (SQLite):**
```rust
#[test]
fn test_query() {
    let mut c = new_connection("").expect("Couldn't open conn.");
    let db = mentat_db::db::ensure_current_version(&mut c).expect("Couldn't open DB.");

    let results = q_uncached(&c, &db.schema, "[:find ?x :where [?x :db/ident ?ident]]", None)
        .expect("Query failed");
}
```

**After (PostgreSQL):**
```rust
#[pg_test]
fn test_pg_query() {
    setup_test_db().expect("Failed to setup test db");
    bootstrap_schema().expect("Failed to bootstrap schema");

    let result = Spi::get_one::<String>(
        "SELECT mentat.mentat_query('[:find ?x :where [?x :db/ident ?ident]]', '{}'::jsonb)"
    ).expect("Query failed");

    let json: serde_json::Value = serde_json::from_str(&result).expect("Failed to parse JSON");
}
```

### 2. Full-Text Search

**Before (SQLite FTS4):**
```rust
// Schema with FTS
CREATE VIRTUAL TABLE docs USING fts4(content);

// Query
SELECT * FROM docs WHERE docs MATCH 'search term';
```

**After (PostgreSQL):**
```rust
// Schema with tsvector
CREATE TABLE docs (
    id SERIAL PRIMARY KEY,
    content TEXT,
    content_tsv TSVECTOR
);

CREATE INDEX ON docs USING GIN(content_tsv);

// Query
SELECT * FROM docs
WHERE content_tsv @@ to_tsquery('english', 'search & term');
```

### 3. Recursive Queries

**Before (SQLite):**
```sql
WITH RECURSIVE ancestor(a, d) AS (
    SELECT parent, child FROM family
    UNION
    SELECT f.parent, a.d FROM family f, ancestor a
    WHERE f.child = a.a
)
SELECT * FROM ancestor;
```

**After (PostgreSQL with pgrx):**
```rust
let result = Spi::get_one::<String>(
    "SELECT mentat.mentat_query('
        [:find ?ancestor ?descendant
         :with
         [[(ancestor ?a ?d) [?a :family/child ?d]]
          [(ancestor ?a ?d) [?a :family/child ?x] (ancestor ?x ?d)]]
         :where (ancestor ?anc ?desc)]
    ', '{}'::jsonb)"
).expect("Query failed");
```

### 4. Time-Travel Queries

**Before (SQLite):**
```rust
// Manual transaction tracking
let tx1 = get_current_tx(&conn);
transact(&mut conn, data);
let db_at_tx1 = restore_db_at_tx(&conn, tx1);
```

**After (PostgreSQL):**
```rust
// Built-in temporal support
let result = Spi::get_one::<String>(
    &format!("SELECT mentat.mentat_query('
        [:find ?e ?v :where [?e :attr ?v]]
    ', '{{\"asOf\": {}}}'::jsonb)", tx_id)
).expect("Query failed");
```

## Common Patterns

### Pattern 1: Basic Query Test

```rust
#[pg_test]
fn test_name() {
    setup_test_db().expect("Failed to setup test db");
    bootstrap_schema().expect("Failed to bootstrap schema");

    // Add test data if needed
    Spi::run("SELECT mentat.mentat_transact('[...]')").expect("Transact failed");

    // Execute query
    let result = Spi::get_one::<String>(
        "SELECT mentat.mentat_query('[...]', '{}'::jsonb)"
    ).expect("Query failed");

    // Parse and assert
    let json: serde_json::Value = serde_json::from_str(&result).expect("Parse failed");
    assert_eq!(json["results"].as_array().unwrap().len(), expected_count);
}
```

### Pattern 2: Transaction Test

```rust
#[pg_test]
fn test_transaction() {
    setup_test_db().expect("Failed to setup test db");
    bootstrap_schema().expect("Failed to bootstrap schema");

    let result = transact("[[:db/add \"e1\" :attr \"value\"]]").expect("Transact failed");
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert!(json["tempids"].is_object());
    assert!(json["tx-data"].is_array());
}
```

### Pattern 3: Temporal Query Test

```rust
#[pg_test]
fn test_temporal() {
    setup_test_db().expect("Failed to setup test db");
    bootstrap_schema().expect("Failed to bootstrap schema");

    // Setup: Create data in tx1
    transact("[[:db/add \"e\" :attr \"val1\"]]").unwrap();
    let tx1 = get_latest_tx();

    // Modify in tx2
    transact("[[:db/add \"e\" :attr \"val2\"]]").unwrap();

    // Query as-of tx1
    let result = query_as_of("[:find ?v :where [?e :attr ?v]]", tx1);
    assert_eq!(parse_scalar(&result), "val1");
}
```

## Helper Functions

### test_common.rs

```rust
pub fn setup_test_db() -> Result<(), Box<dyn std::error::Error>>
pub fn bootstrap_schema() -> Result<(), Box<dyn std::error::Error>>
pub fn cleanup_test_db() -> Result<(), Box<dyn std::error::Error>>

pub fn query(query_edn: &str, inputs_json: &str) -> Result<String, Box<dyn std::error::Error>>
pub fn transact(tx_data: &str) -> Result<String, Box<dyn std::error::Error>>
pub fn entity(entid: i64) -> Result<String, Box<dyn std::error::Error>>
pub fn schema() -> Result<String, Box<dyn std::error::Error>>
```

## Type Mapping

| SQLite Type | PostgreSQL Type | EDN Type | Notes |
|-------------|----------------|----------|-------|
| INTEGER | BIGINT | Long | Entity IDs, integers |
| REAL | DOUBLE PRECISION | Double | Floating point |
| TEXT | TEXT | String/Keyword | Strings, keywords |
| BLOB | BYTEA | - | Binary data |
| NULL | NULL | Nil | Null values |
| - | TIMESTAMPTZ | Instant | Timestamps |
| - | UUID | Uuid | UUIDs |
| - | mentat.EdnValue | Various | Custom EDN type |

## Query Result Formats

### SQLite (Rust structs)
```rust
enum QueryResults {
    Rel(Vec<Vec<Binding>>),
    Scalar(Option<Binding>),
    Tuple(Option<Vec<Binding>>),
    Coll(Vec<Binding>),
}
```

### PostgreSQL (JSON)
```json
{
  "columns": ["?x", "?y"],
  "results": [
    [value1, value2],
    [value3, value4]
  ]
}
```

For scalar:
```json
{
  "result": value
}
```

## Performance Considerations

### SQLite Advantages
- In-memory operation (very fast)
- No network overhead
- Simpler type system

### PostgreSQL Advantages
- Better concurrent access
- More powerful query optimizer
- Native support for complex types
- Production-ready durability

### Expected Overhead
- Setup: ~2-5ms per test (vs <1ms SQLite)
- Query: ~0.5-2ms additional latency
- Transaction: ~1-3ms additional latency

**Mitigation:**
- Use connection pooling in production
- Batch operations where possible
- Leverage PostgreSQL's query caching

## Debugging Tips

### Enable SQL Logging
```rust
// In test
pgrx::log!("SQL: {}", sql_query);
```

### Check Transaction State
```sql
SELECT txid_current();
SELECT * FROM mentat.transactions ORDER BY tx DESC LIMIT 5;
```

### Inspect Datoms
```sql
SELECT e, a, v, tx, added FROM mentat.datoms WHERE e = <entity_id>;
```

### View Schema
```sql
SELECT * FROM mentat.schema;
SELECT * FROM mentat.idents;
```

## Common Pitfalls

### 1. Module Path Issues
**Problem:** `mod common;` doesn't find `test_common.rs`

**Solution:**
```rust
#[path = "test_common.rs"]
mod common;
```

### 2. JSON Parsing
**Problem:** Expecting Rust types, getting JSON strings

**Solution:**
```rust
let json: serde_json::Value = serde_json::from_str(&result)?;
let results = json["results"].as_array().expect("Expected array");
```

### 3. Transaction Isolation
**Problem:** Tests interfere with each other

**Solution:** Each `#[pg_test]` runs in its own transaction that rolls back automatically.

### 4. Schema Bootstrap
**Problem:** Tests fail because schema doesn't exist

**Solution:** Always call `setup_test_db()` and `bootstrap_schema()` at the start of each test.

### 5. EDN Escaping
**Problem:** Single quotes in EDN strings break SQL

**Solution:**
```rust
let sql = format!("SELECT mentat.mentat_query('{}', '{}'::jsonb)",
    query_edn.replace('\'', "''"),  // Double single quotes
    inputs_json.replace('\'', "''")
);
```

## Testing Checklist

Before marking a test as complete:

- [ ] Test compiles without warnings
- [ ] Test runs successfully via `cargo pgrx test`
- [ ] Assertions validate the same behavior as SQLite version
- [ ] Edge cases are handled (empty results, null values, errors)
- [ ] Performance is acceptable (<10x SQLite for similar operations)
- [ ] Test is properly documented with comments
- [ ] Test cleans up after itself (automatic via pgrx transactions)

## References

- pgrx documentation: https://github.com/pgcentralfoundation/pgrx
- PostgreSQL FTS: https://www.postgresql.org/docs/current/textsearch.html
- Original Mentat tests: `/tests/*.rs`, `/*/tests/*.rs`
- TEST_PORT_STATUS.md: Current progress tracking
