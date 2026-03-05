;; Datomic Client Test Suite for mentatd
;; Run with: lein repl or bin/repl from Datomic distribution

(require '[datomic.api :as d])

;; Configuration
(def mentatd-uri "datomic:free://localhost:8080/test-db")

(defn test-connection
  "Test basic connection to mentatd"
  []
  (println "\n=== Testing Connection ===")
  (try
    (d/create-database mentatd-uri)
    (let [conn (d/connect mentatd-uri)]
      (println "✓ Connected to mentatd")
      (println "  URI:" mentatd-uri)
      conn)
    (catch Exception e
      (println "✗ Connection failed:" (.getMessage e))
      nil)))

(defn test-schema
  "Test schema installation"
  [conn]
  (println "\n=== Testing Schema Installation ===")
  (try
    (let [schema [{:db/ident :person/name
                   :db/valueType :db.type/string
                   :db/cardinality :db.cardinality/one
                   :db/doc "Person's name"}
                  {:db/ident :person/age
                   :db/valueType :db.type/long
                   :db/cardinality :db.cardinality/one
                   :db/doc "Person's age"}
                  {:db/ident :person/email
                   :db/valueType :db.type/string
                   :db/cardinality :db.cardinality/one
                   :db/unique :db.unique/identity
                   :db/doc "Person's email"}]
          tx-result @(d/transact conn schema)]
      (println "✓ Schema installed")
      (println "  Entities added:" (count (:tx-data tx-result)))
      tx-result)
    (catch Exception e
      (println "✗ Schema installation failed:" (.getMessage e))
      nil)))

(defn test-data-insert
  "Test inserting data"
  [conn]
  (println "\n=== Testing Data Insert ===")
  (try
    (let [people [{:person/name "Alice"
                   :person/age 30
                   :person/email "alice@example.com"}
                  {:person/name "Bob"
                   :person/age 25
                   :person/email "bob@example.com"}
                  {:person/name "Charlie"
                   :person/age 35
                   :person/email "charlie@example.com"}]
          tx-result @(d/transact conn people)]
      (println "✓ Data inserted")
      (println "  People added:" (count people))
      tx-result)
    (catch Exception e
      (println "✗ Data insert failed:" (.getMessage e))
      nil)))

(defn test-query-find-all
  "Test finding all people"
  [db]
  (println "\n=== Testing Query: Find All ===")
  (try
    (let [query '[:find ?name ?age
                  :where
                  [?e :person/name ?name]
                  [?e :person/age ?age]]
          results (d/q query db)]
      (println "✓ Query executed")
      (println "  Results:" (count results))
      (doseq [[name age] results]
        (println "   -" name "(" age ")"))
      results)
    (catch Exception e
      (println "✗ Query failed:" (.getMessage e))
      nil)))

(defn test-query-with-filter
  "Test query with filter"
  [db]
  (println "\n=== Testing Query: With Filter ===")
  (try
    (let [query '[:find ?name
                  :in $ ?min-age
                  :where
                  [?e :person/name ?name]
                  [?e :person/age ?age]
                  [(>= ?age ?min-age)]]
          results (d/q query db 30)]
      (println "✓ Query with filter executed")
      (println "  Results (age >= 30):" (count results))
      (doseq [[name] results]
        (println "   -" name))
      results)
    (catch Exception e
      (println "✗ Query with filter failed:" (.getMessage e))
      nil)))

