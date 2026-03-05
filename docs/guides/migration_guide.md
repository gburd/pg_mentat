# Migration Guide

Complete guide for migrating to pg_mentat and mentatd from other systems.

## Migration Paths

- [From Datomic](#from-datomic) - Move from Datomic to Mentat
- [From SQLite Mentat](#from-sqlite-mentat) - Upgrade from original Mentat
- [From Other Databases](#from-other-databases) - Import from SQL/NoSQL

## From Datomic

### Overview

Migrating from Datomic to Mentat involves:
1. Schema translation (minimal changes)
2. Data export from Datomic
3. Data import into Mentat
4. Client code updates (if needed)
5. Testing and validation

### Compatibility Assessment

**What Works Unchanged:**
- Schema definitions
- Basic Datalog queries
- Transactions (add/retract)
- Entity lookups
- Cardinality and uniqueness

**What Needs Adaptation:**
- Database functions (:db/fn)
- Partition references
- Advanced pull patterns
- Peer-specific APIs

See [Datomic Compatibility Matrix](../api/datomic_compat.md) for details.

### Step 1: Export Datomic Schema

Export schema from Datomic:

```clojure
(require '[datomic.api :as d])

(def conn (d/connect "datomic:dev://localhost:4334/mydb"))
(def db (d/db conn))

;; Get all schema attributes
(def schema
  (d/q '[:find ?ident ?valueType ?cardinality ?unique ?indexed ?doc
         :where
         [?e :db/ident ?ident]
         [?e :db/valueType ?vt]
         [?vt :db/ident ?valueType]
         [?e :db/cardinality ?c]
         [?c :db/ident ?cardinality]
         [(get-else $ ?e :db/unique nil) ?unique]
         [(get-else $ ?e :db/index false) ?indexed]
         [(get-else $ ?e :db/doc "") ?doc]]
       db))

;; Write to EDN file
(spit "schema.edn"
  (pr-str
    (for [[ident vt card uniq idx doc] schema]
      (cond-> {:db/ident ident
               :db/valueType vt
               :db/cardinality card}
        uniq (assoc :db/unique uniq)
        idx (assoc :db/index true)
        (seq doc) (assoc :db/doc doc)))))
```

### Step 2: Export Datomic Data

Export data in batches:

```clojure
(require '[clojure.java.io :as io])

;; Get all entities (excluding schema)
(def entities
  (d/q '[:find [?e ...]
         :where
         [?e :db/ident _]
         (not [?e :db/ident ?ident]
              [(namespace ?ident) ?ns]
              [(= ?ns "db")])]
       db))

;; Export in batches
(defn export-entities [entities batch-size filename]
  (with-open [w (io/writer filename)]
    (doseq [batch (partition-all batch-size entities)]
      (let [entity-maps (map #(d/entity db %) batch)
            tx-data (map #(into {} %) entity-maps)]
        (.write w (pr-str tx-data))
        (.write w "\n")))))

(export-entities entities 1000 "data.edn")
```

### Step 3: Transform Schema

Remove Datomic-specific attributes:

```bash
# Remove partition references
sed -i 's/:db\/id #db\/id\[:db.part\/db\]/:db\/id "tempid"/g' schema.edn

# Remove :db.install/* attributes
grep -v ':db.install/' schema.edn > schema-clean.edn
```

### Step 4: Import into Mentat

Load schema:

```sql
-- Load pg_mentat extension
CREATE EXTENSION pg_mentat;

-- Import schema
SELECT mentat.mentat_transact(pg_read_file('schema-clean.edn'));

-- Verify schema
SELECT mentat.mentat_schema();
```

Load data:

```bash
# Process data file line by line
while IFS= read -r line; do
  psql -d mentat -c "SELECT mentat.mentat_transact('$line');"
done < data.edn
```

Or use a script:

```python
import psycopg2
import edn_format

conn = psycopg2.connect("postgresql://localhost/mentat")
cur = conn.cursor()

with open('data.edn') as f:
    for line in f:
        tx_data = line.strip()
        cur.execute("SELECT mentat.mentat_transact(%s)", (tx_data,))
        conn.commit()

cur.close()
conn.close()
```

### Step 5: Update Client Code

**Clojure Client:**

```clojure
;; Before (Datomic peer)
(require '[datomic.api :as d])
(def conn (d/connect "datomic:dev://localhost:4334/mydb"))
(def db (d/db conn))

;; After (Datomic client with mentatd)
(require '[datomic.client.api :as d])
(def client (d/client {:server-type :peer-server
                       :endpoint "localhost:8080"}))
(def conn (d/connect client {:db-name "mentat"}))
(def db (d/db conn))

;; Queries remain the same!
(d/q '[:find ?name
       :where [?e :person/name ?name]]
     db)
```

**JavaScript Client:**

```javascript
// Before (Datomic Cloud)
const { Client } = require('datomic-client-js');
const client = new Client({
  endpoint: 'https://api.datomic.com',
  region: 'us-east-1'
});

// After (mentatd)
const client = new Client({
  endpoint: 'http://localhost:8080'
});

// Same API
const conn = await client.connect({ dbName: 'mentat' });
const db = await conn.db();
const results = await db.q({
  query: '[:find ?name :where [?e :person/name ?name]]'
});
```

### Step 6: Migrate Database Functions

Database functions (:db/fn) must be rewritten as application logic.

**Before (Datomic):**

```clojure
;; Schema
[{:db/ident :transfer-funds
  :db/fn #db/fn {:lang :clojure
                 :params [db from-account to-account amount]
                 :code (fn [db from to amt]
                         [[:db/add from :account/balance
                           (- (:account/balance from) amt)]
                          [:db/add to :account/balance
                           (+ (:account/balance to) amt)]])}}]

;; Transaction
[[:transfer-funds account1 account2 100]]
```

**After (Mentat):**

```clojure
;; Application logic
(defn transfer-funds [conn from-id to-id amount]
  (let [db (d/db conn)
        from-bal (d/q '[:find ?bal .
                        :in $ ?e
                        :where [?e :account/balance ?bal]]
                      db from-id)
        to-bal (d/q '[:find ?bal .
                      :in $ ?e
                      :where [?e :account/balance ?bal]]
                    db to-id)]
    @(d/transact conn
       [[:db/add from-id :account/balance (- from-bal amount)]
        [:db/add to-id :account/balance (+ to-bal amount)]])))

;; Use function
(transfer-funds conn account1 account2 100)
```

### Step 7: Validation

Verify migration:

```sql
-- Check entity counts
SELECT COUNT(*) FROM mentat.datoms WHERE added = true;

-- Compare with Datomic count
-- Should match number of datoms exported

-- Verify specific entities
SELECT mentat.mentat_entity(100);

-- Run test queries
SELECT mentat.mentat_query('
  [:find ?name ?email
   :where
   [?e :person/name ?name]
   [?e :person/email ?email]]
', '{}'::jsonb);

-- Check schema completeness
SELECT COUNT(*) FROM jsonb_object_keys(mentat.mentat_schema());
```

### Migration Checklist

- [ ] Export Datomic schema
- [ ] Export Datomic data
- [ ] Transform schema (remove partitions)
- [ ] Import schema into Mentat
- [ ] Import data into Mentat
- [ ] Verify entity counts
- [ ] Test queries
- [ ] Rewrite database functions
- [ ] Update client code
- [ ] Run integration tests
- [ ] Performance testing
- [ ] Update documentation
- [ ] Train team on differences

## From SQLite Mentat

### Overview

The original Mentat uses SQLite for storage. Migration to PostgreSQL involves:
1. Database export
2. Schema migration
3. Data transformation
4. Import into pg_mentat
5. Client API updates

### Step 1: Export SQLite Data

Using original Mentat API:

```rust
use mentat::{Store, NamespacedKeyword};

// Open SQLite database
let mut store = Store::open("/path/to/mentat.db")?;
let conn = store.begin_transaction()?;

// Export schema
let schema = conn.current_schema();
println!("{:?}", schema);

// Export all datoms
let datoms = conn.q("[:find ?e ?a ?v ?tx ?added
                      :where [?e ?a ?v ?tx ?added]]")?;

// Write to file
std::fs::write("export.edn", format!("{:?}", datoms))?;
```

### Step 2: Transform Data Format

SQLite Mentat uses different internal IDs:

```rust
// Transform entity IDs to be PostgreSQL-compatible
// SQLite uses ROWID (1, 2, 3...)
// PostgreSQL uses larger IDs (100, 101, 102...)

let offset = 100;
let transformed_datoms = datoms.iter().map(|d| {
    (d.0 + offset, d.1, d.2, d.3 + offset, d.4)
}).collect();
```

### Step 3: Import into PostgreSQL

```sql
-- Create database and load extension
CREATE DATABASE mentat;
\c mentat
CREATE EXTENSION pg_mentat;

-- Import schema
SELECT mentat.mentat_transact('[schema-definition]');

-- Import data
SELECT mentat.mentat_transact('[data-transactions]');
```

### Step 4: Update Application Code

**Before (SQLite Mentat):**

```rust
use mentat::{Store, QueryResults};

let mut store = Store::open("mentat.db")?;
let conn = store.begin_transaction()?;

let results = conn.q("[:find ?name
                       :where [?e :person/name ?name]]")?;
```

**After (PostgreSQL via SQL):**

```rust
use tokio_postgres::{Client, NoTls};

let (client, connection) = tokio_postgres::connect(
    "postgresql://localhost/mentat",
    NoTls
).await?;

let rows = client.query(
    "SELECT mentat.mentat_query($1, $2)",
    &[
        &"[:find ?name :where [?e :person/name ?name]]",
        &serde_json::json!({})
    ]
).await?;
```

## From Other Databases

### From PostgreSQL

If you have data in regular PostgreSQL tables:

```sql
-- Map existing data to Mentat schema
WITH person_data AS (
  SELECT
    id,
    json_build_object(
      ':db/id', id + 100,
      ':person/name', name,
      ':person/email', email,
      ':person/age', age
    )::text as entity
  FROM users
)
SELECT mentat.mentat_transact(
  '[' || string_agg(entity, ',') || ']'
) FROM person_data;
```

### From MongoDB

Export to JSON, then import:

```javascript
// Export from MongoDB
db.people.find().forEach(doc => {
  const entity = {
    'db/id': `mongo-${doc._id}`,
    'person/name': doc.name,
    'person/email': doc.email,
    'person/age': doc.age
  };
  print(JSON.stringify(entity));
});
```

```bash
# Convert to EDN and import
mongo mydb --quiet export.js | \
  node -e 'process.stdin.on("data", d => console.log(d.toString().replace(/"/g, ":")))' | \
  psql -d mentat -c "SELECT mentat.mentat_transact(stdin)"
```

### From MySQL

```sql
-- MySQL export
SELECT JSON_OBJECT(
  ':db/id', CONCAT('mysql-', id),
  ':person/name', name,
  ':person/email', email
) FROM users INTO OUTFILE '/tmp/export.json';

-- Import into Mentat
\copy (
  SELECT mentat.mentat_transact(entity_json)
  FROM jsonb_array_elements_text(
    pg_read_file('/tmp/export.json')::jsonb
  ) AS entity_json
) TO STDOUT;
```

## Data Transformation Patterns

### Normalize Relationships

Transform foreign keys to refs:

```sql
-- Before: users table with foreign keys
SELECT * FROM users;
-- id | name  | company_id
-- 1  | Alice | 100
-- 2  | Bob   | 100

-- After: Mentat schema
[{:db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one}
 {:db/ident :person/company
  :db/valueType :db.type/ref
  :db/cardinality :db.cardinality/one}
 {:db/ident :company/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one}]

-- Transform and import
WITH person_data AS (
  SELECT id, name, company_id FROM users
)
SELECT mentat.mentat_transact(format('
  [{:db/id %s :person/name "%s" :person/company %s}]
', id + 100, name, company_id + 100))
FROM person_data;
```

### Denormalize Collections

Transform one-to-many to cardinality-many:

```sql
-- Before: user_tags table
SELECT * FROM user_tags;
-- user_id | tag
-- 1       | engineer
-- 1       | manager
-- 2       | developer

-- After: Mentat cardinality-many
SELECT mentat.mentat_transact(format('
  [{:db/id %s :person/tags [%s]}]
', user_id + 100,
  string_agg('"' || tag || '"', ' ')
))
FROM user_tags
GROUP BY user_id;
```

### Handle NULLs

Mentat doesn't store null values:

```sql
-- Before: nullable columns
SELECT id, name, phone FROM users WHERE phone IS NOT NULL;

-- Transform: only include non-null attributes
WITH user_data AS (
  SELECT
    id,
    json_strip_nulls(
      json_build_object(
        ':db/id', id + 100,
        ':person/name', name,
        ':person/phone', NULLIF(phone, '')
      )
    ) as entity
  FROM users
)
SELECT mentat.mentat_transact(entity::text)
FROM user_data;
```

## Validation and Testing

### Data Integrity Checks

```sql
-- Count entities
SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE added = true;

-- Check for orphaned refs
SELECT d.e, d.v
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.value_type = 'ref'
  AND d.added = true
  AND NOT EXISTS (
    SELECT 1 FROM mentat.datoms d2
    WHERE d2.e = d.v::bigint
  );

-- Verify unique constraints
SELECT s.ident, d.v, COUNT(*)
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.unique IS NOT NULL
  AND d.added = true
GROUP BY s.ident, d.v
HAVING COUNT(*) > 1;

-- Check cardinality violations
SELECT d.e, s.ident, COUNT(*)
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.cardinality = 'one'
  AND d.added = true
GROUP BY d.e, s.ident
HAVING COUNT(*) > 1;
```

### Performance Testing

```sql
-- Query performance baseline
EXPLAIN ANALYZE
SELECT mentat.mentat_query('
  [:find ?name ?email
   :where
   [?e :person/name ?name]
   [?e :person/email ?email]]
', '{}'::jsonb);

-- Index usage verification
SELECT schemaname, tablename, indexname, idx_scan
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
ORDER BY idx_scan;

-- Enable query logging
ALTER DATABASE mentat SET log_min_duration_statement = 100;
```

## Rollback Plan

Always prepare a rollback strategy:

```bash
# Backup before migration
pg_dump mentat > backup-before-migration.sql

# If migration fails, restore
dropdb mentat
createdb mentat
psql mentat < backup-before-migration.sql

# Or use point-in-time recovery
# Requires WAL archiving enabled
```

## Common Issues

### Large Dataset Migration

For databases > 1GB:

```bash
# Use parallel workers
cat data.edn | parallel --pipe -N 1000 \
  psql -d mentat -c "SELECT mentat.mentat_transact(line)"

# Or split into chunks
split -l 10000 data.edn chunk-
for file in chunk-*; do
  psql -d mentat -f "$file"
done
```

### Memory Exhaustion

```sql
-- Process in batches
DO $$
DECLARE
  batch_size INT := 1000;
  offset_val INT := 0;
  total INT;
BEGIN
  SELECT COUNT(*) INTO total FROM source_table;

  WHILE offset_val < total LOOP
    PERFORM mentat.mentat_transact(
      (SELECT json_agg(entity) FROM (
        SELECT * FROM source_table
        LIMIT batch_size OFFSET offset_val
      ) batch)::text
    );

    offset_val := offset_val + batch_size;
    RAISE NOTICE 'Processed % / %', offset_val, total;
  END LOOP;
END $$;
```

### Transaction Timeout

```sql
-- Increase timeout for migration
SET statement_timeout = '1h';

-- Or split into smaller transactions
\set ON_ERROR_STOP on
BEGIN;
  SELECT mentat.mentat_transact('[batch-1]');
COMMIT;
BEGIN;
  SELECT mentat.mentat_transact('[batch-2]');
COMMIT;
```

## Best Practices

1. **Backup first** - Always backup before migration
2. **Test with subset** - Migrate 10% of data first
3. **Validate early** - Check integrity after each step
4. **Use batches** - Don't load entire dataset at once
5. **Monitor resources** - Watch memory and disk space
6. **Document mapping** - Keep record of schema transformations
7. **Parallel testing** - Run old and new systems simultaneously
8. **Plan rollback** - Have rollback procedure ready

## Migration Tools

### Custom Migration Script

```python
#!/usr/bin/env python3
import psycopg2
import json
import sys

def migrate_table(conn, table_name, schema_mapping):
    """
    Migrate a SQL table to Mentat format.

    schema_mapping: dict mapping column names to Mentat attributes
    """
    cur = conn.cursor()

    # Get data
    cur.execute(f"SELECT * FROM {table_name}")
    rows = cur.fetchall()
    columns = [desc[0] for desc in cur.description]

    # Transform to Mentat entities
    entities = []
    for i, row in enumerate(rows):
        entity = {':db/id': f'{table_name}-{i}'}
        for col, val in zip(columns, row):
            if col in schema_mapping and val is not None:
                entity[schema_mapping[col]] = val
        entities.append(entity)

    # Import batch
    tx_data = json.dumps(entities)
    cur.execute("SELECT mentat.mentat_transact(%s)", (tx_data,))
    conn.commit()

    return len(entities)

# Usage
conn = psycopg2.connect("postgresql://localhost/mentat")

mapping = {
    'name': ':person/name',
    'email': ':person/email',
    'age': ':person/age'
}

count = migrate_table(conn, 'users', mapping)
print(f"Migrated {count} entities")

conn.close()
```

## See Also

- [Datomic Compatibility Matrix](../api/datomic_compat.md)
- [SQL Function API](../api/sql_functions.md)
- [Quickstart Guide](./quickstart.md)
- [Performance Guide](./performance.md)
