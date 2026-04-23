# pg_mentat Core Concepts

Understanding the fundamental concepts behind pg_mentat will help you use it effectively.

## The EAVT Model

pg_mentat stores data using the **Entity-Attribute-Value-Time** (EAVT) model, inspired by Datomic.

### What is EAVT?

Instead of traditional tables with fixed columns, every piece of data is stored as a **datom** (atomic fact) with four components:

- **E**ntity: Which thing (entity ID as integer)
- **A**ttribute: Which property (attribute ID as integer)
- **V**alue: What value (typed data: string, number, ref, etc.)
- **T**ime: When it was asserted (transaction ID)

Plus a fifth component:
- **added**: Boolean flag (true = assertion, false = retraction)

### Example

Traditional relational model:

| person_id | name  | age | email |
|-----------|-------|-----|-------|
| 1         | Alice | 30  | a@e.com |

EAVT model:

| E (entity) | A (attribute) | V (value) | T (tx) | added |
|------------|---------------|-----------|--------|-------|
| 1          | :person/name  | "Alice"   | 1000   | true  |
| 1          | :person/age   | 30        | 1000   | true  |
| 1          | :person/email | "a@e.com" | 1000   | true  |

### Benefits of EAVT

1. **Schema flexibility** - Add new attributes without ALTER TABLE
2. **Sparse data** - No NULL columns, only store what exists
3. **Full history** - Every change is a new datom, nothing is deleted
4. **Time travel** - Query data as it existed at any point in time
5. **Audit trail** - Complete history of all changes

## Schema as Data

In pg_mentat, schema definitions are just data (datoms) like everything else.

### Schema Attributes

When you define a new attribute:

```sql
{:db/ident :person/name
 :db/valueType :db.type/string
 :db/cardinality :db.cardinality/one}
```

This creates an entity with attributes describing the schema:

```
Entity 100 (the :person/name attribute definition):
  [100 :db/ident :person/name]
  [100 :db/valueType :db.type/string]
  [100 :db/cardinality :db.cardinality/one]
```

### Value Types

pg_mentat supports these value types:

| Type | Description | Example |
|------|-------------|---------|
| `:db.type/string` | Text | `"Hello"` |
| `:db.type/long` | 64-bit integer | `42`, `-1000` |
| `:db.type/double` | 64-bit float | `3.14`, `-2.5` |
| `:db.type/boolean` | True/false | `true`, `false` |
| `:db.type/instant` | Timestamp | `#inst "2024-01-15T10:30:00Z"` |
| `:db.type/keyword` | EDN keyword | `:status/active` |
| `:db.type/uuid` | UUID | `#uuid "550e8400-e29b-41d4-a716-446655440000"` |
| `:db.type/bytes` | Binary data | `#bytes [0x01 0x02 0x03]` |
| `:db.type/ref` | Reference to another entity | Entity ID |

### Cardinality

Attributes have cardinality specifying how many values they can hold:

- **`:db.cardinality/one`** - Single value (like a column in SQL)
  - Example: `:person/name`, `:person/age`

- **`:db.cardinality/many`** - Multiple values (like a one-to-many relationship)
  - Example: `:person/hobbies`, `:person/friend`

### Unique Constraints

Attributes can have uniqueness constraints:

- **`:db.unique/value`** - Value must be unique (like UNIQUE constraint in SQL)
  - Example: `:user/username`
  - Upsert: No (throws error on duplicate)

- **`:db.unique/identity`** - Value must be unique AND enables upsert
  - Example: `:person/email`
  - Upsert: Yes (automatically updates existing entity)

### Indexes

By default, pg_mentat creates indexes on:
- EAVT (entity-attribute-value-time)
- AEVT (attribute-entity-value-time)
- AVET (attribute-value-entity-time) - for attributes marked with `:db/index true`

You can add custom indexes with:

