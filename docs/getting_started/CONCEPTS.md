# pg_mentat Core Concepts

This document explains the ideas behind pg_mentat: why data is stored as datoms, how Datalog queries work, what schema evolution looks like, and when to use Datalog versus SQL.

## The EAVT Model

### Datoms

pg_mentat stores every piece of data as a **datom** -- an atomic fact with five components:

| Component | Meaning | Type |
|-----------|---------|------|
| **E** (Entity) | Which thing | 64-bit integer |
| **A** (Attribute) | Which property | 64-bit integer (resolves to keyword like `:person/name`) |
| **V** (Value) | What value | Typed: string, long, double, boolean, instant, ref, keyword, uuid, bytes |
| **Tx** (Transaction) | When asserted | 64-bit transaction ID |
| **Added** | Assertion or retraction | Boolean |

A single datom is equivalent to the statement: "In transaction Tx, we assert (or retract) that entity E has value V for attribute A."

### Example: Relational vs EAVT

A row in a relational table:

| person_id | name | age | email |
|-----------|------|-----|-------|
| 42 | Alice | 30 | alice@example.com |

The same data as datoms:

| E | A | V | Tx | Added |
|---|---|---|-----|-------|
| 42 | `:person/name` | "Alice" | 1000001 | true |
| 42 | `:person/age` | 30 | 1000001 | true |
| 42 | `:person/email` | "alice@example.com" | 1000001 | true |

When Alice turns 31:

| E | A | V | Tx | Added |
|---|---|---|-----|-------|
| 42 | `:person/age` | 30 | 1000002 | **false** |
| 42 | `:person/age` | 31 | 1000002 | true |

The old value is retracted (added=false) and the new value is asserted (added=true). Neither row is deleted. The complete history is preserved.

### Storage in PostgreSQL

Datoms are stored in `mentat.datoms`:

```sql
CREATE TABLE mentat.datoms (
    e         BIGINT   NOT NULL,  -- entity ID
    a         BIGINT   NOT NULL,  -- attribute ID
    v         BYTEA    NOT NULL,  -- value (binary-encoded)
    value_type_tag SMALLINT NOT NULL,  -- type discriminator
    tx        BIGINT   NOT NULL,  -- transaction ID
    added     BOOLEAN  NOT NULL DEFAULT TRUE
);
```

Four covering indexes support different access patterns:

| Index | Columns | Use Case |
|-------|---------|----------|
| EAVT | `(e, a, v, tx)` | "What do I know about entity 42?" |
| AEVT | `(a, e, v, tx)` | "Which entities have a `:person/name`?" |
| AVET | `(a, v, e, tx)` | "Which entity has email 'alice@example.com'?" |
| VAET | `(v, a, e, tx)` | "Which entities reference entity 42?" (refs only) |

### Why EAVT?

1. **Schema flexibility** -- Add new attributes at any time. No ALTER TABLE, no migrations, no downtime.
2. **Sparse data** -- Entities only have the attributes they need. No NULLs.
3. **Complete history** -- Every change is recorded. Nothing is overwritten or deleted.
4. **Time travel** -- Query the database as it existed at any past transaction.
5. **Audit trail** -- Know who changed what and when, built into the data model.
6. **Graph traversal** -- References between entities are first-class, not bolted on with JOIN tables.

## Datalog Query Language

pg_mentat uses Datalog, a declarative logic programming language for querying data.

### Structure of a Query

```
[:find  <what to return>
 :in    <input parameters>       -- optional
 :where <patterns to match>]
```

### Patterns

A pattern matches datoms in the database:

```clojure
[?e :person/name ?name]
```

This reads: "Find datoms where the attribute is `:person/name`, binding the entity to `?e` and the value to `?name`."

- **Variables** start with `?` -- they bind to values and can be reused across patterns.
- **Constants** are literal values -- they must match exactly.
- **`_`** is a wildcard -- matches anything, binds to nothing.

A pattern has up to 5 positions: `[entity attribute value transaction added]`.

### Implicit Joins

When the same variable appears in multiple patterns, it creates a join:

```clojure
[:find ?name ?email
 :where
 [?e :person/name ?name]     ;; pattern 1: bind ?e and ?name
 [?e :person/email ?email]]  ;; pattern 2: same ?e, bind ?email
```

Because `?e` appears in both patterns, only datoms where the entity ID matches in both patterns are returned. This is equivalent to:

```sql
SELECT d1.v AS name, d2.v AS email
FROM mentat.datoms d1
JOIN mentat.datoms d2 ON d1.e = d2.e
WHERE d1.a = resolve(':person/name')
  AND d2.a = resolve(':person/email');
```

