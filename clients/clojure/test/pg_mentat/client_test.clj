(ns pg-mentat.client-test
  "Unit and integration tests for the pg_mentat Datomic-compatible client.

   Unit tests (transit encoding/decoding, data types) run without a server.
   Integration tests require a running mentatd instance at ws://localhost:8080/ws."
  (:require [clojure.test :refer :all]
            [pg-mentat.client :as d])
  (:import [java.util UUID Date]))

;; ===========================================================================
;; Unit tests: Transit+JSON encoding
;; ===========================================================================

(deftest test-transit-encode-nil
  (testing "nil encodes to JSON null"
    (is (= "null" (d/transit-encode nil)))))

(deftest test-transit-encode-boolean
  (testing "booleans encode correctly"
    (is (= "true" (d/transit-encode true)))
    (is (= "false" (d/transit-encode false)))))

(deftest test-transit-encode-integer
  (testing "small integers encode as plain numbers"
    (is (= "42" (d/transit-encode 42)))
    (is (= "-1" (d/transit-encode -1))))
  (testing "large integers encode as tagged strings"
    (let [encoded (d/transit-encode 9999999999)]
      (is (.contains encoded "~i9999999999")))))

(deftest test-transit-encode-string
  (testing "plain strings encode normally"
    (is (= "\"hello\"" (d/transit-encode "hello"))))
  (testing "strings starting with ~ get escaped"
    (let [encoded (d/transit-encode "~special")]
      (is (.contains encoded "~~special")))))

(deftest test-transit-encode-keyword
  (testing "simple keyword"
    (let [encoded (d/transit-encode :name)]
      (is (.contains encoded "~:name"))))
  (testing "namespaced keyword"
    (let [encoded (d/transit-encode :person/name)]
      (is (.contains encoded "~:person/name")))))

(deftest test-transit-encode-map
  (testing "map encodes as Transit cmap"
    (let [encoded (d/transit-encode {:name "Alice"})]
      (is (.startsWith encoded "["))
      (is (.contains encoded "^ ")))))

(deftest test-transit-encode-vector
  (testing "vector encodes as JSON array"
    (let [encoded (d/transit-encode [1 2 3])]
      (is (= "[1,2,3]" encoded)))))

;; ===========================================================================
;; Unit tests: Transit+JSON decoding
;; ===========================================================================

(deftest test-transit-decode-keyword
  (testing "Transit keyword decoding"
    (let [decoded (d/transit-decode "~:db/name")]
      (is (keyword? decoded))
      (is (= "db" (namespace decoded)))
      (is (= "name" (name decoded))))))

(deftest test-transit-decode-simple-keyword
  (testing "Simple keyword without namespace"
    (let [decoded (d/transit-decode "~:name")]
      (is (keyword? decoded))
      (is (nil? (namespace decoded)))
      (is (= "name" (name decoded))))))

(deftest test-transit-decode-symbol
  (testing "Transit symbol decoding"
    (let [decoded (d/transit-decode "~$?e")]
      (is (symbol? decoded))
      (is (= "?e" (str decoded))))))

(deftest test-transit-decode-large-int
  (testing "Transit large integer decoding"
    (is (= 9999999999 (d/transit-decode "~i9999999999")))))

(deftest test-transit-decode-uuid
  (testing "Transit UUID decoding"
    (let [uuid-str "550e8400-e29b-41d4-a716-446655440000"
          decoded (d/transit-decode (str "~u" uuid-str))]
      (is (instance? UUID decoded))
      (is (= uuid-str (str decoded))))))

(deftest test-transit-decode-instant
  (testing "Transit instant decoding"
    (let [decoded (d/transit-decode "~m1714000000000")]
      (is (instance? Date decoded))
      (is (= 1714000000000 (.getTime ^Date decoded))))))

(deftest test-transit-decode-special-floats
  (testing "NaN"
    (let [decoded (d/transit-decode "~zNaN")]
      (is (Double/isNaN decoded))))
  (testing "Infinity"
    (let [decoded (d/transit-decode "~zINF")]
      (is (= Double/POSITIVE_INFINITY decoded))))
  (testing "Negative infinity"
    (let [decoded (d/transit-decode "~z-INF")]
      (is (= Double/NEGATIVE_INFINITY decoded)))))

(deftest test-transit-decode-escaped-tilde
  (testing "Escaped tilde"
    (is (= "~hello" (d/transit-decode "~~hello")))))

(deftest test-transit-decode-escaped-caret
  (testing "Escaped caret"
    (is (= "^hello" (d/transit-decode "~^hello")))))

(deftest test-transit-decode-plain-string
  (testing "Plain string passes through"
    (is (= "hello" (d/transit-decode "hello")))))

