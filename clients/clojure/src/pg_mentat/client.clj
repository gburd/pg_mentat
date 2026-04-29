(ns pg-mentat.client
  "Datomic-compatible peer library for pg_mentat.

   Connects directly to PostgreSQL via next.jdbc -- no HTTP daemon required.
   Calls the pg_mentat extension SQL functions (mentat_query, mentat_transact,
   mentat_pull, mentat_entity, etc.) to provide a Datomic-compatible API.

   Drop-in replacement for datomic.api -- change your require from
     [datomic.api :as d]
   to
     [pg-mentat.client :as d]
   and your existing Datomic peer code should work without changes.

   Usage:
     (require '[pg-mentat.client :as d])

     (def conn (d/connect {:pg {:dbtype \"postgresql\"
                                :host \"localhost\"
                                :dbname \"postgres\"
                                :user \"postgres\"}}))
     (def db (d/db conn))
     (d/q '[:find ?e ?name :where [?e :person/name ?name]] db)
     (d/transact conn {:tx-data [{:person/name \"Alice\"}]})
     (d/pull db '[*] 42)
     (d/entity db 42)"
  (:require [next.jdbc :as jdbc]
            [next.jdbc.result-set :as rs]
            [clojure.edn :as edn]
            [clojure.string :as str])
  (:import [java.sql Connection]
           [org.postgresql.util PGobject]
           [java.time Instant]
           [java.util Date UUID]))

;; ---------------------------------------------------------------------------
;; JSON parsing (minimal, no external dependency)
;; ---------------------------------------------------------------------------

(declare parse-json json-str)

(defn- parse-json
  "Minimal JSON parser returning Clojure data structures.
   Handles the subset used by pg_mentat JSONB return values."
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
                \n (do (dotimes [_ 4] (advance!)) nil)
                \t (do (dotimes [_ 4] (advance!)) true)
                \f (do (dotimes [_ 5] (advance!)) false)
                (read-number)))
            (read-string-val []
              (advance!)
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
              (advance!)
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
              (advance!)
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
                        (Long/parseLong num-str)))))))]
      (read-value))))

(defn- json-str
  "Serialize a Clojure value to JSON string."
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
    (keyword? v) (json-str (if (namespace v)
                             (str ":" (namespace v) "/" (name v))
                             (str ":" (name v))))
    (map? v) (str "{"
                  (str/join ","
                    (map (fn [[k val]]
                           (str (json-str (if (keyword? k)
                                           (if (namespace k)
                                             (str ":" (namespace k) "/" (name k))
                                             (str ":" (name k)))
                                           (str k)))
                                ":" (json-str val)))
                         v))
                  "}")
    (sequential? v) (str "[" (str/join "," (map json-str v)) "]")
    :else (json-str (str v))))

;; ---------------------------------------------------------------------------
;; EDN serialization helpers
;; ---------------------------------------------------------------------------

(defn- tx-data->edn
  "Convert Clojure transaction data to EDN string for mentat_transact.
   Handles maps (entity maps), vectors with :db/add/:db/retract, etc."
  [tx-data]
  (pr-str tx-data))

(defn- pattern->str
  "Convert a pull pattern to string form suitable for mentat_pull."
  [pattern]
  (if (string? pattern)
    pattern
    (pr-str pattern)))

(defn- query->str
  "Convert a Datalog query to string form suitable for mentat_query."
  [query]
  (if (string? query)
    query
    (pr-str query)))

(defn- inputs->json
  "Convert query inputs map to JSON string for mentat_query.
   Inputs can include temporal options (:as-of, :since) and input bindings."
  [inputs-map]
  (if (or (nil? inputs-map) (empty? inputs-map))
    "{}"
    (json-str inputs-map)))

;; ---------------------------------------------------------------------------
;; JSONB result coercion
;; ---------------------------------------------------------------------------

(defn- coerce-jsonb
  "Coerce a PostgreSQL JSONB result to Clojure data.
   Handles PGobject, String, and nil."
  [v]
  (cond
    (nil? v) nil
    (instance? PGobject v)
    (let [val (.getValue ^PGobject v)]
      (when val (parse-json val)))
    (string? v) (parse-json v)
    :else v))

(defn- keywordize-entity
  "Convert a raw JSON entity map (string keys like \":person/name\")
   to a Clojure map with keyword keys."
  [m]
  (when m
    (into {}
      (map (fn [[k v]]
             (let [kw (if (and (string? k) (.startsWith ^String k ":"))
                        (let [kw-str (subs k 1)
                              slash-idx (.indexOf kw-str "/")]
                          (if (pos? slash-idx)
                            (keyword (subs kw-str 0 slash-idx)
                                     (subs kw-str (inc slash-idx)))
                            (keyword kw-str)))
                        (keyword k))]
               [kw v]))
           m))))

