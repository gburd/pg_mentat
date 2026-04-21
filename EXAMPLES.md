# pg_mentat Examples

Complete guide to using pg_mentat - a Datalog query engine extension for PostgreSQL.

**Like DocumentDB?** Yes! pg_mentat provides rich Datalog/EAVT functionality through PostgreSQL SQL functions, similar to how DocumentDB provides document storage.

---

## Table of Contents

1. [Getting Started](#getting-started)
2. [Schema Definition](#schema-definition)
3. [Basic Data Operations](#basic-data-operations)
4. [Simple Queries](#simple-queries)
5. [Advanced Queries](#advanced-queries)
6. [Temporal Queries](#temporal-queries)
7. [Full-Text Search](#full-text-search)
8. [Rules and Recursion](#rules-and-recursion)
9. [Real-World Examples](#real-world-examples)

---

## Getting Started

### Installation

```sql
-- Create extension
CREATE EXTENSION pg_mentat;

-- Initialize schema (creates datoms table, indexes, etc.)
SELECT mentat.initialize_schema();
```

### What is EAVT?

pg_mentat stores data as **Entity-Attribute-Value-Transaction** (EAVT) tuples:
- **Entity**: Thing being described (person, product, order)
- **Attribute**: Property name (:person/name, :product/price)
- **Value**: Property value ("Alice", 29.99)
- **Transaction**: When the fact was asserted

This flexible schema allows querying across relationships naturally.

---

## Schema Definition

### Example: Person Database

```sql
-- Define attributes using EDN transaction format
SELECT mentat_transact('[
  {:db/id "person-name"
   :db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/doc "A persons full name"}

  {:db/id "person-email"
   :db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity
   :db/doc "Unique email address"}

  {:db/id "person-age"
   :db/ident :person/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}

  {:db/id "person-friend"
   :db/ident :person/friend
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many
   :db/doc "Reference to other person entities"}
]');
```

**Key Concepts:**
- `:db/ident` - Keyword identifier for the attribute
- `:db/valueType` - Data type (string, long, ref, boolean, instant, double, keyword, uuid, bytes)
- `:db/cardinality` - one (single value) or many (multiple values)
- `:db/unique` - Enforce uniqueness (value or identity)
- `:db.type/ref` - References other entities (like foreign keys)

---

## Basic Data Operations

### Insert Data

```sql
-- Add people using tempids
SELECT mentat_transact('[
  {:db/id "alice"
   :person/name "Alice Anderson"
   :person/email "alice@example.com"
   :person/age 30}

  {:db/id "bob"
   :person/name "Bob Brown"
   :person/email "bob@example.com"
   :person/age 25
   :person/friend "alice"}
]');
```

**Tempids:** String IDs like `"alice"` get allocated actual entity IDs. The transaction report shows the mapping:
```json
{
  "tx-id": 101,
  "tx-instant": 1234567890,
  "tempids": {"alice": 100, "bob": 101},
  "datoms-inserted": 7
}
```

### Update Data

```sql
-- Update Alice's age (use actual entity ID from previous transaction)
SELECT mentat_transact('[
  [:db/add 100 :person/age 31]
]');
```

### Retract Data

```sql
-- Remove a single fact
SELECT mentat_transact('[
  [:db/retract 100 :person/age 31]
]');

-- Retract all facts about an entity (coming soon - use manual retractions for now)
```

---

## Simple Queries

### Find All People

```sql
SELECT mentat_query('
[:find ?name ?email
 :where
 [?e :person/name ?name]
 [?e :person/email ?email]]
', '{}');
```

**Result:**
```json
{
  "results": [
    ["Alice Anderson", "alice@example.com"],
    ["Bob Brown", "bob@example.com"]
  ]
}
```

### Find Spec Variants

```sql
-- Relation (multiple rows, multiple columns) - default
[:find ?e ?name :where [?e :person/name ?name]]
-- Returns: [[100, "Alice"], [101, "Bob"]]

-- Tuple (single row)
[:find [?e ?name] :where [?e :person/name ?name]]
-- Returns: [100, "Alice"]  (first match only)

-- Collection (single column)
[:find [?name ...] :where [?e :person/name ?name]]
-- Returns: ["Alice", "Bob"]

-- Scalar (single value)
[:find ?name . :where [?e :person/name ?name]]
-- Returns: "Alice"  (first match only)
```

### Filtering

```sql
-- Find people over 25
SELECT mentat_query('
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(> ?age 25)]]
', '{}');
```

**Supported Predicates:** `>`, `<`, `>=`, `<=`, `=`, `!=`

---

## Advanced Queries

### Query with Input Parameters

```sql
SELECT mentat_query('
[:find ?name
 :in $ ?min-age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(>= ?age ?min-age)]]
', '{"min-age": 30}');
```

### OR Queries

```sql
-- Find people named Alice OR with age > 28
SELECT mentat_query('
[:find ?e ?name
 :where
 (or [?e :person/name "Alice Anderson"]
     (and [?e :person/age ?age]
          [(> ?age 28)]))]
', '{}');
```

### NOT Queries

```sql
-- Find people without an email
SELECT mentat_query('
[:find ?e ?name
 :where
 [?e :person/name ?name]
 (not [?e :person/email _])]
', '{}');
```

### Aggregation

```sql
-- Count total people
SELECT mentat_query('
[:find (count ?e)
 :where
 [?e :person/name _]]
', '{}');

-- Coming soon: sum, avg, min, max, median
```

### Ordering and Limiting

```sql
-- Top 10 oldest people
SELECT mentat_query('
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 :order (desc ?age)
 :limit 10]
', '{}');
```

---

## Temporal Queries

### Point-in-Time Snapshot (as-of)

```sql
-- See database state as it was at transaction 95
SELECT mentat_query('
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]]
', '{"asOf": 95}');
```

### Changes Since Transaction (since)

```sql
-- See what changed after transaction 95
SELECT mentat_query('
[:find ?e ?a ?v ?tx
 :where
 [?e ?a ?v ?tx]]
', '{"since": 95}');
```

### Full History

```sql
-- See all assertions AND retractions
SELECT mentat_query('
[:find ?e ?a ?v ?tx ?added
 :where
 [?e ?a ?v ?tx ?added]]
', '{"history": true}');
```

**The `?added` variable:**
- `true` = assertion (fact was added)
- `false` = retraction (fact was removed)

---

## Full-Text Search

### Define Fulltext Attribute

```sql
SELECT mentat_transact('[
  {:db/id "post-content"
   :db/ident :post/content
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/fulltext true}
]');
```

### Insert Searchable Content

```sql
SELECT mentat_transact('[
  {:db/id "post1"
   :post/content "The quick brown fox jumps over the lazy dog"}
  {:db/id "post2"
   :post/content "A quick analysis of PostgreSQL performance"}
]');
```

### Search

```sql
-- Simple search
SELECT mentat_query('
[:find ?e ?text ?score
 :where
 [(fulltext $ :post/content "quick") [[?e ?text ?tx ?score]]]]
', '{}');

-- Phrase search
SELECT mentat_query('
[:find ?e ?text ?score
 :where
 [(fulltext $ :post/content "\"quick brown\"") [[?e ?text ?tx ?score]]]]
', '{}');
```

**Scoring:** Results include BM25-style relevance scores from PostgreSQL's `ts_rank`.

---

## Rules and Recursion

### Define Rules

```sql
-- Find all ancestors (transitive closure)
SELECT mentat_query('
[:find ?ancestor ?descendant
 :where
 (ancestor ?ancestor ?descendant)]

:rules [
  ;; Base case: parent is ancestor
  [(ancestor ?a ?d)
   [?a :person/child ?d]]

  ;; Recursive case: ancestor of parent is ancestor
  [(ancestor ?a ?d)
   [?a :person/child ?x]
   (ancestor ?x ?d)]
]', '{}');
```

### Example: Org Chart Navigation

```sql
-- Schema
SELECT mentat_transact('[
  {:db/id "reports-to"
   :db/ident :employee/reportsTo
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one}
]');

-- Data
SELECT mentat_transact('[
  {:db/id "alice" :person/name "Alice (CEO)"}
  {:db/id "bob" :person/name "Bob (VP)" :employee/reportsTo "alice"}
  {:db/id "carol" :person/name "Carol (Mgr)" :employee/reportsTo "bob"}
  {:db/id "dave" :person/name "Dave (IC)" :employee/reportsTo "carol"}
]');

-- Query: Who does Dave ultimately report to?
SELECT mentat_query('
[:find ?boss-name
 :in $ ?employee-name
 :where
 [?employee :person/name ?employee-name]
 (reports-up ?employee ?boss)
 [?boss :person/name ?boss-name]]

:rules [
  [(reports-up ?e ?boss)
   [?e :employee/reportsTo ?boss]]

  [(reports-up ?e ?boss)
   [?e :employee/reportsTo ?intermediate]
   (reports-up ?intermediate ?boss)]
]', '{"employee-name": "Dave (IC)"}');

-- Returns: ["Carol (Mgr)", "Bob (VP)", "Alice (CEO)"]
```

---

## Real-World Examples

### Example 1: E-Commerce Product Catalog

```sql
-- Schema
SELECT mentat_transact('[
  {:db/ident :product/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :product/sku :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
  {:db/ident :product/price :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
  {:db/ident :product/category :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
  {:db/ident :product/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
  {:db/ident :product/description :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/fulltext true}

  {:db/ident :category/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :category/parent :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
]');

-- Products
SELECT mentat_transact('[
  {:db/id "electronics" :category/name "Electronics"}
  {:db/id "computers" :category/name "Computers" :category/parent "electronics"}

  {:db/id "laptop1"
   :product/name "ThinkBook X1"
   :product/sku "TB-X1-001"
   :product/price 1299.99
   :product/category "computers"
   :product/tags ["laptop" "business" "portable"]
   :product/description "Professional laptop with excellent keyboard and long battery life"}
]');

-- Query: Find laptops under $1500 in Electronics category or subcategories
SELECT mentat_query('
[:find ?name ?price
 :where
 [?p :product/name ?name]
 [?p :product/price ?price]
 [?p :product/category ?cat]
 (in-category ?cat ?electronics)
 [(< ?price 1500.0)]
 [?p :product/tags "laptop"]]

:rules [
  [(in-category ?c ?target)
   [?target :category/name "Electronics"]
   [?c :category/name _]]

  [(in-category ?c ?target)
   [?c :category/parent ?target]]

  [(in-category ?c ?target)
   [?c :category/parent ?intermediate]
   (in-category ?intermediate ?target)]
]', '{}');
```

### Example 2: Social Network

```sql
-- Schema
SELECT mentat_transact('[
  {:db/ident :user/username :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
  {:db/ident :user/follows :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
  {:db/ident :post/author :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
  {:db/ident :post/content :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/fulltext true}
  {:db/ident :post/timestamp :db/valueType :db.type/instant :db/cardinality :db.cardinality/one}
]');

-- Query: Friend-of-friend recommendations (2nd degree connections not already followed)
SELECT mentat_query('
[:find ?username
 :in $ ?my-username
 :where
 [?me :user/username ?my-username]
 [?me :user/follows ?friend]
 [?friend :user/follows ?fof]
 [?fof :user/username ?username]
 (not [?me :user/follows ?fof])
 [(not= ?me ?fof)]]
', '{"my-username": "alice"}');
```

### Example 3: Project Management

```sql
-- Schema
SELECT mentat_transact('[
  {:db/ident :task/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :task/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
  {:db/ident :task/assignee :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
  {:db/ident :task/blockedBy :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
  {:db/ident :task/project :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
]');

-- Query: Find tasks ready to work (not blocked, assigned, status=todo)
SELECT mentat_query('
[:find ?title ?assignee-name
 :where
 [?t :task/title ?title]
 [?t :task/status :status/todo]
 [?t :task/assignee ?assignee]
 [?assignee :person/name ?assignee-name]
 (not [?t :task/blockedBy ?blocker]
      [?blocker :task/status ?bs]
      [(not= ?bs :status/done)])]
', '{}');
```

---

## Batch Operations and Import/Export

### Batch Processing

Execute multiple operations in a single EDN batch document:

```sql
-- Multiple operations in one call
SELECT mentat.batch('[
  [:query [:find ?e ?name
           :where [?e :person/name ?name]]]

  [:transact [{:db/id "new-person"
               :person/name "Charlie"
               :person/email "charlie@example.com"}]]

  [:pull [:person/name :person/email] 100]

  [:entity 101]

  [:schema]
]');
```

**Result:**
```json
[
  {
    "type": "query",
    "results": [[100, "Alice"], [101, "Bob"]]
  },
  {
    "type": "transact",
    "result": {
      "tx-id": 1001,
      "tempids": {"new-person": 102},
      "datoms-inserted": 2
    }
  },
  {
    "type": "pull",
    "result": {
      ":person/name": "Alice",
      ":person/email": "alice@example.com"
    }
  },
  {
    "type": "entity",
    "result": {
      ":db/id": 101,
      ":person/name": "Bob"
    }
  },
  {
    "type": "schema",
    "result": { ... }
  }
]
```

**Supported Operations:**
- `:query` - Execute Datalog query
- `:transact` - Process transaction
- `:pull` - Pull entity with pattern
- `:entity` - Get full entity
- `:schema` - Get schema

### Export to EDN

Export specific entities:

```sql
-- Export by entity IDs
SELECT mentat.export_edn(ARRAY[100, 101, 102]);
```

**Returns:**
```edn
[
  {:db/id 100
   :person/name "Alice Anderson"
   :person/email "alice@example.com"
   :person/age 30}
  {:db/id 101
   :person/name "Bob Brown"
   :person/age 25}
  {:db/id 102
   :person/name "Carol Chen"
   :person/email "carol@example.com"}
]
```

Query and export matching entities:

```sql
-- Export all people over 25
SELECT mentat.query_export_edn(
  '[:find ?e
    :where
    [?e :person/age ?age]
    [(> ?age 25)]]',
  '{}'::jsonb
);
```

Export entire database:

```sql
-- Warning: Can be very large!
SELECT mentat.export_all_edn();
```

### Import from EDN

Import EDN transaction data:

```sql
-- Import entities with tempids
SELECT mentat.import_edn('[
  {:db/id "alice"
   :person/name "Alice"
   :person/email "alice@example.com"
   :person/age 30}

  {:db/id "bob"
   :person/name "Bob"
   :person/age 25
   :person/friend "alice"}
]');
```

**Returns transaction report:**
```json
{
  "tx-id": 1002,
  "tempids": {
    "alice": 103,
    "bob": 104
  },
  "datoms-inserted": 5
}
```

Import with explicit entity IDs:

```sql
-- Reuse specific entity IDs (be careful with conflicts)
SELECT mentat.import_edn('[
  {:db/id 200
   :person/name "Dave"}
  {:db/id 201
   :person/name "Eve"}
]');
```

### Migration Workflow

Complete database migration between environments:

```sql
-- Source database: Export all data
\o /tmp/mentat_export.edn
SELECT mentat.export_all_edn();
\o

-- Target database: Import data
\set content `cat /tmp/mentat_export.edn`
SELECT mentat.import_edn(:'content');
```

Incremental sync:

```sql
-- Export entities modified since transaction 1000
SELECT mentat.query_export_edn(
  '[:find ?e
    :where
    [?e _ _ ?tx]
    [(> ?tx 1000)]]',
  '{}'::jsonb
);
```

---

## API Reference

### Core Functions

```sql
-- Transaction processing
mentat.mentat_transact(edn_tx TEXT) → TEXT

-- Query execution
mentat.mentat_query(query TEXT, inputs JSONB) → JSONB

-- Entity operations
mentat.mentat_pull(pattern TEXT, entity_id BIGINT) → JSONB
mentat.mentat_entity(entity_id BIGINT) → JSONB
mentat.mentat_schema() → JSONB
```

### Batch and Import/Export Functions

```sql
-- Batch operations
mentat.batch(edn_batch TEXT) → JSONB

-- Export to EDN
mentat.export_edn(entity_ids BIGINT[]) → TEXT
mentat.query_export_edn(query TEXT, inputs JSONB) → TEXT
mentat.export_all_edn() → TEXT

-- Import from EDN
mentat.import_edn(edn_data TEXT) → JSONB
```

### Convenience Helper Functions

```sql
-- Lookup entity by unique attribute value
mentat.lookup_by_ident(attr_ident TEXT, value TEXT) → BIGINT

-- List all attributes for an entity
mentat.entity_attrs(entity_id BIGINT) → JSONB

-- Get all values for an attribute
mentat.attribute_values(attr_ident TEXT) → JSONB

-- Retract all facts about an entity
mentat.retract_entity(entity_id BIGINT) → BIGINT

-- Schema introspection (lower-level)
mentat.resolve_ident(ident TEXT) → BIGINT
mentat.lookup_entity_by_attr(attr_ident TEXT, value TEXT) → BIGINT
mentat.allocate_entid(partition_name TEXT) → BIGINT
```

### EDN Data Types

**Supported in transactions:**
- Strings: `"hello"`
- Integers: `42`
- Booleans: `true`, `false`
- Keywords: `:namespace/name`
- Vectors: `[1 2 3]`
- Maps: `{:key "value"}`

### Attribute Value Types

- `:db.type/string` - Text strings
- `:db.type/long` - 64-bit integers
- `:db.type/boolean` - true/false
- `:db.type/double` - Floating point
- `:db.type/instant` - Timestamps
- `:db.type/keyword` - Keywords
- `:db.type/uuid` - UUIDs
- `:db.type/bytes` - Binary data
- `:db.type/ref` - Entity references

---

## Performance Tips

1. **Use indexes wisely**: Set `:db/index true` on frequently queried attributes
2. **Leverage uniqueness**: Use `:db/unique` for natural keys (email, SKU)
3. **Fulltext search**: Set `:db/fulltext true` only on text needing search
4. **Cardinality matters**: Use `:db.cardinality/one` unless you truly need multiple values
5. **Batch transactions**: Insert multiple entities in one `mentat_transact()` call
6. **Schema is cached**: Ident and schema lookups are cached after first access

---

## Troubleshooting

### Transaction Errors

**"Unique constraint violation"**
```sql
-- Trying to insert duplicate email
SELECT mentat_transact('[
  {:db/id "user1" :person/email "alice@example.com"}  -- OK
  {:db/id "user2" :person/email "alice@example.com"}  -- ERROR!
]');
```

**"Cardinality violation"**
```sql
-- Trying to set multiple values for cardinality/one
SELECT mentat_transact('[
  [:db/add 100 :person/age 30]
  [:db/add 100 :person/age 31]  -- ERROR! age is cardinality/one
]');
```

**"Type mismatch"**
```sql
-- Trying to insert wrong type
SELECT mentat_transact('[
  [:db/add 100 :person/age "thirty"]  -- ERROR! age expects long
]');
```

### Query Errors

**"Failed to resolve attribute"**
- Attribute not defined in schema
- Check spelling: `:person/name` not `:person/names`

**"Variable not bound"**
- Using `?var` before it appears in a pattern
- Solution: Reorder clauses or add binding pattern

---

## Migration from Datomic/Mentat

pg_mentat is designed to be compatible with Datomic/Mentat query syntax with these differences:

1. **SQL-first**: Use PostgreSQL SQL functions, not embedded API
2. **Transaction format**: Same EDN format
3. **Query format**: Same Datalog syntax
4. **Pull patterns**: Basic support (no nesting yet)
5. **Rules**: Fully supported with recursion

---

## Next Steps

- **GitHub**: https://github.com/yourusername/pg_mentat
- **Issues**: Report bugs and request features
- **Contributing**: Pull requests welcome!

---

**License:** Apache 2.0 / MIT (same as Mentat)
