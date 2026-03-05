# pg_mentat SQL Function API

This document describes the SQL functions exposed by the pg_mentat extension for querying and manipulating Mentat datalog data from PostgreSQL.

## Core Functions

### `mentat_transact(edn_tx TEXT) → TEXT`

Process an EDN transaction and return a transaction report.

**Arguments:**
- `edn_tx`: EDN-formatted transaction string containing add/retract operations

**Returns:** JSON string with transaction metadata:
```json
{
  "tx-id": 1001,
  "tx-instant": null,
  "tempids": {"person1": 100, "person2": 101},
  "datoms-inserted": 4
}
```

**Example:**
```sql
SELECT mentat.mentat_transact('
  [[:db/add "person1" :person/name "Alice"]
   [:db/add "person1" :person/age 30]]
');
```

**Transaction Formats:**

1. **Vector notation** - Explicit add/retract operations:
   ```edn
   [[:db/add entity-id attribute value]
    [:db/retract entity-id attribute value]]
   ```

2. **Map notation** - Implicit additions:
   ```edn
   [{:db/id "tempid"
     :person/name "Alice"
     :person/age 30}]
   ```

**Entity IDs:**
- Numeric: `100` (existing entity)
- Tempid: `"person1"` (allocated automatically)
- Ident: `:db/ident` (resolved from schema)

### `mentat_query(query TEXT, inputs JSONB) → JSONB`

Execute a Datalog query and return results as structured JSON.

**Arguments:**
- `query`: Datalog query string in EDN format
- `inputs`: Query input parameters (currently unused, pass `'{}'`)

**Returns:** JSON object with columns and results:
```json
{
  "columns": ["?name", "?age"],
  "results": [
    ["Alice", 30],
    ["Bob", 25]
  ]
}
```

**Example:**
```sql
SELECT mentat.mentat_query('
  [:find ?name ?age
   :where
   [?e :person/name ?name]
   [?e :person/age ?age]]
', '{}'::jsonb);
```

**Query Clauses:**

- **:find** - Variables to return
  ```edn
  [:find ?name ?age]          -- Relation (multiple rows)
  [:find [?name ...]]          -- Collection (single column)
  [:find [?name ?age]]         -- Tuple (single row)
  [:find ?name .]              -- Scalar (single value)
  ```

- **:where** - Pattern matching
  ```edn
  [?e :person/name ?name]      -- Bind variable
  [?e :person/age 30]          -- Match constant
  [?e :person/friend ?f]       -- Join entities
  ```

**Limitations:**
- Current implementation handles basic patterns only
- No support for rules, aggregates, or advanced operators yet
- Full query engine integration planned for future releases

### `mentat_schema() → JSONB`

Return complete schema information for all attributes.

**Returns:** JSON object mapping attribute idents to their properties:
```json
{
  ":person/name": {
    "entid": 65,
    "valueType": "string",
    "cardinality": "one",
    "unique": null,
    "indexed": true,
    "fulltext": false,
    "component": false,
    "noHistory": false
  },
  ":person/age": {
    "entid": 66,
    "valueType": "long",
    "cardinality": "one",
    "unique": null,
    "indexed": false,
    "fulltext": false,
    "component": false,
    "noHistory": false
  }
}
```

**Example:**
```sql
-- Get all schema attributes
SELECT mentat.mentat_schema();

-- Check if an attribute exists
SELECT mentat.mentat_schema() ? ':person/name';

-- Get properties of a specific attribute
SELECT mentat.mentat_schema()->':person/name';

-- List all string attributes
SELECT key, value
FROM jsonb_each(mentat.mentat_schema())
WHERE value->>'valueType' = 'string';
```

**Schema Properties:**

- **entid**: Numeric entity ID for the attribute
- **valueType**: Data type (ref, boolean, long, double, string, keyword, uuid, bytes, instant)
- **cardinality**: "one" (single value) or "many" (collection)
- **unique**: null, "value" (unique but not identity), or "identity" (unique + upsert)
- **indexed**: Whether attribute is indexed for queries
- **fulltext**: Whether attribute supports full-text search
- **component**: Whether values are components (cascade delete)
- **noHistory**: Whether to skip history tracking

### `mentat_entity(entity_id BIGINT) → JSONB`

Fetch all current datoms for a specific entity.

**Arguments:**
- `entity_id`: The numeric entity ID to fetch

**Returns:** JSON object with entity attributes and values:
```json
{
  ":db/id": 100,
  ":person/name": "Alice",
  ":person/age": 30,
  ":person/email": "alice@example.com"
}
```

**Example:**
```sql
-- Get entity by ID
SELECT mentat.mentat_entity(100);

-- Get specific attribute
SELECT mentat.mentat_entity(100)->':person/name';

-- Check if entity has an attribute
SELECT mentat.mentat_entity(100) ? ':person/email';

-- Find entity ID by unique attribute, then fetch full entity
SELECT mentat.mentat_entity(e)
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.ident = 'person:email'
  AND d.v = encode('alice@example.com', 'UTF8')
  AND d.added = true
LIMIT 1;
```

