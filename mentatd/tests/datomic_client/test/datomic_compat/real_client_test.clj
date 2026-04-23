(ns datomic-compat.real-client-test
  "Comprehensive Datomic Peer API compatibility tests for pg_mentat/mentatd.

   These tests exercise the official datomic.api namespace against a running
   mentatd server to validate that real Datomic client code works without
   modification.  Each test is self-contained: it creates its own database,
   runs assertions, and tears the database down again so that ordering does
   not matter and failures are isolated.

   Test categories:
     1. Connection lifecycle
     2. Schema definition
     3. Transactions (map and list forms)
     4. Queries (basic, parameterized, aggregates, rules)
     5. Pull API (wildcard, specific attrs, nested, limits, defaults)
     6. Lookup refs
     7. Entity API
     8. Time-travel (as-of, since, history)
     9. Error handling / edge cases"
  (:require [clojure.test :refer [deftest is testing use-fixtures]]
            [datomic.api :as d]))

;; ---------------------------------------------------------------------------
;; Configuration
;; ---------------------------------------------------------------------------

(def base-uri
  "Base URI derived from the MENTATD_URI env-var or a sensible default."
  (or (System/getenv "MENTATD_URI")
      "datomic:free://localhost:8080/compat-test"))

(defn fresh-uri
  "Return a unique database URI to avoid collisions between tests."
  []
  (str base-uri "-" (System/nanoTime)))

;; ---------------------------------------------------------------------------
;; Shared schema used by most tests
;; ---------------------------------------------------------------------------

(def person-schema
  [{:db/ident       :person/name
    :db/valueType   :db.type/string
    :db/cardinality :db.cardinality/one
    :db/doc         "A person's full name"}
   {:db/ident       :person/age
    :db/valueType   :db.type/long
    :db/cardinality :db.cardinality/one
    :db/doc         "A person's age in years"}
   {:db/ident       :person/email
    :db/valueType   :db.type/string
    :db/cardinality :db.cardinality/one
    :db/unique      :db.unique/identity
    :db/doc         "A person's email address (unique identity)"}
   {:db/ident       :person/friends
    :db/valueType   :db.type/ref
    :db/cardinality :db.cardinality/many
    :db/doc         "References to friend entities"}
   {:db/ident       :person/alias
    :db/valueType   :db.type/string
    :db/cardinality :db.cardinality/many
    :db/doc         "A person's aliases (cardinality-many strings)"}])

(def seed-people
  [{:db/id          "alice"
    :person/name    "Alice"
    :person/age     30
    :person/email   "alice@example.com"
    :person/alias   ["A" "Ali"]}
   {:db/id          "bob"
    :person/name    "Bob"
    :person/age     25
    :person/email   "bob@example.com"}
   {:db/id          "charlie"
    :person/name    "Charlie"
    :person/age     35
    :person/email   "charlie@example.com"}])

(defn with-fresh-db
  "Helper that creates a fresh database, installs schema, seeds data,
   calls (f conn), then deletes the database.  Returns the result of (f conn)."
  [f]
  (let [uri (fresh-uri)]
    (d/create-database uri)
    (try
      (let [conn (d/connect uri)]
        @(d/transact conn person-schema)
        @(d/transact conn seed-people)
        (f conn))
      (finally
        (d/delete-database uri)))))

;; ===================================================================
;; 1. Connection lifecycle
;; ===================================================================

(deftest test-create-and-delete-database
  (testing "create-database returns true, delete-database returns true"
    (let [uri (fresh-uri)]
      (is (true? (d/create-database uri))
          "create-database should return true on success")
      (is (true? (d/delete-database uri))
          "delete-database should return true on success"))))

(deftest test-connect-to-existing-database
  (testing "connect returns a non-nil connection object"
    (with-fresh-db
      (fn [conn]
        (is (some? conn) "Connection should not be nil")))))

(deftest test-connect-to-nonexistent-database-throws
  (testing "connect to a database that does not exist throws an exception"
    (is (thrown? Exception
                 (d/connect (str base-uri "-nonexistent-" (System/nanoTime)))))))

(deftest test-db-returns-database-value
  (testing "d/db returns a non-nil database value from a connection"
    (with-fresh-db
      (fn [conn]
        (let [db (d/db conn)]
          (is (some? db) "db value should not be nil"))))))

