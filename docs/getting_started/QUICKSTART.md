# pg_mentat Quickstart Guide

Get up and running with pg_mentat in under 30 minutes.

## What is pg_mentat?

pg_mentat brings Datomic-style Datalog queries to PostgreSQL. It provides:

- **Datalog query language** - Declarative, logic-based queries
- **Immutable data model** - Full history tracking with time-travel queries
- **EAVT storage** - Entity-Attribute-Value-Time model
- **PostgreSQL integration** - Use alongside regular SQL
- **Datomic compatibility** - Drop-in replacement for many Datomic use cases

## Prerequisites

- PostgreSQL 13+ (tested on 13, 14, 15, 16, 17, 18)
- Rust 1.90.0+ (for building from source)
- cargo-pgrx (for PostgreSQL extension development)

## Installation

### Option 1: From Source (Recommended)

```bash
# Install cargo-pgrx
cargo install --locked cargo-pgrx --version 0.12.13

# Initialize pgrx (first time only)
cargo pgrx init

# Clone and build pg_mentat
git clone https://github.com/your-org/pg_mentat.git
cd pg_mentat/pg_mentat

# Install extension for PostgreSQL 16 (adjust version as needed)
cargo pgrx install --pg-config $(which pg_config)
```

### Option 2: Docker

```bash
# Pull the Docker image (when available)
docker pull pg_mentat:latest

# Run PostgreSQL with pg_mentat
docker run -p 5432:5432 -e POSTGRES_PASSWORD=postgres pg_mentat:latest
```

## Initial Setup

Connect to your PostgreSQL database:

```bash
psql -U postgres
```

Create the extension:

```sql
CREATE EXTENSION pg_mentat;
```

Verify installation:

```sql
SELECT mentat.mentat_query(
  '[:find ?e :where [?e :db/ident :db/ident]]',
  '{}'::jsonb
);
```

You should see a result with entity IDs for the bootstrap schema.

## Your First Schema

Define attributes using EDN (Extensible Data Notation):

```sql
SELECT mentat.mentat_transact($$
[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/doc "A person's full name"}

  {:db/ident :person/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}

  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity
   :db/doc "Unique email address"}

  {:db/ident :person/hobbies
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/many
   :db/doc "A person's hobbies"}

  {:db/ident :person/friend
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many
   :db/doc "Reference to another person entity"}
]
$$);
```

**Schema attributes:**
- `:db/ident` - Unique keyword identifier for the attribute
- `:db/valueType` - Data type (string, long, double, boolean, instant, ref, keyword, uuid, bytes)
- `:db/cardinality` - Either `:db.cardinality/one` or `:db.cardinality/many`
- `:db/unique` - Optional: `:db.unique/value` or `:db.unique/identity`
- `:db/doc` - Optional documentation string

## Your First Transaction

Insert data using EDN map notation:

```sql
SELECT mentat.mentat_transact($$
[
  {:db/id "alice"
   :person/name "Alice Johnson"
   :person/age 30
   :person/email "alice@example.com"
   :person/hobbies ["reading" "hiking" "photography"]}

  {:db/id "bob"
   :person/name "Bob Smith"
   :person/age 28
   :person/email "bob@example.com"
   :person/hobbies ["gaming" "cooking"]
   :person/friend "alice"}

  {:db/id "carol"
   :person/name "Carol Williams"
   :person/age 35
   :person/email "carol@example.com"
   :person/friend ["alice" "bob"]}
]
$$);
```

**Transaction syntax:**
- Use temporary IDs (strings like `"alice"`) to reference entities within the same transaction
- Cardinality-one attributes: single value
- Cardinality-many attributes: vector of values `[...]`
- Ref attributes: reference other entity IDs (temporary or permanent)

**Result:**
```json
{
  "tx": 1000012,
  "tempids": {
    "alice": 10001,
    "bob": 10002,
    "carol": 10003
  }
}
```

## Your First Query

Query data using Datalog:

```sql
SELECT mentat.mentat_query(
  '[:find ?name ?age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]]',
  '{}'::jsonb
);
```