The Datalog version is shorter and the join is implicit -- no ON clause needed.

### Predicates

Filter results with expressions:

```clojure
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(> ?age 25)]]
```

Supported predicates: `<`, `>`, `<=`, `>=`, `=`, `!=`.

### Find Specifications

The `:find` clause controls the shape of the result:

| Spec | Syntax | Returns |
|------|--------|---------|
| Relation | `[:find ?a ?b ...]` | Collection of tuples: `[["Alice" 30] ["Bob" 25]]` |
| Scalar | `[:find ?a . ...]` | Single value: `"Alice"` |
| Collection | `[:find [?a ...] ...]` | Flat list: `["Alice" "Bob" "Carol"]` |
| Tuple | `[:find [?a ?b] ...]` | Single tuple: `["Alice" 30]` |

### Input Parameters

Pass values into queries with `:in`:

```clojure
[:find ?name
 :in ?min-age ?max-age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(>= ?age ?min-age)]
 [(<= ?age ?max-age)]]
```

Called with:

```sql
SELECT mentat_query(
  '[:find ?name :in ?min-age ?max-age
    :where [?e :person/name ?name] [?e :person/age ?age]
    [(>= ?age ?min-age)] [(<= ?age ?max-age)]]',
  '{"inputs": [25, 35]}'::jsonb
);
```

Parameters are bound positionally: first `:in` variable gets `inputs[0]`, second gets `inputs[1]`.

### Aggregates

Aggregate functions wrap variables in the `:find` clause:

```clojure
[:find (count ?e)  :where [?e :person/name]]      ;; count
[:find (avg ?age)  :where [?e :person/age ?age]]   ;; average
[:find (sum ?age)  :where [?e :person/age ?age]]   ;; sum
[:find (min ?age)  :where [?e :person/age ?age]]   ;; minimum
[:find (max ?age)  :where [?e :person/age ?age]]   ;; maximum
```

Group-by is implicit: unaggregated variables in `:find` become grouping keys:

```clojure
[:find ?department (count ?e) (avg ?salary)
 :where
 [?e :employee/department ?department]
 [?e :employee/salary ?salary]]
```

### OR and NOT Clauses

Compose constraints with logical operators:

```clojure
;; OR: match either condition
[:find ?name
 :where
 [?e :person/name ?name]
 (or [?e :person/age 30]
     [?e :person/age 35])]

;; NOT: exclude matches
[:find ?name
 :where
 [?e :person/name ?name]
 (not [?e :person/email _])]  ;; people without an email
```

### Rules

Rules define reusable, composable query logic. They are especially powerful for recursive patterns:

```clojure
[:find ?ancestor-name
 :in $ ?person-name
 :where
 [?person :person/name ?person-name]
 (ancestor ?person ?ancestor)
 [?ancestor :person/name ?ancestor-name]]

:rules [
  ;; Base case: parent is an ancestor
  [(ancestor ?d ?a) [?d :person/parent ?a]]

  ;; Recursive case: ancestor of my parent is also my ancestor
  [(ancestor ?d ?a)
   [?d :person/parent ?p]
   (ancestor ?p ?a)]]
```

This computes the transitive closure of the `:person/parent` relationship -- something that requires `WITH RECURSIVE` in SQL.

## Datalog vs SQL

| Aspect | Datalog (pg_mentat) | SQL (PostgreSQL) |
|--------|---------------------|------------------|
| **Joins** | Implicit via shared variables | Explicit `JOIN ... ON` |
| **Recursion** | Native via rules | `WITH RECURSIVE` (more verbose) |
| **Schema changes** | Transact new attributes | `ALTER TABLE` (may lock) |
| **History** | Built-in (time travel) | Manual (trigger-based audit tables) |
| **Negation** | `(not ...)` | `NOT EXISTS (...)` |
| **Window functions** | Not available | Full support |
| **Bulk updates** | One datom at a time | `UPDATE ... SET ... WHERE` |
| **Analytics** | Basic aggregates | Full analytical SQL |

### When to use Datalog

- **Graph traversal**: following references across entities (friends-of-friends, org charts, dependency trees)
- **Recursive relationships**: transitive closure without hand-writing `WITH RECURSIVE`
- **Flexible schema**: entities with varying attributes
- **Audit/compliance**: built-in history and time travel
- **Multi-model queries**: combining different entity types in one query

### When to use SQL

- **Complex aggregations**: window functions, CTEs, ROLLUP
- **Bulk operations**: updating millions of rows
- **Analytical queries**: reporting, OLAP patterns
- **Integration**: existing SQL tools, BI, ETL pipelines

