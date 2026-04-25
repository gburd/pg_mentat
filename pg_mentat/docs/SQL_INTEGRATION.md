# SQL Integration Guide

pg_mentat exposes a Datomic-compatible Datalog engine as a set of PostgreSQL functions. Every operation -- schema definition, transactions, queries, pulls, and introspection -- is available through standard SQL function calls from any PostgreSQL client.

This guide covers the complete SQL function API, EDN helper functions, batch operations, hybrid SQL/Datalog patterns, and operational tools.

## Table of Contents

- [Core API Functions](#core-api-functions)
  - [mentat_transact](#mentat_transact)
  - [mentat_query](#mentat_query)
  - [mentat_pull](#mentat_pull)
  - [mentat_pull_many](#mentat_pull_many)
  - [mentat_entity](#mentat_entity)
  - [mentat_schema](#mentat_schema)
  - [mentat_explain](#mentat_explain)
- [EDN Helper Functions](#edn-helper-functions)
  - [mentat.batch](#mentatbatch)
  - [mentat.export_edn](#mentatexport_edn)
  - [mentat.import_edn](#mentatimport_edn)
  - [mentat.query_export_edn](#mentatquery_export_edn)
  - [mentat.export_all_edn](#mentatexport_all_edn)
- [Entity Helper Functions](#entity-helper-functions)
  - [mentat.lookup_by_ident](#mentatloookup_by_ident)
  - [mentat.entity_attrs](#menatentity_attrs)
  - [mentat.attribute_values](#mentatattribute_values)
  - [mentat.retract_entity](#mentatretract_entity)
- [Operational Functions](#operational-functions)
  - [mentat_query_stats](#mentat_query_stats)
  - [mentat_storage_stats](#mentat_storage_stats)
  - [mentat_slow_queries](#mentat_slow_queries)
  - [mentat_stmt_cache_stats](#mentat_stmt_cache_stats)
  - [mentat_stmt_cache_clear](#mentat_stmt_cache_clear)
- [EDN Functions (edn type)](#edn-functions-edn-type)
  - [edn_get, edn_nth, edn_count](#collection-access)
  - [Type predicates](#type-predicates)
  - [edn_contains, edn_keys, edn_values](#collection-operations)
- [Temporal Queries](#temporal-queries)
- [Pagination](#pagination)
- [Hybrid SQL/Datalog Patterns](#hybrid-sqldatalog-patterns)
- [GUC Configuration Parameters](#guc-configuration-parameters)

---

## Core API Functions

### mentat_transact

Process EDN transactions: assert facts, retract facts, and retract entire entities.

```sql
mentat_transact(edn_tx TEXT) -> TEXT
```

**Schema definition:**

```sql
SELECT mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
  {:db/ident :person/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
  {:db/ident :person/friends
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many}
]');
```

**Assert facts (map form):**

```sql
SELECT mentat_transact('[
  {:db/id "alice"
   :person/name "Alice"
   :person/email "alice@example.com"
   :person/age 30}
  {:db/id "bob"
   :person/name "Bob"
   :person/email "bob@example.com"
   :person/friends "alice"}
]');
```

**Assert facts (list form):**

```sql
SELECT mentat_transact('[
  [:db/add "alice" :person/name "Alice"]
  [:db/add "alice" :person/email "alice@example.com"]
]');
```

**Retract individual facts:**

```sql
SELECT mentat_transact('[
  [:db/retract 10042 :person/name "Alice"]
]');
```

**Retract entity (all facts):**

```sql
SELECT mentat_transact('[
  [:db/retractEntity 10042]
]');
```

**Return value:** JSON string containing the transaction report with `tx-id`, `tempids` map, and other metadata.

**Schema attributes:**

| Attribute | Type | Description |
|-----------|------|-------------|
| `:db/ident` | keyword | Attribute name (required) |
| `:db/valueType` | ref | One of `:db.type/string`, `:db.type/long`, `:db.type/double`, `:db.type/boolean`, `:db.type/instant`, `:db.type/keyword`, `:db.type/ref`, `:db.type/uuid`, `:db.type/bytes` |
| `:db/cardinality` | ref | `:db.cardinality/one` or `:db.cardinality/many` |
| `:db/unique` | ref | `:db.unique/value` or `:db.unique/identity` (optional) |
| `:db/index` | boolean | Enable AVET index for this attribute (optional) |
| `:db/fulltext` | boolean | Enable full-text search (optional) |
| `:db/isComponent` | boolean | Mark as component (cascade delete) (optional) |
| `:db/noHistory` | boolean | Disable history tracking (optional) |
| `:db/doc` | string | Documentation string (optional) |

---

### mentat_query

Execute a Datalog query with optional inputs and temporal modifiers.

```sql
mentat_query(query TEXT, inputs JSONB) -> JSONB
```

**Basic query:**

```sql
SELECT mentat_query('
  [:find ?name ?email
   :where
   [?e :person/name ?name]
   [?e :person/email ?email]]
', '{}');
```

**With input parameters:**

```sql
SELECT mentat_query('
  [:find ?name
   :in $ ?min-age
   :where
   [?e :person/name ?name]
   [?e :person/age ?age]
   [(> ?age ?min-age)]]
', '{"min-age": 25}');
```

**Find specifications:**

| Find Spec | Example | Returns |
|-----------|---------|---------|
| Relation (default) | `[:find ?name ?age ...]` | Array of tuples: `[[\"Alice\", 30], [\"Bob\", 25]]` |
| Tuple | `[:find ?name ?age . ...]` (dot) | Single tuple: `[\"Alice\", 30]` |
| Collection | `[:find [?name ...] ...]` | Flat array: `[\"Alice\", \"Bob\"]` |
| Scalar | `[:find ?name . ...]` | Single value: `\"Alice\"` |

**Aggregates:**

```sql
SELECT mentat_query('
  [:find (count ?e) (avg ?age) (max ?age) (min ?age)
   :where
   [?e :person/age ?age]]
', '{}');
```

Supported aggregates: `count`, `sum`, `avg`, `min`, `max`.

**Predicates:**

```sql
SELECT mentat_query('
  [:find ?name
   :where
   [?e :person/name ?name]
   [?e :person/age ?age]
   [(>= ?age 30)]
   [(!= ?name "Admin")]]
', '{}');
```

Supported predicates: `>`, `<`, `>=`, `<=`, `=`, `!=`.

**OR and NOT clauses:**

```sql
SELECT mentat_query('
  [:find ?name
   :where
   [?e :person/name ?name]
   (or [?e :person/department ?eng]
       [?e :person/department ?sales])
   (not [?e :person/age 99])]
', '{}');
```

**Rules (recursive):**

```sql
SELECT mentat_query('
  [:find ?boss-name
   :in $ ?emp-name
   :where
   [?e :person/name ?emp-name]
   (reports-to ?e ?boss)
   [?boss :person/name ?boss-name]]
  :rules [
   [(reports-to ?e ?boss) [?e :person/manager ?boss]]
   [(reports-to ?e ?boss)
    [?e :person/manager ?mid]
    (reports-to ?mid ?boss)]]
', '{"emp-name": "Dave Kim"}');
```

**Full-text search:**

```sql
SELECT mentat_query('
  [:find ?name ?score
   :where
   [(fulltext $ :person/bio "database systems") [[?e _ ?score]]]
   [?e :person/name ?name]]
', '{}');
```

**Return value:** JSONB with `results` array.

---

### mentat_pull

Pull attributes for a single entity using a pull pattern.

```sql
mentat_pull(pattern TEXT, entity_id BIGINT) -> JSONB
```

**Wildcard pull (all attributes):**

```sql
SELECT mentat_pull('[*]', 10042);
```

**Specific attributes:**

```sql
SELECT mentat_pull('[:person/name :person/email]', 10042);
```

**Nested ref traversal:**

```sql
SELECT mentat_pull('[
  :person/name
  {:person/department [:dept/name :dept/budget]}
  {:person/friends [:person/name :person/email]}
]', 10042);
```

**Reverse lookups (who references this entity?):**

```sql
SELECT mentat_pull('[:person/name :person/_friends]', 10042);
```

**Limits and defaults:**

```sql
SELECT mentat_pull('[
  (:person/friends :limit 5)
  (:person/bio :default "N/A")
]', 10042);
```

**Recursive pulls:**

```sql
-- Pull manager chain up to 3 levels
SELECT mentat_pull('[
  :person/name
  {:person/manager 3}
]', 10042);
```

---

### mentat_pull_many

Pull attributes for multiple entities at once.

```sql
mentat_pull_many(pattern TEXT, entity_ids BIGINT[]) -> JSONB
```

```sql
SELECT mentat_pull_many(
  '[:person/name :person/email :person/age]',
  ARRAY[10042, 10043, 10044]
);
```

Returns a JSONB array with one entry per entity.

---

### mentat_entity

Fetch all current facts about an entity as a JSON map.

```sql
mentat_entity(entity_id BIGINT) -> JSONB
```

```sql
SELECT mentat_entity(10042);
-- Returns:
-- {
--   ":db/id": 10042,
--   ":person/name": "Alice",
--   ":person/email": "alice@example.com",
--   ":person/age": 30
-- }
```

For cardinality-many attributes, values are returned as JSON arrays.

---

### mentat_schema

Return the full schema as a JSON map keyed by attribute ident.

```sql
mentat_schema() -> JSONB
```

```sql
SELECT mentat_schema();
-- Returns:
-- {
--   ":person/name": {
--     "entid": 65,
--     "valueType": "string",
--     "cardinality": "one",
--     "unique": null,
--     "indexed": true,
--     "fulltext": false,
--     "component": false,
--     "noHistory": false
--   },
--   ...
-- }
```

---

### mentat_explain

Show the execution plan for a Datalog query without executing it.

```sql
mentat_explain(query TEXT, inputs JSONB) -> JSONB
```

```sql
SELECT mentat_explain('
  [:find ?name
   :where
   [?e :person/name ?name]
   [?e :person/age ?age]
   [(> ?age 25)]]
', '{}');
```

Returns the generated SQL, the PostgreSQL EXPLAIN output, and query complexity hints.

---

## EDN Helper Functions

These functions live in the `mentat` schema and provide batch operations and import/export capabilities.

### mentat.batch

Execute multiple operations in a single call.

```sql
mentat.batch(edn_batch TEXT) -> JSONB
```

```sql
SELECT mentat.batch('[
  [:query [:find ?e :where [?e :person/name]]]
  [:transact [{:db/id "new" :person/name "Charlie"}]]
  [:pull [:person/name :person/email] 10042]
  [:entity 10043]
  [:schema]
]');
```

Supported operation types: `:query`, `:transact`, `:pull`, `:entity`, `:schema`.

Returns a JSONB array with one result object per operation:

```json
[
  {"type": "query", "results": [[10042], [10043]]},
  {"type": "transact", "result": {"tx-id": 1001, "tempids": {"new": 10044}}},
  {"type": "pull", "result": {":person/name": "Alice", ...}},
  {"type": "entity", "result": {":db/id": 10043, ...}},
  {"type": "schema", "result": {...}}
]
```

### mentat.export_edn

Export specific entities to EDN transaction format.

```sql
mentat.export_edn(entity_ids BIGINT[]) -> TEXT
```

```sql
SELECT mentat.export_edn(ARRAY[10042, 10043]);
-- Returns:
-- [
--   {:db/id 10042
--    :person/name "Alice"
--    :person/email "alice@example.com"}
--   {:db/id 10043
--    :person/name "Bob"}
-- ]
```

The output can be fed directly into `mentat_transact` or `mentat.import_edn` on another database for data migration.

### mentat.import_edn

Import entities from EDN transaction format.

```sql
mentat.import_edn(edn_data TEXT) -> JSONB
```

```sql
SELECT mentat.import_edn('[
  {:db/id "alice"
   :person/name "Alice"
   :person/email "alice@example.com"}
  {:db/id "bob"
   :person/name "Bob"}
]');
```

This is equivalent to calling `mentat_transact` but returns a JSONB transaction report.

### mentat.query_export_edn

Execute a query and export all matching entities to EDN.

```sql
mentat.query_export_edn(query TEXT, inputs JSONB) -> TEXT
```

```sql
SELECT mentat.query_export_edn(
  '[:find ?e :where [?e :person/department ?d] [?d :dept/name "Engineering"]]',
  '{}'
);
```

### mentat.export_all_edn

Export the entire database as EDN transaction data. Use with caution on large databases.

```sql
mentat.export_all_edn() -> TEXT
```

```sql
SELECT mentat.export_all_edn();
```

---

## Entity Helper Functions

These convenience functions live in the `mentat` schema.

### mentat.lookup_by_ident

Look up an entity ID by a string attribute value.

```sql
mentat.lookup_by_ident(attr_ident TEXT, value TEXT) -> BIGINT
```

```sql
SELECT mentat.lookup_by_ident('person/email', 'alice@example.com');
-- Returns: 10042
```

Returns NULL if no matching entity is found.

### mentat.entity_attrs

Get the list of attribute idents currently asserted on an entity.

```sql
mentat.entity_attrs(entity_id BIGINT) -> JSONB
```

```sql
SELECT mentat.entity_attrs(10042);
-- Returns: [":person/name", ":person/email", ":person/age"]
```

### mentat.attribute_values

Get all current values for a given attribute across all entities.

```sql
mentat.attribute_values(attr_ident TEXT) -> JSONB
```

```sql
SELECT mentat.attribute_values(':person/name');
-- Returns: ["Alice", "Bob", "Carol"]
```

### mentat.retract_entity

Retract all facts about an entity (generating individual retraction datoms).

```sql
mentat.retract_entity(entity_id BIGINT) -> BIGINT
```

```sql
SELECT mentat.retract_entity(10042);
-- Returns: 5  (number of facts retracted)
```

This differs from `[:db/retractEntity eid]` in that it generates individual `[:db/retract ...]` operations for each fact, giving a more explicit retraction trail.

---

## Operational Functions

### mentat_query_stats

Return performance statistics about mentat function calls, database size, and cache status.

```sql
mentat_query_stats() -> JSONB
```

```sql
SELECT mentat_query_stats();
```

Returns:

```json
{
  "functions": [
    {"function": "mentat_query", "calls": 150, "avg_duration_ms": 12.5, ...}
  ],
  "database_stats": {
    "total_datoms": 5000,
    "total_transactions": 42,
    "schema_attributes": 15,
    "partitions": {
      "db.part/db": {"next_entid": 200, "used": 200},
      "db.part/user": {"next_entid": 10500, "used": 500},
      "db.part/tx": {"next_entid": 1000042, "used": 42}
    }
  },
  "cache": {"schema_cache_warmed": true}
}
```

Requires `track_functions = 'all'` in `postgresql.conf` for function call statistics.

### mentat_storage_stats

Return table and index sizes.

```sql
mentat_storage_stats() -> JSONB
```

```sql
SELECT mentat_storage_stats();
```

### mentat_slow_queries

Identify slow mentat functions and heavy transactions.

```sql
mentat_slow_queries(threshold_ms DOUBLE PRECISION DEFAULT 100.0) -> JSONB
```

```sql
SELECT mentat_slow_queries(50.0);
```

### mentat_stmt_cache_stats

Return prepared statement cache hit/miss statistics.

```sql
mentat_stmt_cache_stats() -> JSONB
```

### mentat_stmt_cache_clear

Clear the prepared statement cache. Useful after schema changes.

```sql
mentat_stmt_cache_clear() -> TEXT
```

---

## EDN Functions (edn type)

pg_mentat provides a native `edn` PostgreSQL type. See [EDN_TYPE.md](EDN_TYPE.md) for the full type guide. The following functions operate on `edn` values.

### Collection Access

```sql
-- Get a value from a map by key
SELECT edn_get('{:name "Alice" :age 30}'::edn, ':name'::edn);
-- Returns: "Alice"

-- Get a value from a vector by 0-based index
SELECT edn_nth('[10 20 30]'::edn, 1);
-- Returns: 20

-- Get collection size
SELECT edn_count('[1 2 3]'::edn);
-- Returns: 3
```

### Type Predicates

```sql
SELECT edn_is_nil('nil'::edn);       -- true
SELECT edn_is_boolean('true'::edn);  -- true
SELECT edn_is_integer('42'::edn);    -- true
SELECT edn_is_float('3.14'::edn);    -- true
SELECT edn_is_text('"hello"'::edn);  -- true
SELECT edn_is_keyword(':foo'::edn);  -- true
SELECT edn_is_vector('[1 2]'::edn);  -- true
SELECT edn_is_list('(1 2)'::edn);    -- true
SELECT edn_is_set('#{1 2}'::edn);    -- true
SELECT edn_is_map('{:a 1}'::edn);    -- true
```

### Collection Operations

```sql
-- Check membership
SELECT edn_contains('[1 2 3]'::edn, '2'::edn);
-- Returns: true

-- Extract map keys as a vector
SELECT edn_keys('{:a 1 :b 2}'::edn);
-- Returns: [:a :b]

-- Extract map values as a vector
SELECT edn_values('{:a 1 :b 2}'::edn);
-- Returns: [1 2]
```

---

## Temporal Queries

pg_mentat supports three temporal modes via the `inputs` JSONB parameter.

### As-Of (point-in-time)

See the database as it was at a specific transaction.

```sql
SELECT mentat_query('
  [:find ?name :where [?e :person/name ?name]]
', '{"asOf": 1000005}');
```

### Since

See only facts asserted since a specific transaction.

```sql
SELECT mentat_query('
  [:find ?name :where [?e :person/name ?name]]
', '{"since": 1000005}');
```

### History

See all datoms including retractions. The query must bind `?tx` and `?added` variables.

```sql
SELECT mentat_query('
  [:find ?e ?name ?tx ?added
   :where [?e :person/name ?name ?tx ?added]]
', '{"history": true}');
```

---

## Pagination

The `inputs` JSONB parameter supports `limit` and `offset` for pagination.

```sql
-- First page (10 results)
SELECT mentat_query('
  [:find ?name ?email
   :where [?e :person/name ?name] [?e :person/email ?email]]
', '{"limit": 10}');

-- Second page
SELECT mentat_query('
  [:find ?name ?email
   :where [?e :person/name ?name] [?e :person/email ?email]]
', '{"limit": 10, "offset": 10}');
```

When both a Datalog `:limit` clause and a JSON `limit` input are specified, the JSON input takes precedence.

---

## Hybrid SQL/Datalog Patterns

pg_mentat functions return standard SQL types (JSONB, TEXT), so they compose naturally with PostgreSQL features.

### CTEs combining Datalog results with SQL

```sql
WITH engineers AS (
  SELECT (r->>'results') AS results
  FROM mentat_query('
    [:find ?e ?name ?salary
     :where
     [?e :person/department ?d]
     [?d :dept/name "Engineering"]
     [?e :person/name ?name]
     [?e :person/salary ?salary]]
  ', '{}') AS r
)
SELECT * FROM engineers;
```

### Window functions over Datalog results

```sql
WITH salaries AS (
  SELECT
    elem->>0 AS entity_id,
    elem->>1 AS name,
    (elem->>2)::int AS salary
  FROM mentat_query('
    [:find ?e ?name ?salary
     :where
     [?e :person/name ?name]
     [?e :person/salary ?salary]]
  ', '{}') AS q,
  jsonb_array_elements(q->'results') AS elem
)
SELECT
  name,
  salary,
  RANK() OVER (ORDER BY salary DESC) AS salary_rank,
  salary - AVG(salary) OVER () AS diff_from_avg
FROM salaries;
```

### Joining Mentat data with relational tables

```sql
WITH mentat_people AS (
  SELECT
    elem->>0 AS name,
    elem->>1 AS email
  FROM mentat_query('
    [:find ?name ?email
     :where
     [?e :person/name ?name]
     [?e :person/email ?email]]
  ', '{}') AS q,
  jsonb_array_elements(q->'results') AS elem
)
SELECT
  mp.name,
  te.project_name,
  SUM(te.hours) AS total_hours
FROM mentat_people mp
JOIN time_entries te ON te.person_email = mp.email
GROUP BY mp.name, te.project_name
ORDER BY total_hours DESC;
```

### Pull API in SQL pipelines

```sql
-- Get detailed entity info for query results
WITH entity_ids AS (
  SELECT elem->>0 AS eid
  FROM mentat_query('
    [:find ?e
     :where
     [?e :person/salary ?s]
     [(> ?s 150000)]]
  ', '{}') AS q,
  jsonb_array_elements(q->'results') AS elem
)
SELECT mentat_pull('[*]', eid::bigint)
FROM entity_ids;
```

---

## GUC Configuration Parameters

pg_mentat exposes several GUC (Grand Unified Configuration) parameters for tuning.

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `mentat.enable_optimizer_hints` | boolean | `true` | Enable automatic SET LOCAL optimizer hints during query execution |
| `mentat.default_work_mem` | string | `64MB` | The `work_mem` value applied during complex queries |
| `mentat.max_result_rows` | integer | `0` (unlimited) | Maximum rows returned by `mentat_query` before raising an error |

Set parameters per-session or in `postgresql.conf`:

```sql
-- Per-session
SET mentat.enable_optimizer_hints = off;
SET mentat.max_result_rows = 50000;

-- In postgresql.conf
mentat.enable_optimizer_hints = on
mentat.default_work_mem = '128MB'
mentat.max_result_rows = 100000
```

---

## Internal Schema

pg_mentat stores all data in the `mentat` schema. The core tables are:

| Table | Purpose |
|-------|---------|
| `mentat.datoms` | Core fact table (partitioned by value type) |
| `mentat.schema` | Attribute definitions |
| `mentat.idents` | Keyword to entity ID mappings |
| `mentat.partitions` | Entity ID partition boundaries |
| `mentat.transactions` | Transaction metadata |
| `mentat.fulltext` | Full-text search index table |

The `datoms` table is partitioned by `value_type_tag` (LIST partitioning) for partition pruning and type-specific indexes. Each value type has a dedicated column (`v_ref`, `v_long`, `v_text`, etc.) ensuring native PostgreSQL type comparisons.

### Indexes

| Index | Pattern | Purpose |
|-------|---------|---------|
| `idx_datoms_eavt` | (e, a, tag, tx) WHERE added | Entity-centric lookups |
| `idx_datoms_aevt` | (a, e, tag, tx) WHERE added | Attribute-centric scans |
| `idx_datoms_vaet` | (v_ref, a, e, tx) WHERE added AND tag=0 | Reverse ref lookups |
| `idx_datoms_tx` | (tx DESC) | Transaction history |
| `idx_datoms_avet_ref` | (a, v_ref, e, tx) WHERE added AND tag=0 | Ref value lookups |
| `idx_datoms_avet_long` | (a, v_long, e, tx) WHERE added AND tag=2 | Numeric range queries |
| `idx_datoms_avet_text` | (a, v_text, e, tx) WHERE added AND tag=7 | String equality/prefix |
| `idx_datoms_avet_keyword` | (a, v_keyword, e, tx) WHERE added AND tag=8 | Keyword resolution |

### Value Type Tags

| Tag | Type | Column |
|-----|------|--------|
| 0 | ref | `v_ref` |
| 1 | boolean | `v_bool` |
| 2 | long | `v_long` |
| 3 | double | `v_double` |
| 4 | instant | `v_instant` |
| 7 | string | `v_text` |
| 8 | keyword | `v_keyword` |
| 10 | uuid | `v_uuid` |
| 11 | bytes | `v_bytes` |
