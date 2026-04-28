# Migration from Datomic to pg_mentat

This guide covers migrating existing Datomic applications to pg_mentat: data migration, schema translation, client library changes, and feature compatibility.

---

## Table of Contents

1. [Overview](#overview)
2. [Feature Compatibility Matrix](#feature-compatibility-matrix)
3. [Architecture Differences](#architecture-differences)
4. [Schema Translation](#schema-translation)
5. [Data Migration](#data-migration)
6. [Client Library Migration](#client-library-migration)
7. [Query Migration](#query-migration)
8. [Transaction Migration](#transaction-migration)
9. [API Mapping Reference](#api-mapping-reference)
10. [Performance Comparison](#performance-comparison)
11. [Migration Checklist](#migration-checklist)
12. [When to Migrate (and When Not To)](#when-to-migrate-and-when-not-to)

---

## Overview

pg_mentat is a Datomic-compatible Datalog query engine implemented as a PostgreSQL extension. It stores data as EAVT (Entity-Attribute-Value-Transaction) datoms and supports Datalog queries, the Pull API, and time-travel queries.

**What works:**
- Schema definitions (value types, cardinality, uniqueness)
- Transactions (assert, retract, retractEntity)
- Datalog queries (`:find`, `:where`, `:in`, rules, aggregates, OR/NOT clauses)
- Pull API (wildcards, nested pulls, reverse lookups, limits, defaults)
- Time travel (`asOf`, `since`, `history`)
- Lookup refs

**What does not work yet:**
- Speculative transactions (`with`)
- Transaction functions
- Full-text search integration in Datalog (`fulltext` built-in)
- Datomic Client wire protocol (Transit+JSON)
- Excision
- Datomic Cloud features (ions, client API)

---

## Feature Compatibility Matrix

| Feature | Datomic | pg_mentat | Notes |
|---------|---------|-----------|-------|
| **Schema** | | | |
| Value types (string, long, double, boolean, instant, keyword, ref, uuid, bytes) | Yes | Yes | All 9 types supported |
| Cardinality one/many | Yes | Yes | |
| `:db.unique/identity` | Yes | Partial | Upsert semantics under development |
| `:db.unique/value` | Yes | Yes | |
| `:db/index` | Yes | Yes | |
| `:db/fulltext` | Yes | Partial | tsvector support exists but not via Datalog `(fulltext $)` |
| `:db/isComponent` | Yes | Yes | |
| `:db/noHistory` | Yes | No | All datoms are recorded in history |
| Schema alteration | Yes | No | Must manually ALTER tables |
| **Transactions** | | | |
| `:db/add` | Yes | Yes | |
| `:db/retract` | Yes | Yes | |
| `:db/retractEntity` | Yes | Yes | |
| `:db.fn/cas` (compare-and-swap) | Yes | Yes | No automatic retry on conflict |
| Transaction functions | Yes | No | |
| Tempids | Yes | Yes | |
| Lookup refs in transactions | Yes | Yes | |
| **Queries** | | | |
| `:find` (relation, tuple, collection, scalar) | Yes | Yes | |
| `:where` patterns | Yes | Yes | |
| `:in` bindings | Yes | Yes | |
| Predicates (`>`, `<`, `=`, etc.) | Yes | Yes | |
| Built-in functions (`ground`, `get-else`) | Yes | Yes | |
| Rules (recursive) | Yes | Yes | |
| Aggregates (`count`, `sum`, `avg`, `min`, `max`) | Yes | Yes | |
| OR clauses | Yes | Yes | Single OR clause per query |
| NOT clauses | Yes | Yes | |
| `(fulltext $)` function | Yes | No | Use PostgreSQL full-text search directly |
| **Pull API** | | | |
| Forward attributes | Yes | Yes | |
| Reverse attributes (`_ref`) | Yes | Yes | |
| Wildcards (`*`) | Yes | Yes | |
| Nested pulls | Yes | Yes | |
| Limits and defaults | Yes | Yes | |
| Recursive component pulls | Yes | Yes | |
| **Time Travel** | | | |
| `asOf` | Yes | Yes | |
| `since` | Yes | Yes | |
| `history` | Yes | Yes | Untested with complex query combinations |
| Bitemporal queries | Yes (XTDB) | No | Single-temporal only |
| **Speculative** | | | |
| `with` (speculative transactions) | Yes | No | Planned (Task #6) |
| **Administration** | | | |
| Multiple databases | Yes | Yes | Via multi-store (`mentat_create_store()`) |
| Excision | Yes | No | |
| GC | Automatic | N/A | Append-only; VACUUM handles dead tuples |

---

## Architecture Differences

### Datomic Architecture

```
Client App --> Datomic Peer/Client --> Transactor --> Storage (DynamoDB/Cassandra/SQL)
                                   --> Cache (memcached)
```

- **Peer**: Application includes Datomic library, runs queries in-process
- **Client**: Application talks to Datomic server via Transit wire protocol
- **Transactor**: Single writer; serializes all transactions
- **Storage**: Pluggable (DynamoDB, Cassandra, PostgreSQL, H2)

### pg_mentat Architecture

```
Client App --> PostgreSQL (pg_mentat extension) --> Local storage (PostgreSQL tables)
```

Or optionally:

```
Client App --> mentatd (HTTP/EDN) --> PostgreSQL (pg_mentat extension)
```

Key differences:
- **No transactor**: PostgreSQL handles transaction serialization
- **No separate storage**: Data is stored directly in PostgreSQL tables
- **No peer mode**: All queries execute inside PostgreSQL
- **No in-process caching**: Relies on PostgreSQL's buffer pool and mentatd's query cache

---

## Schema Translation

Datomic and pg_mentat use the same schema attribute format. Most schema definitions work unchanged.

### Direct Translation (No Changes Needed)

```edn
;; This works identically in both Datomic and pg_mentat
[{:db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one
  :db/doc "A person's name"}

 {:db/ident :person/email
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one
  :db/unique :db.unique/identity}

 {:db/ident :person/friends
  :db/valueType :db.type/ref
  :db/cardinality :db.cardinality/many}]
```

In pg_mentat:

```sql
SELECT mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/doc "A person''s name"}
  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
  {:db/ident :person/friends
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many}
]');
```

Note: Single quotes inside SQL strings must be escaped as `''`.

### Attributes Requiring Adaptation

| Datomic Attribute | pg_mentat Support | Workaround |
|-------------------|-------------------|------------|
| `:db/fulltext true` | Accepted but no Datalog integration | Use PostgreSQL `to_tsvector`/`to_tsquery` on the `searchable_text` view |
| `:db/noHistory true` | Ignored | All history is retained |
| `:db/tupleAttrs` | Not supported | Model as separate attributes |
| `:db/tupleTypes` | Not supported | Model as separate attributes |

### Enum Patterns

Datomic enums work the same way:

```edn
;; Define enum values as entities
[{:db/ident :status/active}
 {:db/ident :status/inactive}
 {:db/ident :status/pending}]

;; Use as ref values
{:order/status :status/active}
```

This pattern works identically in pg_mentat.

---

## Data Migration

### Step 1: Export from Datomic

Use the Datomic API to export all datoms:

```clojure
(require '[datomic.api :as d])

(defn export-datoms [db-uri output-file]
  (let [conn (d/connect db-uri)
        db (d/db conn)
        datoms (d/datoms db :eavt)]
    (with-open [w (clojure.java.io/writer output-file)]
      (doseq [d datoms]
        (.write w (pr-str {:e (.e d)
                           :a (d/ident db (.a d))
                           :v (.v d)
                           :tx (.tx d)
                           :added (.added d)}))
        (.write w "\n")))))
```

### Step 2: Transform to pg_mentat Format

Convert the exported datoms to EDN transaction format:

```clojure
(defn datoms->transactions [datoms-file]
  ;; Group by transaction ID
  (let [datoms (map read-string (line-seq (clojure.java.io/reader datoms-file)))
        by-tx (group-by :tx datoms)]
    (for [[tx-id datoms] (sort-by key by-tx)]
      (vec (for [{:keys [e a v added]} datoms]
             (if added
               [:db/add e a v]
               [:db/retract e a v]))))))
```

### Step 3: Import into pg_mentat

```sql
-- 1. Install the schema first
SELECT mentat_transact('[
  ... schema attributes ...
]');

-- 2. Import data in batches
-- Use the transformed EDN transactions
SELECT mentat_transact('[
  [:db/add 10001 :person/name "Alice"]
  [:db/add 10001 :person/email "alice@example.com"]
  ...
]');
```

### Bulk Import Optimization

For large datasets, bypass the transaction layer and insert directly:

```sql
BEGIN;
SET LOCAL synchronous_commit = off;

-- Insert into type-specific tables directly
INSERT INTO mentat.datoms_text_new (store_id, e, a, v, tx, added) VALUES
  (0, 10001, 63, 'Alice', 268435457, true),
  (0, 10001, 64, 'alice@example.com', 268435457, true);

INSERT INTO mentat.datoms_long_new (store_id, e, a, v, tx, added) VALUES
  (0, 10001, 65, 30, 268435457, true);

COMMIT;
ANALYZE;
```

### Entity ID Mapping

Datomic entity IDs will not match pg_mentat entity IDs. You have two options:

**Option A: Let pg_mentat allocate new IDs (recommended)**

Use tempids and let pg_mentat allocate IDs. Build a mapping table to translate old Datomic entity IDs to new pg_mentat IDs.

**Option B: Preserve Datomic entity IDs**

Insert datoms with the original Datomic entity IDs via direct SQL, then reset the pg_mentat partition sequences to values beyond the maximum imported entity ID:

```sql
SELECT setval('mentat.partition_user_seq',
  GREATEST(10000, (SELECT MAX(e) + 1 FROM mentat.datoms WHERE e >= 10000)));
```

---

## Client Library Migration

### Clojure (Datomic Peer to pg_mentat via mentatd)

**Before (Datomic Peer):**

```clojure
(require '[datomic.api :as d])

(def conn (d/connect "datomic:free://localhost:4334/mydb"))
(def db (d/db conn))

;; Query
(d/q '[:find ?e ?name :where [?e :person/name ?name]] db)

;; Transact
(d/transact conn [{:db/id (d/tempid :db.part/user)
                    :person/name "Alice"}])

;; Pull
(d/pull db [:person/name :person/email] entity-id)
```

**After (pg_mentat via mentatd):**

```clojure
(require '[pg-mentat.client :as mentat])

(def conn (mentat/connect "http://localhost:8080"))
(def db (mentat/db conn))

;; Query (same syntax)
(mentat/q '[:find ?e ?name :where [?e :person/name ?name]] db)

;; Transact (tempid handling differs)
(mentat/transact conn [{:db/id "tempid1"
                         :person/name "Alice"}])

;; Pull (same syntax)
(mentat/pull db [:person/name :person/email] entity-id)
```

### Any Language (Direct PostgreSQL)

**Before (Datomic via HTTP):**

```python
# Datomic Client
import datomic_client as d
conn = d.connect("localhost", 8998, "mydb")
results = d.q(conn, '[:find ?e ?name :where [?e :person/name ?name]]')
```

**After (pg_mentat via PostgreSQL):**

```python
# Direct PostgreSQL -- no additional services needed
import psycopg2
conn = psycopg2.connect("dbname=mydb")
cur = conn.cursor()
cur.execute("SELECT mentat_query(%s, %s)", [
    '[:find ?e ?name :where [?e :person/name ?name]]',
    '{}'
])
results = cur.fetchone()[0]  # Returns JSON
```

### Key API Differences

| Operation | Datomic | pg_mentat (SQL) |
|-----------|---------|-----------------|
| Connect | `(d/connect uri)` | `psycopg2.connect(dsn)` |
| Get database value | `(d/db conn)` | Not needed (always current) |
| Query | `(d/q query db)` | `SELECT mentat_query(query, inputs)` |
| Transact | `(d/transact conn data)` | `SELECT mentat_transact(edn)` |
| Pull | `(d/pull db pattern eid)` | `SELECT mentat_pull(pattern, eid)` |
| Entity | `(d/entity db eid)` | `SELECT mentat_entity(eid)` |
| Schema | `(d/schema db)` | `SELECT mentat_schema()` |
| As-of | `(d/as-of db tx)` | `SELECT mentat_query(q, '{"asOf": tx}')` |
| History | `(d/history db)` | `SELECT mentat_query(q, '{"history": true}')` |

---

## Query Migration

Most Datalog queries work without modification. Here are the differences:

### Queries That Work Unchanged

```edn
;; Simple pattern matching
[:find ?name :where [?e :person/name ?name]]

;; Multiple patterns with joins
[:find ?name ?email
 :where
 [?e :person/name ?name]
 [?e :person/email ?email]]

;; Input parameters
[:find ?name
 :in $ ?age-min
 :where
 [?e :person/age ?age]
 [(>= ?age ?age-min)]
 [?e :person/name ?name]]

;; Aggregates
[:find (count ?e) (avg ?age)
 :where
 [?e :person/age ?age]]

;; Rules
[:find ?boss-name
 :in $ ?name
 :where
 [?e :person/name ?name]
 (reports-to ?e ?boss)
 [?boss :person/name ?boss-name]]
```

### Queries That Need Changes

**Full-text search:**

```edn
;; Datomic (not supported in pg_mentat Datalog)
[:find ?e ?score
 :where
 [(fulltext $ :person/bio "engineer") [[?e ?score]]]]
```

Use SQL directly instead:

```sql
-- pg_mentat: Use the searchable_text view
SELECT entity_id, ts_rank(search_vector, to_tsquery('engineer')) AS score
FROM mentat.searchable_text
WHERE search_vector @@ to_tsquery('engineer')
ORDER BY score DESC;
```

**Database function calls:**

```edn
;; Datomic (not supported)
[:find ?e
 :where
 [(missing? $ ?e :person/email)]]
```

Workaround with NOT:

```edn
;; pg_mentat: Use NOT clause
[:find ?e
 :where
 [?e :person/name _]
 (not [?e :person/email _])]
```

**Variable limits:**

```edn
;; Datomic: Variable limit (not supported)
[:find ?name
 :in $ ?limit
 :where
 [?e :person/name ?name]
 :limit ?limit]
```

Use the SQL-level `LIMIT` instead or pass limit via the inputs JSON.

### Time-Travel Query Syntax

**Datomic:**

```clojure
(d/q query (d/as-of db tx-id))
```

**pg_mentat:**

```sql
SELECT mentat_query(
  '[:find ?name :where [?e :person/name ?name]]',
  '{"asOf": 268435500}'
);

-- History queries
SELECT mentat_query(
  '[:find ?e ?name ?tx ?added :where [?e :person/name ?name ?tx ?added]]',
  '{"history": true}'
);
```

---

## Transaction Migration

### Basic Assertions

**Datomic:**

```clojure
@(d/transact conn [{:db/id (d/tempid :db.part/user)
                      :person/name "Alice"
                      :person/age 30}])
```

**pg_mentat:**

```sql
SELECT mentat_transact('[
  {:db/id "tempid1"
   :person/name "Alice"
   :person/age 30}
]');
```

Note: pg_mentat uses string tempids (`"tempid1"`) instead of Datomic's `#db/id` tagged literals.

### Retractions

**Datomic:**

```clojure
@(d/transact conn [[:db/retract entity-id :person/name "Alice"]])
```

**pg_mentat:**

```sql
SELECT mentat_transact('[
  [:db/retract 10001 :person/name "Alice"]
]');
```

### Entity Retraction

Works identically:

```sql
SELECT mentat_transact('[
  [:db/retractEntity 10001]
]');
```

### Compare-and-Swap

**Datomic:**

```clojure
@(d/transact conn [[:db/cas entity-id :person/balance 100 200]])
```

**pg_mentat:**

```sql
SELECT mentat_transact('[
  [:db.fn/cas 10001 :person/balance 100 200]
]');
```

Note: pg_mentat does not automatically retry on CAS failure. Implement retry logic in your application.

### Transaction Functions (Not Supported)

Datomic transaction functions (`(d/function ...)`) are not supported. Rewrite as application-level logic:

```python
# Instead of a Datomic transaction function:
# Move the logic to your application
balance = query_current_balance(entity_id)
new_balance = balance - amount
if new_balance < 0:
    raise InsufficientFunds()
transact_with_cas(entity_id, :account/balance, balance, new_balance)
```

---

## API Mapping Reference

### Datomic Peer API to pg_mentat SQL

| Datomic Peer | pg_mentat SQL |
|-------------|---------------|
| `(d/connect uri)` | `psql` or driver connection |
| `(d/db conn)` | Not needed |
| `(d/q query db & args)` | `SELECT mentat_query(query, inputs)` |
| `(d/pull db pattern eid)` | `SELECT mentat_pull(pattern, eid)` |
| `(d/pull-many db pattern eids)` | `SELECT mentat_pull_many(pattern, eids)` |
| `(d/entity db eid)` | `SELECT mentat_entity(eid)` |
| `@(d/transact conn tx-data)` | `SELECT mentat_transact(edn)` |
| `(d/as-of db tx)` | `inputs = '{"asOf": tx}'` |
| `(d/since db tx)` | `inputs = '{"since": tx}'` |
| `(d/history db)` | `inputs = '{"history": true}'` |
| `(d/datoms db index & components)` | Query type-specific tables directly |
| `(d/schema db)` | `SELECT mentat_schema()` |
| `(d/with db tx-data)` | Not supported |
| `(d/tx-range log start end)` | `SELECT mentat_log(start_tx, end_tx)` |

### Datomic Client API to pg_mentat mentatd

| Datomic Client | mentatd HTTP |
|---------------|--------------|
| `{:op :query :query q :args [db]}` | `POST / {:op :query :query q}` |
| `{:op :transact :data tx-data}` | `POST / {:op :transact :data tx-data}` |
| `{:op :pull :selector pattern :eid eid}` | `POST / {:op :pull :selector pattern :eid eid}` |
| `{:op :datoms :index :eavt}` | `POST / {:op :datoms :index :eavt}` |

Note: mentatd uses EDN wire format, not Transit+JSON. Existing Datomic Client applications will need wire format adaptation.

---

## Performance Comparison

### Expected Performance

| Operation | Datomic (Peer) | pg_mentat (Direct SQL) | Notes |
|-----------|---------------|------------------------|-------|
| Simple entity lookup | 1-5 ms | 2-10 ms | Datomic has in-memory caching |
| Attribute scan | 5-20 ms | 10-50 ms | Depends on table size |
| Complex join (5 patterns) | 10-100 ms | 20-200 ms | PostgreSQL query planner overhead |
| Transaction (10 datoms) | 5-20 ms | 10-50 ms | Includes WAL flush |
| Pull (single entity) | 1-5 ms | 5-20 ms | |
| Rules (recursive, 1K nodes) | 20-100 ms | 50-500 ms | WITH RECURSIVE overhead |

### Where pg_mentat Is Faster

- **Initial connection**: No JVM startup, no peer library initialization
- **Simple SQL queries**: Direct index scans on typed columns
- **Operational tooling**: Standard PostgreSQL monitoring, backup, replication

### Where Datomic Is Faster

- **Repeated queries**: Datomic Peer caches data in JVM heap
- **Large result sets**: In-process data avoids serialization overhead
- **Write throughput**: Datomic's single-writer transactor can batch more efficiently
- **Complex joins**: Datomic's in-memory index structures avoid disk I/O

---

## Migration Checklist

### Phase 1: Assessment

- [ ] Inventory all Datomic schema attributes and types
- [ ] Identify usage of unsupported features (transaction functions, `with`, `fulltext`)
- [ ] Catalog all Datalog queries used in the application
- [ ] Measure current dataset size (entity count, datom count)
- [ ] Identify performance-critical queries

### Phase 2: Schema Migration

- [ ] Translate all schema attributes to pg_mentat format
- [ ] Test schema creation in pg_mentat: `SELECT mentat_transact('[... schema ...]');`
- [ ] Verify all attribute types, cardinalities, and uniqueness constraints
- [ ] Document any schema attributes that needed adaptation

### Phase 3: Data Migration

- [ ] Export datoms from Datomic
- [ ] Transform to pg_mentat transaction format
- [ ] Import schema into pg_mentat
- [ ] Import data in batches
- [ ] Verify row counts match
- [ ] Reset partition sequences to correct values
- [ ] Run `ANALYZE` on all tables

### Phase 4: Application Migration

- [ ] Replace Datomic client library with PostgreSQL driver
- [ ] Update connection setup code
- [ ] Replace `(d/q ...)` calls with `SELECT mentat_query(...)`
- [ ] Replace `(d/transact ...)` calls with `SELECT mentat_transact(...)`
- [ ] Replace `(d/pull ...)` calls with `SELECT mentat_pull(...)`
- [ ] Update time-travel queries to use JSON inputs
- [ ] Add client-side retry logic for serialization failures
- [ ] Rewrite any transaction functions as application logic
- [ ] Replace `fulltext` queries with PostgreSQL full-text search

### Phase 5: Testing

- [ ] Run application test suite against pg_mentat
- [ ] Verify query results match Datomic output for a sample of queries
- [ ] Performance test critical queries
- [ ] Test concurrent access patterns
- [ ] Test backup and restore procedures

### Phase 6: Production Cutover

- [ ] Deploy pg_mentat to production PostgreSQL
- [ ] Run final data sync from Datomic
- [ ] Switch application to pg_mentat
- [ ] Monitor for errors and performance regressions
- [ ] Keep Datomic available for rollback during stabilization period

---

## When to Migrate (and When Not To)

### Good Reasons to Migrate

- **Cost**: Datomic Pro/Cloud licenses are expensive; pg_mentat is free (Apache 2.0)
- **PostgreSQL investment**: Your team already operates PostgreSQL infrastructure
- **Simplicity**: One fewer service to manage (no transactor, no separate storage)
- **SQL ecosystem**: Use standard PostgreSQL tooling for monitoring, backup, replication
- **Dataset size**: Your data fits comfortably in < 10M datoms

### Reasons to Stay on Datomic

- **Scale**: You have > 100M datoms and need proven scalability
- **Feature completeness**: You rely on `with`, transaction functions, or `fulltext` in Datalog
- **Write throughput**: You need > 5,000 transactions per second sustained
- **Bitemporal queries**: You need XTDB-style bitemporal features
- **Support**: You need commercial support and SLAs
- **Proven track record**: Your application is mission-critical and cannot tolerate an early-stage system

### Hybrid Approach

Consider running both systems during migration:

1. Keep Datomic as the source of truth initially
2. Replicate data to pg_mentat for read-only workloads
3. Gradually migrate write paths once confidence is established
4. Decommission Datomic after full validation
