# SQL Function API Reference

Complete reference for pg_mentat SQL functions.

## Transaction Functions

### mentat_transact

Process an EDN transaction and return a transaction report.

**Signature:**
```sql
mentat.mentat_transact(edn_tx TEXT) → TEXT
```

**Arguments:**
- `edn_tx` - EDN-formatted transaction string containing add/retract operations

**Returns:** JSON string with transaction metadata:
```json
{
  "tx-id": 1001,
  "tx-instant": "2026-03-05T10:30:00.000Z",
  "tempids": {"person1": 100, "person2": 101},
  "datoms-inserted": 4
}
```

**Transaction Formats:**

1. Vector notation (explicit operations):
```sql
SELECT mentat.mentat_transact('[
  [:db/add "person1" :person/name "Alice"]
  [:db/add "person1" :person/age 30]
  [:db/retract 100 :person/age 29]
]');
```

2. Map notation (implicit additions):
```sql
SELECT mentat.mentat_transact('[
  {:db/id "person1"
   :person/name "Alice"
   :person/age 30}
]');
```

3. Mixed notation:
```sql
SELECT mentat.mentat_transact('[
  {:db/id "alice" :person/name "Alice"}
  [:db/add "alice" :person/friend "bob"]
  {:db/id "bob" :person/name "Bob"}
]');
```

**Entity IDs:**
- Numeric: `100` (existing entity)
- Tempid: `"person1"` (allocated automatically)
- Lookup ref: `[:person/email "alice@example.com"]` (resolve via unique attribute)
- Ident: `:db/ident` (resolved from schema)

**Schema Transactions:**
```sql
SELECT mentat.mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/doc "Person full name"}
]');
```

**Errors:**
- Invalid EDN syntax: `ERROR: Failed to parse EDN`
- Non-existent attribute: `ERROR: Failed to resolve attribute`
- Type mismatch: `ERROR: Value type mismatch`
- Unique constraint violation: `ERROR: Unique constraint violated`

**Examples:**

```sql
-- Add new person
SELECT mentat.mentat_transact('[
  {:db/id "alice"
   :person/name "Alice Anderson"
   :person/email "alice@example.com"
   :person/age 30}
]');

-- Update existing person (by lookup ref)
SELECT mentat.mentat_transact('[
  [:db/add [:person/email "alice@example.com"] :person/age 31]
]');

-- Retract attribute value
SELECT mentat.mentat_transact('[
  [:db/retract 100 :person/age 30]
]');

-- Add reference (relationship)
SELECT mentat.mentat_transact('[
  [:db/add 100 :person/friend 101]
]');

-- Cardinality-many attribute
SELECT mentat.mentat_transact('[
  {:db/id 100
   :person/tags ["engineer" "manager" "mentor"]}
]');
```

## Query Functions

### mentat_query

Execute a Datalog query and return results as structured JSON.

**Signature:**
```sql
mentat.mentat_query(query TEXT, inputs JSONB) → JSONB
```

**Arguments:**
- `query` - Datalog query string in EDN format
- `inputs` - Query input parameters (currently unused, pass `'{}'`)

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

**Query Clauses:**

`:find` - Variables to return:
```sql
-- Relation (multiple rows, multiple columns)
[:find ?name ?age ...]

-- Collection (single column)
[:find [?name ...]]

-- Tuple (single row)
[:find [?name ?age]]

-- Scalar (single value)
[:find ?name .]
```

`:where` - Pattern matching:
```sql
-- Basic pattern
[?e :person/name ?name]

-- Match constant
[?e :person/age 30]

-- Join entities
[?e :person/friend ?f]
[?f :person/name ?friend-name]
```

`:in` - Input parameters:
```sql
[:find ?name
 :in $ ?min-age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(>= ?age ?min-age)]]
```

**Examples:**

```sql
-- Find all people
SELECT mentat.mentat_query('
  [:find ?name ?email
   :where
   [?e :person/name ?name]
   [?e :person/email ?email]]
', '{}'::jsonb);

-- Find people over 25
SELECT mentat.mentat_query('
  [:find ?name ?age
   :where
   [?e :person/name ?name]
   [?e :person/age ?age]
   [(>= ?age 25)]]
', '{}'::jsonb);

-- Find friends of Alice
SELECT mentat.mentat_query('
  [:find ?friend-name
   :where
   [?alice :person/name "Alice"]
   [?alice :person/friend ?friend]
   [?friend :person/name ?friend-name]]
', '{}'::jsonb);

-- Find mutual friends
SELECT mentat.mentat_query('
  [:find ?name1 ?name2
   :where
   [?p1 :person/name ?name1]
   [?p2 :person/name ?name2]
   [?p1 :person/friend ?p2]
   [?p2 :person/friend ?p1]
   [(< ?p1 ?p2)]]
', '{}'::jsonb);
```

