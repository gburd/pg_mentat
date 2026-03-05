# PostgreSQL Storage Backend Functions

This document describes the PostgreSQL extension functions that implement Mentat's storage backend.

## Overview

The storage backend is implemented as **PostgreSQL extension functions** that run inside PostgreSQL via pgrx. These functions provide SQL-callable APIs for transacting data, executing queries, and pulling entity information.

**Architecture:** Extension functions, NOT a client library. All logic runs server-side.

## Functions

### 1. mentat_transact(edn_tx TEXT) -> TEXT

Process EDN transactions and persist datoms to the database.

**Signature:**
```sql
mentat_transact(edn_tx TEXT) -> TEXT
```

**Input:** EDN transaction vector
```edn
[[:db/add "user1" :person/name "Alice"]
 [:db/add "user1" :person/age 30]
 {:db/id "user2" :person/name "Bob" :person/email "bob@example.com"}]
```

**Output:** JSON TxReport
```json
{
  "tx-id": 536870913,
  "tx-instant": null,
  "tempids": {"user1": 100, "user2": 101},
  "datoms-inserted": 4
}
```

**Implementation Details:**

1. **Parse EDN** - Uses `edn::parse::value()` to parse transaction string
2. **Allocate TX ID** - Calls `mentat.allocate_entid('db.part/tx')`
3. **Create TX Record** - Inserts into `mentat.transactions`
4. **Process Entities**:
   - **Vector notation** `[:db/add e a v]` or `[:db/retract e a v]`
   - **Map notation** `{:db/id "tempid" :attr1 val1 ...}`
5. **Resolve Entity IDs**:
   - Integer entids: used directly
   - Tempids (strings): allocate from `db.part/user` partition
   - Keywords: resolve via `mentat.resolve_ident()`
6. **Encode Values** - Convert EDN values to BYTEA with type tags
7. **Insert Datoms** - Write to `mentat.datoms` table
8. **Build TxReport** - Return JSON with transaction metadata

**Value Encoding:**

| EDN Type | BYTEA Format | Type Tag |
|----------|--------------|----------|
| Boolean | 1 byte (0/1) | 1 |
| Integer | 8 bytes (i64 little-endian) | 2 |
| String | UTF-8 bytes | 7 |
| Keyword | UTF-8 bytes | 8 |

**Error Handling:**

- Invalid EDN syntax: parse error
- Unknown attribute ident: "Failed to resolve attribute"
- Allocation failure: "Failed to allocate entity ID"

**Example:**

```sql
SELECT mentat.mentat_transact('[
  [:db/add "alice" :person/name "Alice"]
  [:db/add "alice" :person/age 30]
]');
```

### 2. mentat_query(query TEXT, inputs JSONB) -> JSONB

Execute Datalog queries against the database.

**Signature:**
```sql
mentat_query(query TEXT, inputs JSONB) -> JSONB
```

**Input:** Datalog query string
```datalog
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]]
```

**Output:** JSON results
```json
{
  "query": "...",
  "results": [
    ["Alice", 30],
    ["Bob", 25]
  ]
}
```

**Status:** Stub implementation. Ready for integration with `mentat_query_algebrizer`.

**Next Steps:**
1. Parse query using algebrizer
2. Generate SQL from algebra tree
3. Execute via SPI
4. Format results as JSON

### 3. mentat_pull(pattern TEXT, entity_id BIGINT) -> JSONB

Pull entity data using pull patterns.

**Signature:**
```sql
mentat_pull(pattern TEXT, entity_id BIGINT) -> JSONB
```

**Input:**
- Pull pattern: `[:person/name :person/age {:person/friends [:person/name]}]`
- Entity ID: `100`

**Output:** JSON entity map
```json
{
  "pattern": "...",
  "entity": 100,
  "attributes": 2
}
```

**Implementation:**

1. Parse pull pattern (currently placeholder)
2. Query `mentat.datoms` for entity: `WHERE e = entity_id AND added = true`
3. Decode values from BYTEA
4. Build nested JSON structure
5. Handle recursive patterns (`:person/friends`)

**Status:** Basic implementation. Queries datoms but needs full pattern parsing.

**Next Steps:**
1. Integrate `mentat_query_pull` crate
2. Support wildcard patterns `[*]`
3. Support nested patterns and recursion
4. Handle cardinality-many attributes

## Helper Functions Used

These functions are defined in `/pg_mentat/sql/05_functions.sql`:

### allocate_entid(partition_name TEXT) -> BIGINT

Allocate a new entity ID from a partition.

```sql
SELECT mentat.allocate_entid('db.part/user');  -- Returns 100, 101, 102...
SELECT mentat.allocate_entid('db.part/tx');    -- Returns 536870913, 536870914...
```

### resolve_ident(keyword TEXT) -> BIGINT

Resolve a keyword ident to its entid.

```sql
SELECT mentat.resolve_ident('db:ident');       -- Returns 1
SELECT mentat.resolve_ident('person:name');    -- Returns entid for :person/name
```

### current_tx() -> BIGINT

Get or create current transaction ID.

```sql
SELECT mentat.current_tx();  -- Allocates from db.part/tx
```

## Database Schema

### mentat.datoms

Core fact table storing all assertions and retractions.