**Result:**
```json
{
  "columns": ["?name", "?age"],
  "results": [
    ["Alice Johnson", 30],
    ["Bob Smith", 28],
    ["Carol Williams", 35]
  ]
}
```

### Query with Filters

```sql
SELECT mentat.mentat_query(
  '[:find ?name
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    [(> ?age 28)]]',
  '{}'::jsonb
);
```

**Result:**
```json
{
  "columns": ["?name"],
  "results": [
    ["Alice Johnson"],
    ["Carol Williams"]
  ]
}
```

### Query with Joins

Find people and their friends' names:

```sql
SELECT mentat.mentat_query(
  '[:find ?name ?friend-name
    :where
    [?e :person/name ?name]
    [?e :person/friend ?friend]
    [?friend :person/name ?friend-name]]',
  '{}'::jsonb
);
```

**Result:**
```json
{
  "columns": ["?name", "?friend-name"],
  "results": [
    ["Bob Smith", "Alice Johnson"],
    ["Carol Williams", "Alice Johnson"],
    ["Carol Williams", "Bob Smith"]
  ]
}
```

### Queries with Inputs

Parameterized queries using `:in`:

```sql
SELECT mentat.mentat_query(
  '[:find ?name
    :in ?min-age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    [(>= ?age ?min-age)]]',
  '{"inputs": [30]}'::jsonb
);
```

**Result:** Returns only people aged 30 or older.

## Your First Pull

Pull API retrieves entity data by pattern:

```sql
-- Get entity ID first
SELECT mentat.mentat_query(
  '[:find ?e .
    :where
    [?e :person/email "alice@example.com"]]',
  '{}'::jsonb
);
-- Returns: {"result": 10001}

-- Pull entity data
SELECT mentat.mentat_pull(
  '[:person/name :person/age :person/email :person/hobbies]',
  10001
);
```

**Result:**
```json
{
  ":db/id": 10001,
  ":person/name": "Alice Johnson",
  ":person/age": 30,
  ":person/email": "alice@example.com",
  ":person/hobbies": ["reading", "hiking", "photography"]
}
```

### Pull with Wildcard

```sql
SELECT mentat.mentat_pull('[*]', 10001);
```

Returns all attributes for the entity.

### Pull with Navigation

```sql
-- Pull entity and follow friend references
SELECT mentat.mentat_pull(
  '[:person/name {:person/friend [:person/name :person/email]}]',
  10002
);
```

**Result:**
```json
{
  ":db/id": 10002,
  ":person/name": "Bob Smith",
  ":person/friend": {
    ":db/id": 10001,
    ":person/name": "Alice Johnson",
    ":person/email": "alice@example.com"
  }
}
```

## Time Travel Queries

pg_mentat maintains full history. Query data as it existed at any point in time:

### As-Of Queries

```sql
-- Find current transaction ID
SELECT mentat.mentat_query('[:find (max ?tx) . :where [_ _ _ ?tx]]', '{}'::jsonb);
-- Returns: {"result": 1000012}

-- Query as-of specific transaction
SELECT mentat.mentat_query(
  '[:find ?name :where [?e :person/name ?name]]',
  '{"asOf": 1000005}'::jsonb
);
```

Returns data as it existed at transaction 1000005.

### Since Queries

```sql
-- Find all changes since transaction 1000010
SELECT mentat.mentat_query(
  '[:find ?e ?a ?v
    :where [?e ?a ?v]]',
  '{"since": 1000010}'::jsonb
);
```

### History Queries

```sql
-- See all assertions AND retractions (full audit log)
SELECT mentat.mentat_query(
  '[:find ?name ?tx ?added
    :where [?e :person/name ?name ?tx ?added]]',
  '{"history": true}'::jsonb
);
```

**Result shows** which transactions asserted/retracted each value:
```json
{
  "results": [
    ["Alice Johnson", 1000012, true],
    ["Old Name", 1000008, true],
    ["Old Name", 1000012, false]
  ]
}
```