**Current Limitations:**
- No rules support yet
- No aggregates (count, sum, etc.)
- No query operators (limit, offset, order-by)
- Basic pattern matching only

### mentat_pull

Pull entity data using a pull pattern.

**Signature:**
```sql
mentat.mentat_pull(pattern TEXT, entity_id BIGINT) → JSONB
```

**Status:** Stub implementation. Returns basic entity data.

**Future Pull Pattern Support:**
```edn
[:person/name]                           -- Select specific attributes
[:person/*]                              -- All attributes in namespace
[*]                                      -- All attributes
[:person/name :as "fullname"]            -- Rename attribute
{:person/friend [:person/name]}          -- Recursive pulls
{:person/friend ...}                     -- Unlimited recursion
{:person/friend 3}                       -- Limited recursion
(:person/nickname :default "Unknown")    -- With defaults
(:person/email :limit 1)                 -- Limit cardinality-many
```

## Schema Functions

### mentat_schema

Return complete schema information for all attributes.

**Signature:**
```sql
mentat.mentat_schema() → JSONB
```

**Returns:** JSON object mapping attribute idents to properties:
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
  }
}
```

**Schema Properties:**
- `entid` - Numeric entity ID for the attribute
- `valueType` - Data type (ref, boolean, long, double, string, keyword, uuid, bytes, instant)
- `cardinality` - "one" (single value) or "many" (collection)
- `unique` - null, "value" (unique but not identity), or "identity" (unique + upsert)
- `indexed` - Whether attribute is indexed for queries
- `fulltext` - Whether attribute supports full-text search
- `component` - Whether values are components (cascade delete)
- `noHistory` - Whether to skip history tracking

**Examples:**

```sql
-- Get all schema
SELECT mentat.mentat_schema();

-- Check if attribute exists
SELECT mentat.mentat_schema() ? ':person/name';

-- Get specific attribute properties
SELECT mentat.mentat_schema()->':person/name';

-- List all string attributes
SELECT key, value
FROM jsonb_each(mentat.mentat_schema())
WHERE value->>'valueType' = 'string';

-- Find indexed attributes
SELECT key
FROM jsonb_each(mentat.mentat_schema())
WHERE (value->>'indexed')::boolean = true;

-- List attributes by cardinality
SELECT key, value->>'cardinality'
FROM jsonb_each(mentat.mentat_schema())
ORDER BY value->>'cardinality';
```

### mentat_entity

Fetch all current datoms for a specific entity.

**Signature:**
```sql
mentat.mentat_entity(entity_id BIGINT) → JSONB
```

**Arguments:**
- `entity_id` - The numeric entity ID to fetch

**Returns:** JSON object with entity attributes and values:
```json
{
  ":db/id": 100,
  ":person/name": "Alice",
  ":person/age": 30,
  ":person/email": "alice@example.com"
}
```

**Cardinality-Many:**
Multiple values returned as JSON array:
```json
{
  ":db/id": 100,
  ":person/name": "Alice",
  ":person/tags": ["engineer", "manager", "mentor"]
}
```

**Examples:**

```sql
-- Get entity by ID
SELECT mentat.mentat_entity(100);

-- Get specific attribute
SELECT mentat.mentat_entity(100)->':person/name';

-- Check if entity has attribute
SELECT mentat.mentat_entity(100) ? ':person/email';

-- Find entity ID by unique attribute, then fetch
SELECT e, mentat.mentat_entity(d.e)
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.ident = 'person:email'
  AND d.v = encode('alice@example.com', 'UTF8')
  AND d.added = true
LIMIT 1;

