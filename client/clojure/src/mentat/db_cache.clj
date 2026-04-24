(ns mentat.db-cache
  "Database snapshot support for efficient batch queries.

   Instead of sending HTTP requests for each query, create a snapshot
   and reuse it across multiple queries to reduce overhead."
  (:require [mentat.client :as client]))

(defn db
  "Get a database snapshot for batch queries.

   Returns a connection with a db-id that can be used for multiple
   queries against the same point-in-time view of the database.

   Example:
     (def conn (mentat/connect \"http://localhost:3000\" \"mydb\"))
     (def db (mentat/db conn))

     ;; All queries use the same snapshot (much faster)
     (mentat/q query1 db)
     (mentat/q query2 db)
     (mentat/q query3 db)"
  [conn]
  (let [request {:op :db-snapshot}
        response (client/send-request conn request)]
    (if (:error response)
      (throw (ex-info "Failed to create db snapshot" response))
      (assoc conn
             :db-id (:db-id response)
             :basis-t (:basis-t response)))))

(defn q
  "Execute a query with optional cached db.

   If db has a :db-id (from calling db), uses the cached snapshot
   for faster batch queries. Otherwise executes a regular query.

   Args:
     query - The Datalog query
     db    - Connection or db snapshot from (db conn)
     inputs - Optional query inputs

   Example:
     ;; Regular query (one HTTP request)
     (q '[:find ?e :where [?e :person/name]] conn)

     ;; Batch queries with snapshot (faster)
     (def db (db conn))
     (q '[:find ?e :where [?e :person/name]] db)
     (q '[:find ?e :where [?e :person/age ?a] [(> ?a 30)]] db)"
  [query db & inputs]
  (let [request (cond-> {:op :q
                          :query query
                          :args (vec inputs)}
                  (:db-id db) (assoc :db-id (:db-id db)))]
    (let [response (client/send-request db request)]
      (if (:error response)
        (throw (ex-info "Query failed" response))
        (:result response)))))

(defn basis-t
  "Get the basis-t (transaction ID) of a db snapshot.

   Returns the point-in-time transaction ID that this snapshot represents."
  [db]
  (:basis-t db))

(defn with-db
  "Execute a function with a database snapshot.

   Creates a snapshot, passes it to the function, and returns the result.
   The snapshot is automatically cleaned up after the configured TTL.

   Example:
     (with-db conn
       (fn [db]
         (let [names (q '[:find ?n :where [_ :person/name ?n]] db)
               ages (q '[:find ?a :where [_ :person/age ?a]] db)]
           {:names names :ages ages})))"
  [conn f]
  (let [db-snapshot (db conn)]
    (f db-snapshot)))

(defn benchmark-batch-queries
  "Benchmark the performance improvement of using db snapshots.

   Runs the same queries with and without snapshots and reports timing."
  [conn queries]
  (println "Benchmarking" (count queries) "queries...")

  ;; Without snapshot (individual HTTP requests)
  (let [start (System/currentTimeMillis)]
    (doseq [query queries]
      (client/q query conn))
    (let [elapsed (- (System/currentTimeMillis) start)]
      (println "Without snapshot:" elapsed "ms")))

  ;; With snapshot (cached basis-t)
  (let [start (System/currentTimeMillis)
        db-snapshot (db conn)]
    (doseq [query queries]
      (q query db-snapshot))
    (let [elapsed (- (System/currentTimeMillis) start)]
      (println "With snapshot:" elapsed "ms"))))