## Retracting Data

### Retract Specific Attribute

```sql
SELECT mentat.mentat_transact($$
[[:db/retract 10001 :person/age 30]]
$$);
```

### Retract Entire Entity

```sql
SELECT mentat.mentat_transact($$
[[:db/retractEntity 10001]]
$$);
```

Retracts all attributes of entity 10001.

## Using with Clojure (mentatd)

If you want to use pg_mentat with the Datomic client library from Clojure:

### Start mentatd daemon

```bash
cd mentatd
cargo build --release
./target/release/mentatd --host 127.0.0.1 --port 8080
```

### Connect from Clojure

```clojure
(require '[datomic.api :as d])

;; Connect using Datomic connection string
(def conn (d/connect "datomic:sql://mentat?jdbc:postgresql://localhost:5432/postgres"))

;; Query
(d/q '[:find ?name
       :where [?e :person/name ?name]]
     (d/db conn))

;; Transact
(d/transact conn
  [{:db/id "new-person"
    :person/name "Dave"
    :person/age 25}])

;; Pull
(d/pull (d/db conn)
        [:person/name :person/age]
        [:person/email "dave@example.com"])
```

## Next Steps

- Read [CONCEPTS.md](./CONCEPTS.md) to understand the EAVT model and Datalog
- Explore [SQL + Datalog Integration](../examples/SQL_PLUS_DATALOG.md) for hybrid queries
- Check [API Reference](../api/POSTGRESQL_FUNCTIONS.md) for all functions
- See [Troubleshooting Guide](../troubleshooting/COMMON_ISSUES.md) for common issues

## Common Patterns

### Upsert (Insert or Update)

```sql
-- Entity with unique :person/email gets updated if exists, inserted if not
SELECT mentat.mentat_transact($$
[{:person/email "alice@example.com"
  :person/name "Alice J. Updated"
  :person/age 31}]
$$);
```

### Aggregates

```sql
-- Count people
SELECT mentat.mentat_query(
  '[:find (count ?e) .
    :where [?e :person/name]]',
  '{}'::jsonb
);

-- Average age
SELECT mentat.mentat_query(
  '[:find (avg ?age) .
    :where [?e :person/age ?age]]',
  '{}'::jsonb
);

-- Group by age
SELECT mentat.mentat_query(
  '[:find ?age (count ?e)
    :where [?e :person/age ?age]]',
  '{}'::jsonb
);
```

### Rules

```sql
-- Define reusable rules
SELECT mentat.mentat_query($$
[:find ?name
 :with [[(adult ?person)
         [?person :person/age ?age]
         [(>= ?age 18)]]]
 :where
 (adult ?e)
 [?e :person/name ?name]]
$$, '{}'::jsonb);
```

### Recursive Rules

```sql
-- Find all ancestors (transitive closure)
SELECT mentat.mentat_query($$
[:find ?ancestor-name
 :in ?person
 :with [[(ancestor ?a ?d)
         [?a :person/parent ?d]]
        [(ancestor ?a ?d)
         [?a :person/parent ?x]
         (ancestor ?x ?d)]]
 :where
 (ancestor ?person ?ancestor)
 [?ancestor :person/name ?ancestor-name]]
$$, '{"inputs": [10001]}'::jsonb);
```

## Tips

1. **Use tempids** for multi-entity transactions
2. **Define unique constraints** on attributes that should be unique (email, username, etc.)
3. **Use Pull API** for nested data retrieval (more efficient than multiple queries)
4. **Query history** for audit trails and debugging
5. **Combine with SQL** for complex aggregations and reporting

## Performance

- pg_mentat uses standard PostgreSQL indexes (automatically created)
- For large datasets, consider adding indexes on frequently queried attributes
- Use `:db/index true` for attributes that need custom indexes
- Query plans can be viewed with `EXPLAIN ANALYZE`

## Support

- GitHub Issues: https://github.com/your-org/pg_mentat/issues
- Docs: https://pg-mentat.dev
- Examples: See `examples/` directory in repository
