# pg_mentat PostgreSQL Functions API Reference

Complete reference for all PostgreSQL functions provided by the pg_mentat extension.

---

## Core Functions

These are the primary API entry points installed in the `public` schema.

---

### mentat_query

Execute a Datalog query against the database.

**Signature:**
```sql
mentat_query(query TEXT, inputs JSONB) -> JSONB
```

**Parameters:**
- `query` (TEXT): Datalog query string in EDN format
- `inputs` (JSONB): Query parameters including:
  - `inputs`: Array of values for `:in` clause bindings
  - `asOf`: Transaction ID for time-travel (as-of query)
  - `since`: Transaction ID for time-travel (since query)
  - `history`: Boolean to include retractions
  - `limit`: Maximum number of result rows
  - `offset`: Number of rows to skip

**Returns:**

For relation finds (`:find ?e ?name`):
```json
{
  "columns": ["?e", "?name"],
  "results": [[10001, "Alice"], [10002, "Bob"]],
  "count": 2
}
```

For scalar finds (`:find ?name .`):
```json
{
  "result": "Alice"
}
```

For collection finds (`:find [?name ...]`):
```json
{
  "result": ["Alice", "Bob", "Carol"]
}
```

For tuple finds (`:find [?e ?name]`):
```json
{
  "result": [10001, "Alice"]
}
```

**Examples:**

```sql
-- Basic query
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);

-- Query with input bindings
SELECT mentat_query(
  '[:find ?name :in ?min-age :where [?e :person/name ?name] [?e :person/age ?age] [(>= ?age ?min-age)]]',
  '{"inputs": [30]}'::jsonb
);

-- Scalar find (count)
SELECT mentat_query(
  '[:find (count ?e) . :where [?e :person/name]]',
  '{}'::jsonb
);

-- As-of query (time travel)
SELECT mentat_query(
  '[:find ?name :where [?e :person/name ?name]]',
  '{"asOf": 1000005}'::jsonb
);

-- Since query (changes since transaction)
SELECT mentat_query(
  '[:find ?e ?a ?v :where [?e ?a ?v]]',
  '{"since": 1000010}'::jsonb
);

-- History query (including retractions)
SELECT mentat_query(
  '[:find ?e ?a ?v ?tx ?added :where [?e ?a ?v ?tx ?added]]',
  '{"history": true}'::jsonb
);

-- Pagination
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{"limit": 10, "offset": 20}'::jsonb
);

-- Lookup ref as input binding
SELECT mentat_query(
  '[:find ?name :in ?e :where [?e :person/name ?name]]',
  '{"inputs": [[":person/email", "alice@example.com"]]}'::jsonb
);

-- Query with aggregates
SELECT mentat_query(
  '[:find ?age (count ?e) :where [?e :person/age ?age]]',
  '{}'::jsonb
);

-- Query with rules
SELECT mentat_query($$
[:find ?descendant-name
 :in ?ancestor-id
 :where
 (descendant ?ancestor-id ?desc)
 [?desc :person/name ?descendant-name]
 :rules
 [[(descendant ?ancestor ?desc)
   [?ancestor :family/child ?desc]]
  [(descendant ?ancestor ?desc)
   [?ancestor :family/child ?x]
   (descendant ?x ?desc)]]]
$$, '{"inputs": [10001]}'::jsonb);
```

**Temporal Options:**

| Key         | Type    | Description                                           |
|-------------|---------|-------------------------------------------------------|
| `"asOf"`    | integer | Only include datoms with `tx <= asOf`.                |
| `"since"`   | integer | Only include datoms with `tx > since`.                |
| `"history"` | boolean | If true, include retracted datoms (full history).     |

When both a Datalog `:limit` clause and an inputs `"limit"` are specified,
the inputs `"limit"` takes precedence.

**Error Conditions:**

| Error Code                        | Cause                                        |
|-----------------------------------|----------------------------------------------|
| EDN parse error                   | Malformed EDN in query string.               |
| `:db.error/attribute-not-found`   | Keyword in pattern not found in schema.      |
| Query compilation error           | Invalid Datalog syntax or unsupported clause. |

**Related Functions:** `mentat_transact`, `mentat_pull`, `mentat_entity`

---

### mentat_transact

Execute a transaction (insert, update, or retract data).

**Signature:**
```sql
mentat_transact(edn_tx TEXT) -> TEXT
```

**Parameters:**
- `edn_tx` (TEXT): Transaction data in EDN format

**Returns:**
JSON string with transaction report:
```json
{
  "tx-id": 1000012,
  "tx-instant": 1705312200000000,
  "tempids": {"alice": 10001, "bob": 10002},
  "datoms-inserted": 4
}
```

