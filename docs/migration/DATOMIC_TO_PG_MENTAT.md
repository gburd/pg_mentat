# Migrating from Datomic to pg_mentat

Complete guide for transitioning from Datomic to pg_mentat with minimal code changes.

## Overview

pg_mentat is designed as a drop-in replacement for many Datomic use cases. It brings Datomic's Datalog query language, immutable data model, and time-travel capabilities to PostgreSQL.

**Key benefits of migrating:**
- Open source (no licensing costs)
- Built on PostgreSQL (mature, widely supported)
- Full SQL access alongside Datalog
- Scales with PostgreSQL (read replicas, partitioning)
- No separate database process (extension model)

## Compatibility Matrix

| Feature | Datomic Free | Datomic Pro | pg_mentat |
|---------|--------------|-------------|-----------|
| **Core Features** |
| Datalog queries | ✅ | ✅ | ✅ |
| Rules (recursive) | ✅ | ✅ | ✅ |
| Aggregates | ✅ | ✅ | ✅ |
| Pull API | ✅ | ✅ | ✅ (recursive, reverse lookups) |
| Time travel (as-of) | ✅ | ✅ | ✅ |
| Time travel (since) | ✅ | ✅ | ✅ |
| Time travel (history) | ✅ | ✅ | ✅ |
| Full-text search | ✅ | ✅ | ✅ |
| Transactions | ✅ | ✅ | ✅ |
| :db/retract | ✅ | ✅ | ✅ |
| :db/retractEntity | ✅ | ✅ | ✅ |
| Lookup refs | ✅ | ✅ | ✅ |
| Unique constraints | ✅ | ✅ | ✅ (identity & value) |
| **Data Types** |
| String, Long, Double | ✅ | ✅ | ✅ |
| Boolean, Instant | ✅ | ✅ | ✅ |
| Keyword, UUID | ✅ | ✅ | ✅ |
| Bytes, Ref | ✅ | ✅ | ✅ |
| BigInt, BigDec | ✅ | ✅ | ❌ (use long/double) |
| **Serialization** |
| EDN | ✅ | ✅ | ✅ |
| Transit | ✅ | ✅ | 🚧 In progress |
| **Architecture** |
| Peer library | ✅ | ✅ | ❌ (client-server only) |
| Client API | ❌ | ✅ | ✅ (via mentatd) |
| Standalone server | ✅ | ✅ | ✅ (PostgreSQL) |
| **Advanced** |
| Transaction functions | ✅ | ✅ | ⚠️ Limited (cas only) |
| Excision | ❌ | ✅ | ❌ |
| Log API | ✅ | ✅ | 🚧 Planned |
| Analytics queries | ❌ | ✅ | ✅ (via SQL) |
| **Operational** |
| Backup/restore | DB-specific | DB-specific | PostgreSQL tools |
| Monitoring | CloudWatch | CloudWatch | PostgreSQL + Prometheus |
| Scaling | Limited | Horizontal | PostgreSQL scaling |
| HA/replication | Limited | Yes | PostgreSQL HA |

**Legend**: ✅ Full support, ⚠️ Partial support, 🚧 In development, ❌ Not supported

## Migration Steps

### Step 1: Install pg_mentat

```bash
# Install PostgreSQL extension
cd pg_mentat
cargo pgrx install --release

# In PostgreSQL
psql -U postgres
CREATE EXTENSION pg_mentat;
```

### Step 2: Export Datomic Data

Export your Datomic database to EDN:

```clojure
(require '[datomic.api :as d])
(require '[clojure.java.io :as io])

(def conn (d/connect "datomic:free://localhost:4334/mydb"))
(def db (d/db conn))

;; Export schema
(def schema-tx
  (d/q '[:find (pull ?e [*])
         :where
         [?e :db/ident ?ident]
         [?e :db/valueType]
         [?e :db/cardinality]]
       db))

(spit "schema.edn" (pr-str schema-tx))

;; Export all datoms
(def all-datoms
  (d/q '[:find ?e ?a ?v ?tx
         :where [?e ?a ?v ?tx true]]
       (d/history db)))

(spit "datoms.edn" (pr-str all-datoms))
```

### Step 3: Import to pg_mentat

```sql
-- Import schema
SELECT mentat.mentat_transact(
  pg_read_file('/path/to/schema.edn')
);

-- Import data (you may need to convert format)
-- For each entity map:
SELECT mentat.mentat_transact($$
[{:db/id <entity-id>
  :attr1 val1
  :attr2 val2
  ...}]
$$);
```

**Note**: Entity IDs are preserved during import for ref integrity.

