(ns pg-mentat.examples.basic-usage
  "Basic usage examples for the pg_mentat Clojure client.

   This demonstrates the Datomic-compatible API: client creation,
   schema definition, transactions, queries, pull, and time travel.

   Prerequisites:
     - mentatd running at ws://localhost:8080/ws
     - A PostgreSQL database with pg_mentat extension installed

   Run:
     clj -M -m pg-mentat.examples.basic-usage"
  (:require [pg-mentat.client :as d]))

;; ---------------------------------------------------------------------------
;; 1. Connect to pg_mentat via mentatd
;; ---------------------------------------------------------------------------

(defn connect-example []
  (println "\n--- 1. Connecting ---")

  ;; Create a client (analogous to datomic.client.api/client)
  (let [client (d/client {:server-type :pg-mentat
                           :endpoint "ws://localhost:8080/ws"})]

    ;; List available databases
    (println "Available databases:" (d/list-databases client))

    ;; Connect to a specific database
    (let [conn (d/connect client {:db-name "example-db"})]
      (println "Connected to:" (:db-name conn))
      (println "Connection ID:" (:connection-id conn))
      conn)))

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
                  [{:db/id       "bob-id"      ;; string tempid
                    :person/name  "Bob Smith"
                    :person/email "bob@example.com"
                    :person/age   42
                    :person/roles [:role/manager]}

                   {:db/id        "alice-id"
                    :person/name  "Alice Johnson"
                    :person/email "alice@example.com"
                    :person/age   35
                    :person/roles [:role/engineer]
                    :person/manager "bob-id"}  ;; reference by tempid

                   {:person/name  "Carol Williams"
                    :person/email "carol@example.com"
                    :person/age   28
                    :person/roles [:role/engineer]
                    :person/manager "bob-id"}]})]

    (println "Transaction result keys:" (keys result))
    (println "Tempid mappings:" (:tempids result))
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
      (doseq [[name] results]
        (println "  -" name)))

    ;; Query with joins
    (println "\nPeople with emails:")
    (let [results (d/q '[:find ?name ?email
                         :where
                         [?e :person/name ?name]
                         [?e :person/email ?email]]
                       db)]
      (doseq [[name email] results]
        (println "  -" name "-" email)))

    ;; Query with input parameter
    (println "\nPeople older than 30:")
    (let [results (d/q '[:find ?name ?age
                         :in $ ?min-age
                         :where
                         [?e :person/name ?name]
                         [?e :person/age ?age]
                         [(>= ?age ?min-age)]]
                       db 30)]
      (doseq [[name age] results]
        (println "  -" name "(age" (str age ")"))))

    ;; Aggregate query
    (println "\nAge statistics:")
    (let [results (d/q '[:find (count ?e) (avg ?age) (min ?age) (max ?age)
                         :where
                         [?e :person/age ?age]]
                       db)]
      (let [[cnt avg-age min-age max-age] (first results)]
        (println "  Count:" cnt
                 "Average:" avg-age
                 "Min:" min-age
                 "Max:" max-age)))

    ;; Find spec: collection (returns flat list)
    (println "\nAll names (collection):")
    (let [results (d/q '[:find [?name ...]
                         :where [_ :person/name ?name]]
                       db)]
      (println "  " results))

    ;; Find spec: scalar (returns single value)
    (println "\nFirst name (scalar):")
    (let [result (d/q '[:find ?name .
                        :where [_ :person/name ?name]]
                      db)]
      (println "  " result))))

;; ---------------------------------------------------------------------------
;; 5. Pull API
;; ---------------------------------------------------------------------------

(defn pull-examples [conn]
  (println "\n--- 5. Pull API ---")
  (let [db (d/db conn)
        ;; First, find Alice's entity ID
        alice-id (d/q '[:find ?e .
                        :where [?e :person/email "alice@example.com"]]
                      db)]

    (when alice-id
      ;; Pull specific attributes
      (println "\nAlice (specific attrs):")
      (println "  " (d/pull db '[:person/name :person/email :person/age] alice-id))

      ;; Pull with wildcard
      (println "\nAlice (all attrs):")
      (println "  " (d/pull db '[*] alice-id))

      ;; Pull with nested reference (resolve manager)
      (println "\nAlice with manager details:")
      (println "  " (d/pull db '[:person/name
                                  {:person/manager [:person/name :person/email]}]
                            alice-id))

      ;; Pull with reverse lookup (who has Alice as manager?)
      (println "\nBob's direct reports:")
      (let [bob-id (d/q '[:find ?e .
                          :where [?e :person/email "bob@example.com"]]
                        db)]
        (when bob-id
          (println "  " (d/pull db '[:person/name
                                      {:person/_manager [:person/name]}]
                                bob-id)))))))

