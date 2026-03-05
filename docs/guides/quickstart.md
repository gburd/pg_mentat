# Mentat PostgreSQL Quickstart

Get up and running with Mentat's PostgreSQL-backed Datalog database in 5 minutes.

## What You'll Build

A simple person database with Datalog queries demonstrating:
- Schema definition
- Data transactions
- Datalog pattern matching
- Time-travel queries

## Prerequisites

- PostgreSQL 13+ installed and running
- Rust toolchain (latest stable via rustup)
- cargo-pgrx installed

## Step 1: Install pg_mentat Extension

```bash
# Install pgrx CLI
cargo install cargo-pgrx --locked

# Initialize pgrx with your PostgreSQL
cargo pgrx init --pg16=/path/to/pg_config

# Example on macOS with Homebrew:
cargo pgrx init --pg16=/opt/homebrew/bin/pg_config

# Build and install pg_mentat
cd pg_mentat
cargo pgrx install
```

## Step 2: Create Database

```bash
# Create a new database
createdb mentat_quickstart

# Connect to it
psql mentat_quickstart
```

## Step 3: Load Extension

```sql
-- Load the pg_mentat extension
CREATE EXTENSION pg_mentat;

-- Verify installation
SELECT mentat.mentat_schema();
```

## Step 4: Define Schema

```sql
-- Add person attributes
SELECT mentat.mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/doc "A person''s full name"}

  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity
   :db/doc "Email address"}

  {:db/ident :person/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one
   :db/index true}

  {:db/ident :person/friend
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many
   :db/doc "Friends are other people"}
]');
```

## Step 5: Add Data

```sql
-- Insert people
SELECT mentat.mentat_transact('[
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

## Step 6: Query with Datalog

```sql
-- Find all people
SELECT mentat.mentat_query('
  [:find ?name ?email
   :where
   [?e :person/name ?name]
   [?e :person/email ?email]]
', '{}'::jsonb);

-- Result:
-- {
--   "columns": ["?name", "?email"],
--   "results": [
--     ["Alice Anderson", "alice@example.com"],
--     ["Bob Brown", "bob@example.com"]
--   ]
-- }

-- Find people over 25
SELECT mentat.mentat_query('
  [:find ?name ?age
   :where
   [?e :person/name ?name]
   [?e :person/age ?age]
   [(>= ?age 25)]]
', '{}'::jsonb);
```

## Step 7: Get Entity Data

```sql
-- Get all data for Alice
SELECT mentat.mentat_entity(100);

-- Result:
-- {
--   ":db/id": 100,
--   ":person/name": "Alice Anderson",
--   ":person/email": "alice@example.com",
--   ":person/age": 30
-- }
```

## Step 8: Update Data

```sql
-- Update Alice's age
SELECT mentat.mentat_transact('[
  [:db/add [:person/email "alice@example.com"] :person/age 31]
]');
```

## Working with EDN Values

The extension provides a custom EDN type for rich data structures:

```sql
-- Create table with EDN column
CREATE TABLE events (
    id SERIAL PRIMARY KEY,
    data mentat.EdnValue
);

-- Insert EDN data
INSERT INTO events (data) VALUES
    (mentat.edn_in('{:event/type :login :user/id 123 :timestamp "2026-03-05"}')),
    (mentat.edn_in('[1 2 3 4 5]'));

-- Query EDN data
SELECT id, mentat.edn_out(data) FROM events;

-- Extract values from EDN maps
SELECT
    id,
    mentat.edn_get(data, mentat.edn_in(':event/type')) as event_type
FROM events
WHERE mentat.edn_is_map(data);
```

## Next Steps

**Learn More:**
- [Installation Guide](../installation/pg_mentat.md) - Detailed setup instructions
- [SQL Function Reference](../api/sql_functions.md) - Complete API documentation
- [Migration Guide](./migration_guide.md) - Move from Datomic or SQLite

**Run mentatd Server:**
- [mentatd Setup](../installation/mentatd.md) - HTTP server with Datomic protocol
- [Configuration Guide](../configuration/mentatd_config.md) - Server configuration

**Explore Features:**
- Time-travel queries (as-of, since)
- Recursive rules
- Aggregate functions
- Pull API

## Troubleshooting

**Extension not found:**
```bash
# Verify installation
ls $(pg_config --sharedir)/extension/pg_mentat*

# If missing, reinstall
cd pg_mentat && cargo pgrx install
```

**pgrx initialization error:**
```bash
# Make sure pgrx is initialized
cargo pgrx init --pg16=$(which pg_config)
```

**Query errors:**
- Check schema with `SELECT mentat.mentat_schema()`
- Verify EDN syntax is valid
- Use `:db/id` or unique attributes to reference entities

## Example: Social Network

Complete example with relationships:

```sql
-- Schema
SELECT mentat.mentat_transact('[
  {:db/ident :user/username
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}

  {:db/ident :user/follows
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many}

  {:db/ident :post/author
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one}

  {:db/ident :post/content
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Data
SELECT mentat.mentat_transact('[
  {:db/id "alice" :user/username "alice"}
  {:db/id "bob" :user/username "bob"}
  {:db/id "carol" :user/username "carol"}

  [:db/add "alice" :user/follows "bob"]
  [:db/add "alice" :user/follows "carol"]
  [:db/add "bob" :user/follows "carol"]

  {:post/author "alice" :post/content "Hello world!"}
  {:post/author "bob" :post/content "Great post, Alice!"}
]');

-- Query: Find all posts by people Alice follows
SELECT mentat.mentat_query('
  [:find ?username ?content
   :where
   [?alice :user/username "alice"]
   [?alice :user/follows ?followed]
   [?followed :user/username ?username]
   [?post :post/author ?followed]
   [?post :post/content ?content]]
', '{}'::jsonb);
```

## Resources

- [GitHub Repository](https://github.com/qpdb/mentat)
- [EDN Format Specification](https://github.com/edn-format/edn)
- [Datomic Query Documentation](https://docs.datomic.com/query/query.html)
- [PostgreSQL Extension Development](https://www.postgresql.org/docs/current/extend-extensions.html)