### Step 4: Update Connection Strings

Change Datomic connection strings to pg_mentat:

```clojure
;; Before (Datomic)
(d/connect "datomic:free://localhost:4334/mydb")
(d/connect "datomic:sql://mydb?jdbc:postgresql://localhost:5432/postgres")

;; After (pg_mentat via mentatd)
(d/connect "datomic:sql://mydb?jdbc:postgresql://localhost:5432/postgres")
;; OR direct PostgreSQL connection
(def conn (jdbc/get-connection "postgresql://localhost:5432/postgres"))
```

### Step 5: Update Code (Minimal Changes Required)

Most code remains unchanged. Key differences:

#### Connection API

```clojure
;; Datomic Peer API - NOT SUPPORTED
(require '[datomic.api :as d])
(d/create-database "datomic:free://localhost:4334/mydb")

;; pg_mentat - Use standard Clojure JDBC
(require '[clojure.java.jdbc :as jdbc])
(def db-spec {:dbtype "postgresql"
              :host "localhost"
              :port 5432
              :dbname "postgres"})
```

#### Queries - UNCHANGED

```clojure
;; Works in both Datomic and pg_mentat
(d/q '[:find ?name ?age
       :where
       [?e :person/name ?name]
       [?e :person/age ?age]
       [(> ?age 25)]]
     (d/db conn))
```

#### Transactions - MOSTLY UNCHANGED

```clojure
;; Datomic
@(d/transact conn
   [{:db/id "alice"
     :person/name "Alice"
     :person/age 30}])

;; pg_mentat - Direct SQL call
(jdbc/execute! db-spec
  ["SELECT mentat.mentat_transact(?)"
   (pr-str [{:db/id "alice"
             :person/name "Alice"
             :person/age 30}])])

;; OR via mentatd (more Datomic-like)
(d/transact conn
  [{:db/id "alice"
    :person/name "Alice"
    :person/age 30}])
```

#### Pull API - UNCHANGED

```clojure
;; Works in both
(d/pull (d/db conn)
        [:person/name :person/age]
        [:person/email "alice@example.com"])
```

## Code Examples

### Before (Datomic)

```clojure
(ns myapp.core
  (:require [datomic.api :as d]))

;; Connect
(def uri "datomic:free://localhost:4334/mydb")
(d/create-database uri)
(def conn (d/connect uri))

;; Schema
@(d/transact conn
   [{:db/ident :person/name
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
    {:db/ident :person/age
     :db/valueType :db.type/long
     :db/cardinality :db.cardinality/one}])

;; Transact
@(d/transact conn
   [{:db/id "alice"
     :person/name "Alice"
     :person/age 30}])

;; Query
(d/q '[:find ?name
       :where [?e :person/name ?name]]
     (d/db conn))

;; Pull
(d/pull (d/db conn) '[*] [:person/name "Alice"])

;; Time travel
(def past-db (d/as-of (d/db conn) #inst "2024-01-01"))
(d/q '[:find ?name :where [?e :person/name ?name]] past-db)
```

### After (pg_mentat)

```clojure
(ns myapp.core
  (:require [clojure.java.jdbc :as jdbc]))

;; Connect (PostgreSQL)
(def db-spec {:dbtype "postgresql"
              :host "localhost"
              :port 5432
              :dbname "postgres"})

;; Helper function
(defn mentat-query [q inputs]
  (-> (jdbc/query db-spec
        ["SELECT mentat.mentat_query(?, ?::jsonb)"
         (pr-str q)
         (json/write-str inputs)])
      first
      :mentat_query
      (json/read-str :key-fn keyword)))

(defn mentat-transact [tx-data]
  (jdbc/execute! db-spec
    ["SELECT mentat.mentat_transact(?)"
     (pr-str tx-data)]))

;; Schema (same EDN)
(mentat-transact
  [{:db/ident :person/name
    :db/valueType :db.type/string
    :db/cardinality :db.cardinality/one}
   {:db/ident :person/age
    :db/valueType :db.type/long
    :db/cardinality :db.cardinality/one}])

;; Transact (same EDN)
(mentat-transact
  [{:db/id "alice"
    :person/name "Alice"
    :person/age 30}])

;; Query (same Datalog)
(mentat-query '[:find ?name :where [?e :person/name ?name]]
              {})

;; Pull
(-> (jdbc/query db-spec
      ["SELECT mentat.mentat_pull(?, ?)"
       (pr-str '[*])
       (first (mentat-query '[:find ?e . :where [?e :person/name "Alice"]]
                            {}))])
    first
    :mentat_pull
    (json/read-str :key-fn keyword))

;; Time travel (same concept, different API)
(mentat-query '[:find ?name :where [?e :person/name ?name]]
              {:asOf 1000005})
```

