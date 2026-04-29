(ns pg-mentat.client-test
  "Comprehensive tests for the pg_mentat Clojure peer library.

   Unit tests (JSON parsing, EDN serialization, data types) run without a database.
   Integration tests require a PostgreSQL instance with the pg_mentat extension."
  (:require [clojure.test :refer :all]
            [pg-mentat.client :as d])
  (:import [java.util UUID Date]))

;; ===========================================================================
;; Unit tests: JSON parser
;; ===========================================================================

(deftest test-parse-json-primitives
  (testing "JSON null"
    (is (nil? (#'d/parse-json "null"))))
  (testing "JSON booleans"
    (is (true? (#'d/parse-json "true")))
    (is (false? (#'d/parse-json "false"))))
  (testing "JSON integers"
    (is (= 42 (#'d/parse-json "42")))
    (is (= -1 (#'d/parse-json "-1")))
    (is (= 0 (#'d/parse-json "0"))))
  (testing "JSON floats"
    (is (= 3.14 (#'d/parse-json "3.14")))
    (is (= -0.5 (#'d/parse-json "-0.5")))
    (is (= 1.0e10 (#'d/parse-json "1.0e10"))))
  (testing "JSON strings"
    (is (= "hello" (#'d/parse-json "\"hello\"")))
    (is (= "" (#'d/parse-json "\"\"")))
    (is (= "hello\nworld" (#'d/parse-json "\"hello\\nworld\"")))
    (is (= "tab\there" (#'d/parse-json "\"tab\\there\"")))
    (is (= "quote\"mark" (#'d/parse-json "\"quote\\\"mark\"")))))

(deftest test-parse-json-arrays
  (testing "empty array"
    (is (= [] (#'d/parse-json "[]"))))
  (testing "array of integers"
    (is (= [1 2 3] (#'d/parse-json "[1,2,3]"))))
  (testing "array with whitespace"
    (is (= [1 2 3] (#'d/parse-json "[ 1 , 2 , 3 ]"))))
  (testing "nested arrays"
    (is (= [[1 2] [3 4]] (#'d/parse-json "[[1,2],[3,4]]"))))
  (testing "mixed types"
    (is (= [1 "hello" true nil] (#'d/parse-json "[1,\"hello\",true,null]")))))

(deftest test-parse-json-objects
  (testing "empty object"
    (is (= {} (#'d/parse-json "{}"))))
  (testing "simple object"
    (let [result (#'d/parse-json "{\"name\":\"Alice\",\"age\":30}")]
      (is (map? result))
      (is (= "Alice" (get result "name")))
      (is (= 30 (get result "age")))))
  (testing "nested object"
    (let [result (#'d/parse-json "{\"person\":{\"name\":\"Bob\"},\"scores\":[1,2,3]}")]
      (is (= "Bob" (get-in result ["person" "name"])))
      (is (= [1 2 3] (get result "scores"))))))

;; ===========================================================================
;; Unit tests: JSON serialization
;; ===========================================================================

(deftest test-json-str-primitives
  (testing "nil serializes to null"
    (is (= "null" (#'d/json-str nil))))
  (testing "booleans"
    (is (= "true" (#'d/json-str true)))
    (is (= "false" (#'d/json-str false))))
  (testing "numbers"
    (is (= "42" (#'d/json-str 42)))
    (is (= "3.14" (#'d/json-str 3.14))))
  (testing "strings"
    (is (= "\"hello\"" (#'d/json-str "hello")))
    (is (= "\"with\\\"quote\"" (#'d/json-str "with\"quote")))
    (is (= "\"with\\nnewline\"" (#'d/json-str "with\nnewline")))))

(deftest test-json-str-collections
  (testing "vector"
    (is (= "[1,2,3]" (#'d/json-str [1 2 3]))))
  (testing "empty vector"
    (is (= "[]" (#'d/json-str []))))
  (testing "map"
    (let [result (#'d/json-str {"name" "Alice"})]
      (is (.contains result "\"name\""))
      (is (.contains result "\"Alice\"")))))

;; ===========================================================================
;; Unit tests: EDN serialization helpers
;; ===========================================================================

(deftest test-tx-data-to-edn
  (testing "entity map serialization"
    (let [edn (#'d/tx-data->edn [{:person/name "Alice" :person/age 30}])]
      (is (string? edn))
      (is (.contains edn ":person/name"))
      (is (.contains edn "Alice"))))
  (testing "explicit operation serialization"
    (let [edn (#'d/tx-data->edn [[:db/add "tempid" :person/name "Bob"]])]
      (is (string? edn))
      (is (.contains edn ":db/add"))
      (is (.contains edn "Bob"))))
  (testing "mixed transaction data"
    (let [edn (#'d/tx-data->edn [{:person/name "Alice"}
                                   [:db/retract 42 :person/email "old@test.com"]])]
      (is (string? edn))
      (is (.contains edn ":person/name"))
      (is (.contains edn ":db/retract")))))

(deftest test-pattern-to-str
  (testing "string pattern passes through"
    (is (= "[*]" (#'d/pattern->str "[*]"))))
  (testing "vector pattern is pr-str'd"
    (let [s (#'d/pattern->str ['*])]
      (is (string? s))
      (is (.contains s "*"))))
  (testing "specific attributes"
    (let [s (#'d/pattern->str [:person/name :person/age])]
      (is (.contains s ":person/name"))
      (is (.contains s ":person/age")))))

(deftest test-query-to-str
  (testing "string query passes through"
    (is (= "[:find ?e :where [?e :db/ident]]"
           (#'d/query->str "[:find ?e :where [?e :db/ident]]"))))
  (testing "vector query is pr-str'd"
    (let [s (#'d/query->str '[:find ?e :where [?e :db/ident]])]
      (is (string? s))
      (is (.contains s ":find"))
      (is (.contains s "?e")))))

(deftest test-inputs-to-json
  (testing "nil inputs"
    (is (= "{}" (#'d/inputs->json nil))))
  (testing "empty map"
    (is (= "{}" (#'d/inputs->json {}))))
  (testing "with as-of"
    (let [j (#'d/inputs->json {"asOf" 1000042})]
      (is (.contains j "asOf"))
      (is (.contains j "1000042")))))

;; ===========================================================================
;; Unit tests: JSONB coercion
;; ===========================================================================

(deftest test-coerce-jsonb-nil
  (testing "nil returns nil"
    (is (nil? (#'d/coerce-jsonb nil)))))

(deftest test-coerce-jsonb-string
  (testing "JSON string is parsed"
    (is (= {"name" "Alice"} (#'d/coerce-jsonb "{\"name\":\"Alice\"}")))))

(deftest test-coerce-jsonb-pgobject
  (testing "PGobject is unwrapped and parsed"
    (let [pg (doto (org.postgresql.util.PGobject.)
               (.setType "jsonb")
               (.setValue "{\"t\":42}"))]
      (is (= {"t" 42} (#'d/coerce-jsonb pg))))))

;; ===========================================================================
;; Unit tests: keywordize-entity
;; ===========================================================================

(deftest test-keywordize-entity
  (testing "nil returns nil"
    (is (nil? (#'d/keywordize-entity nil))))
  (testing "basic entity map"
    (let [result (#'d/keywordize-entity {":person/name" "Alice"
                                          ":person/age" 30
                                          ":db/id" 42})]
      (is (= "Alice" (:person/name result)))
      (is (= 30 (:person/age result)))
      (is (= 42 (:db/id result)))))
  (testing "simple keyword (no namespace)"
    (let [result (#'d/keywordize-entity {":name" "test"})]
      (is (= "test" (:name result))))))

;; ===========================================================================
;; Unit tests: Connection creation
;; ===========================================================================

(deftest test-connect-requires-pg-spec
  (testing "connect throws without :pg"
    (is (thrown? clojure.lang.ExceptionInfo
                (d/connect {})))))

(deftest test-connect-creates-connection
  (testing "connect returns PgMentatConnection"
    (let [conn (d/connect {:pg {:dbtype "postgresql"
                                :host "localhost"
                                :dbname "postgres"
                                :user "postgres"}})]
      (is (instance? pg_mentat.client.PgMentatConnection conn))
      (is (some? (:datasource conn)))
      (is (= "default" (:store-name conn))))))

(deftest test-connect-with-store-name
  (testing "connect accepts custom store name"
    (let [conn (d/connect {:pg {:dbtype "postgresql"
                                :host "localhost"
                                :dbname "postgres"}
                           :store-name "my-store"})]
      (is (= "my-store" (:store-name conn))))))

;; ===========================================================================
;; Unit tests: Time-travel database values
;; ===========================================================================

(deftest test-as-of-db-value
  (testing "as-of returns db with as-of-t set"
    (let [db (d/map->PgMentatDb {:connection nil :basis-t 1000
                                  :as-of-t nil :since-t nil :history? false})
          as-of-db (d/as-of db 500)]
      (is (= 500 (:as-of-t as-of-db)))
      (is (nil? (:since-t as-of-db)))
      (is (false? (:history? as-of-db)))))
  (testing "as-of clears since-t"
    (let [db (d/map->PgMentatDb {:connection nil :basis-t 1000
                                  :as-of-t nil :since-t 200 :history? false})
          as-of-db (d/as-of db 500)]
      (is (= 500 (:as-of-t as-of-db)))
      (is (nil? (:since-t as-of-db))))))

(deftest test-since-db-value
  (testing "since returns db with since-t set"
    (let [db (d/map->PgMentatDb {:connection nil :basis-t 1000
                                  :as-of-t nil :since-t nil :history? false})
          since-db (d/since db 500)]
      (is (= 500 (:since-t since-db)))
      (is (nil? (:as-of-t since-db)))
      (is (false? (:history? since-db))))))

(deftest test-history-db-value
  (testing "history returns db with history? flag"
    (let [db (d/map->PgMentatDb {:connection nil :basis-t 1000
                                  :as-of-t nil :since-t nil :history? false})
          hist-db (d/history db)]
      (is (true? (:history? hist-db)))
      (is (nil? (:as-of-t hist-db)))
      (is (nil? (:since-t hist-db))))))

;; ===========================================================================
;; Unit tests: squuid and tempid
;; ===========================================================================

(deftest test-squuid
  (testing "squuid returns a UUID"
    (let [id (d/squuid)]
      (is (instance? UUID id))))
  (testing "squuids are roughly time-ordered"
    (let [id1 (d/squuid)
          _ (Thread/sleep 10)
          id2 (d/squuid)]
      (is (neg? (compare (str id1) (str id2)))))))

(deftest test-tempid
  (testing "tempid with partition"
    (let [id (d/tempid :db.part/user)]
      (is (string? id))
      (is (.startsWith id "tempid-user-"))))
  (testing "tempid with partition and number"
    (let [id (d/tempid :db.part/user -1)]
      (is (= "tempid-user--1" id)))))

;; ===========================================================================
;; Integration tests (require PostgreSQL with pg_mentat extension)
;; ===========================================================================
;; To run integration tests:
;;   1. Ensure PostgreSQL is running with the pg_mentat extension loaded
;;   2. Set PG_MENTAT_TEST_DB env var if not using defaults
;;   3. Run: clj -X:test :kaocha.filter/focus-meta :integration

(def ^:dynamic *test-db-spec*
  "Default db-spec for integration tests.
   Override via PG_MENTAT_TEST_DB environment variable."
  {:dbtype "postgresql"
   :host (or (System/getenv "PG_MENTAT_HOST") "localhost")
   :port (Integer/parseInt (or (System/getenv "PG_MENTAT_PORT") "5432"))
   :dbname (or (System/getenv "PG_MENTAT_DBNAME") "postgres")
   :user (or (System/getenv "PG_MENTAT_USER") "postgres")
   :password (System/getenv "PG_MENTAT_PASSWORD")})

(defn- test-conn
  "Create a test connection using the configured db-spec."
  []
  (d/connect {:pg *test-db-spec*}))

(deftest ^:integration test-connect-and-db
  (testing "connect and get database value"
    (let [conn (test-conn)
          database (d/db conn)]
      (is (instance? pg_mentat.client.PgMentatDb database))
      (is (number? (:basis-t database)))
      (is (>= (:basis-t database) 0))
      (d/release conn))))

(deftest ^:integration test-schema-query
  (testing "retrieve current schema"
    (let [conn (test-conn)
          database (d/db conn)
          s (d/schema database)]
      (is (some? s))
      (d/release conn))))

(deftest ^:integration test-transact-schema
  (testing "transact schema attributes"
    (let [conn (test-conn)]
      (let [result (d/transact conn
                     {:tx-data [{:db/ident :test.clj/name
                                 :db/valueType :db.type/string
                                 :db/cardinality :db.cardinality/one}
                                {:db/ident :test.clj/age
                                 :db/valueType :db.type/long
                                 :db/cardinality :db.cardinality/one}]})]
        (is (some? result))
        (is (map? result)))
      (d/release conn))))

(deftest ^:integration test-transact-and-query
  (testing "full transact-then-query workflow"
    (let [conn (test-conn)]
      ;; Schema
      (d/transact conn
        {:tx-data [{:db/ident :test.clj.taq/name
                    :db/valueType :db.type/string
                    :db/cardinality :db.cardinality/one}]})
      ;; Data
      (let [tx-result (d/transact conn
                        {:tx-data [{:test.clj.taq/name "Alice"}
                                   {:test.clj.taq/name "Bob"}]})]
        (is (some? tx-result)))
      ;; Query
      (let [database (d/db conn)
            results (d/q '[:find ?name
                           :where [_ :test.clj.taq/name ?name]]
                         database)]
        (is (some? results)))
      (d/release conn))))

(deftest ^:integration test-pull-entity
  (testing "pull entity attributes after transact"
    (let [conn (test-conn)]
      (d/transact conn
        {:tx-data [{:db/ident :test.clj.pull/name
                    :db/valueType :db.type/string
                    :db/cardinality :db.cardinality/one}]})
      (d/transact conn
        {:tx-data [{:test.clj.pull/name "PullTest"}]})
      (let [database (d/db conn)
            results (d/q '[:find ?e
                           :where [?e :test.clj.pull/name "PullTest"]]
                         database)]
        (when (seq results)
          (let [eid (ffirst results)
                pulled (d/pull database [:test.clj.pull/name] eid)]
            (is (some? pulled))
            (is (= "PullTest" (:test.clj.pull/name pulled))))))
      (d/release conn))))

(deftest ^:integration test-entity-lookup
  (testing "entity returns all attributes"
    (let [conn (test-conn)]
      (d/transact conn
        {:tx-data [{:db/ident :test.clj.ent/name
                    :db/valueType :db.type/string
                    :db/cardinality :db.cardinality/one}
                   {:db/ident :test.clj.ent/score
                    :db/valueType :db.type/long
                    :db/cardinality :db.cardinality/one}]})
      (d/transact conn
        {:tx-data [{:test.clj.ent/name "EntityTest"
                    :test.clj.ent/score 99}]})
      (let [database (d/db conn)
            results (d/q '[:find ?e
                           :where [?e :test.clj.ent/name "EntityTest"]]
                         database)]
        (when (seq results)
          (let [eid (ffirst results)
                ent (d/entity database eid)]
            (is (some? ent))
            (is (= eid (:db/id ent))))))
      (d/release conn))))

(deftest ^:integration test-as-of-query
  (testing "as-of returns data at a specific transaction"
    (let [conn (test-conn)]
      (d/transact conn
        {:tx-data [{:db/ident :test.clj.asof/val
                    :db/valueType :db.type/string
                    :db/cardinality :db.cardinality/one}]})
      ;; Transact initial value
      (d/transact conn {:tx-data [{:test.clj.asof/val "v1"}]})
      (let [db-v1 (d/db conn)
            t1 (:basis-t db-v1)]
        ;; Transact second value
        (d/transact conn {:tx-data [{:test.clj.asof/val "v2"}]})
        ;; Query as-of t1 should still see v1
        (let [as-of-db (d/as-of db-v1 t1)]
          (is (= t1 (:as-of-t as-of-db)))))
      (d/release conn))))

(deftest ^:integration test-speculative-with
  (testing "with applies transaction speculatively"
    (let [conn (test-conn)]
      (d/transact conn
        {:tx-data [{:db/ident :test.clj.with/name
                    :db/valueType :db.type/string
                    :db/cardinality :db.cardinality/one}]})
      (let [database (d/db conn)
            result (d/with database
                     {:tx-data [{:test.clj.with/name "Speculative"}]})]
        (is (some? result)))
      (d/release conn))))

(deftest ^:integration test-transact-requires-tx-data
  (testing "transact throws without :tx-data"
    (let [conn (test-conn)]
      (is (thrown? clojure.lang.ExceptionInfo
                  (d/transact conn {})))
      (d/release conn))))