```sql
{:db/ident :person/email
 :db/valueType :db.type/string
 :db/cardinality :db.cardinality/one
 :db/index true}  -- Creates AVET index for fast lookups by value
```

## Datalog Query Language

pg_mentat uses Datalog, a declarative logic-based query language.

### Basic Structure

```clojure
[:find ?variables-to-return
 :where
 [patterns-to-match]]
```

### Pattern Matching

The `:where` clause contains **patterns** that match datoms:

```clojure
[?e :person/name ?name]
```

This pattern matches all datoms where:
- `?e` = any entity (variable)
- `:person/name` = specific attribute (constant)
- `?name` = any value (variable)

### Variables vs Constants

- **Variables** start with `?` (e.g., `?e`, `?name`, `?age`)
  - Bind to values during pattern matching
  - Can be reused across patterns (creates joins)

- **Constants** are literal values (e.g., `:person/name`, `"Alice"`, `30`)
  - Must match exactly

### Pattern Positions

A pattern has up to 5 positions: `[E A V T added]`

Common uses:
```clojure
[?e :person/name ?name]        ; Find all names
[?e :person/name "Alice"]      ; Find entities named "Alice"
[10001 ?a ?v]                  ; Find all attributes/values for entity 10001
[?e ?a ?v]                     ; Find ALL datoms (usually not what you want!)
```

### Joins

Variables used in multiple patterns create implicit joins:

```clojure
[:find ?person-name ?friend-name
 :where
 [?person :person/name ?person-name]      ; Find person's name
 [?person :person/friend ?friend-entity]  ; Find person's friend
 [?friend-entity :person/name ?friend-name]] ; Find friend's name
```

This is like SQL:
```sql
SELECT p1.name, p2.name
FROM person p1
JOIN person_friends pf ON p1.id = pf.person_id
JOIN person p2 ON pf.friend_id = p2.id
```

### Predicates

Filter results with predicate expressions:

```clojure
[:find ?name
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(> ?age 25)]]           ; Only people older than 25
```

Supported predicates:
- Comparison: `<`, `>`, `<=`, `>=`, `=`, `!=`
- Arithmetic: `+`, `-`, `*`, `/`
- String: `str/starts-with?`, `str/ends-with?`, `str/includes?`
- Logic: `and`, `or`, `not`

### Aggregates

Aggregate functions in `:find` clause:

```clojure
[:find (count ?e) .           ; Count entities
 :where [?e :person/name]]

[:find (avg ?age) .           ; Average age
 :where [?e :person/age ?age]]

[:find ?age (count ?e)        ; Count by age (GROUP BY)
 :where [?e :person/age ?age]]
```

Aggregates:
- `count` - Count values
- `sum` - Sum numeric values
- `avg` - Average
- `min` - Minimum
- `max` - Maximum

### Find Specifications

Different ways to return results:

| Find Spec | Returns | Example |
|-----------|---------|---------|
| `[:find ?a ?b]` | Collection of tuples | `[[1 "Alice"] [2 "Bob"]]` |
| `[:find [?a ...]]` | Collection of scalars | `[1 2 3]` |
| `[:find ?a .]` | Single scalar | `42` |
| `[:find [?a ?b]]` | Single tuple | `[1 "Alice"]` |

### Inputs

Pass parameters to queries with `:in`:

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
SELECT mentat.mentat_query(
  '[:find ?name :in ?min-age ?max-age :where ...]',
  '{"inputs": [25, 35]}'::jsonb
);
```

### Rules

Define reusable query logic with rules:

```clojure
[:find ?descendant-name
 :with [[(descendant ?ancestor ?descendant)
         [?ancestor :person/child ?descendant]]
        [(descendant ?ancestor ?descendant)
         [?ancestor :person/child ?x]
         (descendant ?x ?descendant)]]  ; Recursive!
 :where
 (descendant ?alice ?desc)
 [?desc :person/name ?descendant-name]]