-- Get multiple entities
SELECT id, mentat.mentat_entity(id)
FROM generate_series(100, 105) AS id;
```

## EDN Type Functions

### edn_in / edn_out

Convert between EDN text and EdnValue type.

**Signatures:**
```sql
mentat.edn_in(text TEXT) → mentat.EdnValue
mentat.edn_out(value mentat.EdnValue) → TEXT
```

**Examples:**

```sql
-- Parse EDN
SELECT mentat.edn_in('42');
SELECT mentat.edn_in('{:name "Alice" :age 30}');
SELECT mentat.edn_in('[1 2 3 4 5]');

-- Convert to text
SELECT mentat.edn_out(mentat.edn_in('{:a 1}'));

-- Round-trip test
SELECT mentat.edn_out(mentat.edn_in('[1 2 3]')) = '[1 2 3]';
```

### edn_get

Extract value from map by key.

**Signature:**
```sql
mentat.edn_get(map mentat.EdnValue, key mentat.EdnValue) → mentat.EdnValue
```

**Examples:**

```sql
SELECT mentat.edn_get(
  mentat.edn_in('{:name "Alice" :age 30}'),
  mentat.edn_in(':name')
);
-- Result: "Alice"

SELECT mentat.edn_out(mentat.edn_get(
  mentat.edn_in('{:user/id 123 :user/email "a@b.com"}'),
  mentat.edn_in(':user/email')
));
-- Result: "a@b.com"
```

### edn_nth

Get element from vector by index.

**Signature:**
```sql
mentat.edn_nth(vector mentat.EdnValue, index INTEGER) → mentat.EdnValue
```

**Examples:**

```sql
SELECT mentat.edn_nth(mentat.edn_in('[10 20 30]'), 0);
-- Result: 10

SELECT mentat.edn_nth(mentat.edn_in('["a" "b" "c"]'), 2);
-- Result: "c"

-- Out of bounds returns NULL
SELECT mentat.edn_nth(mentat.edn_in('[1 2 3]'), 5);
-- Result: NULL
```

### edn_count

Get collection size.

**Signature:**
```sql
mentat.edn_count(collection mentat.EdnValue) → BIGINT
```

**Examples:**

```sql
SELECT mentat.edn_count(mentat.edn_in('[1 2 3 4 5]'));
-- Result: 5

SELECT mentat.edn_count(mentat.edn_in('{:a 1 :b 2 :c 3}'));
-- Result: 3

SELECT mentat.edn_count(mentat.edn_in('#{1 2 3}'));
-- Result: 3
```

### edn_contains

Check if element exists in collection.

**Signature:**
```sql
mentat.edn_contains(collection mentat.EdnValue, element mentat.EdnValue) → BOOLEAN
```

**Examples:**

```sql
SELECT mentat.edn_contains(
  mentat.edn_in('[1 2 3]'),
  mentat.edn_in('2')
);
-- Result: true

SELECT mentat.edn_contains(
  mentat.edn_in('#{:a :b :c}'),
  mentat.edn_in(':b')
);
-- Result: true

SELECT mentat.edn_contains(
  mentat.edn_in('{:name "Alice"}'),
  mentat.edn_in(':age')
);
-- Result: false
```

### edn_keys

Extract map keys as vector.

**Signature:**
```sql
mentat.edn_keys(map mentat.EdnValue) → mentat.EdnValue
```

**Examples:**

```sql
SELECT mentat.edn_out(mentat.edn_keys(
  mentat.edn_in('{:name "Alice" :age 30}')
));
-- Result: [:name :age]
```

### edn_values

Extract map values as vector.

**Signature:**
```sql
mentat.edn_values(map mentat.EdnValue) → mentat.EdnValue
```

**Examples:**

```sql
SELECT mentat.edn_out(mentat.edn_values(
  mentat.edn_in('{:name "Alice" :age 30}')
));
-- Result: ["Alice" 30]
```

## Type Predicates

Check EDN value types.

**Signatures:**
```sql
mentat.edn_is_nil(value mentat.EdnValue) → BOOLEAN
mentat.edn_is_boolean(value mentat.EdnValue) → BOOLEAN
mentat.edn_is_integer(value mentat.EdnValue) → BOOLEAN
mentat.edn_is_float(value mentat.EdnValue) → BOOLEAN
mentat.edn_is_text(value mentat.EdnValue) → BOOLEAN
mentat.edn_is_keyword(value mentat.EdnValue) → BOOLEAN
mentat.edn_is_vector(value mentat.EdnValue) → BOOLEAN
mentat.edn_is_list(value mentat.EdnValue) → BOOLEAN
mentat.edn_is_set(value mentat.EdnValue) → BOOLEAN
mentat.edn_is_map(value mentat.EdnValue) → BOOLEAN
```

**Examples:**

```sql
SELECT mentat.edn_is_integer(mentat.edn_in('42'));
-- Result: true