| Field             | Type    | Description                                       |
|-------------------|---------|---------------------------------------------------|
| `tx-id`           | integer | The transaction entity ID.                        |
| `tx-instant`      | integer | Timestamp as microseconds since epoch.            |
| `tempids`         | object  | Map of tempid strings to allocated entity IDs.    |
| `datoms-inserted` | integer | Number of datoms written.                         |

**Transaction Formats:**

1. **Map format** (entity-centric):
```sql
SELECT mentat_transact($$
[{:db/id "alice"
  :person/name "Alice Johnson"
  :person/age 30
  :person/email "alice@example.com"}
 {:db/id "bob"
  :person/name "Bob Smith"}]
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
- `:db/add` -- Assert a fact
- `:db/retract` -- Retract a specific fact
- `:db/retractEntity` -- Retract all facts about an entity
- `:db.fn/cas` -- Compare-and-swap (atomically update a value)

**Entity Place Resolution:**

| Type      | Example                                  | Behavior                              |
|-----------|------------------------------------------|---------------------------------------|
| Integer   | `10003`                                  | Direct entity ID.                     |
| String    | `"alice"`                                | Tempid -- allocated or reused within the transaction. |
| Keyword   | `:person/name`                           | Resolved via `mentat.idents`.         |
| Vector    | `[:person/email "alice@example.com"]`    | Lookup ref -- attribute must have `:db.unique/identity` or `:db.unique/value`. |

**Upsert Semantics:**
For attributes with `:db.unique/identity`:
- If entity with unique value exists -> UPDATE
- If not -> INSERT

For cardinality-one attributes:
- New value automatically retracts old value

**Examples:**

```sql
-- Define schema
SELECT mentat_transact($$
[[:db/add "name-attr" :db/ident :person/name]
 [:db/add "name-attr" :db/valueType :db.type/string]
 [:db/add "name-attr" :db/cardinality :db.cardinality/one]
 [:db/add "name-attr" :db/unique :db.unique/identity]
 [:db/add "name-attr" :db/index true]]
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

-- Compare-and-swap
SELECT mentat_transact($$
[[:db.fn/cas 10001 :person/age 30 31]]
$$);

-- Retract specific attribute
SELECT mentat_transact($$
[[:db/retract 10001 :person/age 31]]
$$);

-- Retract entire entity
SELECT mentat_transact($$
[[:db/retractEntity 10001]]
$$);
```

**Error Conditions:**

| Error Code                             | Cause                                                    |
|----------------------------------------|----------------------------------------------------------|
| `:db.error/invalid-transaction`        | Transaction is not a vector of entities.                 |
| `:db.error/allocation-failed`          | Partition is exhausted or missing.                       |
| `:db.error/attribute-not-found`        | Referenced attribute ident not in schema.                |
| `:db.error/wrong-type-for-attribute`   | Value type does not match the attribute's `:db/valueType`.|
| `:db.error/cardinality-violation`      | Multiple values for a cardinality-one attribute.         |
| `:db.error/unique-conflict`            | Value already exists for a unique attribute.             |
| `:db.error/lookup-ref-requires-unique` | Lookup ref used on an attribute without a unique constraint. |
| `:db.error/lookup-ref-not-found`       | Lookup ref did not match any existing entity.            |
| `:db.fn/cas failed`                    | Compare-and-swap old value does not match current value. |

**Related Functions:** `mentat_query`, `mentat_entity`, `mentat.import_edn`

---

### mentat_pull

Retrieve entity data using a pull pattern.

**Signature:**
```sql
mentat_pull(pattern TEXT, entity_id BIGINT) -> JSONB
```

**Parameters:**
- `pattern` (TEXT): Pull pattern in EDN format
- `entity_id` (BIGINT): Entity ID to pull

**Returns:**
JSONB object with entity attributes. Always includes `":db/id"`:
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

**Pull Pattern Elements:**

| Pattern                                | Description                                   |
|----------------------------------------|-----------------------------------------------|
| `[:person/name :person/age]`           | Specific attributes.                          |
| `[*]`                                  | Wildcard -- all attributes.                   |
| `[{:person/friends [:person/name]}]`   | Map spec -- follow refs with sub-pattern.     |
| `[:person/_friends]`                   | Reverse lookup (underscore prefix).           |
| `[{:person/friends ...}]`             | Unbounded recursive pull.                     |
| `[{:person/friends 3}]`               | Bounded recursive pull (depth limit).         |
| `[(:person/name :as "Name")]`         | Rename output key.                            |
| `[(:person/email :default "none")]`   | Default value if attribute is missing.        |
| `[(:person/tags :limit 5)]`           | Limit results for cardinality-many.           |
| `[(:person/tags :limit nil)]`         | Unlimited (remove default 1000 limit).        |
| `[* {:person/friends [:person/name]}]` | Wildcard with override for specific attrs.   |

**Behavior Notes:**
- Missing attributes are omitted (unless `:default` is specified).
- Cardinality-many attributes return JSON arrays.
- Component refs in wildcard pulls are recursively expanded.
- Non-component refs in wildcard pulls return `{":db/id": N}` stubs.
- Cycle detection prevents infinite loops; cycles return `{":db/id": N}`.
- Maximum recursion depth is 100 levels.
- Default limit for cardinality-many results is 1000.

**Examples:**

```sql
-- Basic pull
SELECT mentat_pull('[:person/name :person/age]', 10001);
-- {":db/id": 10001, ":person/name": "Alice", ":person/age": 30}

-- Wildcard pull
SELECT mentat_pull('[*]', 10001);

-- Pull with navigation
SELECT mentat_pull(
  '[:person/name {:person/friends [:person/name]}]',
  10001
);

-- Reverse lookup
SELECT mentat_pull('[:person/name :person/_friends]', 10001);

-- Recursive pull (friends of friends)
SELECT mentat_pull('[{:person/friends 2}]', 10001);

-- Pull with limit and default
SELECT mentat_pull(
  '[(:person/hobbies :limit 3) (:person/email :default "none")]',
  10001
);

-- Pull with rename
SELECT mentat_pull('[(:person/name :as "fullName")]', 10001);
```

**Error Conditions:**

| Error Code                          | Cause                                       |
|-------------------------------------|---------------------------------------------|
| `:db.error/invalid-pull-pattern`    | Pattern is not valid EDN or not a vector.   |
| `:db.error/invalid-limit`           | Limit value is negative or wrong type.      |
| `:db.error/data-corruption`         | Stored value bytes do not match expected format. |

**Related Functions:** `mentat_entity`, `mentat_query`

---

### mentat_entity

Get all current attributes for an entity as JSON.

**Signature:**
```sql
mentat_entity(entity_id BIGINT) -> JSONB
```

**Parameters:**
- `entity_id` (BIGINT): Entity ID

**Returns:**
JSONB object with all current attribute values. Always includes `":db/id"`.
Cardinality-many attributes are returned as arrays.
```json
{
  ":db/id": 10001,
  ":person/name": "Alice",
  ":person/age": 30,
  ":person/tags": ["developer", "manager"]
}
```

**Example:**
```sql
SELECT mentat_entity(10001);
```

**Error Conditions:**

| Error Code                    | Cause                                     |
|-------------------------------|-------------------------------------------|
| `:db.error/data-integrity`    | Missing column in schema join.            |
| `:db.error/data-corruption`   | Value bytes malformed for their type tag. |
| `:db.error/unsupported-type`  | Unknown value type tag encountered.       |

**Related Functions:** `mentat_pull`, `mentat_query`

---

### mentat_schema

Return the complete schema as JSON.

**Signature:**
```sql
mentat_schema() -> JSONB
```

**Returns:**
JSONB object keyed by attribute ident:
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

**Example:**
```sql
SELECT mentat_schema();
```

**Related Functions:** `mentat_transact`, `mentat_query_stats`

---

## Statistics and Monitoring Functions

---

### mentat_query_stats

Return query performance and database statistics.

**Signature:**
```sql
mentat_query_stats() -> JSONB
```

**Returns:**
```json
{
  "functions": [
    {
      "function": "mentat_query",
      "calls": 150,
      "total_duration_ms": 1875.0,
      "self_duration_ms": 1200.0,
      "avg_duration_ms": 12.5
    }
  ],
  "database_stats": {
    "total_datoms": 5000,
    "total_transactions": 42,
    "schema_attributes": 15,
    "partitions": {
      "db.part/user": {"next_entid": 10500, "used": 500}
    }
  }
}
```

**Example:**
```sql
SELECT mentat_query_stats();
```

**Notes:**
- Function-level stats require `pg_stat_user_functions` (set `track_functions = 'all'`
  in `postgresql.conf`).
- If `pg_stat_user_functions` is not available, the `functions` array will be empty.

**Related Functions:** `mentat_slow_queries`, `mentat_storage_stats`

---

### mentat_slow_queries

Return recent transaction history with datom counts.

**Signature:**
```sql
mentat_slow_queries(limit_count INTEGER DEFAULT 20) -> JSONB
```

**Parameters:**
- `limit_count` (INTEGER, default 20): Maximum number of transactions to return.
  If zero or negative, defaults to 20.

**Returns:**
JSONB array of transaction records:
```json
[
  {
    "tx": 1000042,
    "tx_instant": "2025-01-15 10:30:00+00",
    "datom_count": 150,
    "assertions": 140,
    "retractions": 10
  }
]
```

**Example:**
```sql
SELECT mentat_slow_queries(10);
```

**Related Functions:** `mentat_query_stats`, `mentat_storage_stats`

---

### mentat_storage_stats

Return database size and index statistics.

**Signature:**
```sql
mentat_storage_stats() -> JSONB
```

**Returns:**
```json
{
  "tables": {
    "mentat.datoms": {"size": "8192 bytes", "row_estimate": 5000},
    "mentat.schema": {"size": "8192 bytes", "row_estimate": 15}
  },
  "indexes": [
    {"name": "idx_datoms_eavt", "size": "16384 bytes"}
  ]
}
```

**Example:**
```sql
SELECT mentat_storage_stats();
```

**Related Functions:** `mentat_query_stats`, `mentat_slow_queries`

---

### mentat_stmt_cache_stats

Return prepared statement cache statistics.

**Signature:**
```sql
mentat_stmt_cache_stats() -> JSONB
```

**Returns:**
```json
{
  "size": 5,
  "total_hits": 142,
  "entries": [
    {"sql": "SELECT d.e, d.v ...", "hits": 42}
  ]
}
```

**Example:**
```sql
SELECT mentat_stmt_cache_stats();
```

**Notes:**
- The cache is per-backend (per PostgreSQL connection).
- Cache entries survive across SQL transactions within the same backend session.
- Schema changes via `mentat_transact` automatically clear the cache.

**Related Functions:** `mentat_stmt_cache_clear`

---

### mentat_stmt_cache_clear

Clear the prepared statement cache.

**Signature:**
```sql
mentat_stmt_cache_clear() -> TEXT
```

**Returns:** The string `"ok"`.

**Example:**
```sql
SELECT mentat_stmt_cache_clear();
```

**Notes:**
- Call this after schema changes if automatic cache invalidation did not
  occur (e.g., schema was modified directly via SQL rather than `mentat_transact`).
- The cache is per-backend; this only clears the current connection's cache.

**Related Functions:** `mentat_stmt_cache_stats`

---

## Helper Functions (mentat schema)

These functions are installed in the `mentat` schema and provide convenience
operations and batch utilities.

---

### mentat.batch

Execute multiple operations in a single EDN batch document.

**Signature:**
```sql
mentat.batch(edn_batch TEXT) -> JSONB
```

**Parameters:**
- `edn_batch` (TEXT): EDN vector of operation vectors.

**Supported Operations:**

| Operation   | Format                                          |
|-------------|-------------------------------------------------|
| `:query`    | `[:query [:find ?e :where [?e :person/name]]]`  |
| `:transact` | `[:transact [[:db/add "new" :person/name "X"]]]` |
| `:pull`     | `[:pull [:person/name] 10003]`                   |
| `:entity`   | `[:entity 10003]`                                |
| `:schema`   | `[:schema]`                                      |

**Returns:**
JSONB array with results for each operation:
```json
[
  {"type": "query", "results": [[100], [101]]},
  {"type": "transact", "result": {"tx-id": 1001, "tempids": {}}},
  {"type": "pull", "result": {":person/name": "Alice"}},
  {"type": "entity", "result": {":db/id": 101, ":person/name": "Bob"}},
  {"type": "schema", "result": {...}}
]
```

**Example:**
```sql
SELECT mentat.batch('[
  [:query [:find ?e :where [?e :person/name]]]
  [:entity 10003]
  [:schema]
]');
```

**Error Conditions:**

| Error Code                       | Cause                                            |
|----------------------------------|--------------------------------------------------|
| `:db.error/invalid-batch`        | Batch document is not an EDN vector.             |
| `:db.error/unknown-batch-op`     | Unrecognized operation keyword.                  |
| `:db.error/invalid-batch-op`     | Operation is not a vector starting with keyword. |
| `:db.error/batch-missing-arg`    | Required argument missing from operation.        |
| `:db.error/batch-invalid-arg`    | Argument has wrong type (e.g., non-integer ID).  |

**Related Functions:** `mentat_query`, `mentat_transact`, `mentat_pull`, `mentat_entity`

---

### mentat.lookup_by_ident

Look up an entity ID by a string attribute value.

**Signature:**
```sql
mentat.lookup_by_ident(attr_ident TEXT, value TEXT) -> BIGINT
```

**Parameters:**
- `attr_ident` (TEXT): Attribute ident (e.g., `':person/email'`)
- `value` (TEXT): The string value to search for

**Returns:** Entity ID or NULL if not found.

**Example:**
```sql
SELECT mentat.lookup_by_ident(':person/email', 'alice@example.com');
-- Returns: 10003
```

**Notes:** Only supports string-type attribute values (type tag 7).

**Related Functions:** `mentat.lookup_entity_by_attr`, `mentat_entity`

---

### mentat.entity_attrs

Get all attribute idents for an entity.

**Signature:**
```sql
mentat.entity_attrs(entity_id BIGINT) -> JSONB
```

**Parameters:**
- `entity_id` (BIGINT): The entity ID to query

**Returns:** JSONB array of attribute ident strings.
```json
[":person/name", ":person/email", ":person/age"]
```

**Example:**
```sql
SELECT mentat.entity_attrs(10003);
```

**Related Functions:** `mentat_entity`, `mentat_pull`

---

### mentat.attribute_values

Get all current values for an attribute across all entities.

**Signature:**
```sql
mentat.attribute_values(attr_ident TEXT) -> JSONB
```

**Parameters:**
- `attr_ident` (TEXT): Attribute ident (e.g., `':person/name'`)

**Returns:** JSONB array of distinct values. Supports string, long, boolean,
and keyword types.
```json
["Alice", "Bob", "Carol"]
```

**Example:**
```sql
SELECT mentat.attribute_values(':person/name');
```

**Related Functions:** `mentat_query`

---

### mentat.retract_entity

Retract all facts about an entity.

**Signature:**
```sql
mentat.retract_entity(entity_id BIGINT) -> BIGINT
```

**Parameters:**
- `entity_id` (BIGINT): The entity to retract

**Returns:** Number of facts retracted.

**Example:**
```sql
SELECT mentat.retract_entity(10003);
-- Returns: 3
```

**Error Conditions:**

| Error Code                         | Cause                                  |
|------------------------------------|----------------------------------------|
| `:db.error/nothing-to-retract`     | Entity has no current facts.           |
| `:db.error/attribute-not-found`    | Cannot resolve attribute entid to ident.|

**Related Functions:** `mentat_transact` (with `:db/retractEntity`)

---

### mentat.export_edn

Export entities to EDN transaction format.

**Signature:**
```sql
mentat.export_edn(entity_ids BIGINT[]) -> TEXT
```

**Parameters:**
- `entity_ids` (BIGINT[]): Array of entity IDs to export

**Returns:** EDN text suitable for re-import with `mentat.import_edn`:
```edn
[
  {:db/id 10003
   :person/name "Alice"
   :person/email "alice@example.com"}
  {:db/id 10004
   :person/name "Bob"}
]
```

**Example:**
```sql
SELECT mentat.export_edn(ARRAY[10003, 10004]);
```

**Notes:** Entities with no current facts are silently skipped.

**Related Functions:** `mentat.import_edn`, `mentat.export_all_edn`, `mentat.query_export_edn`

---

### mentat.import_edn

Import entities from EDN transaction data.

**Signature:**
```sql
mentat.import_edn(edn_data TEXT) -> JSONB
```

**Parameters:**
- `edn_data` (TEXT): EDN transaction data to import

**Returns:** JSONB transaction report (same format as `mentat_transact`).

**Example:**
```sql
SELECT mentat.import_edn('[
  {:db/id "alice" :person/name "Alice" :person/age 30}
]');
```

**Notes:** This is a convenience wrapper around `mentat_transact`.

**Related Functions:** `mentat.export_edn`, `mentat_transact`

---

### mentat.query_export_edn

Execute a query and export all matching entities to EDN.

**Signature:**
```sql
mentat.query_export_edn(query TEXT, inputs JSONB) -> TEXT
```

**Parameters:**
- `query` (TEXT): Datalog query that returns entity IDs in the first `:find` column
- `inputs` (JSONB): Query input bindings

**Returns:** EDN text of all matching entities.

**Example:**
```sql
SELECT mentat.query_export_edn(
  '[:find ?e :where [?e :person/name]]',
  '{}'::jsonb
);
```

**Related Functions:** `mentat.export_edn`, `mentat_query`

---

### mentat.export_all_edn

Export the entire database to EDN format.

**Signature:**
```sql
mentat.export_all_edn() -> TEXT
```

**Returns:** EDN text of all entities with current facts.

**Example:**
```sql
SELECT mentat.export_all_edn();
```

**Notes:** Can produce very large output for big databases. Consider using
`mentat.query_export_edn` with a targeted query for large datasets.

**Related Functions:** `mentat.export_edn`, `mentat.import_edn`

---

## Storage Functions (mentat schema)

Lower-level storage operations.

---

### mentat.alloc_entid

Allocate a new entity ID from a partition.

**Signature:**
```sql
mentat.alloc_entid(partition_name TEXT) -> BIGINT
```

**Parameters:**
- `partition_name` (TEXT): Partition name (e.g., `'db.part/user'`, `'db.part/tx'`)

**Returns:** The newly allocated entity ID.

**Example:**
```sql
SELECT mentat.alloc_entid('db.part/user');
-- Returns: 10005
```

**Note:** Typically not needed -- `mentat_transact` allocates IDs automatically.

**Related Functions:** `mentat.allocate_entid` (PL/pgSQL)

---

### mentat.resolve_ident_to_entid

Resolve a keyword ident to its entity ID.

**Signature:**
```sql
mentat.resolve_ident_to_entid(ident TEXT) -> BIGINT
```

**Parameters:**
- `ident` (TEXT): Keyword ident (e.g., `':person/name'`)

**Returns:** Entity ID or NULL if not found.

**Example:**
```sql
SELECT mentat.resolve_ident_to_entid(':person/name');
-- Returns: 65
```

**Related Functions:** `mentat.resolve_ident` (PL/pgSQL)

---

### mentat.lookup_entity_by_attr

Look up an entity by a unique attribute value (string-based).

**Signature:**
```sql
mentat.lookup_entity_by_attr(attr_ident TEXT, value_str TEXT) -> BIGINT
```

**Parameters:**
- `attr_ident` (TEXT): Attribute ident (e.g., `':person/email'`)
- `value_str` (TEXT): String value to match

**Returns:** Entity ID or NULL if not found.

**Example:**
```sql
SELECT mentat.lookup_entity_by_attr(':person/email', 'alice@example.com');
```

**Related Functions:** `mentat.lookup_by_ident`

---

### mentat.begin_transaction

Begin a low-level Mentat transaction by creating staging tables.

**Signature:**
```sql
mentat.begin_transaction() -> void
```

**Notes:** Creates temporary tables (`temp_exact_searches`, `temp_inexact_searches`,
`temp_search_results`) for staging datoms. Tables are dropped on commit.
This is an internal function; prefer `mentat_transact` for normal use.

**Related Functions:** `mentat.commit_transaction`

---

### mentat.commit_transaction

Commit a low-level Mentat transaction.

**Signature:**
```sql
mentat.commit_transaction(tx_id BIGINT) -> void
```

**Parameters:**
- `tx_id` (BIGINT): Transaction ID to commit

**Notes:** Moves staged datoms from temporary tables into `mentat.datoms` and
records the transaction. This is an internal function; prefer `mentat_transact`.

**Related Functions:** `mentat.begin_transaction`

---

### mentat.get_entity_datoms

Get all raw datoms for an entity.

**Signature:**
```sql
mentat.get_entity_datoms(entity_id BIGINT)
  -> TABLE(attribute BIGINT, value BYTEA, value_type SMALLINT, transaction BIGINT)
```

**Parameters:**
- `entity_id` (BIGINT): The entity ID to query

**Returns:** Set of rows with raw datom data (only current assertions, `added = true`).

**Example:**
```sql
SELECT * FROM mentat.get_entity_datoms(10003);
```

**Related Functions:** `mentat_entity`, `mentat_pull`

---

## Planner Functions (mentat schema)

Query optimization utilities.

---

### mentat.suggest_index

Suggest the optimal index for a datom access pattern.

**Signature:**
```sql
mentat.suggest_index(access_pattern TEXT) -> TEXT
```

**Parameters:**
- `access_pattern` (TEXT): Access pattern string

| Pattern              | Suggested Index     |
|----------------------|---------------------|
| `'e'`, `'ea'`, `'eav'`, `'eavt'` | `idx_mentat_eavt` |
| `'a'`, `'ae'`, `'aev'`, `'aevt'` | `idx_mentat_aevt` |
| `'av'`, `'ave'`, `'avet'`        | `idx_mentat_avet` |
| `'v'`, `'va'`, `'vae'`, `'vaet'` | `idx_mentat_vaet` |

**Example:**
```sql
SELECT mentat.suggest_index('av');
-- Returns: 'idx_mentat_avet'
```

**Related Functions:** `mentat.estimate_query_cost`, `mentat.analyze_query`

---

### mentat.estimate_query_cost

Estimate the cost of a datom query operation.

**Signature:**
```sql
mentat.estimate_query_cost(access_pattern TEXT, estimated_rows BIGINT) -> DOUBLE PRECISION
```

**Parameters:**
- `access_pattern` (TEXT): Access pattern string
- `estimated_rows` (BIGINT): Expected number of rows

**Returns:** Estimated cost multiplier (lower is better). Uses a logarithmic
cost model weighted by index effectiveness:
- Entity-first: 1.0x
- Attribute-first: 1.2x
- Attribute-Value: 1.1x
- Value-first: 2.0x

**Example:**
```sql
SELECT mentat.estimate_query_cost('e', 1000);
-- Returns: 3.0
```

**Related Functions:** `mentat.suggest_index`, `mentat.analyze_query`

---

### mentat.analyze_query

Analyze a SQL query and provide optimization hints.

**Signature:**
```sql
mentat.analyze_query(query_text TEXT) -> TEXT
```

**Parameters:**
- `query_text` (TEXT): SQL query string to analyze

**Returns:** Human-readable optimization hint string.

**Example:**
```sql
SELECT mentat.analyze_query('SELECT * FROM mentat.datoms WHERE a = 123');
-- Returns: 'Pattern: Attribute-first (use AEVT index)'
```

**Related Functions:** `mentat.suggest_index`, `mentat.get_index_info`

---

### mentat.get_index_info

Get information about all available mentat indexes.

**Signature:**
```sql
mentat.get_index_info()
  -> TABLE(index_name TEXT, access_pattern TEXT, use_when TEXT)
```

**Returns:** Set of rows describing each index:

| index_name       | access_pattern   | use_when                                    |
|------------------|------------------|---------------------------------------------|
| idx_mentat_eavt  | Entity-first     | Lookups by entity ID                        |
| idx_mentat_aevt  | Attribute-first  | Lookups by attribute                        |
| idx_mentat_avet  | Attribute-Value  | Lookups by attribute and value              |
| idx_mentat_vaet  | Value-first      | Reverse lookups (entities referring to another entity) |

**Example:**
```sql
SELECT * FROM mentat.get_index_info();
```

**Related Functions:** `mentat.suggest_index`

---

## PL/pgSQL Functions (mentat schema)

Low-level storage operations used internally by the Rust extension functions.

---

### mentat.allocate_entid

```sql
mentat.allocate_entid(partition_name TEXT) -> BIGINT
```

Increments `next_entid` in `mentat.partitions` and returns the previous value.
Raises an exception if the partition does not exist.

---

### mentat.resolve_ident

```sql
mentat.resolve_ident(keyword TEXT) -> BIGINT
```

Returns the entity ID for the given keyword ident from `mentat.idents`.

---

### mentat.fulltext_update_trigger

```sql
mentat.fulltext_update_trigger() -> TRIGGER
```

Trigger function that updates the `search_vector` tsvector column from
`text_value` using the English text search configuration.

---

## Internal Tables

### mentat.datoms

Core storage table for all facts (Entity-Attribute-Value-Transaction model).

```sql
CREATE TABLE mentat.datoms (
  e BIGINT NOT NULL,              -- Entity ID
  a BIGINT NOT NULL,              -- Attribute ID (references mentat.schema)
  v BYTEA NOT NULL,               -- Value (encoded based on type)
  tx BIGINT NOT NULL,             -- Transaction ID
  added BOOLEAN NOT NULL,         -- true = assertion, false = retraction
  value_type_tag SMALLINT NOT NULL -- Type tag for decoding value
);
```

**Indexes:**
- `idx_datoms_eavt`: `(e, a, value_type_tag, v, tx)` -- entity-first access
- `idx_datoms_aevt`: `(a, e, value_type_tag, v, tx)` -- attribute-first access
- `idx_datoms_avet`: `(a, value_type_tag, v, e, tx)` -- attribute-value access
- `idx_datoms_vaet`: `(v, a, e, tx) WHERE value_type_tag = 0` -- reverse ref lookup
- `idx_datoms_tx`: `(tx)` -- transaction lookup

---

### mentat.schema

Attribute definitions.

```sql
CREATE TABLE mentat.schema (
  entid BIGINT PRIMARY KEY,
  ident TEXT NOT NULL UNIQUE,
  value_type mentat.value_type NOT NULL,
  cardinality mentat.cardinality_type NOT NULL DEFAULT 'one',
  unique_constraint mentat.unique_type,
  indexed BOOLEAN NOT NULL DEFAULT FALSE,
  fulltext BOOLEAN NOT NULL DEFAULT FALSE,
  component BOOLEAN NOT NULL DEFAULT FALSE,
  no_history BOOLEAN NOT NULL DEFAULT FALSE
);
```

---

### mentat.transactions

Transaction metadata.

```sql
CREATE TABLE mentat.transactions (
  tx BIGINT PRIMARY KEY,
  tx_instant TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

---

### mentat.idents

Keyword-to-entity-ID mappings.

```sql
CREATE TABLE mentat.idents (
  ident TEXT PRIMARY KEY,
  entid BIGINT NOT NULL UNIQUE
);
```

---

### mentat.partitions

Entity ID allocation partitions.

```sql
CREATE TABLE mentat.partitions (
  name TEXT PRIMARY KEY,
  start_entid BIGINT NOT NULL,
  end_entid BIGINT NOT NULL,
  next_entid BIGINT NOT NULL,
  allow_excision BOOLEAN NOT NULL DEFAULT FALSE
);
```

**Default Partitions:**

| Partition       | Start    | End       | Purpose                  |
|-----------------|----------|-----------|--------------------------|
| `db.part/db`    | 0        | 10000     | System/schema entities   |
| `db.part/user`  | 10000    | 1000000   | User-defined entities    |
| `db.part/tx`    | 1000000  | 2000000   | Transaction entities     |

---

### mentat.fulltext

Full-text search support table.

```sql
CREATE TABLE mentat.fulltext (
  rowid BIGSERIAL PRIMARY KEY,
  text_value TEXT NOT NULL,
  search_vector TSVECTOR
);
```

Automatically maintained by the `fulltext_update_trigger`. Indexed with GIN
for fast text search.

---

## Value Type Reference

All values are stored as BYTEA with an integer type tag:

| Type Tag | Value Type | EDN Literal        | Encoding                     |
|----------|------------|--------------------|------------------------------|
| 0        | ref        | `10003` or ident   | i64 little-endian (8 bytes)  |
| 1        | boolean    | `true` / `false`   | Single byte (0 or 1)        |
| 2        | long       | `42`               | i64 little-endian (8 bytes)  |
| 3        | double     | `3.14`             | f64 little-endian (8 bytes)  |
| 4        | instant    | `#inst "..."`      | i64 microseconds since epoch (LE, 8 bytes) |
| 7        | string     | `"hello"`          | UTF-8 bytes                  |
| 8        | keyword    | `:person/name`     | UTF-8 bytes (without leading colon) |
| 10       | uuid       | `#uuid "..."`      | 16 bytes (big-endian)        |
| 11       | bytes      | N/A                | Raw bytes                    |

---

## Schema Attribute Properties

Properties available when defining schema attributes via `mentat_transact`:

| Property            | Type    | Required | Default | Description                             |
|---------------------|---------|----------|---------|-----------------------------------------|
| `:db/ident`         | keyword | Yes      | --      | The attribute's keyword name.           |
| `:db/valueType`     | ref     | Yes      | --      | One of the `:db.type/*` values.         |
| `:db/cardinality`   | ref     | No       | `one`   | `:db.cardinality/one` or `:db.cardinality/many`. |
| `:db/unique`        | ref     | No       | null    | `:db.unique/value` or `:db.unique/identity`. |
| `:db/index`         | boolean | No       | false   | Whether to index this attribute.        |
| `:db/fulltext`      | boolean | No       | false   | Whether to enable full-text search.     |
| `:db/isComponent`   | boolean | No       | false   | Whether referenced entities are components. |
| `:db/noHistory`     | boolean | No       | false   | Whether to skip history for this attr.  |
| `:db/doc`           | string  | No       | null    | Documentation string.                   |

**Value Types (`:db/valueType`):**

| Keyword              | Description                       |
|----------------------|-----------------------------------|
| `:db.type/ref`       | Reference to another entity       |
| `:db.type/keyword`   | Keyword (e.g., `:status/active`)  |
| `:db.type/long`      | 64-bit integer                    |
| `:db.type/double`    | 64-bit floating point             |
| `:db.type/string`    | UTF-8 string                      |
| `:db.type/boolean`   | true or false                     |
| `:db.type/instant`   | Point in time                     |
| `:db.type/uuid`      | UUID                              |
| `:db.type/bytes`     | Raw byte array                    |

---

## See Also

- [Datalog Reference](./DATALOG_REFERENCE.md) -- Datalog query language details
- [mentatd Protocol](./MENTATD_PROTOCOL.md) -- HTTP daemon API