;; ---------------------------------------------------------------------------
;; 6. Time travel
;; ---------------------------------------------------------------------------

(defn time-travel-example [conn]
  (println "\n--- 6. Time Travel ---")
  (let [db (d/db conn)]

    ;; Record the current transaction ID
    (let [current-t (:t db)]
      (println "Current t:" current-t)

      ;; Make a change
      (d/transact conn {:tx-data [[:db/add
                                    (d/q '[:find ?e .
                                           :where [?e :person/email "alice@example.com"]]
                                         db)
                                    :person/age 36]]})

      ;; Query the current state
      (let [new-db (d/db conn)]
        (println "\nAlice's current age:"
                 (d/q '[:find ?age .
                        :where
                        [?e :person/email "alice@example.com"]
                        [?e :person/age ?age]]
                      new-db))

        ;; as-of: see the database before the change
        (let [old-db (d/as-of new-db current-t)]
          (println "Alice's age as-of t" (str current-t ":")
                   (d/q '[:find ?age .
                          :where
                          [?e :person/email "alice@example.com"]
                          [?e :person/age ?age]]
                        old-db)))

        ;; history: see all values over time
        (println "\nAlice's age history:")
        (let [hist-db (d/history new-db)
              results (d/q '[:find ?age ?tx ?added
                             :where
                             [?e :person/email "alice@example.com"]
                             [?e :person/age ?age ?tx ?added]]
                           hist-db)]
          (doseq [[age tx added] (sort-by second results)]
            (println "  t=" tx "age=" age (if added "asserted" "retracted"))))))))

;; ---------------------------------------------------------------------------
;; 7. Datoms and index access
;; ---------------------------------------------------------------------------

(defn datoms-example [conn]
  (println "\n--- 7. Datoms Index Access ---")
  (let [db (d/db conn)]

    ;; Access EAVT index
    (println "EAVT datoms (first 5):")
    (let [datoms (d/datoms db {:index :eavt})]
      (doseq [d (take 5 datoms)]
        (println "  " d)))

    ;; Access AEVT index (by attribute)
    (println "\nAEVT datoms for :person/name:")
    (let [datoms (d/datoms db {:index :aevt
                                :components [:person/name]})]
      (doseq [d datoms]
        (println "  " d)))))

;; ---------------------------------------------------------------------------
;; 8. Advanced query patterns
;; ---------------------------------------------------------------------------

(defn advanced-queries [conn]
  (println "\n--- 8. Advanced Query Patterns ---")
  (let [db (d/db conn)]

    ;; NOT clause: people without a manager
    (println "People without a manager:")
    (let [results (d/q '[:find ?name
                         :where
                         [?e :person/name ?name]
                         (not [?e :person/manager _])]
                       db)]
      (doseq [[name] results]
        (println "  -" name)))

    ;; OR clause: engineers or managers
    (println "\nEngineers or managers:")
    (let [results (d/q '[:find ?name ?role
                         :where
                         [?e :person/name ?name]
                         [?e :person/roles ?role]
                         (or [?e :person/roles :role/engineer]
                             [?e :person/roles :role/manager])]
                       db)]
      (doseq [[name role] results]
        (println "  -" name "(" role ")")))

    ;; Query with rules
    (println "\nUsing rules -- find all reports (direct + indirect):")
    (let [rules '[[(reports-to ?person ?manager)
                   [?person :person/manager ?manager]]
                  [(reports-to ?person ?manager)
                   [?person :person/manager ?middle]
                   (reports-to ?middle ?manager)]]
          results (d/q '[:find ?person-name ?manager-name
                         :in $ %
                         :where
                         (reports-to ?person ?manager)
                         [?person :person/name ?person-name]
                         [?manager :person/name ?manager-name]]
                       db rules)]
      (doseq [[person manager] results]
        (println "  " person "reports to" manager)))))

;; ---------------------------------------------------------------------------
;; 9. Connection cleanup
;; ---------------------------------------------------------------------------

(defn cleanup-example [conn]
  (println "\n--- 9. Cleanup ---")
  (d/release conn)
  (println "Connection released."))

;; ---------------------------------------------------------------------------
;; Main entry point
;; ---------------------------------------------------------------------------

(defn -main [& args]
  (println "=== pg_mentat Clojure Client -- Basic Usage ===")
  (println "Connecting to mentatd at ws://localhost:8080/ws")

  (try
    (let [conn (connect-example)]
      (schema-example conn)
      (transact-example conn)
      (query-examples conn)
      (pull-examples conn)
      (time-travel-example conn)
      (datoms-example conn)
      (advanced-queries conn)
      (cleanup-example conn))

    (catch Exception e
      (println "\nError:" (.getMessage e))
      (when-let [data (ex-data e)]
        (println "Details:" data))))

  (println "\n=== Done ===")
  (shutdown-agents))
