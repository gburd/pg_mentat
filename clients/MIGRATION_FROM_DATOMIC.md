# Migrating from Datomic to pg_mentat Client Libraries

This guide covers migrating Datomic application code to use pg_mentat's
client libraries. For general data migration and schema translation, see
[MIGRATION_FROM_DATOMIC.md](../MIGRATION_FROM_DATOMIC.md) in the project root.

This document focuses specifically on the **client library** migration path:
how to change your application code from using `datomic.client.api` (Clojure)
or a Datomic REST/HTTP client (Python) to pg_mentat's drop-in replacement
libraries.

---

## Table of Contents

1. [Choose Your Access Path](#choose-your-access-path)
2. [API Compatibility Matrix](#api-compatibility-matrix)
3. [Clojure Migration](#clojure-migration)
4. [Python Migration](#python-migration)
5. [Schema Translation](#schema-translation)
6. [Query Syntax Differences](#query-syntax-differences)
7. [Transaction Format Differences](#transaction-format-differences)
8. [Time-Travel Queries](#time-travel-queries)
9. [Error Handling](#error-handling)
10. [Performance Considerations](#performance-considerations)
11. [Common Migration Pitfalls](#common-migration-pitfalls)
12. [Step-by-Step Migration Checklist](#step-by-step-migration-checklist)

---

## Choose Your Access Path

pg_mentat offers two ways to connect. Choose the one that fits your migration:

| Path | When to Use | Wire Format |
|------|-------------|-------------|
| **Direct PostgreSQL** | New projects, polyglot teams, simplest deployment | SQL protocol |
| **mentatd (Datomic-compatible)** | Migrating from Datomic, need Datomic API semantics | Transit+JSON over WebSocket |

The **direct PostgreSQL** path (`clients/python/pg_mentat_client.py`,
`clients/nodejs/`, `clients/go/`, `clients/rust/`) calls pg_mentat SQL
functions directly. No daemon needed. This is the recommended default.

The **Datomic-compatible** path (`clients/clojure/`, `clients/python/pg_mentat/`)
connects to `mentatd` via WebSocket using Transit+JSON encoding. This provides
a drop-in replacement API for Datomic applications.

---

## API Compatibility Matrix

### Datomic Client API Functions

| Datomic Function | Clojure (pg-mentat.client) | Python (pg_mentat) | Status |
|-----------------|---------------------------|--------------------|----|
| `client` | `(d/client {:endpoint ...})` | `pg_mentat.client(endpoint=...)` | Full |
| `connect` | `(d/connect client {:db-name ...})` | `pg_mentat.connect(c, db_name=...)` | Full |
| `db` | `(d/db conn)` | `pg_mentat.db(conn)` | Full |
| `q` | `(d/q query db & inputs)` | `pg_mentat.q(query, db, *inputs)` | Full |
| `transact` | `(d/transact conn {:tx-data ...})` | `pg_mentat.transact(conn, tx_data=...)` | Full |
| `pull` | `(d/pull db pattern eid)` | `pg_mentat.pull(db, pattern, eid)` | Full |
| `pull-many` | `(d/pull-many db pattern eids)` | `pg_mentat.pull_many(db, pattern, eids)` | Full |
| `datoms` | `(d/datoms db {:index ...})` | `pg_mentat.datoms(db, index=...)` | Full |
| `with` | `(d/with db {:tx-data ...})` | `pg_mentat.with_db(db, tx_data=...)` | Partial |
| `tx-range` | `(d/tx-range conn {:start ...})` | `pg_mentat.tx_range(conn, start=...)` | Full |
| `as-of` | `(d/as-of db t)` | `pg_mentat.as_of(db, t)` | Full |
| `since` | `(d/since db t)` | `pg_mentat.since(db, t)` | Full |
| `history` | `(d/history db)` | `pg_mentat.history(db)` | Full |
| `list-databases` | `(d/list-databases client)` | `pg_mentat.list_databases(c)` | Full |
| `create-database` | `(d/create-database client {:db-name ...})` | `pg_mentat.create_database(c, db_name=...)` | Full |
| `delete-database` | `(d/delete-database client {:db-name ...})` | `pg_mentat.delete_database(c, db_name=...)` | Full |
| `index-range` | `(d/index-range db {:attrid ...})` | Not yet | Clojure only |

### Direct PostgreSQL API (All Languages)

| Method | SQL Function | Description |
|--------|-------------|-------------|
| `transact(edn)` | `mentat_transact(edn)` | Process EDN transactions |
| `query(datalog, inputs)` | `mentat_query(query, inputs::jsonb)` | Execute Datalog queries |
| `pull(pattern, id)` | `mentat_pull(pattern, id)` | Pull entity attributes |
| `pull_many(pattern, ids)` | `mentat_pull_many(pattern, ids)` | Pull multiple entities |
| `entity(id)` | `mentat_entity(id)` | Get all entity attributes |
| `schema()` | `mentat_schema()` | Return current schema |
| `explain(datalog, inputs)` | `mentat_explain(query, inputs)` | Query execution plan |

### Features Not Yet Supported

| Feature | Datomic | pg_mentat | Workaround |
|---------|---------|-----------|------------|
| Transaction functions | `d/function` | No | Application-level logic |
| `(fulltext $ ...)` in Datalog | Built-in | No | Use `find_text()` SQL function |
| Speculative `with` | Full | Partial | Server-side support in progress |
| `missing?` predicate | Built-in | No | Use `(not [?e :attr _])` |
| Tuple attributes | `db/tupleAttrs` | No | Model as separate attributes |
| Schema alteration | `db.alter/attribute` | No | Recreate attribute |

---

## Clojure Migration

### Step 1: Update Dependencies

**Before (Datomic):**

```clojure
;; deps.edn
{:deps {com.datomic/client-cloud {:mvn/version "1.0.123"}}}

;; or for Peer:
{:deps {com.datomic/datomic-free {:mvn/version "0.9.5697"}}}
```

**After (pg_mentat):**

```clojure
;; deps.edn
{:deps {org.clojure/clojure {:mvn/version "1.11.1"}}
 :paths ["src"]
 ;; Copy clients/clojure/src/pg_mentat/client.clj into your project
 ;; No external dependencies required (uses java.net.http built into JDK 11+)
 }
```

### Step 2: Change the Require

**Before:**

```clojure
(ns my-app.core
  (:require [datomic.client.api :as d]))
```

**After:**

```clojure
(ns my-app.core
  (:require [pg-mentat.client :as d]))
```

That single change makes most existing code work. The `pg-mentat.client`
namespace implements the same function signatures as `datomic.client.api`.

### Step 3: Update Client Configuration

**Before (Datomic Cloud):**

```clojure
(def client (d/client {:server-type :cloud
                       :region "us-east-1"
                       :system "my-system"
                       :endpoint "https://..."
                       :proxy-port 8182}))
```

**Before (Datomic Peer):**

```clojure
(def conn (d/connect "datomic:free://localhost:4334/mydb"))
```

**After (pg_mentat via mentatd):**

```clojure
(def client (d/client {:server-type :pg-mentat
                       :endpoint "ws://localhost:8080/ws"}))
(def conn (d/connect client {:db-name "mydb"}))
```

### Step 4: Verify Core Operations

```clojure
;; All of these work identically to Datomic:

;; Get database value
(def database (d/db conn))

;; Query
(d/q '[:find ?e ?name
        :where [?e :person/name ?name]]
     database)

;; Transact
(d/transact conn {:tx-data [{:person/name "Alice"
                              :person/email "alice@example.com"}]})

;; Pull
(d/pull database '[*] entity-id)

;; Time travel
(d/q '[:find ?name :where [?e :person/name ?name]]
     (d/as-of database tx-id))

;; Release connection when done
(d/release conn)
```

### Clojure: What Changes

| Operation | Datomic | pg_mentat | Notes |
|-----------|---------|-----------|-------|
| Tempids | `(d/tempid :db.part/user)` or `#db/id` | String tempids `"tempid-1"` | Use string tempids in tx-data maps |
| Connect | URI string | Client + connect | Two-step process |
| Cleanup | GC handles it | `(d/release conn)` | Close WebSocket explicitly |
| Partitions | `:db.part/user`, etc. | Single user partition | Partition arg ignored |

---

## Python Migration

### Step 1: Install the Client

```bash
# For Datomic-compatible API (via mentatd):
pip install websocket-client
# Then add clients/python/pg_mentat/ to your Python path

# For direct PostgreSQL access (recommended for new code):
pip install psycopg2-binary
# Then use clients/python/pg_mentat_client.py
```

### Step 2: Choose Your API Style

**Option A: Datomic-compatible API (via mentatd)**

```python
import pg_mentat

# Create client and connect
c = pg_mentat.client(endpoint="ws://localhost:8080/ws")
conn = pg_mentat.connect(c, db_name="mydb")

# Get database value
database = pg_mentat.db(conn)

# Query
results = pg_mentat.q(
    '[:find ?e ?name :where [?e :person/name ?name]]',
    database
)

# Transact
pg_mentat.transact(conn, tx_data='[{:person/name "Alice"}]')

# Pull
entity = pg_mentat.pull(database, "[*]", 10001)

# Time travel
old_db = pg_mentat.as_of(database, 1000)
old_results = pg_mentat.q(
    '[:find ?e ?name :where [?e :person/name ?name]]',
    old_db
)

# Cleanup
conn.close()
```

**Option B: Direct PostgreSQL (no mentatd needed)**

```python
from pg_mentat_client import MentatClient

with MentatClient("dbname=mydb") as m:
    # Schema
    m.transact('''[{:db/ident :person/name
                    :db/valueType :db.type/string
                    :db/cardinality :db.cardinality/one}]''')

    # Data
    m.transact('[{:person/name "Alice"}]')

    # Query
    results = m.query('[:find ?e ?name :where [?e :person/name ?name]]')

    # Pull
    entity = m.pull('[*]', 10001)

    # Time travel via inputs
    old_results = m.query(
        '[:find ?name :where [?e :person/name ?name]]',
        inputs={"asOf": 1000}
    )
```

### Python: What Changes

| Operation | Datomic (hypothetical Python) | pg_mentat (Datomic-compat) | pg_mentat (Direct SQL) |
|-----------|------|------|------|
| Connect | HTTP client setup | `pg_mentat.client()` + `connect()` | `MentatClient(dsn)` |
| Query | REST call + EDN parse | `pg_mentat.q(query, db)` | `m.query(datalog)` |
| Transact | REST call + EDN | `pg_mentat.transact(conn, tx_data=edn)` | `m.transact(edn)` |
| Pull | REST call | `pg_mentat.pull(db, pattern, eid)` | `m.pull(pattern, eid)` |
| Time travel | Modify db reference | `pg_mentat.as_of(db, t)` | `inputs={"asOf": t}` |
| Cleanup | Close HTTP client | `conn.close()` | `with` block or `m.close()` |

---

## Schema Translation

Schema definitions use the same EDN format in both Datomic and pg_mentat.
Most schema transactions work without modification.

### Works Unchanged

```edn
;; All of these work identically in Datomic and pg_mentat:

[{:db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one
  :db/doc "A person's full name"}

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

 {:db/ident :person/avatar
  :db/valueType :db.type/bytes
  :db/cardinality :db.cardinality/one}]
```

### Needs Adaptation

```edn
;; :db/fulltext -- accepted but not integrated into Datalog queries
;; Use PostgreSQL full-text search (find_text) for text search
{:db/ident :article/body
 :db/valueType :db.type/string
 :db/cardinality :db.cardinality/one
 :db/fulltext true}  ;; Accepted, but (fulltext $ :article/body "query") not available

;; :db/noHistory -- accepted but ignored (all history is kept)
{:db/ident :session/token
 :db/valueType :db.type/string
 :db/cardinality :db.cardinality/one
 :db/noHistory true}  ;; Accepted, but history is still retained

;; Tuple attributes -- NOT supported
;; {:db/ident :reg/semester+course
;;  :db/valueType :db.type/tuple
;;  :db/tupleAttrs [:reg/semester :reg/course]}
;; Workaround: model as separate attributes and query with joins
```

### Supported Value Types

| Type | Datomic | pg_mentat | Storage |
|------|---------|-----------|---------|
| `:db.type/string` | Yes | Yes | TEXT |
| `:db.type/long` | Yes | Yes | BIGINT |
| `:db.type/double` | Yes | Yes | DOUBLE PRECISION |
| `:db.type/boolean` | Yes | Yes | BOOLEAN |
| `:db.type/instant` | Yes | Yes | BIGINT (millis) |
| `:db.type/keyword` | Yes | Yes | TEXT |
| `:db.type/ref` | Yes | Yes | BIGINT |
| `:db.type/uuid` | Yes | Yes | UUID |
| `:db.type/bytes` | Yes | Yes | BYTEA |
| `:db.type/bigint` | Yes | No | -- |
| `:db.type/bigdec` | Yes | No | -- |
| `:db.type/float` | Yes | No | Use double |
| `:db.type/tuple` | Yes | No | -- |
| `:db.type/symbol` | Yes | No | -- |

---

## Query Syntax Differences

### Queries That Work Unchanged

```edn
;; Simple pattern
[:find ?name :where [?e :person/name ?name]]

;; Joins
[:find ?name ?email
 :where
 [?e :person/name ?name]
 [?e :person/email ?email]]

;; Input parameters
[:find ?name
 :in $ ?min-age
 :where
 [?e :person/age ?age]
 [(>= ?age ?min-age)]
 [?e :person/name ?name]]

;; Aggregates
[:find (count ?e) (avg ?age)
 :where [?e :person/age ?age]]

;; Find specs (relation, tuple, collection, scalar)
[:find [?name ...] :where [_ :person/name ?name]]  ;; collection
[:find ?name . :where [?e :person/name ?name]]       ;; scalar

;; Rules
[:find ?name
 :in $ %
 :where (ancestor ?e ?ancestor)
        [?ancestor :person/name ?name]]

;; NOT clauses
[:find ?name
 :where
 [?e :person/name ?name]
 (not [?e :person/deceased true])]

;; OR clauses
[:find ?name
 :where
 [?e :person/name ?name]
 (or [?e :person/role :role/admin]
     [?e :person/role :role/superadmin])]
```

### Queries That Need Changes

**`fulltext` built-in:**

```edn
;; Datomic -- NOT supported in pg_mentat
[:find ?e ?score
 :where [(fulltext $ :article/body "machine learning") [[?e ?score]]]]
```

```python
# pg_mentat workaround: use find_text() via direct SQL
results = client.find_text("machine learning")
# Returns: [{"entity_id": 42, "attribute": ":article/body", "value": "...", "rank": 0.8}]
```

**`missing?` predicate:**

```edn
;; Datomic
[:find ?e :where [(missing? $ ?e :person/email)]]

;; pg_mentat: rewrite with NOT
[:find ?e
 :where
 [?e :person/name _]
 (not [?e :person/email _])]
```

**`get-else` function:**

```edn
;; Datomic
[:find ?name ?bio
 :where
 [?e :person/name ?name]
 [(get-else $ ?e :person/bio "No bio") ?bio]]

;; pg_mentat: get-else IS supported, same syntax works
;; This query runs identically
```

---

## Transaction Format Differences

### Tempid Handling

**Datomic Peer:**

```clojure
;; Datomic uses tagged literal tempids
@(d/transact conn [{:db/id (d/tempid :db.part/user)
                      :person/name "Alice"}])
;; or with reader literal
@(d/transact conn [{:db/id #db/id[:db.part/user]
                      :person/name "Alice"}])
```

**pg_mentat:**

```clojure
;; pg_mentat uses string tempids
(d/transact conn {:tx-data [{:db/id "tempid-alice"
                               :person/name "Alice"}]})

;; Or omit :db/id entirely for auto-generated IDs
(d/transact conn {:tx-data [{:person/name "Alice"}]})
```

### Transaction Return Values

**Datomic:**

```clojure
;; Returns a future (Peer) or promise (Client)
@(d/transact conn [{:person/name "Alice"}])
;; => {:db-before ... :db-after ... :tx-data [...] :tempids {...}}
```

**pg_mentat:**

```clojure
;; Returns the result directly (no future/deref needed)
(d/transact conn {:tx-data [{:person/name "Alice"}]})
;; => {:db-before ... :db-after ... :tx-data [...] :tempids {...}}
```

### Compare-and-Swap

```clojure
;; Datomic
@(d/transact conn [[:db/cas entity-id :account/balance 100 200]])

;; pg_mentat -- same syntax, different function name
(d/transact conn {:tx-data [[:db.fn/cas entity-id :account/balance 100 200]]})
;; NOTE: No automatic retry on conflict. Implement retry logic in your app.
```

---

## Time-Travel Queries

Both Datomic and pg_mentat support as-of, since, and history queries with
the same API:

```clojure
;; Clojure -- identical in both systems
(def database (d/db conn))

;; As-of: see database at a past transaction
(def old-db (d/as-of database tx-id))
(d/q '[:find ?name :where [?e :person/name ?name]] old-db)

;; Since: see only changes after a transaction
(def changes-db (d/since database tx-id))

;; History: see all assertions and retractions
(def hist-db (d/history database))
(d/q '[:find ?e ?name ?tx ?added
        :where [?e :person/name ?name ?tx ?added]]
     hist-db)
```

```python
# Python (Datomic-compatible API)
database = pg_mentat.db(conn)

old_db = pg_mentat.as_of(database, tx_id)
results = pg_mentat.q('[:find ?name :where [?e :person/name ?name]]', old_db)

changes_db = pg_mentat.since(database, tx_id)

hist_db = pg_mentat.history(database)
```

```python
# Python (Direct SQL) -- pass time-travel params as inputs
results = client.query(
    '[:find ?name :where [?e :person/name ?name]]',
    inputs={"asOf": 268435500}
)

results = client.query(
    '[:find ?e ?name ?tx ?added :where [?e :person/name ?name ?tx ?added]]',
    inputs={"history": True}
)
```

---

## Error Handling

### Clojure

```clojure
;; Datomic raises ExceptionInfo
(try
  (d/transact conn {:tx-data [[:db/add nil :bad/attr "x"]]})
  (catch clojure.lang.ExceptionInfo e
    (let [data (ex-data e)]
      ;; Datomic: {:cognitect.anomalies/category :cognitect.anomalies/incorrect}
      ;; pg_mentat: same format
      (println (:cognitect.anomalies/category data))
      (println (:cognitect.anomalies/message data)))))
```

### Python

```python
from pg_mentat.client import PgMentatError

try:
    pg_mentat.transact(conn, tx_data='[{:bad/attr "x"}]')
except PgMentatError as e:
    print(f"Category: {e.category}")  # e.g., "incorrect"
    print(f"Message: {e}")
    print(f"Response: {e.response}")
```

```python
# Direct SQL client
from pg_mentat_client import MentatClient, MentatError

try:
    client.transact('[{:bad/attr "x"}]')
except MentatError as e:
    print(f"Error: {e}")
```

---

## Performance Considerations

### Latency Comparison

| Operation | Datomic Peer (in-process) | pg_mentat via mentatd | pg_mentat Direct SQL |
|-----------|--------------------------|----------------------|---------------------|
| Simple query | 1-5 ms | 5-15 ms | 2-10 ms |
| Complex join | 10-100 ms | 30-200 ms | 20-200 ms |
| Transaction (10 datoms) | 5-20 ms | 15-50 ms | 10-50 ms |
| Pull (single entity) | 1-5 ms | 5-20 ms | 5-20 ms |
| Schema lookup | < 1 ms (cached) | 2-5 ms | 1-5 ms |

### Key Differences

- **No in-process caching**: Datomic Peer caches data in the JVM heap; pg_mentat
  relies on PostgreSQL's shared buffers. For read-heavy workloads, consider
  using direct SQL for lower latency.

- **Connection overhead**: The mentatd WebSocket adds ~2-5ms per request. For
  high-throughput scenarios, use the direct SQL client or batch operations.

- **PostgreSQL connection pooling**: Use PgBouncer or the built-in driver
  pooling for production deployments. The direct SQL client supports passing
  an existing connection.

### Optimization Tips

1. **Use direct SQL** for simple lookups and high-throughput paths.
2. **Batch transactions**: Group multiple assertions into single `transact` calls.
3. **Use `EXPLAIN`** (`mentat_explain()`) to debug slow queries.
4. **Index attributes** that appear in query predicates (`>`, `<`, `>=`, etc.).
5. **Use SQL views** (`facts`, `text_values`, `numeric_values`) for analytics
   and reporting instead of Datalog.

---

## Common Migration Pitfalls

### 1. Forgetting to Start mentatd

The Datomic-compatible client libraries require a running `mentatd` instance.
If you get connection errors, verify:

```bash
# Start mentatd
./mentatd --port 8080 --database-url "postgresql://localhost/mydb"
```

The direct SQL client (`pg_mentat_client.py`) does **not** need mentatd.

### 2. Using Datomic Tempid Syntax

```clojure
;; WRONG: Datomic tagged literals don't work
{:db/id #db/id[:db.part/user] :person/name "Alice"}

;; CORRECT: Use string tempids
{:db/id "alice-tempid" :person/name "Alice"}

;; CORRECT: Omit :db/id for auto-generated IDs
{:person/name "Alice"}
```

### 3. Dereferencing Transaction Results

```clojure
;; WRONG: pg_mentat returns results directly, not futures
@(d/transact conn {:tx-data [...]})

;; CORRECT: No deref needed
(d/transact conn {:tx-data [...]})
```

### 4. Relying on Partition Semantics

Datomic's `:db.part/user`, `:db.part/db`, and `:db.part/tx` partition entity
ID ranges. pg_mentat uses a single partition for user entities. Entity IDs
will differ between systems.

### 5. Expecting (fulltext $) to Work

```edn
;; This does NOT work in pg_mentat:
[:find ?e :where [(fulltext $ :article/body "search term") [[?e]]]]
```

Use the direct SQL `find_text()` function instead:

```python
results = client.find_text("search term")
```

### 6. SQL String Escaping

When using the direct SQL client, single quotes in EDN must be doubled:

```sql
-- WRONG
SELECT mentat_transact('[{:db/doc "Alice's doc"}]');

-- CORRECT
SELECT mentat_transact('[{:db/doc "Alice''s doc"}]');
```

This is not an issue with the parameterized Python/Clojure clients.

### 7. Transaction Functions

Datomic transaction functions are not supported. Move that logic to your
application layer:

```clojure
;; Datomic: Transaction function
;; (d/transact conn [[:my/increment-counter entity-id :counter/val]])

;; pg_mentat: Application-level logic
(let [db (d/db conn)
      current (ffirst (d/q '[:find ?v :in $ ?e
                              :where [?e :counter/val ?v]]
                           db entity-id))]
  (d/transact conn {:tx-data [[:db.fn/cas entity-id :counter/val current (inc current)]]}))
```

### 8. Schema Alteration

Datomic allows changing attribute cardinality and adding indexes via
`:db.alter/attribute`. pg_mentat does not support this. Plan your schema
carefully upfront, or create new attributes and migrate data.

### 9. Connection Lifecycle

```python
# WRONG: Forgetting to close connections
c = pg_mentat.client(endpoint="ws://localhost:8080/ws")
conn = pg_mentat.connect(c, db_name="mydb")
# ... use conn ...
# Connection leak!

# CORRECT: Always close
try:
    conn = pg_mentat.connect(c, db_name="mydb")
    # ... use conn ...
finally:
    conn.close()

# CORRECT: Use context manager (direct SQL client)
with MentatClient("dbname=mydb") as m:
    m.transact('[...]')
    # Auto-closed
```

### 10. Expecting Datomic Cloud Features

Ions, client proxies, and Datomic Cloud-specific features are not available.
pg_mentat is a PostgreSQL extension, not a managed cloud service.

---

## Step-by-Step Migration Checklist

### Phase 1: Setup

- [ ] Install PostgreSQL with pg_mentat extension
- [ ] Start mentatd (if using Datomic-compatible API)
- [ ] Add pg_mentat client library to your project
- [ ] Verify connectivity with a health check

### Phase 2: Code Changes

- [ ] Replace `datomic.client.api` require/import with `pg-mentat.client` / `pg_mentat`
- [ ] Update client configuration (endpoint URL)
- [ ] Replace `d/tempid` calls with string tempids
- [ ] Remove `@` / `deref` from transaction calls (Clojure)
- [ ] Replace `(fulltext ...)` queries with `find_text()`
- [ ] Replace `(missing? ...)` with `(not [?e :attr _])`
- [ ] Move transaction functions to application logic
- [ ] Add explicit connection cleanup (`.close()` / `release`)

### Phase 3: Schema Migration

- [ ] Export schema from Datomic
- [ ] Review schema for unsupported features (tuples, bigdec, etc.)
- [ ] Transact schema into pg_mentat
- [ ] Verify with `mentat_schema()`

### Phase 4: Data Migration

- [ ] Export datoms from Datomic (see root MIGRATION_FROM_DATOMIC.md)
- [ ] Import into pg_mentat in batches
- [ ] Verify entity counts and key values

### Phase 5: Testing

- [ ] Run existing test suite against pg_mentat
- [ ] Compare query results for a sample of queries
- [ ] Load test critical paths
- [ ] Test error handling and edge cases

### Phase 6: Deployment

- [ ] Deploy pg_mentat to staging
- [ ] Run parallel reads against both systems
- [ ] Cut over writes
- [ ] Monitor for errors
- [ ] Decommission Datomic after stabilization