**Cardinality-many attributes:**

When an entity has multiple values for a cardinality-many attribute, they're returned as a JSON array:
```json
{
  ":db/id": 100,
  ":person/name": "Alice",
  ":person/tags": ["engineer", "manager", "mentor"]
}
```

### `mentat_pull(pattern TEXT, entity_id BIGINT) → JSONB`

Pull entity data using a pull pattern (stub implementation).

**Status:** Partially implemented. Currently returns basic entity data but doesn't fully parse pull patterns.

**Arguments:**
- `pattern`: EDN pull pattern (currently parsed but not fully processed)
- `entity_id`: The entity ID to pull data for

**Future Implementation:**

Full pull pattern support will include:
```edn
[:person/name]                 -- Select specific attributes
[:person/*]                    -- All attributes in namespace
[*]                            -- All attributes
[:person/name :as "fullname"]  -- Rename attribute
{:person/friend [:person/name]} -- Recursive pulls
```

## Data Types

### Value Type Mapping

| Mentat Type | PostgreSQL Type | JSON Type | Notes |
|-------------|----------------|-----------|-------|
| :db.type/boolean | BYTEA (1 byte) | boolean | 0=false, 1=true |
| :db.type/long | BYTEA (8 bytes) | number | i64 little-endian |
| :db.type/string | BYTEA (UTF-8) | string | Variable length |
| :db.type/keyword | BYTEA (UTF-8) | string | Stored without leading : |
| :db.type/ref | BYTEA (8 bytes) | number | Entity ID reference |
| :db.type/instant | BYTEA | string | ISO 8601 timestamp (TBD) |
| :db.type/double | BYTEA (8 bytes) | number | f64 little-endian (TBD) |
| :db.type/uuid | BYTEA (16 bytes) | string | RFC 4122 format (TBD) |
| :db.type/bytes | BYTEA | string | Base64 encoded (TBD) |

**Note:** Types marked (TBD) are not yet implemented in encode/decode functions.

## Error Handling

All functions return errors as PostgreSQL exceptions:

```sql
-- Invalid EDN syntax
SELECT mentat.mentat_transact('[not valid edn]');
-- ERROR: Failed to parse EDN

-- Non-existent attribute
SELECT mentat.mentat_transact('[[:db/add 1 :invalid/attr "value"]]');
-- ERROR: Failed to resolve attribute

-- Type mismatch
SELECT mentat.mentat_transact('[[:db/add 1 :person/age "not a number"]]');
-- ERROR: Value type mismatch
```

## Performance Considerations

### Query Optimization

1. **Use indexed attributes** - Define `:db/index true` for frequently queried attributes
2. **Limit result sets** - Use LIMIT in SQL or :limit in Datalog (when supported)
3. **Batch transactions** - Combine multiple operations in a single transaction
4. **Cache schema** - Call `mentat_schema()` once and cache results

### Indexing Strategy

The datoms table has four covering indexes for different access patterns:
- **EAVT** - Entity lookup (mentat_entity)
- **AEVT** - Attribute scan (range queries)
- **AVET** - Unique lookups and joins
- **VAET** - Reverse reference navigation

Choose attributes to index based on query patterns:
```sql
-- Enable indexing for frequently queried attributes
UPDATE mentat.schema
SET indexed = true
WHERE ident IN ('person:email', 'order:date', 'product:sku');
```

## Integration Examples

### Application Patterns

**Entity CRUD:**
```sql
-- Create
SELECT mentat.mentat_transact('[{:person/name "Alice" :person/age 30}]');

-- Read
SELECT mentat.mentat_entity(100);

-- Update
SELECT mentat.mentat_transact('[[:db/add 100 :person/age 31]]');

-- Delete (retract)
SELECT mentat.mentat_transact('[[:db/retract 100 :person/age 31]]');
```

**Joins with Regular SQL:**
```sql
-- Combine mentat queries with traditional SQL tables
SELECT
    u.username,
    (mentat.mentat_entity(u.profile_entity_id))->':person/name' as full_name
FROM users u
WHERE u.active = true;
```

**Schema Evolution:**
```sql
-- Add new attribute
INSERT INTO mentat.schema (entid, ident, value_type, cardinality)
VALUES (
    mentat.allocate_entid('db.part/db'),
    'person:phone',
    'string',
    'one'
);
```

## Future Enhancements

Planned additions to the SQL API:

1. **Query optimization** - Full integration with mentat query algebrizer
2. **Pull patterns** - Complete pull API implementation
3. **Aggregates** - count, sum, min, max, avg in queries
4. **Rules** - Recursive datalog rules support
5. **Time travel** - asOf and since temporal queries
6. **Full-text search** - Integration with PostgreSQL FTS
7. **Explain** - Query plan visualization
8. **Bulk operations** - Efficient batch imports/exports

## See Also

- [Storage Schema Documentation](../sql/README.md)
- [Bootstrap Data](../sql/06_bootstrap_data.sql)
- [Test Examples](../test/sql/api_functions.sql)