### Combining both

pg_mentat runs inside PostgreSQL. You can use both in the same session:

```sql
-- Datalog to find entity IDs
SELECT mentat_query(
  '[:find [?e ...] :where [?e :person/age ?age] [(> ?age 25)]]',
  '{}'::jsonb
) AS person_ids;

-- SQL for analytics on the raw datom table
SELECT a, COUNT(*) as datom_count
FROM mentat.datoms
WHERE added = true
GROUP BY a
ORDER BY datom_count DESC;
```

## Schema Evolution

### Adding attributes

New attributes are added by transacting their definitions. No downtime, no migration:

```sql
SELECT mentat_transact('[
  {:db/ident :person/phone
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');
```

Existing entities are unaffected. They simply do not have the new attribute until you assert it.

### Schema is data

Schema definitions are themselves datoms. The `:person/name` attribute is an entity with attributes like `:db/ident`, `:db/valueType`, and `:db/cardinality`:

```
Entity 100 (the attribute definition):
  [100 :db/ident       :person/name]
  [100 :db/valueType   :db.type/string]
  [100 :db/cardinality :db.cardinality/one]
```

You can query the schema the same way you query data:

```sql
SELECT mentat_schema();
```

Or directly:

```sql
SELECT * FROM mentat.schema;
```

### Value types

| Type | EDN Syntax | PostgreSQL Storage |
|------|-----------|-------------------|
| `:db.type/string` | `"hello"` | BYTEA (UTF-8) |
| `:db.type/long` | `42` | BYTEA (LE i64) |
| `:db.type/double` | `3.14` | BYTEA (LE f64) |
| `:db.type/boolean` | `true`, `false` | BYTEA (1 byte) |
| `:db.type/instant` | `#inst "2024-01-15T10:30:00Z"` | BYTEA (microseconds since epoch) |
| `:db.type/keyword` | `:status/active` | BYTEA (UTF-8) |
| `:db.type/uuid` | `#uuid "550e8400-..."` | BYTEA (16 bytes) |
| `:db.type/bytes` | Binary data | BYTEA (raw) |
| `:db.type/ref` | Entity ID | BYTEA (LE i64) |

### Cardinality

- **`:db.cardinality/one`** -- Single value. Asserting a new value for the same entity+attribute automatically retracts the old value. Like a column in SQL.
- **`:db.cardinality/many`** -- Set of values. Multiple values coexist. Like a join table in SQL.

### Uniqueness and upsert

- **`:db.unique/value`** -- Enforces that no two entities share this value. Attempting a duplicate raises an error.
- **`:db.unique/identity`** -- Same uniqueness guarantee, plus **upsert semantics**: transacting a value that already exists updates the existing entity instead of creating a new one. This is the idiomatic way to handle "insert or update."

## Time Travel

### How it works

Every assertion and retraction is a datom with a transaction ID. pg_mentat never deletes datoms. Current state is the set of datoms where `added = true` and no later retraction exists.

### Three temporal modes

**As-of**: See the database as it existed at a specific transaction.

```sql
SELECT mentat_query(
  '[:find ?name :where [?e :person/name ?name]]',
  '{"asOf": 1000001}'::jsonb
);
```

Returns only datoms with `tx <= 1000001`, applying retractions up to that point.

**Since**: See only changes after a specific transaction.

```sql
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{"since": 1000001}'::jsonb
);
```

Returns datoms with `tx > 1000001`.

**History**: See all datoms including retractions.

```sql
SELECT mentat_query(
  '[:find ?e ?name ?tx ?added
    :where [?e :person/name ?name ?tx ?added]]',
  '{"history": true}'::jsonb
);
```

Returns every datom ever written, with the `?added` flag distinguishing assertions from retractions.

### Use cases

