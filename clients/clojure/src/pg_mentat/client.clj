(ns pg-mentat.client
  "Datomic-compatible client library for pg_mentat.

   Implements the Datomic Client API protocol over WebSocket connections
   using Transit+JSON encoding. This is a drop-in replacement for
   datomic.client.api -- change your require from
     [datomic.client.api :as d]
   to
     [pg-mentat.client :as d]
   and your existing Datomic code should work without changes.

   Protocol:
     Messages are Transit+JSON encoded maps sent over WebSocket.
     Each request is a map with :op and :args keys.
     Responses contain :result on success or :error on failure.

   Usage:
     (def client (d/client {:server-type :pg-mentat
                            :endpoint \"ws://localhost:8080/ws\"}))
     (def conn (d/connect client {:db-name \"my-db\"}))
     (def db (d/db conn))
     (d/q '[:find ?e ?name :where [?e :person/name ?name]] db)
     (d/transact conn {:tx-data [{:person/name \"Alice\"}]})"
  (:require [clojure.edn :as edn]
            [clojure.string])
  (:import [java.net URI]
           [java.net.http HttpClient WebSocket WebSocket$Listener]
           [java.nio ByteBuffer]
           [java.util UUID]
           [java.util.concurrent CompletableFuture CompletionStage
            ConcurrentHashMap LinkedBlockingQueue TimeUnit]))

;; ---------------------------------------------------------------------------
;; Transit+JSON encoding/decoding (minimal, no external dependency)
;; ---------------------------------------------------------------------------

(declare transit-encode transit-decode transit-json-str)

(defn- transit-encode-value
  "Encode a Clojure value as a Transit+JSON compatible structure."
  [v]
  (cond
    (nil? v) nil
    (keyword? v) (str "~:" (if (namespace v)
                             (str (namespace v) "/" (name v))
                             (name v)))
    (symbol? v) (str "~$" (str v))
    (string? v) (if (or (.startsWith ^String v "~")
                        (.startsWith ^String v "^"))
                  (str "~" v)
                  v)
    (boolean? v) v
    (integer? v) (if (or (> v Integer/MAX_VALUE)
                         (< v Integer/MIN_VALUE))
                   (str "~i" v)
                   v)
    (float? v) v
    (double? v) v
    (instance? UUID v) (str "~u" v)
    (instance? java.util.Date v) (str "~m" (.getTime ^java.util.Date v))
    (map? v) (let [entries (mapcat (fn [[k val]]
                                    [(transit-encode-value k)
                                     (transit-encode-value val)])
                                  v)]
               (into ["^ "] entries))
    (sequential? v) (mapv transit-encode-value v)
    (set? v) ["~#set" (mapv transit-encode-value (seq v))]
    :else (str v)))

(defn transit-encode
  "Encode a Clojure map as a Transit+JSON string."
  [m]
  (let [encoded (transit-encode-value m)]
    ;; Simple JSON serialization for Transit-encoded structures
    (transit-json-str encoded)))

(defn- transit-json-str
  "Serialize a Transit-encoded value to JSON string."
  [v]
  (cond
    (nil? v) "null"
    (string? v) (str "\"" (-> v
                              (.replace "\\" "\\\\")
                              (.replace "\"" "\\\"")
                              (.replace "\n" "\\n")
                              (.replace "\r" "\\r")
                              (.replace "\t" "\\t"))
                     "\"")
    (boolean? v) (str v)
    (number? v) (str v)
    (vector? v) (str "[" (clojure.string/join "," (map transit-json-str v)) "]")
    :else (transit-json-str (str v))))

(defn- parse-json
  "Minimal JSON parser returning Clojure data structures."
  [^String s]
  (let [idx (volatile! 0)
        len (.length s)]
    (letfn [(current [] (when (< @idx len) (.charAt s @idx)))
            (advance! [] (vswap! idx inc))
            (skip-ws! []
              (while (and (< @idx len)
                          (Character/isWhitespace (.charAt s @idx)))
                (advance!)))
            (read-value []
              (skip-ws!)
              (case (current)
                \" (read-string-val)
                \[ (read-array)
                \{ (read-object)
                \n (do (dotimes [_ 4] (advance!)) nil)  ; null
                \t (do (dotimes [_ 4] (advance!)) true) ; true
                \f (do (dotimes [_ 5] (advance!)) false) ; false
                (read-number)))
            (read-string-val []
              (advance!) ; skip opening "
              (let [sb (StringBuilder.)]
                (loop []
                  (let [c (current)]
                    (advance!)
                    (cond
                      (= c \") (.toString sb)
                      (= c \\) (let [esc (current)]
                                 (advance!)
                                 (case esc
                                   \n (.append sb \newline)
                                   \r (.append sb \return)
                                   \t (.append sb \tab)
                                   \" (.append sb \")
                                   \\ (.append sb \\)
                                   \/ (.append sb \/)
                                   \u (let [hex (subs s @idx (+ @idx 4))]
                                        (vswap! idx + 4)
                                        (.append sb (char (Integer/parseInt hex 16)))))
                                 (recur))
                      :else (do (.append sb c) (recur)))))))
            (read-array []
              (advance!) ; skip [
              (skip-ws!)
              (if (= (current) \])
                (do (advance!) [])
                (loop [result [(read-value)]]
                  (skip-ws!)
                  (if (= (current) \])
                    (do (advance!) result)
                    (do
                      (when (= (current) \,) (advance!))
                      (recur (conj result (read-value))))))))
            (read-object []
              (advance!) ; skip {
              (skip-ws!)
              (if (= (current) \})
                (do (advance!) {})
                (loop [result {}]
                  (skip-ws!)
                  (let [k (read-value)]
                    (skip-ws!)
                    (when (= (current) \:) (advance!))
                    (let [v (read-value)]
                      (skip-ws!)
                      (let [result (assoc result k v)]
                        (if (= (current) \})
                          (do (advance!) result)
                          (do
                            (when (= (current) \,) (advance!))
                            (recur result)))))))))
            (read-number []
              (let [start @idx]
                (when (= (current) \-) (advance!))
                (while (and (< @idx len)
                            (Character/isDigit (.charAt s @idx)))
                  (advance!))
                (let [has-dot (and (< @idx len) (= (current) \.))]
                  (when has-dot
                    (advance!)
                    (while (and (< @idx len)
                                (Character/isDigit (.charAt s @idx)))
                      (advance!)))
                  (let [has-exp (and (< @idx len)
                                    (contains? #{\e \E} (current)))]
                    (when has-exp
                      (advance!)
                      (when (contains? #{\+ \-} (current)) (advance!))
                      (while (and (< @idx len)
                                  (Character/isDigit (.charAt s @idx)))
                        (advance!)))
                    (let [num-str (subs s start @idx)]
                      (if (or has-dot has-exp)
                        (Double/parseDouble num-str)
                        (let [n (Long/parseLong num-str)]
                          n)))))))]
      (read-value))))

(defn- decode-transit-tagged
  "Decode a Transit tagged string to its Clojure value."
  [^String s]
  (cond
    (.startsWith s "~:") (let [kw-str (subs s 2)
                               slash-idx (.indexOf kw-str "/")]
                           (if (pos? slash-idx)
                             (keyword (subs kw-str 0 slash-idx)
                                      (subs kw-str (inc slash-idx)))
                             (keyword kw-str)))
    (.startsWith s "~$") (symbol (subs s 2))
    (.startsWith s "~i") (Long/parseLong (subs s 2))
    (.startsWith s "~u") (UUID/fromString (subs s 2))
    (.startsWith s "~m") (java.util.Date. (Long/parseLong (subs s 2)))
    (.startsWith s "~zNaN") Double/NaN
    (.startsWith s "~zINF") Double/POSITIVE_INFINITY
    (.startsWith s "~z-INF") Double/NEGATIVE_INFINITY
    (.startsWith s "~~") (subs s 1)  ; escaped tilde
    (.startsWith s "~^") (str "^" (subs s 2))  ; escaped caret
    :else s))

(defn transit-decode
  "Decode a Transit+JSON value to Clojure data."
  [v]
  (cond
    (nil? v) nil
    (string? v) (decode-transit-tagged v)
    (boolean? v) v
    (number? v) v
    (vector? v) (if (and (seq v) (= (first v) "^ "))
                  ;; cmap: ["^ ", k1, v1, k2, v2, ...]
                  (let [pairs (partition 2 (rest v))]
                    (into {} (map (fn [[k val]]
                                   [(transit-decode k) (transit-decode val)])
                                 pairs)))
                  ;; Check for tagged values
                  (if (and (= 2 (count v)) (string? (first v)))
                    (let [tag (first v)]
                      (cond
                        (= tag "~#list") (map transit-decode (second v))
                        (= tag "~#set") (set (map transit-decode (second v)))
                        :else (mapv transit-decode v)))
                    (mapv transit-decode v)))
    (map? v) (into {} (map (fn [[k val]]
                             [(transit-decode k) (transit-decode val)])
                           v))
    :else v))

(defn- parse-transit-json
  "Parse a Transit+JSON string to Clojure data."
  [^String s]
  (transit-decode (parse-json s)))

;; ---------------------------------------------------------------------------
;; WebSocket connection management
;; ---------------------------------------------------------------------------

(defrecord WsConnection [^WebSocket ws
                         ^LinkedBlockingQueue response-queue
                         ^ConcurrentHashMap pending-requests
                         session-id
                         closed?])

(defn- create-ws-listener
  "Create a WebSocket listener that accumulates message fragments
   and dispatches complete messages to the response queue."
  [^LinkedBlockingQueue response-queue
   ^ConcurrentHashMap pending-requests]
  (let [buffer (StringBuilder.)]
    (reify WebSocket$Listener
      (onOpen [_ ws]
        (.request ws 1))
      (onText [_ ws data last?]
        (.append buffer data)
        (when last?
          (let [msg (.toString buffer)]
            (.setLength buffer 0)
            (let [parsed (parse-transit-json msg)]
              ;; Route by request-id if present, else put on general queue
              (if-let [rid (get parsed :request-id)]
                (when-let [^CompletableFuture fut (.remove pending-requests rid)]
                  (.complete fut parsed))
                (.put response-queue parsed)))))
        (.request ws 1)
        nil)
      (onClose [_ ws status-code reason]
        (.put response-queue {:closed true
                              :status-code status-code
                              :reason (str reason)}))
      (onError [_ ws error]
        (.put response-queue {:error true
                              :message (.getMessage error)})))))

(defn- open-ws
  "Open a WebSocket connection to the given endpoint."
  [^String endpoint]
  (let [response-queue (LinkedBlockingQueue.)
        pending-requests (ConcurrentHashMap.)
        http-client (-> (HttpClient/newBuilder)
                        (.build))
        listener (create-ws-listener response-queue pending-requests)
        ws-future (-> http-client
                      (.newWebSocketBuilder)
                      (.buildAsync (URI/create endpoint) listener))
        ws (.get ws-future 10 TimeUnit/SECONDS)
        ;; Wait for welcome message
        welcome (.poll response-queue 10 TimeUnit/SECONDS)]
    (when (nil? welcome)
      (throw (ex-info "Timeout waiting for WebSocket welcome message"
                      {:endpoint endpoint})))
    (->WsConnection ws response-queue pending-requests
                    (get welcome :session-id)
                    (atom false))))

(defn- send-ws-request
  "Send a Transit+JSON request over WebSocket and wait for response."
  [^WsConnection conn request & {:keys [timeout-ms] :or {timeout-ms 30000}}]
  (when @(:closed? conn)
    (throw (ex-info "Connection is closed" {:connection conn})))
  (let [request-id (str (UUID/randomUUID))
        request-with-id (assoc request :request-id request-id)
        fut (CompletableFuture.)
        _ (.put ^ConcurrentHashMap (:pending-requests conn) request-id fut)
        msg (transit-encode request-with-id)]
    (.sendText ^WebSocket (:ws conn) msg true)
    (let [response (try
                     (.get fut timeout-ms TimeUnit/MILLISECONDS)
                     (catch java.util.concurrent.TimeoutException _
                       (.remove ^ConcurrentHashMap (:pending-requests conn) request-id)
                       (throw (ex-info "Request timeout"
                                       {:request-id request-id
                                        :timeout-ms timeout-ms}))))]
      (when-let [error (get response :error)]
        (throw (ex-info (or (get error :cognitect.anomalies/message) "Server error")
                        {:cognitect.anomalies/category
                         (get error :cognitect.anomalies/category
                              :cognitect.anomalies/fault)
                         :response response})))
      (get response :result))))

(defn- close-ws
  "Close a WebSocket connection."
  [^WsConnection conn]
  (when (compare-and-set! (:closed? conn) false true)
    (.sendClose ^WebSocket (:ws conn)
                WebSocket/NORMAL_CLOSURE "client disconnect")))

;; ---------------------------------------------------------------------------
;; Datomic Client API
;; ---------------------------------------------------------------------------

(defrecord Client [config ws-endpoint])
(defrecord Connection [client ws-conn db-name connection-id])
(defrecord Db [connection db-name database-id t next-t as-of-t since-t history?])

(defn client
  "Create a pg_mentat client. Drop-in replacement for datomic.client.api/client.

   Config map keys:
     :server-type - Must be :pg-mentat
     :endpoint    - WebSocket endpoint URL (e.g., \"ws://localhost:8080/ws\")
     :api-key     - Optional API key for authentication

   Returns a client object for use with connect, list-databases, etc."
  [{:keys [endpoint] :as config}]
  (when-not endpoint
    (throw (ex-info "Missing required :endpoint in client config"
                    {:config config})))
  (->Client config endpoint))

(defn connect
  "Connect to a database. Drop-in replacement for datomic.client.api/connect.

   Args:
     client - Client from (client config)
     arg-map - Map with :db-name key

   Returns a connection object."
  [^Client client {:keys [db-name] :as arg-map}]
  (when-not db-name
    (throw (ex-info "Missing required :db-name" {:arg-map arg-map})))
  (let [ws-conn (open-ws (:ws-endpoint client))
        result (send-ws-request ws-conn {:op :connect
                                         :args {:db-name db-name}})]
    (->Connection client ws-conn db-name
                  (get result :database-id))))

(defn db
  "Get current database value. Drop-in replacement for datomic.client.api/db.

   Args:
     conn - Connection from (connect ...)

   Returns an immutable database value for use with q, pull, etc."
  [^Connection conn]
  (let [result (send-ws-request (:ws-conn conn)
                                {:op :db
                                 :args {:db-name (:db-name conn)}})]
    (->Db conn
          (:db-name conn)
          (get result :database-id)
          (get result :t)
          (get result :next-t)
          nil nil false)))

(defn q
  "Execute a Datalog query. Drop-in replacement for datomic.client.api/q.

   Args (positional, Datomic-style):
     query  - Datalog query (EDN vector or string)
     db     - Database value from (db conn)
     & inputs - Optional query input sources

   Or args (map style, Datomic Client API):
     {:query <query> :args [<db> <inputs...>]}

   Returns a collection of result tuples."
  [& args]
  (let [[arg-map inputs]
        (if (map? (first args))
          ;; Map-style invocation: (q {:query ... :args [db ...]})
          [(first args) nil]
          ;; Positional: (q query db & inputs)
          (let [query (first args)
                sources (rest args)
                db (first sources)
                extra-inputs (rest sources)]
            [{:query query :db db :inputs extra-inputs} nil]))
        {:keys [query db inputs]} (if (:db arg-map)
                                    arg-map
                                    ;; Handle {:query ... :args [db ...]}
                                    (let [q-args (:args arg-map)]
                                      {:query (:query arg-map)
                                       :db (first q-args)
                                       :inputs (rest q-args)}))
        conn (:connection db)
        ws-conn (:ws-conn conn)
        query-str (if (string? query) query (pr-str query))
        args-vec (mapv pr-str (or inputs []))]
    (send-ws-request ws-conn
                     {:op :q
                      :args (cond-> {:query query-str
                                     :args args-vec}
                              (:as-of-t db)
                              (assoc :as-of (:as-of-t db))
                              (:since-t db)
                              (assoc :since (:since-t db))
                              (:history? db)
                              (assoc :history true))})))

(defn transact
  "Execute a transaction. Drop-in replacement for datomic.client.api/transact.

   Args:
     conn    - Connection from (connect ...)
     arg-map - Map with :tx-data key (vector of transaction data)

   Returns a map with :db-before, :db-after, :tx-data, :tempids."
  [^Connection conn {:keys [tx-data] :as arg-map}]
  (when-not tx-data
    (throw (ex-info "Missing required :tx-data" {:arg-map arg-map})))
  (send-ws-request (:ws-conn conn)
                   {:op :transact
                    :args {:connection-id (str (:connection-id conn))
                           :tx-data (pr-str tx-data)}}))

(defn pull
  "Pull entity attributes. Drop-in replacement for datomic.client.api/pull.

   Args:
     db      - Database value from (db conn)
     pattern - Pull pattern (vector of attribute specs)
     eid     - Entity ID or lookup ref

   Returns a map of entity attributes."
  [^Db db pattern eid]
  (let [conn (:connection db)
        ws-conn (:ws-conn conn)
        entity-id (if (number? eid) eid
                      ;; Lookup ref -- resolve first
                      (first (first (q '[:find ?e :in $ ?a ?v
                                         :where [?e ?a ?v]]
                                       db (first eid) (second eid)))))]
    (send-ws-request ws-conn
                     {:op :pull
                      :args {:pattern (pr-str pattern)
                             :entity-id entity-id}})))

(defn pull-many
  "Pull multiple entities. Calls pull for each entity ID.

   Args:
     db      - Database value
     pattern - Pull pattern
     eids    - Collection of entity IDs

   Returns a vector of entity attribute maps."
  [^Db db pattern eids]
  (mapv #(pull db pattern %) eids))

(defn datoms
  "Access raw datoms from an index.
   Drop-in replacement for datomic.client.api/datoms.

   Args:
     db      - Database value
     arg-map - Map with :index (:eavt, :aevt, :avet, :vaet)
               and optional :components

   Returns a collection of datom vectors [e a v tx added?]."
  [^Db db {:keys [index components] :as arg-map}]
  (let [conn (:connection db)
        ws-conn (:ws-conn conn)]
    (send-ws-request ws-conn
                     {:op :datoms
                      :args {:index (pr-str index)
                             :components (mapv pr-str (or components []))}})))

(defn with
  "Speculative transaction. Drop-in replacement for datomic.client.api/with.

   Applies tx-data speculatively and returns the result without
   actually committing. Useful for testing transactions.

   Args:
     db      - Database value
     arg-map - Map with :tx-data key

   Returns a map with :db-after and :tx-data showing what would happen."
  [^Db db {:keys [tx-data] :as arg-map}]
  (let [conn (:connection db)
        ws-conn (:ws-conn conn)]
    (send-ws-request ws-conn
                     {:op :with
                      :args {:tx-data (pr-str tx-data)}})))

(defn tx-range
  "Query the transaction log.
   Drop-in replacement for datomic.client.api/tx-range.

   Args:
     conn    - Connection
     arg-map - Map with optional :start and :end transaction IDs

   Returns a collection of transaction log entries."
  [^Connection conn {:keys [start end] :as arg-map}]
  (send-ws-request (:ws-conn conn)
                   {:op :tx-range
                    :args (cond-> {}
                            start (assoc :start start)
                            end (assoc :end end))}))

(defn index-range
  "Access a range of the AVET index.

   Args:
     db      - Database value
     arg-map - Map with :attrid, :start, :end

   Returns a collection of datoms in the specified range."
  [^Db db {:keys [attrid start end] :as arg-map}]
  (let [conn (:connection db)
        ws-conn (:ws-conn conn)]
    (send-ws-request ws-conn
                     {:op :index-range
                      :args (cond-> {:attr (pr-str attrid)}
                              start (assoc :start (pr-str start))
                              end (assoc :end (pr-str end)))})))

;; ---------------------------------------------------------------------------
;; Time-travel database values
;; ---------------------------------------------------------------------------

(defn as-of
  "Return a database value as of a specific transaction.

   Args:
     db - Database value
     t  - Transaction ID or instant

   Returns a new database value filtered to that point in time."
  [^Db db t]
  (assoc db :as-of-t t :history? false))

(defn since
  "Return a database value showing only changes since a transaction.

   Args:
     db - Database value
     t  - Transaction ID or instant

   Returns a new database value filtered to changes since t."
  [^Db db t]
  (assoc db :since-t t :history? false))

(defn history
  "Return a database value including all history (assertions and retractions).

   Args:
     db - Database value

   Returns a new database value with full history."
  [^Db db]
  (assoc db :history? true))

;; ---------------------------------------------------------------------------
;; Catalog operations
;; ---------------------------------------------------------------------------

(defn list-databases
  "List available databases.

   Args:
     client - Client from (client config)

   Returns a collection of database name strings."
  [^Client client]
  (let [ws-conn (open-ws (:ws-endpoint client))]
    (try
      (send-ws-request ws-conn {:op :list-dbs :args {}})
      (finally
        (close-ws ws-conn)))))

(defn create-database
  "Create a new database.

   Args:
     client  - Client from (client config)
     arg-map - Map with :db-name

   Returns true on success."
  [^Client client {:keys [db-name] :as arg-map}]
  (when-not db-name
    (throw (ex-info "Missing required :db-name" {:arg-map arg-map})))
  (let [ws-conn (open-ws (:ws-endpoint client))]
    (try
      (send-ws-request ws-conn {:op :create-db
                                :args {:db-name db-name}})
      (finally
        (close-ws ws-conn)))))

(defn delete-database
  "Delete a database.

   Args:
     client  - Client from (client config)
     arg-map - Map with :db-name

   Returns true on success."
  [^Client client {:keys [db-name] :as arg-map}]
  (when-not db-name
    (throw (ex-info "Missing required :db-name" {:arg-map arg-map})))
  (let [ws-conn (open-ws (:ws-endpoint client))]
    (try
      (send-ws-request ws-conn {:op :delete-db
                                :args {:db-name db-name}})
      (finally
        (close-ws ws-conn)))))

;; ---------------------------------------------------------------------------
;; Connection lifecycle
;; ---------------------------------------------------------------------------

(defn release
  "Release a connection and its WebSocket.
   After calling release, the connection should not be used."
  [^Connection conn]
  (close-ws (:ws-conn conn)))

;; ---------------------------------------------------------------------------
;; Convenience aliases matching datomic.client.api
;; ---------------------------------------------------------------------------

(def ^{:doc "Alias for client -- matches datomic.client.api."} d-client client)
(def ^{:doc "Alias for connect -- matches datomic.client.api."} d-connect connect)
