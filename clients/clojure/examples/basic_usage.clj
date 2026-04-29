(ns pg-mentat.examples.basic-usage
  "Basic usage examples for the pg_mentat Clojure peer library.

   This demonstrates the Datomic-compatible API: connection, schema definition,
   transactions, queries, pull, entity, and time travel.

   Prerequisites:
     - PostgreSQL running with the pg_mentat extension installed

   Run:
     clj -M:dev -m pg-mentat.examples.basic-usage"
  (:require [pg-mentat.client :as d]))

;; ---------------------------------------------------------------------------
;; 1. Connect to PostgreSQL directly (peer library -- no daemon required)
;; ---------------------------------------------------------------------------

(defn connect-example []
  (println "\n--- 1. Connecting ---")

  ;; Connect directly to PostgreSQL via next.jdbc.
  ;; No mentatd daemon needed.
  (let [conn (d/connect {:pg {:dbtype "postgresql"
                              :host "localhost"
                              :dbname "postgres"
                              :user "postgres"}})]
    (println "Connected to PostgreSQL (store:" (:store-name conn) ")")
    conn))

;; ---------------------------------------------------------------------------
;; 2. Define a schema
;; ---------------------------------------------------------------------------

(defn schema-example [conn]
  (println "\n--- 2. Schema Definition ---")

  ;; Schema attributes use the same EDN format as Datomic.
  ;; All standard value types are supported: string, long, double,
  ;; boolean, instant, keyword, ref, uuid, bytes.
  (d/transact conn
    {:tx-data
     [;; Person attributes
      {:db/ident       :person/name
       :db/valueType   :db.type/string
       :db/cardinality :db.cardinality/one
       :db/doc         "A person's full name"}

      {:db/ident       :person/email
       :db/valueType   :db.type/string
       :db/cardinality :db.cardinality/one
       :db/unique      :db.unique/identity
       :db/doc         "A person's email (unique identity)"}

      {:db/ident       :person/age
       :db/valueType   :db.type/long
       :db/cardinality :db.cardinality/one}

      {:db/ident       :person/roles
       :db/valueType   :db.type/keyword
       :db/cardinality :db.cardinality/many
       :db/doc         "Set of role keywords"}

      {:db/ident       :person/manager
       :db/valueType   :db.type/ref
       :db/cardinality :db.cardinality/one
       :db/doc         "Reference to this person's manager"}

      ;; Enum values (defined as named entities, just like Datomic)
      {:db/ident :role/engineer}
      {:db/ident :role/manager}
      {:db/ident :role/director}]})

  (println "Schema transacted successfully."))

;; ---------------------------------------------------------------------------
;; 3. Transact data
;; ---------------------------------------------------------------------------

(defn transact-example [conn]
  (println "\n--- 3. Transacting Data ---")

  ;; Map-form transactions (most common)
  (let [result (d/transact conn
                 {:tx-data
                  [{:db/id       "bob-id"
                    :person/name  "Bob Smith"
                    :person/email "bob@example.com"
                    :person/age   42
                    :person/roles [:role/manager]}

                   {:db/id        "alice-id"
                    :person/name  "Alice Johnson"
                    :person/email "alice@example.com"
                    :person/age   35
                    :person/roles [:role/engineer]
                    :person/manager "bob-id"}

                   {:person/name  "Carol Williams"
                    :person/email "carol@example.com"
                    :person/age   28
                    :person/roles [:role/engineer]
                    :person/manager "bob-id"}]})]

    (println "Transaction result keys:" (when (map? result) (keys result)))
    result))

;; ---------------------------------------------------------------------------
;; 4. Query data
;; ---------------------------------------------------------------------------

(defn query-examples [conn]
  (println "\n--- 4. Queries ---")
  (let [db (d/db conn)]

    ;; Simple query: find all names
    (println "\nAll people:")
    (let [results (d/q '[:find ?name
                         :where [?e :person/name ?name]]
                       db)]
      (doseq [row results]
        (println "  -" (first row))))

    ;; Query with joins
    (println "\nPeople with emails:")
    (let [results (d/q '[:find ?name ?email
                         :where
                         [?e :person/name ?name]
                         [?e :person/email ?email]]
                       db)]
      (doseq [[name email] results]
        (println "  -" name "-" email)))))

