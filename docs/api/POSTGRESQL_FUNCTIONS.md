# pg_mentat PostgreSQL Functions API Reference

Complete reference for all PostgreSQL functions provided by the pg_mentat extension.

## Core Functions

### mentat_query

Execute a Datalog query against the database.

**Signature:**
```sql
mentat_query(query TEXT, inputs JSONB) → JSONB
```

**Parameters:**
- `query` (TEXT): Datalog query string in EDN format
- `inputs` (JSONB): Query parameters including:
  - `inputs`: Array of values for `:in` clause bindings
  - `asOf`: Transaction ID for time-travel (as-of query)
  - `since`: Transaction ID for time-travel (since query)
  - `history`: Boolean to include retractions

**Returns:**
JSONB object with structure:
```json
{
  "columns": ["?var1", "?var2"],
  "results": [[val1, val2], [val3, val4], ...],
  "count": N
}
```

For scalar finds (`. `), returns:
```json
{
  "result": value
}
```

For collection finds (`[?var ...]`), returns:
```json
{
  "result": [val1, val2, val3]
}
```

For tuple finds (`[?var1 ?var2]`), returns:
```json
{
  "result": [val1, val2]
}
```

**Examples:**

```sql
-- Basic query
SELECT mentat_query(
  '[:find ?e ?name
    :where
    [?e :person/name ?name]]',
  '{}'::jsonb
);
-- Returns: {"columns": ["?e", "?name"], "results": [[10001, "Alice"], ...]}

-- Query with inputs
SELECT mentat_query(
  '[:find ?name
    :in ?min-age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    [(>= ?age ?min-age)]]',
  '{"inputs": [30]}'::jsonb
);

-- Scalar find
SELECT mentat_query(
  '[:find (count ?e) .
    :where [?e :person/name]]',
  '{}'::jsonb
);
-- Returns: {"result": 42}

-- As-of query (time travel)
SELECT mentat_query(
  '[:find ?name :where [?e :person/name ?name]]',
  '{"asOf": 1000005}'::jsonb
);

-- Since query (changes only)
SELECT mentat_query(
  '[:find ?e ?a ?v
    :where [?e ?a ?v]]',
  '{"since": 1000010}'::jsonb
);

-- History query (including retractions)
SELECT mentat_query(
  '[:find ?e ?a ?v ?tx ?added
    :where [?e ?a ?v ?tx ?added]]',
  '{"history": true}'::jsonb
);

-- Query with rules
SELECT mentat_query($$
[:find ?descendant-name
 :with [[(descendant ?ancestor ?desc)
         [?ancestor :family/child ?desc]]
        [(descendant ?ancestor ?desc)
         [?ancestor :family/child ?x]
         (descendant ?x ?desc)]]
 :in ?ancestor-id
 :where
 (descendant ?ancestor-id ?desc)
 [?desc :person/name ?descendant-name]]
$$, '{"inputs": [10001]}'::jsonb);

-- Query with aggregates
SELECT mentat_query(
  '[:find ?age (count ?e)
    :where [?e :person/age ?age]]',
  '{}'::jsonb
);
```

**Error Handling:**
Returns error as JSONB if query fails:
```json
{
  "error": "Error message"
}
```

---

### mentat_transact

Execute a transaction (insert, update, or retract data).

**Signature:**
```sql
mentat_transact(edn_tx TEXT) → TEXT
```

**Parameters:**
- `edn_tx` (TEXT): Transaction data in EDN format

**Returns:**
JSON string with structure:
```json
{
  "tx": 1000012,
  "tempids": {
    "tempid1": 10001,
    "tempid2": 10002
  }
}
```

**Transaction Formats:**

1. **Map format** (entity-centric):
```sql
SELECT mentat_transact($$
[{:db/id "alice"
  :person/name "Alice Johnson"
  :person/age 30
  :person/email "alice@example.com"}
 {:db/id "bob"
  :person/name "Bob Smith"
  :person/friend "alice"}]
$$);
```

2. **Vector format** (datom-centric):
```sql
SELECT mentat_transact($$
[[:db/add "alice" :person/name "Alice"]
 [:db/add "alice" :person/age 30]
 [:db/retract 10001 :person/age 29]
 [:db/retractEntity 10002]]
$$);
```

**Operations:**
- `:db/add` - Assert a fact
- `:db/retract` - Retract a specific fact
- `:db/retractEntity` - Retract all facts about an entity

**Tempids:**
- String tempids (e.g., `"alice"`) are allocated entity IDs
- Tempids can be referenced within the same transaction
- Return value includes mapping: `{"tempids": {"alice": 10001}}`

