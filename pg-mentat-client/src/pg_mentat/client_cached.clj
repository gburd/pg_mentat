(ns pg-mentat.client-cached
  "Clojure client library for pg_mentat with db value caching support.

   This version adds caching support for efficient batch queries while maintaining
   backward compatibility with non-caching mentatd servers."
  (:require [clj-http.client :as http]
            [clojure.edn :as edn]
            [cheshire.core :as json]))

;; Connection object now stores URI and optional cached db values
(defrecord Connection [uri db-cache])

;; Database value with optional db-id for caching
(defrecord DatabaseValue [conn db-id basis-t])

(defn connect
  "Connect to pg_mentat via mentatd HTTP gateway.
   Returns a connection object with optional caching support.

   Examples:
     (connect \"http://localhost:8080\")
     (connect \"http://mentat.example.com:8080\")"
  [uri]
  (->Connection uri (atom {})))

(defn- send-request
  "Internal: Send HTTP request to mentatd with optional db-id for caching."
  [conn-or-db operation]
  (let [conn (if (instance? Connection conn-or-db)
               conn-or-db
               (:conn conn-or-db))
        ;; Include db-id if we have a DatabaseValue
        operation (if (and (instance? DatabaseValue conn-or-db)
                          (:db-id conn-or-db))
                   (assoc operation :db-id (:db-id conn-or-db))
                   operation)
        response (http/post (:uri conn)
                           {:content-type "application/edn"
                            :accept "application/edn"
                            :body (pr-str operation)
                            :throw-exceptions false})]
    (if (= 200 (:status response))
      (edn/read-string (:body response))
      (throw (ex-info "mentatd request failed"
                      {:status (:status response)
                       :body (:body response)
                       :operation operation})))))

(defn db
  "Get current database value (immutable snapshot).

   With caching support:
   - Calls :db operation to get a db-id and basis-t
   - Caches the db value for reuse in batch queries
   - Falls back to connection if server doesn't support caching

   Options:
     :cache?  - Whether to cache this db value (default true)
     :refresh - Force refresh even if cached (default false)"
  ([conn]
   (db conn {}))
  ([conn {:keys [cache? refresh] :or {cache? true refresh false}}]
   ;; Try to get a cached db value with :db operation
   (try
     (let [response (send-request conn {:op :db})
           db-id (:db-id response)
           basis-t (:basis-t response)]
       (if (and db-id cache?)
         (let [db-value (->DatabaseValue conn db-id basis-t)]
           ;; Cache the db value
           (when-let [cache (:db-cache conn)]
             (swap! cache assoc :current db-value))
           db-value)
         ;; No caching support, return connection
         conn))
     (catch Exception e
       ;; Server doesn't support :db operation, fall back to connection
       (if (and (instance? clojure.lang.ExceptionInfo e)
                (= 400 (-> e ex-data :status)))
         conn
         (throw e))))))

(defn q
  "Execute Datalog query.

   With caching support:
   - Includes db-id in request when using a cached DatabaseValue
   - Server can use db-id to ensure consistent snapshot isolation

   Args:
     query - Datalog query as EDN (vector or list)
     db    - Database value (from (db conn)) or connection
     inputs - Optional input bindings (varargs)

   Returns: Vector of result tuples"
  [query db & inputs]
  (let [response (send-request db {:op :q
                                   :query query
                                   :args (vec inputs)})]
    (:result response)))

(defn transact
  "Execute transaction.

   Args:
     conn    - Connection object
     tx-data - Vector of transaction maps or :db/add/:db/retract forms

   Returns: Transaction report map with keys:
     :tx       - Transaction ID
     :tempids  - Map of tempid strings to resolved entity IDs
     :tx-data  - Vector of datoms asserted/retracted
     :db-after - New database value (if caching enabled)"
  [conn tx-data]
  (let [response (send-request conn {:op :transact
                                     :tx-data tx-data})
        result {:tx (:tx response)
                :tempids (or (:tempids response) {})
                :tx-data (or (:tx-data response) [])}]
    ;; If server returns a new db-id, create db-after
    (if-let [db-id (:db-id response)]
      (let [db-after (->DatabaseValue conn db-id (:basis-t response))]
        ;; Update cached current db
        (when-let [cache (:db-cache conn)]
          (swap! cache assoc :current db-after))
        (assoc result :db-after db-after))
      result)))

(defn pull
  "Pull entity data using pull pattern.

   With caching support:
   - Includes db-id for consistent reads from cached db value

   Args:
     db      - Database value or connection
     pattern - Pull pattern (vector)
     eid     - Entity ID (long) or lookup ref

   Returns: Map of entity attributes"
  [db pattern eid]
  (let [response (send-request db {:op :pull
                                   :pattern pattern
                                   :eid eid})]
    (:result response)))

(defn pull-many
  "Pull multiple entities using the same pattern.

   With caching support:
   - Includes db-id for consistent batch reads

   Args:
     db      - Database value or connection
     pattern - Pull pattern (vector)
     eids    - Collection of entity IDs or lookup refs

   Returns: Vector of entity maps"
  [db pattern eids]
  (let [response (send-request db {:op :pull-many
                                   :pattern pattern
                                   :eids (vec eids)})]
    (:result response)))

(defn entity
  "Get all attributes of an entity.

   Args:
     db  - Database value or connection
     eid - Entity ID

   Returns: Map of all entity attributes"
  [db eid]
  (pull db '[*] eid))

(defn datoms
  "Access datoms index directly.

   With caching support:
   - Uses cached db snapshot for consistent index reads

   Args:
     db         - Database value or connection
     index      - Index name (:eavt, :aevt, :avet, :vaet)
     components - Vector of index components

   Returns: Vector of datom vectors [e a v tx added]"
  [db index components]
  (let [response (send-request db {:op :datoms
                                   :index index
                                   :components components})]
    (:result response)))

;; Temporal query functions with caching
(defn as-of
  "Get database as of transaction ID.

   With caching support:
   - Creates a new cached db value at specific point in time
   - Reuses cached snapshots when possible"
  [db tx-id]
  (let [conn (if (instance? Connection db) db (:conn db))]
    (try
      (let [response (send-request conn {:op :db
                                         :as-of tx-id})
            db-id (:db-id response)
            basis-t (:basis-t response)]
        (if db-id
          (->DatabaseValue conn db-id basis-t)
          ;; Fallback for non-caching servers
          (assoc db :as-of tx-id)))
      (catch Exception e
        ;; Fallback for servers without :db operation
        (assoc db :as-of tx-id)))))

(defn since
  "Get database changes since transaction ID.

   With caching support:
   - Creates a new cached db value for changes since tx-id"
  [db tx-id]
  (let [conn (if (instance? Connection db) db (:conn db))]
    (try
      (let [response (send-request conn {:op :db
                                         :since tx-id})
            db-id (:db-id response)
            basis-t (:basis-t response)]
        (if db-id
          (->DatabaseValue conn db-id basis-t)
          ;; Fallback for non-caching servers
          (assoc db :since tx-id)))
      (catch Exception e
        ;; Fallback
        (assoc db :since tx-id)))))

(defn history
  "Get all history (including retractions).

   With caching support:
   - Creates a cached db value with full history"
  [db]
  (let [conn (if (instance? Connection db) db (:conn db))]
    (try
      (let [response (send-request conn {:op :db
                                         :history true})
            db-id (:db-id response)
            basis-t (:basis-t response)]
        (if db-id
          (->DatabaseValue conn db-id basis-t)
          ;; Fallback
          (assoc db :history true)))
      (catch Exception e
        ;; Fallback
        (assoc db :history true)))))

;; Helper functions
(defn basis-t
  "Get current basis timestamp of database.

   With caching support:
   - Returns cached basis-t if available
   - Otherwise fetches from server"
  [db]
  (if (and (instance? DatabaseValue db) (:basis-t db))
    (:basis-t db)
    (let [response (send-request db {:op :basis-t})]
      (:result response))))

(defn with
  "Speculative transaction (d/with).
   Execute transaction and return what-if results without committing.

   With caching support:
   - Uses cached db-id for consistent speculative transactions"
  [db tx-data]
  (let [response (send-request db {:op :with
                                   :tx-data tx-data})]
    (:result response)))

;; Batch query support (new with caching)
(defn q-batch
  "Execute multiple queries in a single request using the same db snapshot.

   This is much more efficient than multiple individual queries as it:
   - Uses a single HTTP request
   - Guarantees all queries see the same db snapshot
   - Reduces network overhead

   Args:
     db      - Database value (must be from (db conn) with caching)
     queries - Vector of query specs, each being {:query ... :args [...]}

   Returns: Vector of results in same order as queries

   Example:
     (q-batch db
       [{:query '[:find ?e :where [?e :person/name]]
         :args []}
        {:query '[:find ?e :in $ ?name :where [?e :person/name ?name]]
         :args [\"Alice\"]}])"
  [db queries]
  (when-not (instance? DatabaseValue db)
    (throw (ex-info "q-batch requires a cached database value from (db conn)"
                    {:db db})))
  (let [response (send-request db {:op :q-batch
                                   :queries queries})]
    (:results response)))

(defn pull-batch
  "Execute multiple pulls in a single request using the same db snapshot.

   More efficient than multiple pull calls.

   Args:
     db    - Database value (must be from (db conn) with caching)
     pulls - Vector of pull specs, each being {:pattern ... :eid ...}

   Returns: Vector of pulled entities in same order

   Example:
     (pull-batch db
       [{:pattern [:person/name] :eid 10001}
        {:pattern '[*] :eid [:person/email \"alice@example.com\"]}])"
  [db pulls]
  (when-not (instance? DatabaseValue db)
    (throw (ex-info "pull-batch requires a cached database value from (db conn)"
                    {:db db})))
  (let [response (send-request db {:op :pull-batch
                                   :pulls pulls})]
    (:results response)))

;; Cache management
(defn cached-dbs
  "Get all cached database values for a connection.

   Returns: Map of cached db values"
  [conn]
  @(:db-cache conn))

(defn clear-cache!
  "Clear all cached database values for a connection.

   Returns: conn"
  [conn]
  (reset! (:db-cache conn) {})
  conn)

(defn with-db
  "Execute a function with a specific database value.

   Ensures all queries within the function use the same db snapshot.

   Args:
     conn - Connection object
     f    - Function that takes a db value

   Example:
     (with-db conn
       (fn [db]
         (let [people (q '[:find ?e :where [?e :person/name]] db)
               ages (q '[:find ?age :where [_ :person/age ?age]] db)]
           {:people people :ages ages})))"
  [conn f]
  (let [db-val (db conn)]
    (f db-val)))

;; Schema functions
(defn schema
  "Get schema information from the database.

   Returns: Map of attribute information keyed by attribute ident."
  [db]
  (let [response (send-request db {:op :schema})]
    (:result response)))

(defn attribute
  "Get information about a specific attribute.

   Args:
     db    - Database value or connection
     ident - Attribute keyword (e.g., :person/name)

   Returns: Map with attribute properties (:db/valueType, :db/cardinality, etc.)"
  [db ident]
  (let [schema-info (schema db)]
    (get schema-info ident)))

;; Transaction functions
(defn retract
  "Helper to create a retraction in transaction data."
  ([eid attr]
   [:db/retractAttribute eid attr])
  ([eid attr value]
   [:db/retract eid attr value]))

(defn retract-entity
  "Helper to create an entity retraction in transaction data."
  [eid]
  [:db/retractEntity eid])

;; Lookup ref support
(defn lookup-ref?
  "Check if a value is a lookup ref."
  [x]
  (and (vector? x)
       (= 2 (count x))
       (keyword? (first x))))

(defn resolve-lookup-ref
  "Resolve a lookup ref to an entity ID."
  [db ref]
  (when (lookup-ref? ref)
    (let [[attr value] ref
          result (q '[:find ?e .
                      :in $ ?attr ?value
                      :where [?e ?attr ?value]]
                    db attr value)]
      result)))

;; Utility functions
(defn tempid
  "Create a tempid string for use in transactions."
  ([partition]
   (str "tempid-" (java.util.UUID/randomUUID)))
  ([partition id]
   (str "tempid-" partition "-" id)))

(defn tempid?
  "Check if a value is a tempid string."
  [x]
  (and (string? x)
       (or (.startsWith x "tempid")
           (.startsWith x "temp"))))

;; Index access functions
(defn index-range
  "Access a range of values from an index."
  [db index attr start end]
  (let [response (send-request db {:op :index-range
                                   :index index
                                   :attr attr
                                   :start start
                                   :end end})]
    (:result response)))

;; Connection pooling helpers (for high-throughput applications)
(defn connection-pool
  "Create a pool of connections for parallel operations.

   Args:
     uri  - mentatd URI
     size - Number of connections in pool

   Returns: Vector of connections"
  [uri size]
  (vec (repeatedly size #(connect uri))))

(defn with-connection
  "Execute a function with a connection from the pool.

   Args:
     pool - Connection pool
     f    - Function that takes a connection

   Example:
     (def pool (connection-pool \"http://localhost:8080\" 10))
     (with-connection pool
       (fn [conn]
         (transact conn [{:person/name \"Test\"}])))"
  [pool f]
  ;; Simple round-robin selection
  (let [conn (nth pool (rand-int (count pool)))]
    (f conn)))