```

Rules enable:
- Code reuse across queries
- Recursive logic (transitive closures)
- Complex derived relationships

## Transactions

Transactions in pg_mentat are atomic and create a new transaction entity.

### Transaction Entities

Every transaction is itself an entity with a special attribute `:db/txInstant`:

```clojure
[tx-entity-id :db/txInstant #inst "2024-01-15T10:30:00Z"]
```

You can query transactions:

```clojure
[:find ?tx ?time
 :where
 [_ _ _ ?tx]              ; Any datom in transaction ?tx
 [?tx :db/txInstant ?time]]
```

### Transaction Functions

Special operations in transactions:

| Operation | Description | Example |
|-----------|-------------|---------|
| `:db/add` | Assert a fact | `[:db/add "tempid" :person/name "Alice"]` |
| `:db/retract` | Retract a fact | `[:db/retract 10001 :person/age 30]` |
| `:db/retractEntity` | Retract all facts about entity | `[:db/retractEntity 10001]` |
| `:db.fn/cas` | Compare-and-swap | `[:db.fn/cas 10001 :person/age 30 31]` |

### Tempids

Temporary IDs are strings used within a transaction:

```clojure
[{:db/id "alice"              ; Tempid
  :person/name "Alice"}
 {:db/id "bob"                 ; Tempid
  :person/friend "alice"}]    ; Reference Alice's tempid
```

Transaction returns mapping of tempids to real entity IDs:

```json
{
  "tempids": {
    "alice": 10001,
    "bob": 10002
  }
}
```

### Upsert Semantics

For attributes with `:db.unique/identity`:

```clojure
{:person/email "alice@example.com"  ; Unique identity
 :person/name "Alice Updated"}
```

If an entity with this email exists → UPDATE
If not → INSERT

For cardinality-one attributes:

```clojure
{:db/id 10001
 :person/name "New Name"}  ; Automatically retracts old name
```

## Time Travel

pg_mentat maintains complete history, enabling time-travel queries.

### Database Basis

A "database basis" is a point-in-time view identified by transaction ID.

### As-Of Queries

Query data as it existed at a specific transaction:

```sql
SELECT mentat.mentat_query(
  '[:find ?name :where [?e :person/name ?name]]',
  '{"asOf": 1000005}'::jsonb
);
```

Returns data from transaction 1000005 or earlier, ignoring later changes.

### Since Queries

Query only changes that occurred after a transaction:

```sql
SELECT mentat.mentat_query(
  '[:find ?e ?a ?v :where [?e ?a ?v]]',
  '{"since": 1000010}'::jsonb
);
```

Returns datoms with transaction ID > 1000010.

### History Queries

Include both assertions AND retractions:

```sql
SELECT mentat.mentat_query(
  '[:find ?e ?a ?v ?tx ?added
    :where [?e ?a ?v ?tx ?added]]',
  '{"history": true}'::jsonb
);
```

Returns all datoms including retracted ones (added=false).

### Use Cases

- **Audit trails** - Who changed what and when
- **Debugging** - See what data looked like when a bug occurred
- **Compliance** - Legal requirements for data retention
- **Analytics** - Analyze trends over time
- **Undo/rollback** - Revert to previous state

## Pull API

The Pull API retrieves entity data by pattern, complementing Datalog queries.

### When to Use Pull

- **After a query** - Query finds entity IDs, pull gets their attributes
- **Nested data** - Traverse relationships in one call
- **Known entity ID** - Lookup by unique attribute first, then pull

### Basic Pull

```clojure
[:person/name :person/age]  ; Specific attributes
[*]                         ; All attributes
```

### Wildcard

```clojure
[*]                                    ; All attributes
[* {:exclude [:person/password]}]     ; All except password
```

### Navigation

Follow references (`:db.type/ref` attributes):

```clojure
[:person/name
 {:person/friend [:person/name :person/email]}]  ; Follow friend ref