**Lookup Refs:**
Reference entities by unique attribute:
```sql
SELECT mentat_transact($$
[{:db/id [:person/email "alice@example.com"]
  :person/age 31}]
$$);
```

**Upsert Semantics:**
For attributes with `:db.unique/identity`:
- If entity with unique value exists → UPDATE
- If not → INSERT

For cardinality-one attributes:
- New value automatically retracts old value

**Examples:**

```sql
-- Define schema
SELECT mentat_transact($$
[{:db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one}
 {:db/ident :person/age
  :db/valueType :db.type/long
  :db/cardinality :db.cardinality/one}
 {:db/ident :person/email
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one
  :db/unique :db.unique/identity}]
$$);

-- Insert data
SELECT mentat_transact($$
[{:db/id "alice"
  :person/name "Alice"
  :person/age 30
  :person/email "alice@example.com"}]
$$);

-- Update using lookup ref
SELECT mentat_transact($$
[{:db/id [:person/email "alice@example.com"]
  :person/age 31}]
$$);

-- Retract specific attribute
SELECT mentat_transact($$
[[:db/retract 10001 :person/age 31]]
$$);

-- Retract entire entity
SELECT mentat_transact($$
[[:db/retractEntity 10001]]
$$);

-- Multiple operations
SELECT mentat_transact($$
[{:db/id "bob"
  :person/name "Bob"
  :person/age 28}
 [:db/add "bob" :person/friend 10001]
 [:db/retract 10002 :person/status "active"]]
$$);
```

---

### mentat_pull

Retrieve entity data using a pull pattern.

**Signature:**
```sql
mentat_pull(pattern TEXT, entity_id BIGINT) → JSONB
```

**Parameters:**
- `pattern` (TEXT): Pull pattern in EDN format
- `entity_id` (BIGINT): Entity ID to pull

**Returns:**
JSONB object with entity attributes:
```json
{
  ":db/id": 10001,
  ":person/name": "Alice",
  ":person/age": 30,
  ":person/friends": [
    {":db/id": 10002, ":person/name": "Bob"}
  ]
}
```

**Pull Patterns:**

1. **Simple attributes:**
```sql
SELECT mentat_pull('[:person/name :person/age]', 10001);
```

2. **Wildcard (all attributes):**
```sql
SELECT mentat_pull('[*]', 10001);
```

3. **Map specification (follow refs):**
```sql
SELECT mentat_pull(
  '[:person/name {:person/friends [:person/name :person/email]}]',
  10001
);
```

4. **Reverse lookup:**
```sql
-- Find all entities referencing this one
SELECT mentat_pull('[:person/name :person/_friends]', 10001);
-- :person/_friends finds entities with :person/friends pointing to 10001
```

5. **Recursive pull:**
```sql
-- Unbounded recursion
SELECT mentat_pull('[{:person/friends ...}]', 10001);

-- Bounded recursion (depth limit)
SELECT mentat_pull('[{:person/friends 3}]', 10001);
```

6. **Limits:**
```sql
-- Limit collection results
SELECT mentat_pull('[(:person/hobbies :limit 5)]', 10001);
```

7. **Defaults:**
```sql
-- Provide default for missing attributes
SELECT mentat_pull('[(:person/email :default "no-email")]', 10001);
```

8. **Rename:**
```sql
-- Rename attribute in result
SELECT mentat_pull('[(:person/name :as "fullName")]', 10001);
```

**Examples:**

```sql
-- Basic pull
SELECT mentat_pull('[:person/name :person/age]', 10001);
-- Returns: {":db/id": 10001, ":person/name": "Alice", ":person/age": 30}

-- Pull with wildcard
SELECT mentat_pull('[*]', 10001);
-- Returns all attributes

-- Pull with navigation
SELECT mentat_pull(
  '[:person/name {:person/friends [:person/name]}]',
  10001
);
-- Returns: {":person/friends": [{":person/name": "Bob"}, ...]}

-- Pull with reverse lookup
SELECT mentat_pull('[:person/name :person/_friends]', 10001);
-- Returns entities that have :person/friends pointing to 10001

-- Recursive pull (friends of friends)
SELECT mentat_pull('[{:person/friends 2}]', 10001);

-- Pull with limit and default
SELECT mentat_pull(
  '[(:person/hobbies :limit 3)
    (:person/email :default "none")]',
  10001
);
```

---

### mentat_entity

Get a simple entity map (all attributes).

**Signature:**
```sql
mentat_entity(entity_id BIGINT) → JSONB
```

**Parameters:**
- `entity_id` (BIGINT): Entity ID

**Returns:**
JSONB object with entity attributes:
```json
{
  ":db/id": 10001,
  ":person/name": "Alice",
  ":person/age": 30
}
```

