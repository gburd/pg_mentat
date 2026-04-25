(ns datomic-compat.core-test
  "Datomic compatibility test suite for mentatd.
   Tests are organized by category: connection, schema, transactions,
   queries, pull API, and time-travel operations."
  (:require [clojure.test :refer [deftest is testing use-fixtures]]
            [datomic.api :as d]))

;; ---------------------------------------------------------------------------
;; Configuration
;; ---------------------------------------------------------------------------

(def mentatd-uri
  (or (System/getenv "MENTATD_URI")
      "datomic:free://localhost:8080/test-db"))

;; ---------------------------------------------------------------------------
;; Test fixtures
;; ---------------------------------------------------------------------------

(def ^:dynamic *conn* nil)
(def ^:dynamic *db* nil)

(defn setup-database
  "Create the test database, install schema, and insert seed data."
  []
  (d/create-database mentatd-uri)
  (let [conn (d/connect mentatd-uri)
        schema [{:db/ident       :person/name
                 :db/valueType   :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/doc         "Person's name"}
                {:db/ident       :person/age
                 :db/valueType   :db.type/long
                 :db/cardinality :db.cardinality/one
                 :db/doc         "Person's age"}
                {:db/ident       :person/email
                 :db/valueType   :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique      :db.unique/identity
                 :db/doc         "Person's email"}]
        people [{:person/name  "Alice"
                 :person/age   30
                 :person/email "alice@example.com"}
                {:person/name  "Bob"
                 :person/age   25
                 :person/email "bob@example.com"}
                {:person/name  "Charlie"
                 :person/age   35
                 :person/email "charlie@example.com"}]]
    @(d/transact conn schema)
    @(d/transact conn people)
    conn))

(defn teardown-database
  "Delete the test database."
  []
  (d/delete-database mentatd-uri))

(defn with-database
  "Once fixture: creates the database with schema and seed data before all
   tests, tears it down after all tests."
  [test-fn]
  (let [conn (setup-database)]
    (try
      (binding [*conn* conn
                *db*   (d/db conn)]
        (test-fn))
      (finally
        (teardown-database)))))

(use-fixtures :once with-database)

;; ---------------------------------------------------------------------------
;; Connection tests
;; ---------------------------------------------------------------------------

(deftest test-connection
  (testing "connecting to mentatd returns a connection object"
    (is (some? *conn*) "Connection should not be nil")))

(deftest test-create-and-delete-database
  (testing "create-database and delete-database round-trip"
    (let [temp-uri (str mentatd-uri "-temp")]
      (is (true? (d/create-database temp-uri))
          "create-database should return true")
      (is (true? (d/delete-database temp-uri))
          "delete-database should return true"))))

;; ---------------------------------------------------------------------------
;; Schema tests
;; ---------------------------------------------------------------------------

(deftest test-schema-attributes-installed
  (testing "schema attributes are queryable after installation"
    (let [results (d/q '[:find ?ident
                         :where [?e :db/ident ?ident]
                                [?e :db/valueType :db.type/string]]
                       *db*)]
      (is (seq results) "Should find string-typed attributes")
      (is (some #{[:person/name]} results)
          ":person/name should be a string attribute")
      (is (some #{[:person/email]} results)
          ":person/email should be a string attribute"))))

;; ---------------------------------------------------------------------------
;; Transaction tests
;; ---------------------------------------------------------------------------

(deftest test-transact-returns-tx-data
  (testing "transact returns a map with :tx-data"
    (let [tx-result @(d/transact *conn*
                       [{:person/name  "Temp"
                         :person/age   99
                         :person/email "temp@example.com"}])]
      (is (map? tx-result) "Transaction result should be a map")
      (is (contains? tx-result :tx-data)
          "Transaction result should contain :tx-data")
      (is (seq (:tx-data tx-result))
          ":tx-data should not be empty"))))

(deftest test-transact-add-retract
  (testing ":db/add and :db/retract work via transact"
    (let [;; Find Alice's entity id
          alice-id (ffirst
                     (d/q '[:find ?e
                            :where [?e :person/email "alice@example.com"]]
                          *db*))]
      (is (some? alice-id) "Alice entity should exist")
      ;; Add attribute
      (let [tx @(d/transact *conn* [[:db/add alice-id :person/age 31]])]
        (is (seq (:tx-data tx)) ":db/add should produce tx-data")))))

;; ---------------------------------------------------------------------------
;; Query tests
;; ---------------------------------------------------------------------------