;; ---------------------------------------------------------------------------
;; 5. Pull API
;; ---------------------------------------------------------------------------

(defn pull-examples [conn]
  (println "\n--- 5. Pull API ---")
  (let [db (d/db conn)
        ;; Find Alice's entity ID
        results (d/q '[:find ?e
                       :where [?e :person/name "Alice Johnson"]]
                     db)]

    (when (seq results)
      (let [alice-id (ffirst results)]
        ;; Pull specific attributes
        (println "\nAlice (specific attrs):")
        (println "  " (d/pull db [:person/name :person/email :person/age] alice-id))

        ;; Pull with wildcard
        (println "\nAlice (all attrs):")
        (println "  " (d/pull db '[*] alice-id))))))

;; ---------------------------------------------------------------------------
;; 6. Entity API
;; ---------------------------------------------------------------------------

(defn entity-examples [conn]
  (println "\n--- 6. Entity API ---")
  (let [db (d/db conn)
        results (d/q '[:find ?e
                       :where [?e :person/name "Bob Smith"]]
                     db)]
    (when (seq results)
      (let [bob-id (ffirst results)
            bob (d/entity db bob-id)]
        (println "Bob's full entity:")
        (println "  " bob)
        (println "  Name:" (:person/name bob))
        (println "  Email:" (:person/email bob))))))

;; ---------------------------------------------------------------------------
;; 7. Time travel
;; ---------------------------------------------------------------------------

(defn time-travel-example [conn]
  (println "\n--- 7. Time Travel ---")
  (let [db (d/db conn)
        current-t (:basis-t db)]
    (println "Current basis-t:" current-t)

    ;; as-of: see the database at a specific point in time
    (let [old-db (d/as-of db current-t)]
      (println "as-of t" current-t ":")
      (let [results (d/q '[:find ?name
                           :where [?e :person/name ?name]]
                         old-db)]
        (doseq [[name] results]
          (println "  -" name))))))

;; ---------------------------------------------------------------------------
;; 8. Speculative transactions
;; ---------------------------------------------------------------------------

(defn speculative-example [conn]
  (println "\n--- 8. Speculative Transaction (with) ---")
  (let [db (d/db conn)
        result (d/with db {:tx-data [{:person/name "Speculative Person"
                                      :person/email "spec@example.com"}]})]
    (println "Speculative result:" (when (map? result) (keys result)))
    (println "(No data persisted)")))

;; ---------------------------------------------------------------------------
;; 9. Schema inspection
;; ---------------------------------------------------------------------------

(defn schema-inspection-example [conn]
  (println "\n--- 9. Schema Inspection ---")
  (let [db (d/db conn)
        s (d/schema db)]
    (println "Schema:" (type s))
    (when (and s (map? s))
      (println "Schema keys:" (take 5 (keys s))))))

;; ---------------------------------------------------------------------------
;; 10. Cleanup
;; ---------------------------------------------------------------------------

(defn cleanup-example [conn]
  (println "\n--- 10. Cleanup ---")
  (d/release conn)
  (println "Connection released."))

;; ---------------------------------------------------------------------------
;; Main entry point
;; ---------------------------------------------------------------------------

(defn -main [& args]
  (println "=== pg_mentat Clojure Peer Library -- Basic Usage ===")
  (println "Connecting directly to PostgreSQL (no daemon required)")

  (try
    (let [conn (connect-example)]
      (schema-example conn)
      (transact-example conn)
      (query-examples conn)
      (pull-examples conn)
      (entity-examples conn)
      (time-travel-example conn)
      (speculative-example conn)
      (schema-inspection-example conn)
      (cleanup-example conn))

    (catch Exception e
      (println "\nError:" (.getMessage e))
      (when-let [data (ex-data e)]
        (println "Details:" data))))

  (println "\n=== Done ===")
  (shutdown-agents))
