# pg_mentat Clojure Peer Library

Datomic-compatible Clojure peer library for pg_mentat. Connects directly to
PostgreSQL via [next.jdbc](https://github.com/seancorfield/next-jdbc) -- no HTTP
daemon required.

## Overview

This library provides the Datomic Peer API (`datomic.api`) semantics on top of
the pg_mentat PostgreSQL extension. It calls the extension's SQL functions
(`mentat_query`, `mentat_transact`, `mentat_pull`, `mentat_entity`, etc.)
directly through a JDBC connection.

**Key features:**
- 100% Datomic API compatibility (connect, db, q, transact, pull, pull-many, entity, as-of)
- Direct PostgreSQL connection -- no intermediate HTTP daemon
- EDN transaction data and Datalog query syntax
- Time-travel queries (as-of, since, history)
- Speculative transactions (with)
- Connection pooling via HikariCP (optional)

## Quick Start

### deps.edn

```clojure
{:deps {com.pg-mentat/pg-mentat-client {:local/root "../clients/clojure"}
        ;; Or when published:
        ;; com.pg-mentat/pg-mentat-client {:mvn/version "0.1.0"}
        }}
```

### project.clj

```clojure
:dependencies [[com.pg-mentat/pg-mentat-client "0.1.0-SNAPSHOT"]]
```

### Usage

```clojure
(require '[pg-mentat.client :as d])

;; Connect directly to PostgreSQL
(def conn (d/connect {:pg {:dbtype "postgresql"
                           :host "localhost"
                           :dbname "postgres"
                           :user "postgres"}}))

;; Get current database value
(def db (d/db conn))

;; Define schema
(d/transact conn
  {:tx-data [{:db/ident       :person/name
              :db/valueType   :db.type/string
              :db/cardinality :db.cardinality/one}
             {:db/ident       :person/email
              :db/valueType   :db.type/string
              :db/cardinality :db.cardinality/one
              :db/unique      :db.unique/identity}
             {:db/ident       :person/age
              :db/valueType   :db.type/long
              :db/cardinality :db.cardinality/one}]})

;; Transact data
(d/transact conn
  {:tx-data [{:person/name  "Alice"
              :person/email "alice@example.com"
              :person/age   30}
             {:person/name  "Bob"
              :person/email "bob@example.com"
              :person/age   25}]})

;; Query
(def db (d/db conn))
(d/q '[:find ?name ?email
        :where
        [?e :person/name ?name]
        [?e :person/email ?email]]
     db)
;; => [["Alice" "alice@example.com"] ["Bob" "bob@example.com"]]

;; Pull
(d/pull db '[:person/name :person/age] entity-id)
;; => {:person/name "Alice", :person/age 30}

;; Pull many
(d/pull-many db '[:person/name] [id1 id2])
;; => [{:person/name "Alice"} {:person/name "Bob"}]

;; Entity
(d/entity db entity-id)
;; => {:db/id 10001, :person/name "Alice", :person/email "alice@example.com", :person/age 30}

;; Release when done
(d/release conn)
```

## Time-Travel Queries

```clojure
;; Get a database value at a specific transaction
(def old-db (d/as-of db tx-id))
(d/q '[:find ?name :where [?e :person/name ?name]] old-db)

;; See only changes since a transaction
(def changes-db (d/since db tx-id))
(d/q '[:find ?name :where [?e :person/name ?name]] changes-db)

;; Full history (including retractions)
(def hist-db (d/history db))
```

## Speculative Transactions

```clojure
;; Preview a transaction without committing
(d/with db {:tx-data [{:person/name "Charlie"}]})
;; => {:db-before {...}, :db-after {...}, :tx-data [...], :tempids {...}}
;; No data is persisted
```

## Connection Pooling (HikariCP)

For production use, pass a HikariCP datasource configuration:

```clojure
(require '[next.jdbc.connection :as connection])

(def pool (connection/->pool
            com.zaxxer.hikari.HikariDataSource
            {:dbtype "postgresql"
             :host "localhost"
             :dbname "postgres"
             :user "postgres"
             :maximumPoolSize 10}))

;; Pass the pool as the :pg value
(def conn (d/connect {:pg pool}))
```

## Multi-Store Support

pg_mentat supports multiple independent stores in the same database.
Specify a store name when connecting:

```clojure
(def conn (d/connect {:pg db-spec :store-name "my-store"}))
```

The default store name is `"default"`.

## Migration from Datomic

Replace your namespace require:

```clojure
;; Before (Datomic Peer)
(require '[datomic.api :as d])
(def conn (d/connect "datomic:sql://my-db?..."))

;; After (pg_mentat)
(require '[pg-mentat.client :as d])
(def conn (d/connect {:pg {:dbtype "postgresql"
                           :host "localhost"
                           :dbname "postgres"}}))
```

The API functions (`q`, `transact`, `pull`, `pull-many`, `entity`, `as-of`,
`since`, `history`, `with`) use the same signatures and return compatible data.

## API Reference

| Function | Datomic Equivalent | Description |
|----------|-------------------|-------------|
| `(connect config)` | `datomic.api/connect` | Create PostgreSQL connection |
| `(db conn)` | `datomic.api/db` | Get current database value |
| `(q query db & inputs)` | `datomic.api/q` | Execute Datalog query |
| `(transact conn {:tx-data ...})` | `datomic.api/transact` | Execute transaction |
| `(pull db pattern eid)` | `datomic.api/pull` | Pull entity attributes |
| `(pull-many db pattern eids)` | `datomic.api/pull-many` | Pull multiple entities |
| `(entity db eid)` | `datomic.api/entity` | Get entity as map |
| `(as-of db t)` | `datomic.api/as-of` | Time-travel to transaction t |
| `(since db t)` | `datomic.api/since` | Changes since transaction t |
| `(history db)` | `datomic.api/history` | Full history database |
| `(with db {:tx-data ...})` | `datomic.api/with` | Speculative transaction |
| `(schema db)` | N/A | Get current schema |
| `(release conn)` | `datomic.api/release` | Release connection |
| `(squuid)` | `datomic.api/squuid` | Generate semi-sequential UUID |
| `(tempid partition)` | `datomic.api/tempid` | Generate temporary ID |

## Running Tests

```bash
# Unit tests only (no database required)
clj -X:test :kaocha.filter/skip-meta :integration

# All tests (requires PostgreSQL with pg_mentat)
clj -X:test

# With custom database config
PG_MENTAT_HOST=myhost PG_MENTAT_DBNAME=mydb clj -X:test
```

## License

Apache-2.0
