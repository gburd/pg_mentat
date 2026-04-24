(ns pg-mentat.client
  "Clojure client library for pg_mentat Datalog database.

   Provides a Datomic-like API for interacting with pg_mentat via mentatd HTTP gateway."
  (:require [clj-http.client :as http]
            [clojure.edn :as edn]
            [cheshire.core :as json]))

;; Connection object (just stores URI for now)
(defrecord Connection [uri])

(defn connect
  "Connect to pg_mentat via mentatd HTTP gateway.
   Returns a connection object.

   Examples:
     (connect \"http://localhost:8080\")
     (connect \"http://mentat.example.com:8080\")"
  [uri]
  (->Connection uri))

(defn- send-request
  "Internal: Send HTTP request to mentatd."
  [conn operation]
  (let [response (http/post (:uri conn)
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
   In current implementation, this is a no-op that returns the connection.
   Future: Will cache db-id for batch queries."
  [conn]
  conn)

(defn q
  "Execute Datalog query.

   Args:
     query - Datalog query as EDN (vector or list)
     db    - Database value (from (db conn))
     inputs - Optional input bindings (varargs)

   Returns: Vector of result tuples

   Examples:
     (q '[:find ?e :where [?e :person/name]] db)
     (q '[:find ?e :in $ ?name :where [?e :person/name ?name]] db \"Alice\")"
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

   Examples:
     (transact conn [{:db/id \"tempid1\" :person/name \"Alice\"}])
     (transact conn [[:db/add 10001 :person/age 30]])"
  [conn tx-data]
  (let [response (send-request conn {:op :transact
                                     :tx-data tx-data})]
    {:tx (:tx response)
     :tempids (or (:tempids response) {})
     :tx-data (or (:tx-data response) [])}))

(defn pull
  "Pull entity data using pull pattern.

   Args:
     db      - Database value
     pattern - Pull pattern (vector)
     eid     - Entity ID (long) or lookup ref

   Returns: Map of entity attributes

   Examples:
     (pull db [:person/name :person/email] 10001)
     (pull db '[*] [:person/email \"alice@example.com\"])
     (pull db [:person/name {:person/friends [:person/name]}] 10001)"
  [db pattern eid]
  (let [response (send-request db {:op :pull
                                   :pattern pattern
                                   :eid eid})]
    (:result response)))

(defn pull-many
  "Pull multiple entities using the same pattern.

   Args:
     db      - Database value
     pattern - Pull pattern (vector)
     eids    - Collection of entity IDs or lookup refs

   Returns: Vector of entity maps

   Examples:
     (pull-many db [:person/name :person/email] [10001 10002 10003])"
  [db pattern eids]
  (let [response (send-request db {:op :pull-many
                                   :pattern pattern
                                   :eids (vec eids)})]
    (:result response)))

(defn entity
  "Get all attributes of an entity (lazy in Datomic, eager here).

   Args:
     db  - Database value
     eid - Entity ID

   Returns: Map of all entity attributes"
  [db eid]
  (pull db '[*] eid))

(defn datoms
  "Access datoms index directly.

   Args:
     db         - Database value
     index      - Index name (:eavt, :aevt, :avet, :vaet)
     components - Vector of index components

   Returns: Vector of datom vectors [e a v tx added]

   Examples:
     (datoms db :eavt [10001])          ; All datoms for entity 10001
     (datoms db :avet [:person/name \"Alice\"])  ; Entities with name=Alice"
  [db index components]
  (let [response (send-request db {:op :datoms
                                   :index index
                                   :components components})]
    (:result response)))

;; Temporal query functions
(defn as-of
  "Get database as of transaction ID.
   Returns a filtered db value."
  [db tx-id]
  ;; Future: Cache as-of snapshot
  (assoc db :as-of tx-id))

(defn since
  "Get database changes since transaction ID."
  [db tx-id]
  (assoc db :since tx-id))

(defn history
  "Get all history (including retractions)."
  [db]
  (assoc db :history true))

;; Helper functions
(defn basis-t
  "Get current basis timestamp of database."
  [db]
  (let [response (send-request db {:op :basis-t})]
    (:result response)))

(defn with
  "Speculative transaction (d/with).
   Execute transaction and return what-if results without committing."
  [db tx-data]
  (let [response (send-request db {:op :with
                                   :tx-data tx-data})]
    (:result response)))

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
     db    - Database value
     ident - Attribute keyword (e.g., :person/name)

   Returns: Map with attribute properties (:db/valueType, :db/cardinality, etc.)"
  [db ident]
  (let [schema-info (schema db)]
    (get schema-info ident)))

;; Transaction functions
(defn retract
  "Helper to create a retraction in transaction data.

   Args:
     eid   - Entity ID
     attr  - Attribute keyword
     value - Value to retract (optional, retracts all if omitted)

   Returns: Retraction form for use in transact."
  ([eid attr]
   [:db/retractAttribute eid attr])
  ([eid attr value]
   [:db/retract eid attr value]))

(defn retract-entity
  "Helper to create an entity retraction in transaction data.

   Args:
     eid - Entity ID to retract completely

   Returns: Entity retraction form for use in transact."
  [eid]
  [:db/retractEntity eid])

;; Lookup ref support
(defn lookup-ref?
  "Check if a value is a lookup ref (vector with 2 elements, first is keyword)."
  [x]
  (and (vector? x)
       (= 2 (count x))
       (keyword? (first x))))

(defn resolve-lookup-ref
  "Resolve a lookup ref to an entity ID.

   Args:
     db  - Database value
     ref - Lookup ref like [:person/email \"alice@example.com\"]

   Returns: Entity ID or nil if not found."
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
  "Create a tempid string for use in transactions.

   Args:
     partition - Partition keyword (ignored in pg_mentat, for Datomic compat)
     id        - Optional numeric ID suffix

   Returns: Tempid string"
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
  "Access a range of values from an index.

   Args:
     db         - Database value
     index      - Index name (:avet typically)
     attr       - Attribute keyword
     start      - Start value (inclusive)
     end        - End value (exclusive)

   Returns: Vector of datoms in range."
  [db index attr start end]
  (let [response (send-request db {:op :index-range
                                   :index index
                                   :attr attr
                                   :start start
                                   :end end})]
    (:result response)))

;; Error handling
(defn tx-report-queue
  "Create a transaction report queue (for monitoring transactions).
   Note: Returns a simple atom in this implementation."
  [conn]
  (atom []))

(defn listen!
  "Listen for transaction reports (simplified implementation).

   Args:
     conn     - Connection object
     key      - Listener key (for removal)
     callback - Function to call with tx-report

   Returns: conn"
  [conn key callback]
  ;; In a real implementation, this would use WebSockets or SSE
  ;; For now, this is a no-op placeholder
  conn)

(defn unlisten!
  "Remove a transaction listener.

   Args:
     conn - Connection object
     key  - Listener key used in listen!

   Returns: conn"
  [conn key]
  ;; Placeholder for listener removal
  conn)