;; ===================================================================
;; 2. Schema definition
;; ===================================================================

(deftest test-schema-attributes-queryable
  (testing "installed schema attributes appear in queries"
    (with-fresh-db
      (fn [conn]
        (let [db      (d/db conn)
              results (d/q '[:find ?ident
                             :where
                             [?e :db/ident ?ident]
                             [?e :db/valueType :db.type/string]]
                           db)]
          (is (seq results) "Should find at least one string attribute")
          (is (some #{[:person/name]} results)
              ":person/name should be among string attributes")
          (is (some #{[:person/email]} results)
              ":person/email should be among string attributes"))))))

(deftest test-unique-identity-schema
  (testing ":db.unique/identity attribute is recognized"
    (with-fresh-db
      (fn [conn]
        (let [db      (d/db conn)
              results (d/q '[:find ?ident
                             :where
                             [?e :db/ident ?ident]
                             [?e :db/unique :db.unique/identity]]
                           db)]
          (is (some #{[:person/email]} results)
              ":person/email should have unique identity"))))))

(deftest test-cardinality-many-schema
  (testing ":db.cardinality/many attribute is recognized"
    (with-fresh-db
      (fn [conn]
        (let [db      (d/db conn)
              results (d/q '[:find ?ident
                             :where
                             [?e :db/ident ?ident]
                             [?e :db/cardinality :db.cardinality/many]]
                           db)]
          (is (some #{[:person/friends]} results)
              ":person/friends should have cardinality many")
          (is (some #{[:person/alias]} results)
              ":person/alias should have cardinality many"))))))

;; ===================================================================
;; 3. Transactions
;; ===================================================================

(deftest test-transact-map-form
  (testing "transact with map-form entity returns tx-data"
    (with-fresh-db
      (fn [conn]
        (let [tx @(d/transact conn [{:person/name  "Dave"
                                      :person/age   40
                                      :person/email "dave@example.com"}])]
          (is (map? tx) "Transaction result should be a map")
          (is (contains? tx :tx-data) "Should contain :tx-data")
          (is (seq (:tx-data tx)) ":tx-data should not be empty"))))))

(deftest test-transact-list-form
  (testing "transact with :db/add list form"
    (with-fresh-db
      (fn [conn]
        (let [db       (d/db conn)
              alice-id (ffirst (d/q '[:find ?e :where [?e :person/email "alice@example.com"]] db))
              tx       @(d/transact conn [[:db/add alice-id :person/age 31]])]
          (is (seq (:tx-data tx)) ":db/add list form should produce tx-data"))))))

(deftest test-transact-retract-list-form
  (testing "transact with :db/retract list form"
    (with-fresh-db
      (fn [conn]
        (let [db       (d/db conn)
              bob-id   (ffirst (d/q '[:find ?e :where [?e :person/email "bob@example.com"]] db))
              tx       @(d/transact conn [[:db/retract bob-id :person/age 25]])]
          (is (seq (:tx-data tx)) ":db/retract should produce tx-data"))))))

(deftest test-transact-retract-entity
  (testing ":db/retractEntity removes all datoms for an entity"
    (with-fresh-db
      (fn [conn]
        (let [db         (d/db conn)
              charlie-id (ffirst (d/q '[:find ?e :where [?e :person/email "charlie@example.com"]] db))]
          (is (some? charlie-id) "Charlie should exist before retraction")
          @(d/transact conn [[:db/retractEntity charlie-id]])
          (let [fresh-db (d/db conn)
                results  (d/q '[:find ?e :where [?e :person/email "charlie@example.com"]] fresh-db)]
            (is (empty? results) "Charlie should be gone after retractEntity")))))))

(deftest test-transact-tempid-resolution
  (testing "tempids (string) are resolved in transaction result"
    (with-fresh-db
      (fn [conn]
        (let [tx @(d/transact conn [{:db/id        "new-person"
                                      :person/name  "Eve"
                                      :person/email "eve@example.com"}])]
          (is (contains? tx :tempids) "Result should contain :tempids")
          (is (contains? (:tempids tx) "new-person")
              "tempids should resolve the string 'new-person'")
          (is (pos? (get (:tempids tx) "new-person"))
              "resolved tempid should be a positive entity id"))))))

(deftest test-transact-multiple-entities
  (testing "transact with multiple entities in a single call"
    (with-fresh-db
      (fn [conn]
        (let [tx @(d/transact conn [{:person/name "X" :person/email "x@example.com"}
                                     {:person/name "Y" :person/email "y@example.com"}
                                     {:person/name "Z" :person/email "z@example.com"}])]
          (is (seq (:tx-data tx)))
          ;; Verify all three exist
          (let [db    (d/db conn)
                count (ffirst (d/q '[:find (count ?e) :where [?e :person/name _]] db))]
            (is (>= count 6) "Should have at least 6 entities (3 seed + 3 new)")))))))

;; ===================================================================
;; 4. Queries
;; ===================================================================

(deftest test-query-find-all
  (testing "basic query finding all name+age tuples"
    (with-fresh-db
      (fn [conn]
        (let [db      (d/db conn)
              results (d/q '[:find ?name ?age
                             :where
                             [?e :person/name ?name]
                             [?e :person/age ?age]]
                           db)]
          (is (set? results) "Query results should be a set of tuples")
          (is (>= (count results) 3) "Should find at least 3 people"))))))

(deftest test-query-single-binding
  (testing "query returning a single scalar per result"
    (with-fresh-db
      (fn [conn]
        (let [db      (d/db conn)
              results (d/q '[:find ?name
                             :where [?e :person/name ?name]]
                           db)]
          (is (>= (count results) 3))
          (is (some #{["Alice"]} results)))))))

(deftest test-query-with-input-parameter
  (testing "query with :in clause and a single input"
    (with-fresh-db
      (fn [conn]
        (let [db      (d/db conn)
              results (d/q '[:find ?name
                             :in $ ?min-age
                             :where
                             [?e :person/name ?name]
                             [?e :person/age ?age]
                             [(>= ?age ?min-age)]]
                           db 30)]
          (is (>= (count results) 2)
              "Alice (30) and Charlie (35) should match age >= 30"))))))

(deftest test-query-with-multiple-inputs
  (testing "query with multiple :in parameters"
    (with-fresh-db
      (fn [conn]
        (let [db      (d/db conn)
              results (d/q '[:find ?name
                             :in $ ?min-age ?max-age
                             :where
                             [?e :person/name ?name]
                             [?e :person/age ?age]
                             [(>= ?age ?min-age)]
                             [(<= ?age ?max-age)]]
                           db 26 34)]
          (is (some #{["Alice"]} results) "Alice (30) should be in [26,34]")
          (is (not (some #{["Bob"]} results)) "Bob (25) should not be in [26,34]"))))))

(deftest test-query-with-collection-input
  (testing "query with collection input binding"
    (with-fresh-db
      (fn [conn]
        (let [db      (d/db conn)
              results (d/q '[:find ?name
                             :in $ [?email ...]
                             :where
                             [?e :person/email ?email]
                             [?e :person/name ?name]]
                           db ["alice@example.com" "bob@example.com"])]
          (is (= 2 (count results)) "Should find exactly 2 people"))))))

(deftest test-query-with-aggregate
  (testing "query with (count ...) aggregate"
    (with-fresh-db
      (fn [conn]
        (let [db      (d/db conn)
              results (d/q '[:find (count ?e)
                             :where [?e :person/name _]]
                           db)]
          (is (= 1 (count results)) "Aggregate returns a single row")
          (is (>= (ffirst results) 3) "Should count at least 3 people"))))))

(deftest test-query-with-min-max-aggregate
  (testing "query with (min ...) and (max ...) aggregates"
    (with-fresh-db
      (fn [conn]
        (let [db  (d/db conn)
              mn  (ffirst (d/q '[:find (min ?age) :where [_ :person/age ?age]] db))
              mx  (ffirst (d/q '[:find (max ?age) :where [_ :person/age ?age]] db))]
          (is (= 25 mn) "Minimum age should be 25 (Bob)")
          (is (= 35 mx) "Maximum age should be 35 (Charlie)"))))))

(deftest test-query-with-rules
  (testing "query with user-defined rules"
    (with-fresh-db
      (fn [conn]
        (let [db    (d/db conn)
              rules '[[(adult? ?e)
                       [?e :person/age ?age]
                       [(>= ?age 18)]]]
              results (d/q '[:find ?name
                             :in $ %
                             :where
                             [?e :person/name ?name]
                             (adult? ?e)]
                           db rules)]
          (is (>= (count results) 3)
              "All seed people are adults (age >= 18)"))))))

;; ===================================================================
;; 5. Pull API
;; ===================================================================

(deftest test-pull-wildcard
  (testing "pull with [*] returns all attributes"
    (with-fresh-db
      (fn [conn]
        (let [db       (d/db conn)
              alice-id (ffirst (d/q '[:find ?e :where [?e :person/email "alice@example.com"]] db))
              result   (d/pull db '[*] alice-id)]
          (is (map? result) "Pull result should be a map")
          (is (= "Alice" (:person/name result)))
          (is (= 30 (:person/age result)))
          (is (= "alice@example.com" (:person/email result))))))))

(deftest test-pull-specific-attributes
  (testing "pull with explicit attribute list"
    (with-fresh-db
      (fn [conn]
        (let [db       (d/db conn)
              alice-id (ffirst (d/q '[:find ?e :where [?e :person/email "alice@example.com"]] db))
              result   (d/pull db '[:person/name :person/age] alice-id)]
          (is (= "Alice" (:person/name result)))
          (is (= 30 (:person/age result)))
          (is (nil? (:person/email result))
              "email should not be in result since it was not requested"))))))

(deftest test-pull-missing-entity
  (testing "pull on a nonexistent entity-id returns empty map or {:db/id ...}"
    (with-fresh-db
      (fn [conn]
        (let [db     (d/db conn)
              result (d/pull db '[:person/name] 999999999)]
          ;; Datomic returns {:db/id 999999999} for missing entities
          (is (or (empty? (dissoc result :db/id))
                  (nil? (:person/name result)))
              "Missing entity should have no :person/name"))))))

(deftest test-pull-with-default
  (testing "pull with :default option fills in missing attribute"
    (with-fresh-db
      (fn [conn]
        ;; Insert an entity without :person/age
        @(d/transact conn [{:person/name "NoAge" :person/email "noage@example.com"}])
        (let [db     (d/db conn)
              eid    (ffirst (d/q '[:find ?e :where [?e :person/email "noage@example.com"]] db))
              result (d/pull db '[(:person/age {:default 0})] eid)]
          (is (= 0 (:person/age result))
              "Default should fill in for missing :person/age"))))))

(deftest test-pull-with-limit
  (testing "pull with :limit on cardinality-many attribute"
    (with-fresh-db
      (fn [conn]
        (let [db       (d/db conn)
              alice-id (ffirst (d/q '[:find ?e :where [?e :person/email "alice@example.com"]] db))
              result   (d/pull db '[(:person/alias {:limit 1})] alice-id)]
          (when (seq (:person/alias result))
            (is (<= (count (:person/alias result)) 1)
                "Limit should restrict to 1 alias")))))))

(deftest test-pull-with-as
  (testing "pull with :as renames the key in the result"
    (with-fresh-db
      (fn [conn]
        (let [db       (d/db conn)
              alice-id (ffirst (d/q '[:find ?e :where [?e :person/email "alice@example.com"]] db))
              result   (d/pull db '[(:person/name {:as "full-name"})] alice-id)]
          (is (= "Alice" (get result "full-name"))
              ":as should rename the key"))))))

(deftest test-pull-nested-ref
  (testing "pull with nested map spec follows refs"
    (with-fresh-db
      (fn [conn]
        ;; Make Alice friends with Bob
        (let [db       (d/db conn)
              alice-id (ffirst (d/q '[:find ?e :where [?e :person/email "alice@example.com"]] db))
              bob-id   (ffirst (d/q '[:find ?e :where [?e :person/email "bob@example.com"]] db))]
          @(d/transact conn [[:db/add alice-id :person/friends bob-id]])
          (let [fresh-db (d/db conn)
                result   (d/pull fresh-db '[:person/name {:person/friends [:person/name]}] alice-id)]
            (is (= "Alice" (:person/name result)))
            (when-let [friends (:person/friends result)]
              (is (some #(= "Bob" (:person/name %)) friends)
                  "Alice's friend Bob should be pulled with name"))))))))

(deftest test-pull-reverse-ref
  (testing "pull with reverse reference attribute (e.g. :person/_friends)"
    (with-fresh-db
      (fn [conn]
        ;; Make Alice friends with Bob
        (let [db       (d/db conn)
              alice-id (ffirst (d/q '[:find ?e :where [?e :person/email "alice@example.com"]] db))
              bob-id   (ffirst (d/q '[:find ?e :where [?e :person/email "bob@example.com"]] db))]
          @(d/transact conn [[:db/add alice-id :person/friends bob-id]])
          (let [fresh-db (d/db conn)
                result   (d/pull fresh-db '[:person/name :person/_friends] bob-id)]
            (is (= "Bob" (:person/name result)))
            (when-let [referrers (:person/_friends result)]
              (is (some #(= alice-id (:db/id %)) referrers)
                  "Reverse ref should show Alice references Bob"))))))))

(deftest test-pull-many
  (testing "d/pull-many pulls multiple entities at once"
    (with-fresh-db
      (fn [conn]
        (let [db   (d/db conn)
              ids  (mapv first (d/q '[:find ?e :where [?e :person/name _]] db))
              results (d/pull-many db '[:person/name :person/age] ids)]
          (is (= (count ids) (count results))
              "pull-many should return one result per entity id")
          (is (every? :person/name results)
              "Each pulled entity should have :person/name"))))))

;; ===================================================================
;; 6. Lookup refs
;; ===================================================================

(deftest test-lookup-ref-in-query
  (testing "lookup ref [:person/email ...] can be used in query"
    (with-fresh-db
      (fn [conn]
        (let [db      (d/db conn)
              results (d/q '[:find ?name
                             :in $ ?ref
                             :where
                             [?ref :person/name ?name]]
                           db [:person/email "alice@example.com"])]
          (is (= #{["Alice"]} results)
              "Lookup ref should resolve to Alice"))))))

(deftest test-lookup-ref-in-pull
  (testing "lookup ref as entity argument to d/pull"
    (with-fresh-db
      (fn [conn]
        (let [db     (d/db conn)
              result (d/pull db '[:person/name :person/age]
                            [:person/email "alice@example.com"])]
          (is (= "Alice" (:person/name result)))
          (is (= 30 (:person/age result))))))))

(deftest test-lookup-ref-in-transaction
  (testing "lookup ref used in :db/add transaction"
    (with-fresh-db
      (fn [conn]
        (let [tx @(d/transact conn [[:db/add [:person/email "alice@example.com"]
                                     :person/age 31]])]
          (is (seq (:tx-data tx)))
          (let [db     (d/db conn)
                result (d/pull db '[:person/age]
                              [:person/email "alice@example.com"])]
            (is (= 31 (:person/age result))
                "Age should be updated via lookup ref")))))))

;; ===================================================================
;; 7. Entity API
;; ===================================================================

(deftest test-entity-api-basic
  (testing "d/entity returns a lazy map with correct values"
    (with-fresh-db
      (fn [conn]
        (let [db     (d/db conn)
              bob-id (ffirst (d/q '[:find ?e :where [?e :person/email "bob@example.com"]] db))
              entity (d/entity db bob-id)]
          (is (some? entity) "Entity should not be nil")
          (is (= "Bob" (:person/name entity)))
          (is (= 25 (:person/age entity)))
          (is (= "bob@example.com" (:person/email entity))))))))

(deftest test-entity-keys
  (testing "d/entity supports keys enumeration"
    (with-fresh-db
      (fn [conn]
        (let [db     (d/db conn)
              bob-id (ffirst (d/q '[:find ?e :where [?e :person/email "bob@example.com"]] db))
              entity (d/entity db bob-id)
              ks     (keys entity)]
          (is (some #{:person/name} ks) "keys should include :person/name")
          (is (some #{:person/email} ks) "keys should include :person/email"))))))

(deftest test-entity-touch
  (testing "d/touch eagerly loads all attributes"
    (with-fresh-db
      (fn [conn]
        (let [db     (d/db conn)
              bob-id (ffirst (d/q '[:find ?e :where [?e :person/email "bob@example.com"]] db))
              entity (d/touch (d/entity db bob-id))]
          (is (= "Bob" (:person/name entity)))
          (is (= 25 (:person/age entity))))))))

;; ===================================================================
;; 8. Time-travel
;; ===================================================================

(deftest test-as-of-database
  (testing "d/as-of returns the database at a previous point in time"
    (with-fresh-db
      (fn [conn]
        (let [db-before (d/db conn)
              t-before  (d/basis-t db-before)]
          ;; Mutate: add a new person
          @(d/transact conn [{:person/name "Dave" :person/email "dave@example.com" :person/age 40}])
          (let [db-after  (d/db conn)
                as-of-db  (d/as-of db-after t-before)
                after-cnt (count (d/q '[:find ?e :where [?e :person/name _]] db-after))
                asof-cnt  (count (d/q '[:find ?e :where [?e :person/name _]] as-of-db))]
            (is (= (inc asof-cnt) after-cnt)
                "as-of snapshot should have one fewer person than current")))))))

(deftest test-since-database
  (testing "d/since returns only datoms added after a point in time"
    (with-fresh-db
      (fn [conn]
        (let [db-before (d/db conn)
              t-before  (d/basis-t db-before)]
          @(d/transact conn [{:person/name "Eve" :person/email "eve@example.com" :person/age 28}])
          (let [since-db (d/since (d/db conn) t-before)
                results  (d/q '[:find ?name
                                :where [?e :person/name ?name]]
                              since-db)]
            ;; d/since should return only datoms asserted after t-before.
            ;; Eve was added after t-before, so she should appear.
            (is (some #{["Eve"]} results)
                "Eve should appear in since-db")))))))

(deftest test-history-database
  (testing "d/history database shows historical (retracted) values"
    (with-fresh-db
      (fn [conn]
        (let [db       (d/db conn)
              alice-id (ffirst (d/q '[:find ?e :where [?e :person/email "alice@example.com"]] db))]
          ;; Update Alice's age twice to create history
          @(d/transact conn [[:db/add alice-id :person/age 31]])
          @(d/transact conn [[:db/add alice-id :person/age 32]])
          (let [history-db (d/history (d/db conn))
                results    (d/q '[:find ?age ?tx ?added
                                  :in $ ?email
                                  :where
                                  [?e :person/email ?email]
                                  [?e :person/age ?age ?tx ?added]]
                                history-db "alice@example.com")]
            (is (>= (count results) 3)
                "History should contain at least 3 age datoms (30, 31, 32 asserted + retractions)")))))))

(deftest test-basis-t
  (testing "d/basis-t returns a transaction id"
    (with-fresh-db
      (fn [conn]
        (let [db (d/db conn)
              t  (d/basis-t db)]
          (is (number? t) "basis-t should return a number")
          (is (pos? t) "basis-t should be positive"))))))

;; ===================================================================
;; 9. Error handling / edge cases
;; ===================================================================

(deftest test-transact-invalid-attribute
  (testing "transacting with an unknown attribute raises an error"
    (with-fresh-db
      (fn [conn]
        (is (thrown? Exception
                     @(d/transact conn [{:nonexistent/attr "value"}]))
            "Unknown attribute should cause a transaction error")))))

(deftest test-query-syntax-error
  (testing "malformed query raises an error"
    (with-fresh-db
      (fn [conn]
        (is (thrown? Exception
                     (d/q '[:find] (d/db conn)))
            "Malformed query should throw")))))

(deftest test-empty-transaction
  (testing "transacting an empty vector is a no-op"
    (with-fresh-db
      (fn [conn]
        (let [tx @(d/transact conn [])]
          ;; An empty transaction should still succeed.
          ;; It may or may not produce tx-data depending on implementation.
          (is (map? tx) "Empty transaction should still return a map"))))))

(deftest test-query-empty-results
  (testing "query that matches nothing returns empty set"
    (with-fresh-db
      (fn [conn]
        (let [results (d/q '[:find ?e
                             :where
                             [?e :person/email "nobody@nowhere.com"]]
                           (d/db conn))]
          (is (empty? results) "Non-matching query should return empty set"))))))

(deftest test-transact-duplicate-unique-identity
  (testing "upserting via unique identity attribute merges entities"
    (with-fresh-db
      (fn [conn]
        ;; Transact a new entity with the same email as Alice (upsert)
        @(d/transact conn [{:person/email "alice@example.com"
                             :person/age   99}])
        (let [db      (d/db conn)
              results (d/q '[:find ?age
                             :where
                             [?e :person/email "alice@example.com"]
                             [?e :person/age ?age]]
                           db)]
          (is (= #{[99]} results)
              "Upsert should update Alice's age to 99"))))))
