# pg-mentat-client

Clojure client library for pg_mentat Datalog database. Provides a Datomic-like API for interacting with pg_mentat via the mentatd HTTP gateway.

## Installation

### Leiningen

Add to your `project.clj`:

```clojure
[pg-mentat-client "0.1.0-SNAPSHOT"]
```

### deps.edn

Add to your `deps.edn`:

```clojure
{pg-mentat-client/pg-mentat-client {:mvn/version "0.1.0-SNAPSHOT"}}
```

## Quick Start

```clojure
(require '[pg-mentat.client :as mentat])

;; Connect to mentatd
(def conn (mentat/connect "http://localhost:8080"))

;; Get database value
(def db (mentat/db conn))

;; Query - Find all people
(mentat/q '[:find ?e ?name
           :where [?e :person/name ?name]]
          db)
;; => [[10001 "Alice"] [10002 "Bob"]]

;; Transact - Add new entity
(mentat/transact conn
  [{:db/id "tempid1"
    :person/name "Charlie"
    :person/email "charlie@example.com"}])
;; => {:tx 1000003 :tempids {"tempid1" 10003} :tx-data [...]}

;; Pull - Get entity attributes
(mentat/pull db [:person/name :person/email] 10001)
;; => {:person/name "Alice" :person/email "alice@example.com"}

;; Pull with wildcard
(mentat/pull db '[*] 10001)
;; => {:db/id 10001 :person/name "Alice" :person/email "alice@example.com" ...}
```

## API Documentation

### Connection Management

#### `connect [uri]`
Connect to pg_mentat via mentatd HTTP gateway.

```clojure
(def conn (mentat/connect "http://localhost:8080"))
```

#### `db [conn]`
Get current database value (immutable snapshot).

```clojure
(def db (mentat/db conn))
```

### Querying

#### `q [query db & inputs]`
Execute Datalog query with optional input bindings.

```clojure
;; Simple query
(mentat/q '[:find ?e ?name
           :where [?e :person/name ?name]]
          db)

;; Query with inputs
(mentat/q '[:find ?e
           :in $ ?name
           :where [?e :person/name ?name]]
          db "Alice")

;; Query with multiple inputs
(mentat/q '[:find ?e ?age
           :in $ ?min-age
           :where [?e :person/age ?age]
                  [(>= ?age ?min-age)]]
          db 25)
```

### Transactions

#### `transact [conn tx-data]`
Execute transaction with entity maps or explicit datoms.

```clojure
;; Add new entity with tempid
(mentat/transact conn
  [{:db/id "tempid1"
    :person/name "David"
    :person/age 28}])

;; Add facts to existing entity
(mentat/transact conn
  [[:db/add 10001 :person/age 31]])

;; Retract specific value
(mentat/transact conn
  [[:db/retract 10001 :person/age 30]])

;; Retract entire entity
(mentat/transact conn
  [[:db/retractEntity 10001]])

;; Mixed operations
(mentat/transact conn
  [{:db/id "new-person" :person/name "Eve"}
   [:db/add 10002 :person/friends "new-person"]
   [:db/retract 10003 :person/active false]])
```

### Pull API

#### `pull [db pattern eid]`
Pull entity data using pull pattern.

```clojure
;; Pull specific attributes
(mentat/pull db [:person/name :person/email] 10001)

;; Pull all attributes
(mentat/pull db '[*] 10001)

;; Pull with nested entities
(mentat/pull db
  [:person/name
   {:person/friends [:person/name]}]
  10001)

;; Pull with lookup ref
(mentat/pull db '[*] [:person/email "alice@example.com"])
```

#### `pull-many [db pattern eids]`
Pull multiple entities with the same pattern.

```clojure
(mentat/pull-many db [:person/name :person/age] [10001 10002 10003])
```

#### `entity [db eid]`
Get all attributes of an entity (equivalent to `pull` with `[*]`).

```clojure
(mentat/entity db 10001)
```

### Index Access

#### `datoms [db index components]`
Access datoms index directly.

```clojure
;; All datoms for entity 10001
(mentat/datoms db :eavt [10001])

;; Find entities with specific attribute value
(mentat/datoms db :avet [:person/name "Alice"])

;; All values for an attribute
(mentat/datoms db :aevt [:person/name])
```

### Temporal Queries

#### `as-of [db tx-id]`
Get database as of a specific transaction.

```clojure
(def past-db (mentat/as-of db 1000))
(mentat/q '[:find ?e ?name :where [?e :person/name ?name]] past-db)
```

#### `since [db tx-id]`
Get database changes since a specific transaction.

```clojure
(def recent-db (mentat/since db 1000))
```

#### `history [db]`
Get full history including retractions.

```clojure
(def history-db (mentat/history db))
```

### Helper Functions

#### `tempid [partition] [partition id]`
Create a temporary ID for use in transactions.