## Feature-by-Feature Migration

### Schema Definition

**No changes needed** - Same EDN format:

```clojure
[{:db/ident :person/email
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one
  :db/unique :db.unique/identity}]
```

### Transactions

**Minimal wrapper needed**:

```clojure
;; Datomic
@(d/transact conn [{...}])

;; pg_mentat
(mentat-transact [{...}])  ;; Custom function wrapping JDBC call
```

### Queries

**No changes** - Identical Datalog syntax:

```clojure
'[:find ?e ?name
  :in ?min-age
  :where
  [?e :person/name ?name]
  [?e :person/age ?age]
  [(>= ?age ?min-age)]]
```

### Pull API

**No changes** - Same pattern syntax:

```clojure
[:person/name {:person/friends [:person/name :person/email]}]
```

### Rules

**No changes** - Identical rule syntax:

```clojure
'[:find ?desc
  :with [[(ancestor ?a ?d)
          [?a :family/child ?d]]
         [(ancestor ?a ?d)
          [?a :family/child ?x]
          (ancestor ?x ?d)]]
  :where
  (ancestor ?root ?desc)]
```

### Time Travel

**Different parameter format**:

```clojure
;; Datomic
(d/as-of db #inst "2024-01-01")
(d/as-of db tx-id)
(d/since db tx-id)
(d/history db)

;; pg_mentat (pass as query inputs)
(mentat-query query {:asOf tx-id})
(mentat-query query {:since tx-id})
(mentat-query query {:history true})
```

### Entity API

Datomic's lazy entity map not directly supported. Use Pull API instead:

```clojure
;; Datomic
(def e (d/entity db 42))
(:person/name e)

;; pg_mentat - Use Pull
(def e (mentat-pull '[*] 42))
(:person/name e)
```

### Transaction Functions

**Limited support**:

```clojure
;; Datomic - Full support
[:db.fn/cas eid :person/age 30 31]
(d/function {:lang :clojure
             :params [db eid]
             :code "(custom-logic)"})

;; pg_mentat - Only cas supported
[:db.fn/cas eid :person/age 30 31]
;; Custom functions not supported (security risk)
```

## Common Migration Issues

### Issue 1: Peer API Not Available

**Problem**: Code uses `datomic.api/create-database`, `datomic.api/delete-database`

**Solution**: Use PostgreSQL database management:

```sql
-- Create database
CREATE DATABASE mydb;
CREATE EXTENSION pg_mentat;

-- Delete database
DROP DATABASE mydb;
```

### Issue 2: Transaction Results Format Different

**Problem**: Datomic returns `{:db-before, :db-after, :tx-data, :tempids}`

**Solution**: pg_mentat returns `{:tx, :tempids}`. Extract transaction ID from `:tx` key.

### Issue 3: BigInt/BigDecimal Not Supported

**Problem**: Datomic supports arbitrary precision numbers

**Solution**: Use `:db.type/long` (64-bit) or `:db.type/double`. Store large numbers as strings if needed.

### Issue 4: No Excision

**Problem**: Datomic Pro allows removing historical data (excision)

**Solution**: pg_mentat maintains full history. For compliance (GDPR), use PostgreSQL-level solutions:
- Encrypt sensitive data
- Use separate PostgreSQL instance for sensitive data with retention policies
- Manual data archival/deletion at PostgreSQL level

### Issue 5: Connection Pooling

**Problem**: Datomic Peer manages connections automatically

**Solution**: Use standard JDBC connection pooling (HikariCP, c3p0):

```clojure
(require '[hikari-cp.core :as hikari])

(def datasource
  (hikari/make-datasource
    {:adapter "postgresql"
     :database-name "postgres"
     :server-name "localhost"
     :port-number 5432
     :maximum-pool-size 10}))
```

## Performance Considerations

### Query Performance

| Aspect | Datomic | pg_mentat |
|--------|---------|-----------|
| Indexing | Automatic (EAVT, AEVT, AVET, VAET) | Automatic (EAVT, AEVT, AVET) |
| Caching | Memory-resident index | PostgreSQL buffer pool |
| Query planner | Datalog optimizer | PostgreSQL planner |
| Joins | In-memory | PostgreSQL joins |

**Optimization tips:**
- Mark frequently-queried attributes with `:db/index true`
- Use PostgreSQL query analysis: `EXPLAIN ANALYZE`
- Tune PostgreSQL settings (shared_buffers, work_mem)
- Consider read replicas for query-heavy workloads