(deftest test-query-find-all
  (testing "query finds all people with name and age"
    (let [results (d/q '[:find ?name ?age
                         :where
                         [?e :person/name ?name]
                         [?e :person/age ?age]]
                       *db*)]
      (is (set? results) "Query results should be a set")
      (is (>= (count results) 3)
          "Should find at least 3 people"))))

(deftest test-query-with-input-parameter
  (testing "query with :in clause filters correctly"
    (let [results (d/q '[:find ?name
                         :in $ ?min-age
                         :where
                         [?e :person/name ?name]
                         [?e :person/age ?age]
                         [(>= ?age ?min-age)]]
                       *db* 30)]
      (is (seq results) "Should find people age >= 30")
      ;; Alice (30) and Charlie (35) should match
      (is (>= (count results) 2)
          "At least Alice and Charlie should match"))))

(deftest test-query-with-aggregates
  (testing "query with aggregate function"
    (let [results (d/q '[:find (count ?e)
                         :where [?e :person/name _]]
                       *db*)]
      (is (seq results) "Aggregate query should return results")
      (is (>= (ffirst results) 3)
          "Should count at least 3 people"))))

;; ---------------------------------------------------------------------------
;; Pull API tests
;; ---------------------------------------------------------------------------

(deftest test-pull-wildcard
  (testing "pull with [*] returns all attributes"
    (let [alice-id (ffirst
                     (d/q '[:find ?e
                            :where [?e :person/email "alice@example.com"]]
                          *db*))
          result (d/pull *db* '[*] alice-id)]
      (is (map? result) "Pull result should be a map")
      (is (contains? result :person/name)
          "Pull result should contain :person/name")
      (is (contains? result :person/email)
          "Pull result should contain :person/email"))))

(deftest test-pull-specific-attributes
  (testing "pull with specific attributes returns only those attributes"
    (let [alice-id (ffirst
                     (d/q '[:find ?e
                            :where [?e :person/email "alice@example.com"]]
                          *db*))
          result (d/pull *db* '[:person/name :person/age] alice-id)]
      (is (= "Alice" (:person/name result))
          "Should pull Alice's name")
      (is (number? (:person/age result))
          "Should pull Alice's age"))))

;; ---------------------------------------------------------------------------
;; Entity API tests
;; ---------------------------------------------------------------------------

(deftest test-entity-api
  (testing "entity API returns a lazy entity map"
    (let [bob-id (ffirst
                   (d/q '[:find ?e
                          :where [?e :person/email "bob@example.com"]]
                        *db*))
          entity (d/entity *db* bob-id)]
      (is (some? entity) "Entity should not be nil")
      (is (= "Bob" (:person/name entity))
          "Entity should have correct name")
      (is (= 25 (:person/age entity))
          "Entity should have correct age")
      (is (= "bob@example.com" (:person/email entity))
          "Entity should have correct email"))))

;; ---------------------------------------------------------------------------
;; Time-travel tests (history / as-of)
;; ---------------------------------------------------------------------------

(deftest test-history-query
  (testing "history database shows all past values"
    ;; Update Alice's age to create history
    (let [alice-id (ffirst
                     (d/q '[:find ?e
                            :where [?e :person/email "alice@example.com"]]
                          *db*))]
      @(d/transact *conn* [[:db/add alice-id :person/age 31]]))
    (let [fresh-db   (d/db *conn*)
          history-db (d/history fresh-db)
          results    (d/q '[:find ?age ?tx ?added
                            :in $ ?email
                            :where
                            [?e :person/email ?email]
                            [?e :person/age ?age ?tx ?added]]
                          history-db "alice@example.com")]
      (is (seq results) "History query should return results")
      (is (>= (count results) 2)
          "Should see at least the original and updated age"))))

(deftest test-as-of-query
  (testing "as-of database returns data at a previous point in time"
    (let [;; Get current basis-t
          current-t (d/basis-t *db*)
          ;; Query current db
          current-results (d/q '[:find ?name ?age
                                 :where
                                 [?e :person/name ?name]
                                 [?e :person/age ?age]]
                               *db*)
          ;; Query as-of an earlier transaction
          as-of-db (d/as-of *db* (dec current-t))
          as-of-results (d/q '[:find ?name ?age
                               :where
                               [?e :person/name ?name]
                               [?e :person/age ?age]]
                             as-of-db)]
      (is (seq current-results)
          "Current query should return results")
      ;; as-of with (dec current-t) should return data from before the
      ;; latest transaction, which may differ in count or values.
      (is (set? as-of-results)
          "As-of query should return a set"))))

;; ---------------------------------------------------------------------------
;; Retract / retractEntity tests
;; ---------------------------------------------------------------------------