SELECT mentat.edn_is_map(mentat.edn_in('{:a 1}'));
-- Result: true

-- Filter by type
SELECT id, data
FROM events
WHERE mentat.edn_is_map(data);

-- Type-safe extraction
SELECT
  CASE
    WHEN mentat.edn_is_map(data) THEN mentat.edn_get(data, mentat.edn_in(':type'))
    ELSE NULL
  END as event_type
FROM events;
```

## Operators

### Equality

Compare EDN values.

**Operators:**
```sql
= (equals)
<> (not equals)
```

**Examples:**

```sql
SELECT mentat.edn_in('42') = mentat.edn_in('42');
-- Result: true

SELECT mentat.edn_in('{:a 1}') = mentat.edn_in('{:a 1}');
-- Result: true

SELECT mentat.edn_in('[1 2 3]') <> mentat.edn_in('[3 2 1]');
-- Result: true

-- Use in WHERE clauses
SELECT * FROM events
WHERE data = mentat.edn_in('{:type :login}');
```

## Data Type Mapping

| Mentat Type | PostgreSQL Type | JSON Type | Notes |
|-------------|----------------|-----------|-------|
| :db.type/boolean | BYTEA (1 byte) | boolean | 0=false, 1=true |
| :db.type/long | BYTEA (8 bytes) | number | i64 little-endian |
| :db.type/string | BYTEA (UTF-8) | string | Variable length |
| :db.type/keyword | BYTEA (UTF-8) | string | Stored without leading : |
| :db.type/ref | BYTEA (8 bytes) | number | Entity ID reference |
| :db.type/instant | BYTEA | string | ISO 8601 timestamp |
| :db.type/double | BYTEA (8 bytes) | number | f64 little-endian |
| :db.type/uuid | BYTEA (16 bytes) | string | RFC 4122 format |
| :db.type/bytes | BYTEA | string | Base64 encoded |

## Performance Considerations

### Query Optimization

1. **Use indexed attributes:**
```sql
UPDATE mentat.schema
SET indexed = true
WHERE ident IN ('person:email', 'order:date');
```

2. **Limit result sets:**
```sql
SELECT * FROM (
  SELECT mentat.mentat_entity(d.e)
  FROM mentat.datoms d
  WHERE d.a = 65
  LIMIT 100
) entities;
```

3. **Cache schema:**
```sql
CREATE TEMP TABLE schema_cache AS
SELECT mentat.mentat_schema();

-- Query from cache
SELECT * FROM schema_cache;
```

### Indexing Strategy

Four covering indexes for different access patterns:
- **EAVT** - Entity lookup (mentat_entity)
- **AEVT** - Attribute scan
- **AVET** - Unique lookups and joins
- **VAET** - Reverse reference navigation

## Integration Patterns

### Combine with Regular SQL

```sql
-- Join Mentat data with SQL tables
SELECT
  u.username,
  (mentat.mentat_entity(u.profile_id))->':person/name' as full_name
FROM users u
WHERE u.active = true;
```

### Entity CRUD Operations

```sql
-- Create
SELECT mentat.mentat_transact('[{:person/name "Alice"}]');

-- Read
SELECT mentat.mentat_entity(100);

-- Update
SELECT mentat.mentat_transact('[[:db/add 100 :person/age 31]]');

-- Delete (retract all attributes)
SELECT mentat.mentat_transact('[[:db/retractEntity 100]]');
```

### Schema Evolution

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

## Error Handling

All functions return PostgreSQL exceptions on error:

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

## Future Enhancements

Planned additions:
- Full pull pattern support
- Aggregates (count, sum, min, max, avg)
- Rules and recursive queries
- Time-travel (asOf, since)
- Full-text search integration
- Query plan visualization
- Bulk operations

## See Also

- [Quickstart Guide](../guides/quickstart.md)
- [Installation Guide](../installation/pg_mentat.md)
- [Datomic Compatibility](./datomic_compat.md)
- [Configuration Guide](../configuration/pg_mentat_config.md)