```sql
CREATE TABLE mentat.datoms (
    e BIGINT NOT NULL,              -- Entity ID
    a BIGINT NOT NULL,              -- Attribute ID
    v BYTEA NOT NULL,               -- Value (encoded)
    tx BIGINT NOT NULL,             -- Transaction ID
    added BOOLEAN NOT NULL,         -- true=assert, false=retract
    value_type_tag SMALLINT NOT NULL -- Type discriminator
);
```

### mentat.transactions

Transaction metadata.

```sql
CREATE TABLE mentat.transactions (
    tx_id BIGINT PRIMARY KEY,
    instant TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

### mentat.schema

Attribute definitions.

```sql
CREATE TABLE mentat.schema (
    entid BIGINT PRIMARY KEY,
    ident TEXT UNIQUE NOT NULL,
    value_type mentat.value_type NOT NULL,
    cardinality mentat.cardinality_type NOT NULL,
    unique_constraint mentat.unique_type,
    indexed BOOLEAN NOT NULL,
    fulltext BOOLEAN NOT NULL,
    component BOOLEAN NOT NULL,
    no_history BOOLEAN NOT NULL
);
```

## Usage Examples

### Basic Transaction

```sql
-- Transact a person
SELECT mentat.mentat_transact('[
  {:db/id "alice"
   :person/name "Alice"
   :person/age 30
   :person/email "alice@example.com"}
]');
```

### Vector Notation

```sql
-- Add attributes
SELECT mentat.mentat_transact('[
  [:db/add 100 :person/name "Alice"]
  [:db/add 100 :person/age 30]
]');

-- Retract attribute
SELECT mentat.mentat_transact('[
  [:db/retract 100 :person/email "old@example.com"]
]');
```

### Query (Placeholder)

```sql
-- Find all people with their names
SELECT mentat.mentat_query('
  [:find ?name
   :where
   [?e :person/name ?name]]
', '{}');
```

### Pull

```sql
-- Pull person entity
SELECT mentat.mentat_pull('[:person/name :person/age]', 100);
```

## Error Handling

All functions return `Result<T, Box<dyn Error>>` which pgrx converts to PostgreSQL errors:

**Common Errors:**

1. **Parse Errors**
   - "Transaction must be a vector of entities"
   - EDN syntax errors from parser

2. **Resolution Errors**
   - "Failed to resolve attribute"
   - "Failed to resolve ident"
   - "Unknown keyword ident: ..."

3. **Allocation Errors**
   - "Failed to allocate entity ID"
   - "Failed to allocate transaction ID"

4. **Type Errors**
   - "Invalid entity place"
   - "Invalid attribute"
   - "Unsupported value type"

## Performance Considerations

### Batching

Transactions process multiple entities in a single call, reducing round-trips.

### Tempid Tracking

Tempid map is maintained in memory during transaction processing, avoiding duplicate allocations.

### SPI Overhead

Each `Spi::run()` call has overhead. Future optimization: batch INSERT statements.

### Index Usage

Queries leverage existing indexes:
- `idx_mentat_eavt` for entity lookups
- `idx_mentat_aevt` for attribute scans
- `idx_mentat_avet` for value lookups
- `idx_mentat_vaet` for reverse references

## Future Enhancements

### 1. Expand Value Types

Add encoding for:
- Float (type tag 3)
- UUID (type tag 5)
- Instant (type tag 6)
- Ref (type tag 9)

### 2. Query Integration

Connect `mentat_query()` to:
- `mentat_query_algebrizer` - parse and plan queries
- `mentat_query_projector` - format results
- Generate optimized SQL

### 3. Pull Enhancement

Integrate `mentat_query_pull`:
- Full pattern parsing
- Nested pull expressions
- Wildcard support `[*]`
- Reverse navigation `[{:person/_friends [:person/name]}]`

### 4. Transaction Validation

Add schema validation:
- Check attribute exists
- Validate value type matches schema
- Enforce cardinality constraints
- Check unique constraints

### 5. Transaction Functions

Support transaction functions:
- `(transaction-tx)` - current TX ID
- `(tempid partition)` - allocate tempid
- Custom transaction functions

### 6. Batched Inserts

Optimize datom insertion:
- Build multi-row INSERT statements
- Reduce SPI call overhead
- Use PostgreSQL COPY for bulk loads

### 7. Error Detail

Enhance error messages:
- Include entity/attribute context
- Suggest corrections for typos
- Provide line numbers for parse errors

## Testing

### Unit Tests

```rust
#[pg_test]
fn test_transact_basic() {
    let result = Spi::get_one::<String>(
        "SELECT mentat.mentat_transact('[[:db/add 1 :db/ident :test/attr]]')"
    ).ok().flatten();
    assert!(result.is_some());
}
```

### Integration Tests

```sql
-- test/sql/transact.sql
CREATE EXTENSION pg_mentat;

-- Test basic transaction
SELECT mentat.mentat_transact('[
  {:db/id "u1" :person/name "Alice"}
]');

-- Verify datom was inserted
SELECT COUNT(*) FROM mentat.datoms WHERE e = 100;
```

## References

- Implementation: `/pg_mentat/src/functions/`
- SQL schema: `/pg_mentat/sql/`
- Helper functions: `/pg_mentat/sql/05_functions.sql`
- EDN entities: `/edn/src/entities.rs`
- Transaction core: `/transaction/src/lib.rs`