```

### Reverse Lookups

Find entities that reference this entity:

```clojure
[:person/name
 :person/_friend]  ; Find all entities with :person/friend pointing here
```

The underscore prefix (`_`) means "reverse lookup."

### Recursion (Planned Feature)

```clojure
[:person/name
 {:person/friend ...}]  ; Recursively follow friends
```

### Limits (Planned Feature)

```clojure
[{:person/friend [:limit 5]}]  ; At most 5 friends
```

### Defaults (Planned Feature)

```clojure
[{:person/email :default "no-email@example.com"}]
```

## Datalog vs SQL

| Aspect | Datalog | SQL |
|--------|---------|-----|
| **Style** | Declarative logic | Declarative relational |
| **Joins** | Implicit (shared variables) | Explicit (JOIN clauses) |
| **Recursion** | Native support (rules) | Limited (WITH RECURSIVE) |
| **Negation** | `not` clause | `NOT EXISTS` |
| **Aggregation** | In `:find` clause | `GROUP BY` + `HAVING` |
| **Schema** | Flexible (add attributes anytime) | Fixed (ALTER TABLE) |
| **History** | Built-in (time travel) | Manual (audit tables) |

### When to Use Each

**Use Datalog when:**
- Traversing graphs or hierarchies
- Recursive relationships (ancestors, dependencies)
- Flexible schema needed
- History/audit trail important
- Logic programming paradigm fits

**Use SQL when:**
- Complex aggregations and window functions
- Bulk operations (INSERT/UPDATE many rows)
- Integration with existing SQL tools
- Performance-critical analytical queries

**Combine both:**
- Datalog for graph traversal, SQL for aggregation
- SQL for bulk operations, Datalog for validation
- Use both in the same database!

## EDN (Extensible Data Notation)

pg_mentat uses EDN for data representation.

### Basic Types

```clojure
; Numbers
42
3.14
-1000

; Strings
"hello"
"multi\nline"

; Booleans
true
false

; Keywords (like symbols/enums)
:person/name
:status/active
:db.cardinality/one

; Vectors (ordered collection)
[1 2 3]
["Alice" 30 "alice@example.com"]

; Maps (key-value pairs)
{:name "Alice" :age 30}

; Sets
#{1 2 3}

; Nil (null)
nil
```

### Tagged Literals

Special values with type tags:

```clojure
#inst "2024-01-15T10:30:00Z"                           ; Instant (timestamp)
#uuid "550e8400-e29b-41d4-a716-446655440000"          ; UUID
```

### Why EDN?

- Human-readable
- Compact syntax
- Rich type system
- Same format for schema, data, and queries
- Compatible with Clojure and Datomic

## Performance Considerations

### Indexes

pg_mentat creates these indexes automatically:
- **EAVT** - Lookup all facts about an entity
- **AEVT** - Lookup all entities with an attribute
- **AVET** - Lookup entities by attribute value (if indexed)

Mark frequently-queried attributes with `:db/index true`.

### Query Planning

Datalog patterns are converted to SQL with CTEs. PostgreSQL's query planner optimizes the execution.

View query plan:
```sql
EXPLAIN ANALYZE
SELECT mentat.mentat_query('[:find ?e :where [?e :person/name]]', '{}');
```

### Best Practices

1. **Be specific** - Use constants where possible
2. **Index appropriately** - Mark commonly-queried attributes
3. **Limit results** - Use predicates to filter early
4. **Batch transactions** - Group multiple assertions
5. **Use Pull API** - More efficient than multiple queries

## Next Steps

- Try the examples in [QUICKSTART.md](./QUICKSTART.md)
- Explore [SQL + Datalog Integration](../examples/SQL_PLUS_DATALOG.md)
- Read [API Reference](../api/POSTGRESQL_FUNCTIONS.md)
- Check [Performance Tuning Guide](../operations/PERFORMANCE_TUNING.md)