### Write Performance

- **Datomic**: Serialized writes through transactor
- **pg_mentat**: PostgreSQL MVCC allows concurrent writes

pg_mentat can handle **higher write throughput** than Datomic Free.

### Storage

- **Datomic**: Append-only log + covering index
- **pg_mentat**: PostgreSQL MVCC with VACUUM

pg_mentat requires periodic `VACUUM` to reclaim space.

## Deployment Differences

### Datomic Deployment

```
[Peer Library] ←→ [Transactor] ←→ [Storage Backend]
                                   (H2, PostgreSQL, DynamoDB)
```

### pg_mentat Deployment

```
[Application] ←→ [PostgreSQL with pg_mentat extension]
                  ↓
             [Storage]
```

**Simpler architecture** - No separate transactor process.

### Scaling Strategies

| Approach | Datomic | pg_mentat |
|----------|---------|-----------|
| Read scaling | Peer cache + horizontal peers | PostgreSQL read replicas |
| Write scaling | Single transactor (bottleneck) | PostgreSQL write capacity |
| Geo-distribution | Complex (peer + storage) | PostgreSQL logical replication |

## Testing Migration

### Create Test Environment

```bash
# Set up pg_mentat test database
createdb mentat_test
psql mentat_test -c "CREATE EXTENSION pg_mentat;"

# Run side-by-side comparison
# 1. Query Datomic
# 2. Query pg_mentat
# 3. Compare results
```

### Validation Script

```clojure
(defn compare-results [datomic-conn pg-conn query]
  (let [datomic-result (d/q query (d/db datomic-conn))
        pg-result (mentat-query query {})]
    (= (set datomic-result)
       (set (:results pg-result)))))

;; Run validation suite
(deftest migration-validation
  (testing "Query parity"
    (is (compare-results datomic-conn pg-conn
          '[:find ?e ?name :where [?e :person/name ?name]]))))
```

## Rollback Plan

Keep Datomic and pg_mentat running side-by-side during migration:

1. **Dual-write phase**: Write to both databases
2. **Validation phase**: Compare query results
3. **Cutover phase**: Switch reads to pg_mentat
4. **Monitoring phase**: Monitor for issues
5. **Decommission**: Remove Datomic after stability confirmed

## Migration Checklist

- [ ] Install pg_mentat in test environment
- [ ] Export Datomic schema and data
- [ ] Import to pg_mentat
- [ ] Update connection code
- [ ] Replace Peer API calls with JDBC
- [ ] Test queries return same results
- [ ] Test transactions work correctly
- [ ] Test time-travel queries
- [ ] Update deployment scripts
- [ ] Set up monitoring (PostgreSQL + application metrics)
- [ ] Load test to verify performance
- [ ] Train team on pg_mentat operations
- [ ] Document differences for team
- [ ] Plan gradual rollout (dual-write → cutover)
- [ ] Set up rollback procedure
- [ ] Perform cutover
- [ ] Monitor for 7+ days
- [ ] Decommission Datomic

## Getting Help

- **Documentation**: [Quickstart Guide](../getting_started/QUICKSTART.md), [Concepts](../getting_started/CONCEPTS.md)
- **GitHub Issues**: https://github.com/your-org/pg_mentat/issues
- **PostgreSQL Community**: Standard PostgreSQL support channels
- **Migration Assistance**: File an issue tagged "migration-help"

## Frequently Asked Questions

**Q: Can I use existing Datomic Clojure client libraries?**
A: Yes, with mentatd daemon. Some features may require code changes.

**Q: How long does migration typically take?**
A: Small database (<1M datoms): 1-2 days. Large database (>100M datoms): 1-2 weeks including testing.

**Q: What about production traffic during migration?**
A: Use dual-write strategy with gradual cutover to minimize downtime.

**Q: Is data integrity guaranteed?**
A: Yes, pg_mentat uses PostgreSQL's ACID transactions.

**Q: Can I go back to Datomic after migrating?**
A: Yes, maintain Datomic backup during initial migration period.

**Q: What about transaction functions?**
A: Most cases can use application-level logic. Only :db.fn/cas supported currently.

**Q: How do I handle very large databases (>1TB)?**
A: pg_mentat scales with PostgreSQL. Use partitioning, compression, and tablespaces.

## Next Steps

1. Review [Quickstart Guide](../getting_started/QUICKSTART.md) to learn pg_mentat basics
2. Set up test environment and try simple queries
3. Export small Datomic dataset and test import
4. Run validation suite on test data
5. Plan production migration timeline
6. Reach out with questions in GitHub Issues

Good luck with your migration!