```clojure
(mentat/tempid :db.part/user)
;; => "tempid-550e8400-e29b-41d4-a716-446655440000"

(mentat/tempid :db.part/user 42)
;; => "tempid-db.part/user-42"
```

#### `tempid? [x]`
Check if a value is a temporary ID.

```clojure
(mentat/tempid? "tempid-123") ;; => true
(mentat/tempid? "regular-string") ;; => false
```

#### `lookup-ref? [x]`
Check if a value is a lookup ref.

```clojure
(mentat/lookup-ref? [:person/email "alice@example.com"]) ;; => true
(mentat/lookup-ref? 10001) ;; => false
```

#### `resolve-lookup-ref [db ref]`
Resolve a lookup ref to an entity ID.

```clojure
(mentat/resolve-lookup-ref db [:person/email "alice@example.com"])
;; => 10001
```

#### `retract [eid attr] [eid attr value]`
Create a retraction for use in transactions.

```clojure
;; Retract specific value
(mentat/retract 10001 :person/age 30)
;; => [:db/retract 10001 :person/age 30]

;; Retract all values for attribute
(mentat/retract 10001 :person/email)
;; => [:db/retractAttribute 10001 :person/email]
```

#### `retract-entity [eid]`
Create an entity retraction.

```clojure
(mentat/retract-entity 10001)
;; => [:db/retractEntity 10001]
```

### Schema Functions

#### `schema [db]`
Get complete schema information.

```clojure
(mentat/schema db)
;; => {:person/name {:db/valueType :db.type/string ...} ...}
```

#### `attribute [db ident]`
Get information about a specific attribute.

```clojure
(mentat/attribute db :person/name)
;; => {:db/valueType :db.type/string :db/cardinality :db.cardinality/one ...}
```

### Speculative Transactions

#### `with [db tx-data]`
Execute speculative transaction without committing.

```clojure
(mentat/with db
  [{:db/id "temp1" :person/name "Speculative User"}])
;; Returns what-if results without modifying database
```

## Complete Example

```clojure
(require '[pg-mentat.client :as mentat])

;; Connect to mentatd
(def conn (mentat/connect "http://localhost:8080"))

;; Define schema (if not already present)
(mentat/transact conn
  [{:db/ident :person/name
    :db/valueType :db.type/string
    :db/cardinality :db.cardinality/one}
   {:db/ident :person/email
    :db/valueType :db.type/string
    :db/cardinality :db.cardinality/one
    :db/unique :db.unique/identity}
   {:db/ident :person/age
    :db/valueType :db.type/long
    :db/cardinality :db.cardinality/one}
   {:db/ident :person/friends
    :db/valueType :db.type/ref
    :db/cardinality :db.cardinality/many}])

;; Create some people
(let [tx-result (mentat/transact conn
                  [{:db/id "alice"
                    :person/name "Alice"
                    :person/email "alice@example.com"
                    :person/age 30}
                   {:db/id "bob"
                    :person/name "Bob"
                    :person/email "bob@example.com"
                    :person/age 25
                    :person/friends "alice"}
                   {:db/id "charlie"
                    :person/name "Charlie"
                    :person/email "charlie@example.com"
                    :person/age 35
                    :person/friends ["alice" "bob"]}])]
  (println "Created entities:" (:tempids tx-result)))

;; Query for all people and their ages
(def db (mentat/db conn))
(mentat/q '[:find ?name ?age
           :where [?e :person/name ?name]
                  [?e :person/age ?age]]
          db)
;; => [["Alice" 30] ["Bob" 25] ["Charlie" 35]]

;; Find people over 25
(mentat/q '[:find ?name
           :where [?e :person/name ?name]
                  [?e :person/age ?age]
                  [(> ?age 25)]]
          db)
;; => [["Alice"] ["Charlie"]]

;; Pull Charlie's data with friends
(mentat/pull db
  [:person/name
   :person/age
   {:person/friends [:person/name]}]
  [:person/email "charlie@example.com"])
;; => {:person/name "Charlie"
;;     :person/age 35
;;     :person/friends [{:person/name "Alice"} {:person/name "Bob"}]}

;; Update Alice's age
(mentat/transact conn
  [[:db/add [:person/email "alice@example.com"] :person/age 31]])

;; Check historical data
(def history-db (mentat/history (mentat/db conn)))
(mentat/q '[:find ?age ?tx
           :where [?e :person/email "alice@example.com"]
                  [?e :person/age ?age ?tx]]
          history-db)
;; Shows both age 30 and 31 with their transaction IDs
```

## Error Handling

The client throws `ExceptionInfo` on errors with details in the exception data:

```clojure
(try
  (mentat/q "invalid-query" db)
  (catch clojure.lang.ExceptionInfo e
    (println "Error:" (.getMessage e))
    (println "Details:" (ex-data e))))
```

## Testing

Run tests with:

```bash
lein test
```

For continuous testing during development:

```bash
lein test-refresh
```

## Performance Considerations

- Connections are lightweight - just store the URI
- All operations are synchronous HTTP calls
- Consider connection pooling for high-throughput applications
- Use `pull-many` instead of multiple `pull` calls when fetching multiple entities
- Batch transactions when possible

## Batch Queries and Caching (Advanced)

When using a mentatd server with caching support (via `client-cached` namespace), you can leverage db value caching for efficient batch operations:

### Database Value Caching

```clojure
(require '[pg-mentat.client-cached :as mentat])

;; Get a cached database snapshot
(def conn (mentat/connect "http://localhost:8080"))
(def db (mentat/db conn))
;; => DatabaseValue with :db-id and :basis-t

;; All queries on this db value see the same snapshot
(mentat/q '[:find ?e :where [?e :person/name]] db)
(mentat/q '[:find ?age :where [_ :person/age ?age]] db)
;; Both queries guaranteed to see exact same db state
```

### Batch Query Execution

Execute multiple queries in a single HTTP request:

```clojure
;; Execute multiple queries atomically on same snapshot
(mentat/q-batch db
  [{:query '[:find ?e ?name
            :where [?e :person/name ?name]]
    :args []}
   {:query '[:find ?e
            :in $ ?email
            :where [?e :person/email ?email]]
    :args ["alice@example.com"]}
   {:query '[:find (count ?e)
            :where [?e :person/age ?age]
                   [(> ?age 25)]]
    :args []}])
;; => [[[10001 "Alice"] [10002 "Bob"]]  ; Results from query 1
;;     [[10001]]                         ; Results from query 2
;;     [2]]                              ; Results from query 3
```

### Batch Pull Operations

Pull multiple entities efficiently:

```clojure
(mentat/pull-batch db
  [{:pattern [:person/name :person/email]
    :eid 10001}
   {:pattern '[*]
    :eid [:person/email "bob@example.com"]}
   {:pattern [:person/name {:person/friends [:person/name]}]
    :eid 10003}])
;; => [{:person/name "Alice" :person/email "alice@example.com"}
;;     {:db/id 10002 :person/name "Bob" ...}
;;     {:person/name "Charlie" :person/friends [{:person/name "Alice"} ...]}]
```

### Snapshot Isolation

Ensure consistent reads across multiple operations:

```clojure
(mentat/with-db conn
  (fn [db]
    ;; All operations here use the same db snapshot
    (let [people (mentat/q '[:find ?e ?name
                            :where [?e :person/name ?name]] db)
          count (mentat/q '[:find (count ?e) .
                           :where [?e :person/name]] db)
          alice (mentat/pull db '[*] [:person/email "alice@example.com"])]
      {:people people
       :count count
       :alice alice})))
;; All three operations guaranteed to see same db state
```

### Transaction with Cached DB After

```clojure
;; Transactions return db-after with caching
(let [tx-result (mentat/transact conn
                  [{:db/id "new-person"
                    :person/name "Diana"
                    :person/email "diana@example.com"}])]
  ;; Use db-after for immediate queries on new state
  (when-let [db-after (:db-after tx-result)]
    (mentat/q '[:find ?name
               :where [?e :person/name ?name]] db-after)))
;; Queries on db-after see transaction results immediately
```

### Connection Pooling for High Throughput

```clojure
;; Create a connection pool
(def pool (mentat/connection-pool "http://localhost:8080" 10))

;; Use connections from pool for parallel operations
(require '[clojure.core.async :as async])

(let [results (async/chan 100)]
  ;; Launch parallel transactions
  (doseq [i (range 100)]
    (async/go
      (mentat/with-connection pool
        (fn [conn]
          (let [tx (mentat/transact conn
                     [{:db/id (str "person-" i)
                       :person/name (str "Person " i)}])]
            (async/>! results tx))))))

  ;; Collect results
  (dotimes [_ 100]
    (async/<!! results)))
```

### Performance Best Practices with Caching

1. **Use batch operations**: Combine multiple queries/pulls into single requests
2. **Reuse db values**: Get db once, use for multiple read operations
3. **Leverage caching**: Let mentatd cache db snapshots for consistent reads
4. **Connection pooling**: Use multiple connections for parallel write operations
5. **Minimize round trips**: Batch related operations together

## Differences from Datomic

While this library aims to provide a Datomic-like API, there are some differences:

1. **No lazy entities** - `entity` returns all attributes immediately
2. **No partitions** - `tempid` accepts partition for compatibility but ignores it
3. **Simplified listeners** - Transaction listeners are placeholder implementations
4. **HTTP-based** - All operations go through HTTP (no direct database connection)
5. **No local caching** - Each operation makes a server request

## Contributing

1. Fork the repository
2. Create a feature branch
3. Write tests for new functionality
4. Ensure all tests pass
5. Submit a pull request

## License

Copyright 2024 Greg Burd

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.