**Example:**
```sql
SELECT mentat_entity(10001);
-- Equivalent to: SELECT mentat_pull('[*]', 10001);
```

**Note:** Prefer `mentat_pull` for more control over returned attributes.

---

## Schema Functions

### resolve_ident

Resolve a keyword ident to an entity ID.

**Signature:**
```sql
resolve_ident(ident TEXT) → BIGINT
```

**Parameters:**
- `ident` (TEXT): Keyword ident (e.g., `:person/name`)

**Returns:**
Entity ID (BIGINT), or NULL if not found

**Example:**
```sql
SELECT resolve_ident(':person/name');
-- Returns: 101 (attribute entity ID)

SELECT resolve_ident(':person/name.invalid');
-- Returns: NULL
```

---

### allocate_entid

Allocate a new entity ID from a partition.

**Signature:**
```sql
allocate_entid(partition TEXT) → BIGINT
```

**Parameters:**
- `partition` (TEXT): Partition name (e.g., `'db.part/user'`)

**Returns:**
New entity ID (BIGINT)

**Example:**
```sql
SELECT allocate_entid('db.part/user');
-- Returns: 10015 (next available ID in user partition)
```

**Note:** Typically not needed - `mentat_transact` allocates IDs automatically.

---

## Schema Inspection

### Query Schema

Schema is stored as data, so you can query it:

```sql
-- List all attributes
SELECT mentat_query(
  '[:find ?ident ?type ?card
    :where
    [?e :db/ident ?ident]
    [?e :db/valueType ?type-e]
    [?type-e :db/ident ?type]
    [?e :db/cardinality ?card-e]
    [?card-e :db/ident ?card]]',
  '{}'::jsonb
);

-- Find unique attributes
SELECT mentat_query(
  '[:find ?ident
    :where
    [?e :db/ident ?ident]
    [?e :db/unique]]',
  '{}'::jsonb
);

-- Find indexed attributes
SELECT mentat_query(
  '[:find ?ident
    :where
    [?e :db/ident ?ident]
    [?e :db/index true]]',
  '{}'::jsonb
);
```

### Direct Table Access

Schema stored in `mentat.schema` table:

```sql
-- View schema
SELECT
  entid,
  ident,
  value_type,
  cardinality,
  unique_constraint,
  indexed,
  fulltext,
  component
FROM mentat.schema
ORDER BY entid;

-- Find attribute by ident
SELECT * FROM mentat.schema WHERE ident = ':person/name';

-- Find ref-type attributes
SELECT * FROM mentat.schema WHERE value_type = 'ref';
```

---

## Internal Tables

### mentat.datoms

Core storage table for all facts.

**Schema:**
```sql
CREATE TABLE mentat.datoms (
  e BIGINT NOT NULL,           -- Entity ID
  a BIGINT NOT NULL,           -- Attribute ID
  v BYTEA NOT NULL,            -- Value (encoded based on type)
  tx BIGINT NOT NULL,          -- Transaction ID
  added BOOLEAN NOT NULL,      -- true = assertion, false = retraction
  value_type_tag SMALLINT NOT NULL,  -- Type tag for decoding value

  PRIMARY KEY (e, a, v, tx)
);
```

**Indexes:**
- EAVT: `(e, a, v, tx)`
- AEVT: `(a, e, v, tx)`
- AVET: `(a, v, e, tx)` - for indexed attributes

**Type Tags:**
| Tag | Type | Encoding |
|-----|------|----------|
| 0 | ref | i64 little-endian |
| 1 | boolean | Single byte (0/1) |
| 2 | long | i64 little-endian |
| 3 | double | f64 little-endian |
| 4 | instant | i64 microseconds since epoch |
| 7 | string | UTF-8 bytes |
| 8 | keyword | UTF-8 bytes (without leading colon) |
| 10 | uuid | 16 bytes |
| 11 | bytes | Raw bytes |

**Example Queries:**

```sql
-- Find all datoms for entity
SELECT * FROM mentat.datoms WHERE e = 10001 AND added = true;

-- Find all current values for attribute
SELECT e, v FROM mentat.datoms
WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':person/name')
  AND added = true;

-- View history for entity
SELECT
  s.ident AS attribute,
  d.v,
  d.tx,
  t.tx_instant,
  d.added
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
JOIN mentat.transactions t ON d.tx = t.tx
WHERE d.e = 10001
ORDER BY d.tx;
```

---

### mentat.schema

Attribute definitions.

**Schema:**
```sql
CREATE TABLE mentat.schema (
  entid BIGINT PRIMARY KEY,
  ident TEXT NOT NULL UNIQUE,
  value_type TEXT NOT NULL,
  cardinality TEXT NOT NULL,
  unique_constraint TEXT,
  indexed BOOLEAN NOT NULL DEFAULT false,
  fulltext BOOLEAN NOT NULL DEFAULT false,
  component BOOLEAN NOT NULL DEFAULT false
);
```