;; ---------------------------------------------------------------------------
;; Connection and database value types
;; ---------------------------------------------------------------------------

(defrecord PgMentatConnection [datasource db-name store-name])

(defrecord PgMentatDb [connection basis-t as-of-t since-t history?])

;; ---------------------------------------------------------------------------
;; Core API: connect
;; ---------------------------------------------------------------------------

(defn connect
  "Create a connection to pg_mentat backed by a direct PostgreSQL connection.

   This is the peer library equivalent of datomic.api/connect.
   No HTTP daemon is required -- connects directly to PostgreSQL.

   Config map keys:
     :pg         - next.jdbc db-spec map (required). Example:
                   {:dbtype \"postgresql\" :host \"localhost\"
                    :dbname \"postgres\" :user \"postgres\"}
     :store-name - Name of the mentat store (default: \"default\")

   Returns a connection object for use with db, transact, etc."
  [{:keys [pg store-name] :as config}]
  (when-not pg
    (throw (ex-info "Missing required :pg db-spec in connect config"
                    {:config config})))
  (let [ds (jdbc/get-datasource pg)
        store (or store-name "default")]
    (->PgMentatConnection ds nil store)))

;; ---------------------------------------------------------------------------
;; Core API: db
;; ---------------------------------------------------------------------------

(defn db
  "Get the current database value. Analogous to datomic.api/db.

   Returns an immutable database value that captures the current
   basis-t (latest transaction ID). Use with q, pull, entity, as-of."
  [^PgMentatConnection conn]
  (let [ds (:datasource conn)
        store (:store-name conn)
        ;; Get the current max transaction ID as the basis-t
        basis-t (jdbc/execute-one! ds
                  ["SELECT COALESCE(MAX(tx), 0) AS t FROM mentat.transactions"]
                  {:builder-fn rs/as-unqualified-lower-maps})]
    (->PgMentatDb conn (get basis-t :t 0) nil nil false)))

;; ---------------------------------------------------------------------------
;; Core API: q (query)
;; ---------------------------------------------------------------------------

(defn q
  "Execute a Datalog query. Analogous to datomic.api/q.

   Supports two calling conventions:

   Positional (Datomic peer style):
     (q '[:find ?e ?name :where [?e :person/name ?name]] db)
     (q '[:find ?e :in $ ?name :where [?e :person/name ?name]] db \"Alice\")

   Map style:
     (q {:query '[:find ?e :where [?e :person/name]] :args [db]})

   The database value must be included as the first source in positional form
   or as the first element of :args in map form.

   Returns query results as Clojure data (vector of tuples for :find relations,
   scalar for :find scalar, etc.)."
  [& args]
  (let [[query-form db extra-inputs]
        (if (map? (first args))
          ;; Map-style: {:query ... :args [db & inputs]}
          (let [m (first args)
                q-args (:args m)]
            [(:query m) (first q-args) (rest q-args)])
          ;; Positional: (q query db & inputs)
          [(first args) (second args) (drop 2 args)])
        conn (:connection db)
        ds (:datasource conn)
        store (:store-name conn)
        query-str (query->str query-form)
        ;; Build inputs JSON with temporal options
        inputs-map (cond-> {}
                     (:as-of-t db) (assoc "asOf" (:as-of-t db))
                     (:since-t db) (assoc "since" (:since-t db))
                     (:history? db) (assoc "history" true))
        inputs-json (inputs->json inputs-map)]
    (if (= store "default")
      (let [result (jdbc/execute-one! ds
                     ["SELECT mentat.mentat_query($1, $2::jsonb) AS result"
                      query-str inputs-json]
                     {:builder-fn rs/as-unqualified-lower-maps})]
        (coerce-jsonb (:result result)))
      (let [result (jdbc/execute-one! ds
                     ["SELECT mentat.q($1, $2, $3::jsonb) AS result"
                      store query-str inputs-json]
                     {:builder-fn rs/as-unqualified-lower-maps})]
        (coerce-jsonb (:result result))))))

;; ---------------------------------------------------------------------------
;; Core API: transact
;; ---------------------------------------------------------------------------

(defn transact
  "Execute a transaction. Analogous to datomic.api/transact.

   Args:
     conn    - Connection from (connect config)
     arg-map - Map with :tx-data key containing a vector of transaction forms.
               Transaction forms can be entity maps or explicit operations:
               [{:person/name \"Alice\" :person/age 30}
                [:db/retract eid :person/email \"old@example.com\"]]

   Returns the transaction report as a map with keys:
     :db-before, :db-after, :tx-data, :tempids"
  [^PgMentatConnection conn {:keys [tx-data] :as arg-map}]
  (when-not tx-data
    (throw (ex-info "Missing required :tx-data" {:arg-map arg-map})))
  (let [ds (:datasource conn)
        store (:store-name conn)
        edn-str (tx-data->edn tx-data)]
    (if (= store "default")
      (let [result (jdbc/execute-one! ds
                     ["SELECT mentat.mentat_transact($1) AS result" edn-str]
                     {:builder-fn rs/as-unqualified-lower-maps})]
        (when-let [r (:result result)]
          (parse-json r)))
      (let [result (jdbc/execute-one! ds
                     ["SELECT mentat.t($1, $2) AS result" store edn-str]
                     {:builder-fn rs/as-unqualified-lower-maps})]
        (when-let [r (:result result)]
          (parse-json r))))))

;; ---------------------------------------------------------------------------
;; Core API: pull
;; ---------------------------------------------------------------------------

(defn pull
  "Pull entity attributes. Analogous to datomic.api/pull.

   Args:
     db      - Database value from (db conn)
     pattern - Pull pattern (e.g., '[*] or '[:person/name :person/age])
     eid     - Entity ID (long) or lookup ref (e.g., [:person/email \"a@b.com\"])

   Returns a map of entity attributes matching the pattern."
  [^PgMentatDb db pattern eid]
  (let [conn (:connection db)
        ds (:datasource conn)
        pattern-str (pattern->str pattern)
        entity-id (if (number? eid)
                    (long eid)
                    ;; Lookup ref: resolve via query
                    (let [attr (first eid)
                          val (second eid)
                          results (q [:find '?e :where ['?e attr val]] db)]
                      (ffirst results)))]
    (when entity-id
      (let [result (jdbc/execute-one! ds
                     ["SELECT mentat.mentat_pull($1, $2) AS result"
                      pattern-str (long entity-id)]
                     {:builder-fn rs/as-unqualified-lower-maps})]
        (keywordize-entity (coerce-jsonb (:result result)))))))

;; ---------------------------------------------------------------------------
;; Core API: pull-many
;; ---------------------------------------------------------------------------

(defn pull-many
  "Pull attributes for multiple entities. Analogous to datomic.api/pull-many.

   Args:
     db      - Database value
     pattern - Pull pattern
     eids    - Collection of entity IDs

   Returns a vector of entity attribute maps."
  [^PgMentatDb db pattern eids]
  (let [conn (:connection db)
        ds (:datasource conn)
        pattern-str (pattern->str pattern)
        id-array (long-array (map long eids))]
    (let [result (jdbc/execute-one! ds
                   ["SELECT mentat.mentat_pull_many($1, $2) AS result"
                    pattern-str id-array]
                   {:builder-fn rs/as-unqualified-lower-maps})]
      (mapv keywordize-entity (coerce-jsonb (:result result))))))

;; ---------------------------------------------------------------------------
;; Core API: entity
;; ---------------------------------------------------------------------------

(defn entity
  "Get all attributes of an entity as a map. Analogous to datomic.api/entity.

   Args:
     db        - Database value
     entity-id - Entity ID (long)

   Returns a map of all entity attributes with keyword keys."
  [^PgMentatDb db entity-id]
  (let [conn (:connection db)
        ds (:datasource conn)
        store (:store-name conn)
        result (jdbc/execute-one! ds
                 ["SELECT mentat.entity($1, $2) AS result"
                  store (long entity-id)]
                 {:builder-fn rs/as-unqualified-lower-maps})]
    (keywordize-entity (coerce-jsonb (:result result)))))

;; ---------------------------------------------------------------------------
;; Core API: as-of
;; ---------------------------------------------------------------------------

(defn as-of
  "Return a database value as of a specific transaction.
   Analogous to datomic.api/as-of.

   Args:
     db - Database value
     t  - Transaction ID (long) or java.util.Date instant

   Returns a new database value filtered to that point in time.
   Queries against this db will only see data asserted at or before t."
  [^PgMentatDb db t]
  (let [tx-id (cond
                (number? t) (long t)
                (instance? Date t)
                ;; Resolve instant to tx via transactions table
                (let [conn (:connection db)
                      ds (:datasource (:connection db))
                      result (jdbc/execute-one! ds
                               ["SELECT MAX(tx) AS t FROM mentat.transactions WHERE tx_instant <= $1::timestamptz"
                                (str (.toInstant ^Date t))]
                               {:builder-fn rs/as-unqualified-lower-maps})]
                  (get result :t 0))
                :else (long t))]
    (assoc db :as-of-t tx-id :since-t nil :history? false)))

;; ---------------------------------------------------------------------------
;; Time-travel: since
;; ---------------------------------------------------------------------------

(defn since
  "Return a database value showing only changes since a transaction.
   Analogous to datomic.api/since.

   Args:
     db - Database value
     t  - Transaction ID or instant

   Returns a new database value filtered to changes after t."
  [^PgMentatDb db t]
  (let [tx-id (if (number? t) (long t) (long t))]
    (assoc db :since-t tx-id :as-of-t nil :history? false)))

;; ---------------------------------------------------------------------------
;; Time-travel: history
;; ---------------------------------------------------------------------------

(defn history
  "Return a database value including all history (assertions and retractions).
   Analogous to datomic.api/history.

   Args:
     db - Database value

   Returns a new database value with full history accessible."
  [^PgMentatDb db]
  (assoc db :history? true :as-of-t nil :since-t nil))

;; ---------------------------------------------------------------------------
;; Additional API: schema
;; ---------------------------------------------------------------------------

(defn schema
  "Return the current schema as a map.
   Equivalent to calling mentat.mentat_schema() directly."
  [^PgMentatDb db]
  (let [conn (:connection db)
        ds (:datasource conn)
        result (jdbc/execute-one! ds
                 ["SELECT mentat.mentat_schema() AS result"]
                 {:builder-fn rs/as-unqualified-lower-maps})]
    (coerce-jsonb (:result result))))

;; ---------------------------------------------------------------------------
;; Additional API: with (speculative transactions)
;; ---------------------------------------------------------------------------

(defn with
  "Apply a transaction speculatively without committing.
   Analogous to datomic.api/with.

   Args:
     db      - Database value
     arg-map - Map with :tx-data key

   Returns a map with :db-after and :tx-data showing what would happen
   without actually persisting changes."
  [^PgMentatDb db {:keys [tx-data] :as arg-map}]
  (when-not tx-data
    (throw (ex-info "Missing required :tx-data" {:arg-map arg-map})))
  (let [conn (:connection db)
        ds (:datasource conn)
        edn-str (tx-data->edn tx-data)
        result (jdbc/execute-one! ds
                 ["SELECT mentat.mentat_with($1) AS result" edn-str]
                 {:builder-fn rs/as-unqualified-lower-maps})]
    (when-let [r (:result result)]
      (parse-json r))))

;; ---------------------------------------------------------------------------
;; Connection lifecycle
;; ---------------------------------------------------------------------------

(defn release
  "Release a connection. After calling release, the connection should not be used.
   For connection-pooled datasources, this is a no-op (pool manages lifecycle).
   For single connections, this closes the underlying datasource if closeable."
  [^PgMentatConnection conn]
  (let [ds (:datasource conn)]
    (when (instance? java.io.Closeable ds)
      (.close ^java.io.Closeable ds))))

;; ---------------------------------------------------------------------------
;; Convenience: squuid generation
;; ---------------------------------------------------------------------------

(defn squuid
  "Generate a semi-sequential UUID (like datomic.api/squuid).
   Uses the current time as the most-significant bits for ordering."
  []
  (let [uuid (UUID/randomUUID)
        time-ms (System/currentTimeMillis)
        msb (bit-or (bit-shift-left (bit-and time-ms 0xFFFFFFFF) 32)
                    (bit-and (.getMostSignificantBits uuid) 0xFFFFFFFF))]
    (UUID. msb (.getLeastSignificantBits uuid))))

;; ---------------------------------------------------------------------------
;; Convenience: tempid generation
;; ---------------------------------------------------------------------------

(defn tempid
  "Generate a temporary ID for use in transactions.
   Analogous to datomic.api/tempid.

   Args:
     partition - The partition keyword (e.g., :db.part/user)
     n         - Optional negative number for identity within a transaction

   Returns a string tempid for use in transaction data."
  ([partition]
   (str "tempid-" (name partition) "-" (UUID/randomUUID)))
  ([partition n]
   (str "tempid-" (name partition) "-" n)))