| Use Case | Temporal Mode |
|----------|---------------|
| "What was Alice's age on January 1?" | As-of |
| "What changed since my last sync?" | Since |
| "Show me the full edit history of this entity" | History |
| "Reproduce the bug that was reported yesterday" | As-of (with yesterday's tx) |
| "Regulatory audit: prove data was correct at time T" | As-of + History |

## Transaction Semantics

### Atomicity

Each `mentat_transact()` call is a single PostgreSQL transaction. All assertions and retractions within it succeed or fail together.

### Transaction entities

Every transaction is itself an entity with a `:db/txInstant` attribute recording when it occurred. You can query transactions:

```clojure
[:find ?tx ?time
 :where
 [_ _ _ ?tx]                  ;; any datom in transaction ?tx
 [?tx :db/txInstant ?time]]   ;; timestamp of that transaction
```

### Tempids

Temporary IDs are strings used within a single transaction to reference entities before they have permanent IDs:

```clojure
[{:db/id "alice" :person/name "Alice"}
 {:db/id "bob" :person/friend "alice"}]  ;; bob references alice
```

The transaction report maps each tempid to its permanent entity ID.

### Transaction operations

| Operation | Syntax | Effect |
|-----------|--------|--------|
| Assert (map) | `{:db/id "t" :attr val}` | Assert multiple attributes on an entity |
| Assert (vector) | `[:db/add eid :attr val]` | Assert a single fact |
| Retract | `[:db/retract eid :attr val]` | Retract a specific fact |
| Retract entity | `[:db/retractEntity eid]` | Retract all facts about an entity |

## Pull API Patterns

The Pull API complements Datalog queries. Queries find entity IDs; pulls retrieve structured data for those entities.

### Basic pull

```clojure
[:person/name :person/age]           ;; specific attributes
[*]                                   ;; all attributes
```

### Navigation (following refs)

```clojure
[:person/name
 {:person/friend [:person/name :person/email]}]
```

Follows `:person/friend` references and pulls the specified attributes from the referenced entities.

### Reverse lookups

```clojure
[:person/name :person/_friend]
```

The `_` prefix reverses the direction: "find entities whose `:person/friend` points to this entity."

### Combining query and pull

A common pattern is to query for entity IDs, then pull their data:

```sql
-- Step 1: Find entity IDs
SELECT mentat_query(
  '[:find [?e ...] :where [?e :person/age ?age] [(> ?age 25)]]',
  '{}'::jsonb
);
-- Returns: [10000, 10002]

-- Step 2: Pull each entity
SELECT mentat_pull('[*]', 10000);
SELECT mentat_pull('[*]', 10002);
```

Or use `mentat_entity()` for a simpler all-attributes view:

```sql
SELECT mentat_entity(10000);
```

## EDN (Extensible Data Notation)

pg_mentat uses EDN for schemas, transactions, and queries. EDN is a data format from the Clojure ecosystem -- similar to JSON but with richer types.

### Syntax reference

```clojure
;; Comments start with semicolons

;; Scalars
42                                     ;; integer
3.14                                   ;; float
"hello"                                ;; string
true false                             ;; booleans
nil                                    ;; null

;; Keywords (like enums or symbols)
:person/name                           ;; namespaced keyword
:db.cardinality/one                    ;; multi-segment namespace

;; Collections
[1 2 3]                                ;; vector (ordered)
{:name "Alice" :age 30}               ;; map (key-value)
#{1 2 3}                               ;; set (unique, unordered)

;; Tagged literals
#inst "2024-01-15T10:30:00Z"          ;; timestamp
#uuid "550e8400-e29b-41d4-a716-446655440000"  ;; UUID
```

### EDN vs JSON

| Feature | EDN | JSON |
|---------|-----|------|
| Keywords | `:person/name` | `"person/name"` (string) |
| Comments | `;; comment` | Not supported |
| Sets | `#{1 2 3}` | Not supported |
| Integers | Exact `42` | Number `42` (float) |
| Tagged values | `#inst "..."` | Not supported |
| Trailing commas | Allowed (whitespace) | Not allowed |

## Performance Considerations

### Index selection

pg_mentat automatically creates four indexes. Marking an attribute with `:db/index true` optimizes AVET lookups (find entity by attribute value):

```sql
{:db/ident :person/email
 :db/valueType :db.type/string
 :db/cardinality :db.cardinality/one
 :db/unique :db.unique/identity
 :db/index true}
```

### Query tips

1. **Be specific in patterns** -- use constants where possible to let indexes narrow the search early.
2. **Order patterns** -- place the most selective patterns first.
3. **Use input parameters** -- they enable prepared statement reuse.
4. **Batch transactions** -- group multiple assertions in one `mentat_transact()` call.
5. **Use Pull for nested data** -- more efficient than multiple round-trip queries.

### Viewing query plans

Since Datalog queries compile to SQL under the hood, you can use PostgreSQL's EXPLAIN:

```sql
EXPLAIN ANALYZE SELECT mentat_query(
  '[:find ?e :where [?e :person/name "Alice"]]',
  '{}'::jsonb
);
```

## Next Steps

- Follow the hands-on [QUICKSTART.md](./QUICKSTART.md) to try these concepts
- See [EXAMPLES.md](../../EXAMPLES.md) for real-world patterns
- Explore the [API reference](../api/) for complete function documentation