**Example:**
```sql
SELECT * FROM mentat.schema WHERE ident = ':person/email';
-- entid: 105
-- value_type: string
-- cardinality: one
-- unique_constraint: identity
-- indexed: true
```

---

### mentat.transactions

Transaction metadata.

**Schema:**
```sql
CREATE TABLE mentat.transactions (
  tx BIGINT PRIMARY KEY,
  tx_instant TIMESTAMPTZ NOT NULL
);
```

**Example:**
```sql
-- Recent transactions
SELECT * FROM mentat.transactions ORDER BY tx DESC LIMIT 10;

-- Find transaction by time
SELECT tx FROM mentat.transactions
WHERE tx_instant > '2024-01-15 10:00:00'
  AND tx_instant < '2024-01-15 11:00:00';
```

---

### mentat.idents

Keyword-to-entity-ID mappings.

**Schema:**
```sql
CREATE TABLE mentat.idents (
  ident TEXT PRIMARY KEY,
  entid BIGINT NOT NULL
);
```

**Example:**
```sql
-- Resolve ident
SELECT entid FROM mentat.idents WHERE ident = ':person/name';

-- List all idents
SELECT * FROM mentat.idents ORDER BY entid;
```

---

### mentat.partitions

Entity ID partitions.

**Schema:**
```sql
CREATE TABLE mentat.partitions (
  ident TEXT PRIMARY KEY,
  start_id BIGINT NOT NULL,
  next_id BIGINT NOT NULL
);
```

**Default Partitions:**
- `db.part/db` - System entities (1-99)
- `db.part/user` - User entities (10000+)
- `db.part/tx` - Transaction entities (1000000+)

**Example:**
```sql
SELECT * FROM mentat.partitions;
-- ident: db.part/user
-- start_id: 10000
-- next_id: 10025
```

---

## Performance Tips

### Use Indexes

Mark frequently-queried attributes with `:db/index true`:

```sql
SELECT mentat_transact($$
[{:db/id 105
  :db/index true}]
$$);
```

### Analyze Queries

Use `EXPLAIN ANALYZE` to understand query performance:

```sql
EXPLAIN ANALYZE
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);
```

### Batch Transactions

Group multiple assertions into one transaction:

```sql
-- GOOD: Single transaction
SELECT mentat_transact($$
[{:db/id "p1" :person/name "Alice"}
 {:db/id "p2" :person/name "Bob"}
 {:db/id "p3" :person/name "Carol"}]
$$);

-- BAD: Multiple transactions (slower)
SELECT mentat_transact('[{:db/id "p1" :person/name "Alice"}]');
SELECT mentat_transact('[{:db/id "p2" :person/name "Bob"}]');
SELECT mentat_transact('[{:db/id "p3" :person/name "Carol"}]');
```

### Use Pull Instead of Multiple Queries

```sql
-- GOOD: Single pull with navigation
SELECT mentat_pull(
  '[:person/name {:person/friends [:person/name :person/email]}]',
  10001
);

-- BAD: Multiple queries
SELECT mentat_pull('[:person/name]', 10001);
SELECT mentat_query(
  '[:find ?friend-name :where [10001 :person/friends ?f] [?f :person/name ?friend-name]]',
  '{}'::jsonb
);
```

---

## Error Messages

### Common Errors

**"Attribute not found"**
```
ERROR: Attribute :person/xyz not found in schema
```
**Solution:** Define the attribute before using it.

**"Type mismatch"**
```
ERROR: Type mismatch for attribute 105: expected type string (tag 7), got tag 2
```
**Solution:** Use correct value type (e.g., `"string"` not `42`).

**"Unique constraint violation"**
```
ERROR: Unique constraint violation: attribute 105 has unique constraint 'identity'
but value already exists for entity 10001
```
**Solution:** Use different value or rely on upsert behavior.

**"Cardinality violation"**
```
ERROR: Cardinality violation: attribute 103 has cardinality 'one'
but transaction contains 2 assertions for entity 10001
```
**Solution:** Only assert one value per cardinality-one attribute per transaction.

---

## See Also

- [Quickstart Guide](../getting_started/QUICKSTART.md) - Getting started tutorial
- [Concepts](../getting_started/CONCEPTS.md) - Understanding pg_mentat
- [Datalog Reference](./DATALOG_REFERENCE.md) - Datalog query language
- [mentatd Protocol](./MENTATD_PROTOCOL.md) - HTTP daemon API
- [Troubleshooting](../troubleshooting/COMMON_ISSUES.md) - Common issues