(defn test-pull-api
  "Test pull API"
  [db]
  (println "\n=== Testing Pull API ===")
  (try
    (let [query '[:find ?e
                  :where [?e :person/email "alice@example.com"]]
          entity-id (ffirst (d/q query db))
          pull-result (d/pull db '[*] entity-id)]
      (println "✓ Pull API executed")
      (println "  Entity:" pull-result)
      pull-result)
    (catch Exception e
      (println "✗ Pull API failed:" (.getMessage e))
      nil)))

(defn test-entity-api
  "Test entity API"
  [db]
  (println "\n=== Testing Entity API ===")
  (try
    (let [query '[:find ?e
                  :where [?e :person/email "bob@example.com"]]
          entity-id (ffirst (d/q query db))
          entity (d/entity db entity-id)]
      (println "✓ Entity API executed")
      (println "  Name:" (:person/name entity))
      (println "  Age:" (:person/age entity))
      (println "  Email:" (:person/email entity))
      entity)
    (catch Exception e
      (println "✗ Entity API failed:" (.getMessage e))
      nil)))

(defn test-history-query
  "Test history queries"
  [conn db]
  (println "\n=== Testing History Queries ===")
  (try
    ;; Update Alice's age
    (let [alice-id (ffirst (d/q '[:find ?e
                                   :where [?e :person/email "alice@example.com"]]
                                db))]
      @(d/transact conn [[:db/add alice-id :person/age 31]]))

    ;; Query history
    (let [history-db (d/history db)
          query '[:find ?age ?tx ?added
                  :in $ ?email
                  :where
                  [?e :person/email ?email]
                  [?e :person/age ?age ?tx ?added]]
          results (d/q query history-db "alice@example.com")]
      (println "✓ History query executed")
      (println "  Age changes:" (count results))
      (doseq [[age tx added] results]
        (println "   -" age (if added "added" "retracted") "in tx" tx))
      results)
    (catch Exception e
      (println "✗ History query failed:" (.getMessage e))
      nil)))

(defn test-as-of-query
  "Test as-of queries"
  [db]
  (println "\n=== Testing As-Of Queries ===")
  (try
    (let [;; Get a transaction basis
          tx-basis (d/basis-t db)
          ;; Query as of previous transaction
          as-of-db (d/as-of db (dec tx-basis))
          query '[:find ?name ?age
                  :where
                  [?e :person/name ?name]
                  [?e :person/age ?age]]
          current-results (d/q query db)
          as-of-results (d/q query as-of-db)]
      (println "✓ As-of query executed")
      (println "  Current results:" (count current-results))
      (println "  As-of results:" (count as-of-results))
      as-of-results)
    (catch Exception e
      (println "✗ As-of query failed:" (.getMessage e))
      nil)))

(defn test-retract
  "Test retracting data"
  [conn db]
  (println "\n=== Testing Retract ===")
  (try
    (let [charlie-id (ffirst (d/q '[:find ?e
                                     :where [?e :person/email "charlie@example.com"]]
                                  db))
          tx-result @(d/transact conn [[:db/retractEntity charlie-id]])]
      (println "✓ Entity retracted")
      (println "  Transaction:" (:tx-data tx-result))
      tx-result)
    (catch Exception e
      (println "✗ Retract failed:" (.getMessage e))
      nil)))

(defn cleanup
  "Cleanup test database"
  []
  (println "\n=== Cleanup ===")
  (try
    (d/delete-database mentatd-uri)
    (println "✓ Database deleted")
    (catch Exception e
      (println "✗ Cleanup failed:" (.getMessage e)))))

(defn run-all-tests
  "Run all tests"
  []
  (println "========================================")
  (println "Datomic Client Test Suite for mentatd")
  (println "========================================")

  (when-let [conn (test-connection)]
    (test-schema conn)
    (test-data-insert conn)

    (let [db (d/db conn)]
      (test-query-find-all db)
      (test-query-with-filter db)
      (test-pull-api db)
      (test-entity-api db)
      (test-history-query conn db)
      (test-as-of-query db)
      (test-retract conn db))

    (cleanup))

  (println "\n========================================")
  (println "Test Suite Complete")
  (println "========================================"))

;; Run tests when loaded
(comment
  ;; To run from REPL:
  (run-all-tests)

  ;; Or run individual tests:
  (def conn (test-connection))
  (test-schema conn)
  (test-data-insert conn)
  (def db (d/db conn))
  (test-query-find-all db)
  )