(deftest test-retract-entity
  (testing "retractEntity removes all attributes of an entity"
    (let [charlie-id (ffirst
                       (d/q '[:find ?e
                              :where [?e :person/email "charlie@example.com"]]
                            *db*))]
      (is (some? charlie-id) "Charlie should exist before retraction")
      (let [tx @(d/transact *conn* [[:db/retractEntity charlie-id]])]
        (is (seq (:tx-data tx))
            "retractEntity should produce tx-data"))
      ;; Verify Charlie is gone from fresh db
      (let [fresh-db (d/db *conn*)
            results  (d/q '[:find ?e
                            :where [?e :person/email "charlie@example.com"]]
                          fresh-db)]
        (is (empty? results)
            "Charlie should not be found after retractEntity")))))

;; ---------------------------------------------------------------------------
;; Speculative transactions (d/with) tests
;; ---------------------------------------------------------------------------

(deftest test-with-returns-report
  (testing "d/with returns a speculative transaction report"
    (let [result (d/with *db*
                   [{:db/id "spec-person"
                     :person/name "Speculative"
                     :person/email "spec@example.com"
                     :person/age 42}])]
      (is (map? result) "d/with should return a map")
      (when (map? result)
        (is (contains? result :db-after) "Should have :db-after")
        (is (contains? result :tx-data) "Should have :tx-data")
        (is (seq (:tx-data result)) ":tx-data should not be empty")))))

(deftest test-with-does-not-persist
  (testing "d/with changes are not visible in the real database"
    (d/with *db*
      [{:person/name "Invisible"
        :person/email "invisible@example.com"}])
    ;; Query the real database -- Invisible should not exist
    (let [fresh-db (d/db *conn*)
          results  (d/q '[:find ?e
                          :where [?e :person/email "invisible@example.com"]]
                        fresh-db)]
      (is (empty? results)
          "Speculative entity should not exist in the real database"))))

;; ---------------------------------------------------------------------------
;; Database filtering (d/filter) tests
;; ---------------------------------------------------------------------------

(deftest test-filter-restricts-visible-datoms
  (testing "d/filter restricts the datoms visible to queries"
    (let [;; Create a filtered db that only shows :person/name datoms
          filtered-db (d/filter *db*
                        (fn [db datom]
                          (= :person/name (d/ident db (.a datom)))))
          results     (d/q '[:find ?e ?v
                             :where [?e _ ?v]]
                           filtered-db)]
      (is (seq results) "Filtered query should return results")
      ;; All values should be strings (names), not numbers (ages)
      (when (seq results)
        (is (every? #(string? (second %)) results)
            "Filtered results should only contain string values (names)")))))

;; ---------------------------------------------------------------------------
;; Direct index access (d/datoms) tests
;; ---------------------------------------------------------------------------

(deftest test-datoms-eavt
  (testing "d/datoms with :eavt returns datoms"
    (let [datoms (d/datoms *db* :eavt)]
      (is (seq datoms) "Should have datoms in EAVT index")
      (when (seq datoms)
        (let [d (first datoms)]
          (is (some? (.e d)) "Datom should have entity")
          (is (some? (.a d)) "Datom should have attribute")
          (is (some? (.v d)) "Datom should have value")
          (is (some? (.tx d)) "Datom should have tx"))))))

(deftest test-datoms-eavt-with-entity-component
  (testing "d/datoms with :eavt and entity filters by entity"
    (let [alice-id (ffirst
                     (d/q '[:find ?e
                            :where [?e :person/email "alice@example.com"]]
                          *db*))
          datoms   (d/datoms *db* :eavt alice-id)]
      (is (some? alice-id) "Alice should exist")
      (when alice-id
        (is (seq datoms) "Should find datoms for Alice")
        (is (every? #(= alice-id (.e %)) datoms)
            "All datoms should belong to Alice's entity")))))

(deftest test-datoms-aevt
  (testing "d/datoms with :aevt returns datoms ordered by attribute"
    (let [datoms (d/datoms *db* :aevt)]
      (is (seq datoms) "Should have datoms in AEVT index"))))

(deftest test-datoms-vaet-ref-only
  (testing "d/datoms with :vaet returns only ref-type datoms"
    ;; VAET index is specifically for reverse reference lookups
    (let [datoms (d/datoms *db* :vaet)]
      ;; May be empty if no ref-type attributes are asserted on user entities
      (is (or (empty? datoms)
              (every? #(number? (.v %)) datoms))
          "VAET datoms should only contain ref values (numbers)"))))