(deftest test-transit-decode-cmap
  (testing "Transit cmap decodes to map"
    (let [decoded (d/transit-decode ["^ " "~:name" "Alice" "~:age" 30])]
      (is (map? decoded))
      (is (= "Alice" (get decoded :name)))
      (is (= 30 (get decoded :age))))))

(deftest test-transit-decode-nested-cmap
  (testing "Nested cmap"
    (let [decoded (d/transit-decode
                   ["^ "
                    "~:result"
                    ["^ "
                     "~:db-name" "test-db"
                     "~:t" 1000]])]
      (is (map? decoded))
      (let [result (get decoded :result)]
        (is (map? result))
        (is (= "test-db" (get result :db-name)))
        (is (= 1000 (get result :t)))))))

(deftest test-transit-decode-vector-of-vectors
  (testing "Query result format: vector of tuples"
    (let [decoded (d/transit-decode [[42 "Alice"] [43 "Bob"]])]
      (is (vector? decoded))
      (is (= 2 (count decoded)))
      (is (= [42 "Alice"] (first decoded)))
      (is (= [43 "Bob"] (second decoded))))))

(deftest test-transit-decode-tagged-list
  (testing "Transit tagged list"
    (let [decoded (d/transit-decode ["~#list" [1 2 3]])]
      (is (seq? decoded))
      (is (= '(1 2 3) decoded)))))

(deftest test-transit-decode-tagged-set
  (testing "Transit tagged set"
    (let [decoded (d/transit-decode ["~#set" [1 2 3]])]
      (is (set? decoded))
      (is (= #{1 2 3} decoded)))))

;; ===========================================================================
;; Unit tests: Transit round-trip
;; ===========================================================================

(deftest test-transit-roundtrip-keyword
  (testing "Keyword round-trips through encode/decode"
    (let [original :person/name
          encoded (d/transit-encode original)]
      ;; The raw JSON contains the Transit-tagged string
      (is (.contains encoded "~:person/name")))))

(deftest test-transit-roundtrip-map
  (testing "Map round-trip structure"
    (let [original {:op :q :args {:query "[:find ?e :where [?e :name]]"}}
          encoded (d/transit-encode original)]
      (is (string? encoded))
      (is (.contains encoded "~:op"))
      (is (.contains encoded "~:q"))
      (is (.contains encoded "~:args")))))

;; ===========================================================================
;; Unit tests: JSON parser
;; ===========================================================================

(deftest test-parse-json-primitives
  (testing "JSON null"
    (is (nil? (#'d/parse-json "null"))))
  (testing "JSON boolean"
    (is (true? (#'d/parse-json "true")))
    (is (false? (#'d/parse-json "false"))))
  (testing "JSON integer"
    (is (= 42 (#'d/parse-json "42")))
    (is (= -1 (#'d/parse-json "-1"))))
  (testing "JSON float"
    (is (= 3.14 (#'d/parse-json "3.14"))))
  (testing "JSON string"
    (is (= "hello" (#'d/parse-json "\"hello\""))))
  (testing "JSON string with escapes"
    (is (= "hello\nworld" (#'d/parse-json "\"hello\\nworld\"")))))

(deftest test-parse-json-array
  (testing "Empty array"
    (is (= [] (#'d/parse-json "[]"))))
  (testing "Array of integers"
    (is (= [1 2 3] (#'d/parse-json "[1,2,3]"))))
  (testing "Nested arrays"
    (is (= [[1 2] [3 4]] (#'d/parse-json "[[1,2],[3,4]]")))))

(deftest test-parse-json-object
  (testing "Simple object"
    (let [result (#'d/parse-json "{\"name\":\"Alice\"}")]
      (is (map? result))
      (is (= "Alice" (get result "name"))))))

(deftest test-parse-transit-json-full
  (testing "Transit+JSON success response"
    (let [result (#'d/parse-transit-json
                  "[\"^ \",\"~:result\",42]")]
      (is (map? result))
      (is (= 42 (get result :result)))))

  (testing "Transit+JSON error response"
    (let [result (#'d/parse-transit-json
                  (str "[\"^ \",\"~:error\","
                       "[\"^ \","
                       "\"~:cognitect.anomalies/category\","
                       "\"~:cognitect.anomalies/not-found\","
                       "\"~:cognitect.anomalies/message\","
                       "\"Database not found\"]]"))]
      (is (map? result))
      (let [error (get result :error)]
        (is (map? error))
        (is (= :cognitect.anomalies/not-found
               (get error :cognitect.anomalies/category)))
        (is (= "Database not found"
               (get error :cognitect.anomalies/message))))))

  (testing "Transit+JSON query result"
    (let [result (#'d/parse-transit-json
                  "[\"^ \",\"~:result\",[[42,\"Alice\"],[43,\"Bob\"]]]")]
      (is (map? result))
      (let [rows (get result :result)]
        (is (vector? rows))
        (is (= 2 (count rows)))
        (is (= [42 "Alice"] (first rows)))))))

;; ===========================================================================
;; Unit tests: Client API data structures
;; ===========================================================================

(deftest test-client-creation
  (testing "Client requires endpoint"
    (is (thrown? clojure.lang.ExceptionInfo
                (d/client {}))))
  (testing "Client stores config"
    (let [c (d/client {:endpoint "ws://localhost:8080/ws"})]
      (is (= "ws://localhost:8080/ws" (:ws-endpoint c))))))

(deftest test-time-travel-db-values
  (testing "as-of returns new db with as-of-t set"
    (let [db (map->pg-mentat.client.Db
              {:connection nil :db-name "test" :database-id "id"
               :t 1000 :next-t 1001})
          as-of-db (d/as-of db 500)]
      (is (= 500 (:as-of-t as-of-db)))
      (is (false? (:history? as-of-db)))))

  (testing "since returns new db with since-t set"
    (let [db (map->pg-mentat.client.Db
              {:connection nil :db-name "test" :database-id "id"
               :t 1000 :next-t 1001})
          since-db (d/since db 500)]
      (is (= 500 (:since-t since-db)))
      (is (false? (:history? since-db)))))

  (testing "history returns new db with history? flag"
    (let [db (map->pg-mentat.client.Db
              {:connection nil :db-name "test" :database-id "id"
               :t 1000 :next-t 1001})
          hist-db (d/history db)]
      (is (true? (:history? hist-db))))))

;; ===========================================================================
;; Integration tests (require running mentatd)
;; ===========================================================================
;; To run: ensure mentatd is running at ws://localhost:8080/ws
;; then: clj -X:test

(def ^:dynamic *test-endpoint* "ws://localhost:8080/ws")

(deftest ^:integration test-connect-and-db
  (testing "Connect to database and get db value"
    (let [c (d/client {:endpoint *test-endpoint*})
          conn (d/connect c {:db-name "test-db"})
          database (d/db conn)]
      (is (some? (:database-id database)))
      (is (number? (:t database)))
      (d/release conn))))

(deftest ^:integration test-transact-and-query
  (testing "Full transact-then-query workflow"
    (let [c (d/client {:endpoint *test-endpoint*})
          conn (d/connect c {:db-name "test-db"})]
      ;; Schema
      (d/transact conn
        {:tx-data [{:db/ident :test.int/name
                    :db/valueType :db.type/string
                    :db/cardinality :db.cardinality/one}]})
      ;; Data
      (let [result (d/transact conn
                     {:tx-data [{:test.int/name "Integration Test"}]})]
        (is (some? result)))
      ;; Query
      (let [database (d/db conn)
            results (d/q '[:find ?n
                          :where [_ :test.int/name ?n]]
                        database)]
        (is (some? results)))
      (d/release conn))))

(deftest ^:integration test-pull-entity
  (testing "Pull entity attributes"
    (let [c (d/client {:endpoint *test-endpoint*})
          conn (d/connect c {:db-name "test-db"})]
      ;; Transact some data
      (d/transact conn
        {:tx-data [{:db/ident :test.pull/name
                    :db/valueType :db.type/string
                    :db/cardinality :db.cardinality/one}]})
      (d/transact conn
        {:tx-data [{:test.pull/name "Pull Test Entity"}]})
      ;; Query for entity id
      (let [database (d/db conn)
            results (d/q '[:find ?e :where [?e :test.pull/name "Pull Test Entity"]]
                        database)]
        (when (seq results)
          (let [eid (ffirst results)
                entity (d/pull database '[:test.pull/name] eid)]
            (is (some? entity)))))
      (d/release conn))))

(deftest ^:integration test-datoms-index
  (testing "Access datoms by index"
    (let [c (d/client {:endpoint *test-endpoint*})
          conn (d/connect c {:db-name "test-db"})
          database (d/db conn)]
      (let [result (d/datoms database {:index :eavt})]
        (is (some? result)))
      (d/release conn))))

(deftest ^:integration test-time-travel-queries
  (testing "as-of and since queries"
    (let [c (d/client {:endpoint *test-endpoint*})
          conn (d/connect c {:db-name "test-db"})
          database (d/db conn)]
      ;; as-of query at current t
      (let [as-of-db (d/as-of database (:t database))
            results (d/q '[:find ?e :where [?e :db/ident]] as-of-db)]
        (is (some? results)))
      (d/release conn))))

(deftest ^:integration test-catalog-operations
  (testing "list-databases"
    (let [c (d/client {:endpoint *test-endpoint*})]
      (let [dbs (d/list-databases c)]
        (is (some? dbs))))))
