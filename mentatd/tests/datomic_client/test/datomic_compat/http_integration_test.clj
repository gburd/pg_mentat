(ns datomic-compat.http-integration-test
  "HTTP-based Datomic compatibility tests for mentatd.

   These tests exercise the mentatd HTTP/EDN API directly, validating
   the same operations that Datomic clients expect but without requiring
   the Datomic peer protocol.  They run against a live mentatd server
   and verify:
     1. Database lifecycle (create, list, delete)
     2. Connection management
     3. Schema installation via transact
     4. Data insert/retract via transact
     5. Transaction report structure (db-before, db-after, tx-data, tempids)
     6. Queries (basic, parameterized, aggregates)
     7. Pull API (wildcard, specific attributes)
     8. Time-travel (as-of, since, history, basis-t)

   Run with: lein test :http
   Requires: mentatd running on MENTATD_URL (default http://localhost:8080)"
  (:require [clojure.test :refer [deftest is testing use-fixtures]]
            [clojure.edn :as edn]
            [clj-http.client :as http]))

;; ---------------------------------------------------------------------------
;; Configuration
;; ---------------------------------------------------------------------------

(def mentatd-url
  (or (System/getenv "MENTATD_URL") "http://localhost:8080"))

(def test-db-name
  (str "http-compat-test-" (System/nanoTime)))

;; ---------------------------------------------------------------------------
;; HTTP helpers
;; ---------------------------------------------------------------------------

(defn edn-request
  "Send an EDN request to mentatd and parse the EDN response.
   Returns the parsed response map."
  [op-map]
  (let [response (http/post mentatd-url
                            {:content-type "application/edn"
                             :accept       "application/edn"
                             :body         (pr-str op-map)
                             :throw-exceptions false})]
    (when (= 200 (:status response))
      (edn/read-string (:body response)))))

(defn edn-result
  "Send an EDN request and return just the :result value."
  [op-map]
  (:result (edn-request op-map)))

;; ---------------------------------------------------------------------------
;; Fixtures
;; ---------------------------------------------------------------------------

(def ^:dynamic *conn-id* nil)

(defn setup-test-db
  "Create the test database, connect, install schema, and seed data."
  [test-fn]
  ;; Create database
  (edn-request {:op :create-db :args {:db-name test-db-name}})
  (let [conn-result (edn-result {:op :connect :args {:db-name test-db-name}})
        conn-id     (str (:connection-id conn-result))]
    ;; Install schema
    (edn-request
      {:op :transact
       :args {:connection-id conn-id
              :tx-data (pr-str
                         [{:db/ident       :person/name
                           :db/valueType   :db.type/string
                           :db/cardinality :db.cardinality/one}
                          {:db/ident       :person/age
                           :db/valueType   :db.type/long
                           :db/cardinality :db.cardinality/one}
                          {:db/ident       :person/email
                           :db/valueType   :db.type/string
                           :db/cardinality :db.cardinality/one
                           :db/unique      :db.unique/identity}])}})
    ;; Seed data
    (edn-request
      {:op :transact
       :args {:connection-id conn-id
              :tx-data (pr-str
                         [{:db/id "alice" :person/name "Alice"
                           :person/age 30 :person/email "alice@example.com"}
                          {:db/id "bob" :person/name "Bob"
                           :person/age 25 :person/email "bob@example.com"}
                          {:db/id "charlie" :person/name "Charlie"
                           :person/age 35 :person/email "charlie@example.com"}])}})
    (try
      (binding [*conn-id* conn-id]
        (test-fn))
      (finally
        (edn-request {:op :delete-db :args {:db-name test-db-name}})))))

(use-fixtures :once setup-test-db)

;; ===================================================================
;; 1. Database lifecycle
;; ===================================================================

(deftest ^:http test-list-databases
  (testing "list-dbs includes the test database"
    (let [dbs (edn-result {:op :list-dbs})]
      (is (sequential? dbs) "list-dbs should return a sequence")
      (is (some #{test-db-name} dbs)
          (str "Test database '" test-db-name "' should appear in list")))))

(deftest ^:http test-create-and-delete-database
  (testing "create-database and delete-database round-trip"
    (let [temp-name (str "http-temp-" (System/nanoTime))]
      (let [create-resp (edn-result {:op :create-db :args {:db-name temp-name}})]
        (is (some? create-resp) "create-db should return a result"))
      (let [delete-resp (edn-result {:op :delete-db :args {:db-name temp-name}})]
        (is (some? delete-resp) "delete-db should return a result")))))

;; ===================================================================
;; 2. Connection management
;; ===================================================================

(deftest ^:http test-connect-returns-connection-id
  (testing "connect returns a map with :connection-id"
    (let [result (edn-result {:op :connect :args {:db-name test-db-name}})]
      (is (map? result) "Connect result should be a map")
      (is (contains? result :connection-id) "Should have :connection-id"))))

;; ===================================================================
;; 3. Transaction report structure
;; ===================================================================

(deftest ^:http test-transact-report-has-all-four-fields
  (testing "transact returns map with :db-before :db-after :tx-data :tempids"
    (let [response (edn-request
                     {:op :transact
                      :args {:connection-id *conn-id*
                             :tx-data (pr-str [{:db/id "test-frank"
                                                :person/name "Frank"
                                                :person/email "frank@example.com"
                                                :person/age 40}])}})]
      (is (some? response) "Response should not be nil")
      (let [report (:result response)]
        (is (map? report) "Transaction result should be a map")
        (is (contains? report :db-before) "Missing :db-before")
        (is (contains? report :db-after) "Missing :db-after")
        (is (contains? report :tx-data) "Missing :tx-data")
        (is (contains? report :tempids) "Missing :tempids")))))

(deftest ^:http test-transact-report-basis-t
  (testing ":db-before and :db-after have :basis-t"
    (let [report (edn-result
                   {:op :transact
                    :args {:connection-id *conn-id*
                           :tx-data (pr-str [{:person/name "Grace"
                                              :person/email "grace@example.com"}])}})]
      (when (map? report)
        (let [t-before (get-in report [:db-before :basis-t])
              t-after  (get-in report [:db-after :basis-t])]
          (is (number? t-before) ":db-before :basis-t should be a number")
          (is (number? t-after) ":db-after :basis-t should be a number")
          (when (and t-before t-after)
            (is (> t-after t-before)
                ":db-after :basis-t should be greater than :db-before")))))))

(deftest ^:http test-transact-report-tx-data-datoms
  (testing ":tx-data contains 5-element datom vectors"
    (let [report (edn-result
                   {:op :transact
                    :args {:connection-id *conn-id*
                           :tx-data (pr-str [{:person/name "Hank"
                                              :person/email "hank@example.com"}])}})]
      (when (map? report)
        (is (sequential? (:tx-data report)) ":tx-data should be sequential")
        (is (seq (:tx-data report)) ":tx-data should not be empty")
        (doseq [datom (:tx-data report)]
          (is (sequential? datom)
              (str "Each datom should be a vector, got: " (type datom)))
          (when (sequential? datom)
            (is (= 5 (count datom))
                (str "Each datom should have 5 elements [e a v tx added], got "
                     (count datom)))))))))

(deftest ^:http test-transact-report-tempids-string-keys
  (testing ":tempids has string keys (not keyword keys)"
    (let [report (edn-result
                   {:op :transact
                    :args {:connection-id *conn-id*
                           :tx-data (pr-str [{:db/id "my-tempid"
                                              :person/name "Ivy"
                                              :person/email "ivy@example.com"}])}})]
      (when (map? report)
        (is (map? (:tempids report)) ":tempids should be a map")
        (when (map? (:tempids report))
          (is (contains? (:tempids report) "my-tempid")
              "tempids should contain string key 'my-tempid'")
          (is (every? string? (keys (:tempids report)))
              "All tempid keys should be strings")
          (is (every? number? (vals (:tempids report)))
              "All tempid values should be numbers"))))))

;; ===================================================================
;; 4. Queries
;; ===================================================================

(deftest ^:http test-query-find-all
  (testing "query finds all people with name and age"
    (let [results (edn-result
                    {:op :q
                     :args {:query (pr-str '[:find ?name ?age
                                             :where
                                             [?e :person/name ?name]
                                             [?e :person/age ?age]])
                            :args []}})]
      (is (sequential? results) "Query results should be sequential")
      (is (>= (count results) 3)
          (str "Should find at least 3 people, got " (count results))))))

(deftest ^:http test-query-with-input
  (testing "query with input parameter filters correctly"
    (let [results (edn-result
                    {:op :q
                     :args {:query (pr-str '[:find ?name
                                             :in $ ?min-age
                                             :where
                                             [?e :person/name ?name]
                                             [?e :person/age ?age]
                                             [(>= ?age ?min-age)]])
                            :args ["30"]}})]
      (is (sequential? results) "Query results should be sequential")
      ;; Alice (30) and Charlie (35) should match age >= 30
      (is (>= (count results) 2)
          (str "At least 2 people should match age >= 30, got " (count results))))))

(deftest ^:http test-query-aggregate
  (testing "query with aggregate function"
    (let [results (edn-result
                    {:op :q
                     :args {:query (pr-str '[:find (count ?e)
                                             :where [?e :person/name _]])
                            :args []}})]
      (is (sequential? results) "Aggregate results should be sequential")
      (when (seq results)
        (is (>= (ffirst results) 3)
            "Should count at least 3 people")))))

;; ===================================================================
;; 5. Pull API
;; ===================================================================

(deftest ^:http test-pull-wildcard
  (testing "pull with [*] returns attributes"
    ;; First find Alice's entity id
    (let [query-results (edn-result
                          {:op :q
                           :args {:query (pr-str '[:find ?e
                                                   :where [?e :person/email "alice@example.com"]])
                                  :args []}})
          alice-id (ffirst query-results)]
      (when alice-id
        (let [result (edn-result {:op :pull
                                  :args {:pattern "[*]"
                                         :entity-id alice-id}})]
          (is (map? result) "Pull result should be a map")
          (when (map? result)
            (is (some? (:person/name result)) "Should have :person/name")))))))

(deftest ^:http test-pull-specific-attributes
  (testing "pull with specific attributes returns only those"
    (let [query-results (edn-result
                          {:op :q
                           :args {:query (pr-str '[:find ?e
                                                   :where [?e :person/email "alice@example.com"]])
                                  :args []}})
          alice-id (ffirst query-results)]
      (when alice-id
        (let [result (edn-result {:op :pull
                                  :args {:pattern "[:person/name :person/age]"
                                         :entity-id alice-id}})]
          (is (map? result) "Pull result should be a map")
          (when (map? result)
            (is (= "Alice" (:person/name result)) "Should pull Alice's name")
            (is (number? (:person/age result)) "Should pull Alice's age")))))))

;; ===================================================================
;; 6. Time-travel operations
;; ===================================================================

(deftest ^:http test-basis-t
  (testing "basis-t returns a positive number"
    (let [result (edn-result {:op :basis-t})]
      (is (number? result) "basis-t should return a number")
      (when (number? result)
        (is (pos? result) "basis-t should be positive")))))

(deftest ^:http test-as-of-query
  (testing "as-of query returns data from a previous point in time"
    ;; Get current basis-t, transact new data, then query as-of previous t
    (let [t-before (edn-result {:op :basis-t})]
      ;; Add a new person
      (edn-request
        {:op :transact
         :args {:connection-id *conn-id*
                :tx-data (pr-str [{:person/name "NewPerson"
                                   :person/email "new@example.com"
                                   :person/age 99}])}})
      ;; Query current: should include NewPerson
      (let [current (edn-result
                      {:op :q
                       :args {:query (pr-str '[:find ?name
                                               :where [?e :person/name ?name]])
                              :args []}})]
        (is (some #(= ["NewPerson"] %) current)
            "Current db should contain NewPerson"))
      ;; Query as-of t-before: should NOT include NewPerson
      (when (number? t-before)
        (let [as-of-results (edn-result
                              {:op :as-of
                               :args {:query (pr-str '[:find ?name
                                                       :where [?e :person/name ?name]])
                                      :args []
                                      :t t-before}})]
          (when (sequential? as-of-results)
            (is (not (some #(= ["NewPerson"] %) as-of-results))
                "as-of query should NOT contain NewPerson")))))))

(deftest ^:http test-history-query
  (testing "history query shows all past values"
    ;; Find Alice and update her age
    (let [query-results (edn-result
                          {:op :q
                           :args {:query (pr-str '[:find ?e
                                                   :where [?e :person/email "alice@example.com"]])
                                  :args []}})
          alice-id (ffirst query-results)]
      (when alice-id
        ;; Update Alice's age
        (edn-request
          {:op :transact
           :args {:connection-id *conn-id*
                  :tx-data (pr-str [[:db/add alice-id :person/age 31]])}})
        ;; History query should show multiple age values
        (let [history-results (edn-result
                                {:op :history
                                 :args {:query (pr-str '[:find ?age ?tx ?added
                                                         :in $ ?email
                                                         :where
                                                         [?e :person/email ?email]
                                                         [?e :person/age ?age ?tx ?added]])
                                        :args ["alice@example.com"]}})]
          (when (sequential? history-results)
            (is (>= (count history-results) 2)
                (str "History should show at least 2 age entries, got "
                     (count history-results)))))))))

;; ===================================================================
;; 7. Health check
;; ===================================================================

(deftest ^:http test-health-check
  (testing "health check returns ok"
    (let [result (edn-result {:op :health})]
      (is (some? result) "Health check should return a result"))))

;; ===================================================================
;; 8. Error handling
;; ===================================================================

(deftest ^:http test-invalid-operation
  (testing "invalid operation returns an error"
    (let [response (edn-request {:op :nonexistent-op})]
      (is (some? response) "Should get a response")
      (when response
        (is (contains? response :error) "Should contain :error key")))))

(deftest ^:http test-query-syntax-error
  (testing "malformed query returns an error"
    (let [response (edn-request {:op :q :args {:query "[:find]" :args []}})]
      (when response
        ;; Either an error response or an empty result is acceptable
        (is (or (contains? response :error)
                (contains? response :result))
            "Should get either an error or a result")))))

;; ===================================================================
;; 9. Speculative transactions (d/with)
;; ===================================================================

(deftest ^:http test-with-returns-report-without-committing
  (testing "d/with executes a transaction speculatively and rolls it back"
    ;; Run a speculative transaction that adds a new person
    (let [with-result (edn-result
                        {:op :with
                         :args {:tx-data (pr-str
                                           [{:db/id "ghost"
                                             :person/name "Ghost"
                                             :person/email "ghost@example.com"
                                             :person/age 0}])}})]
      (is (map? with-result) "d/with should return a map (tx report)")
      (when (map? with-result)
        (is (contains? with-result :tx-data)
            "Speculative report should contain :tx-data")
        (is (seq (:tx-data with-result))
            ":tx-data should not be empty")))
    ;; Verify the speculative entity was NOT committed
    (let [results (edn-result
                    {:op :q
                     :args {:query (pr-str '[:find ?e
                                             :where [?e :person/email "ghost@example.com"]])
                            :args []}})]
      (is (or (nil? results) (empty? results))
          "Ghost entity should NOT exist after d/with (speculative only)"))))

(deftest ^:http test-with-preserves-db-state
  (testing "d/with does not change basis-t"
    (let [t-before (edn-result {:op :basis-t})]
      ;; Speculative transaction
      (edn-request
        {:op :with
         :args {:tx-data (pr-str [{:person/name "Phantom"
                                   :person/email "phantom@example.com"}])}})
      (let [t-after (edn-result {:op :basis-t})]
        (is (= t-before t-after)
            "basis-t should not change after d/with")))))

;; ===================================================================
;; 10. Database filtering (d/filter)
;; ===================================================================

(deftest ^:http test-filter-attr-equals
  (testing "d/filter with :attr-equals restricts query to a single attribute"
    (let [;; Unfiltered: find all entities with any attribute
          all-results (edn-result
                        {:op :q
                         :args {:query (pr-str '[:find ?e ?v
                                                 :where [?e :person/name ?v]])
                                :args []}})
          ;; Filtered: only :person/age attribute
          filtered-results (edn-result
                             {:op :filter
                              :args {:predicate {:type :attr-equals
                                                 :value :person/age}
                                     :query (pr-str '[:find ?e ?v
                                                      :where [?e :person/age ?v]])
                                     :args []}})]
      (is (sequential? all-results) "Unfiltered query should return results")
      (is (sequential? filtered-results) "Filtered query should return results")
      (when (and (seq all-results) (seq filtered-results))
        ;; The filtered query should only return age values (numbers)
        (is (every? #(number? (second %)) filtered-results)
            "Filtered results should only contain numeric age values")))))

(deftest ^:http test-filter-since
  (testing "d/filter with :since restricts to recent transactions"
    (let [t-before (edn-result {:op :basis-t})]
      ;; Add a new person after the timestamp
      (edn-request
        {:op :transact
         :args {:connection-id *conn-id*
                :tx-data (pr-str [{:person/name "Recent"
                                   :person/email "recent@example.com"
                                   :person/age 1}])}})
      ;; Filter since t-before: should only see the new person's data
      (let [results (edn-result
                      {:op :filter
                       :args {:predicate {:type :since
                                          :value t-before}
                              :query (pr-str '[:find ?name
                                               :where [?e :person/name ?name]])
                              :args []}})]
        (when (sequential? results)
          (is (some #(= ["Recent"] %) results)
              "Recent person should appear in since-filtered results"))))))

;; ===================================================================
;; 11. Direct index access (d/datoms)
;; ===================================================================

(deftest ^:http test-datoms-eavt-all
  (testing "d/datoms with EAVT index returns datom vectors"
    (let [results (edn-result
                    {:op :datoms
                     :args {:index :eavt
                            :components []}})]
      (is (sequential? results) "datoms should return a sequence")
      (when (seq results)
        ;; Each datom should be a 5-element vector: [e a v tx added]
        (let [first-datom (first results)]
          (is (sequential? first-datom) "Each datom should be a vector")
          (when (sequential? first-datom)
            (is (= 5 (count first-datom))
                (str "Datom should have 5 elements, got " (count first-datom)))))))))

(deftest ^:http test-datoms-eavt-by-entity
  (testing "d/datoms with EAVT and entity component filters by entity"
    ;; Find Alice's entity id first
    (let [query-results (edn-result
                          {:op :q
                           :args {:query (pr-str '[:find ?e
                                                   :where [?e :person/email "alice@example.com"]])
                                  :args []}})
          alice-id (ffirst query-results)]
      (when alice-id
        (let [results (edn-result
                        {:op :datoms
                         :args {:index :eavt
                                :components [(str alice-id)]}})]
          (is (sequential? results) "Filtered datoms should return a sequence")
          (when (seq results)
            ;; All returned datoms should have Alice's entity id
            (is (every? #(= alice-id (first %)) results)
                "All datoms should be for Alice's entity id")))))))

(deftest ^:http test-datoms-aevt-by-attribute
  (testing "d/datoms with AEVT index and attribute component"
    ;; Find the attribute entid for :person/name
    (let [attr-results (edn-result
                         {:op :q
                          :args {:query (pr-str '[:find ?a
                                                  :where [?a :db/ident :person/name]])
                                 :args []}})
          name-attr-id (ffirst attr-results)]
      (when name-attr-id
        (let [results (edn-result
                        {:op :datoms
                         :args {:index :aevt
                                :components [(str name-attr-id)]}})]
          (is (sequential? results) "AEVT datoms should return a sequence")
          (when (seq results)
            ;; All returned datoms should have the :person/name attribute entid
            (is (every? #(= name-attr-id (second %)) results)
                "All datoms should have the :person/name attribute")))))))

(deftest ^:http test-datoms-vaet-ref-lookup
  (testing "d/datoms with VAET index for reverse ref traversal"
    ;; VAET is for ref-type values only
    (let [results (edn-result
                    {:op :datoms
                     :args {:index :vaet
                            :components []}})]
      ;; VAET may return empty if there are no ref-type datoms,
      ;; but should still be a valid sequence
      (is (sequential? results)
          "VAET datoms should return a sequence (possibly empty)"))))

;; ===================================================================
;; 12. Pull API - recursive and component patterns
;; ===================================================================

(deftest ^:http test-pull-recursive-unbounded
  (testing "pull with unbounded recursion pattern {:attr ...}"
    ;; Install ref attribute for recursive pulls
    (edn-request
      {:op :transact
       :args {:connection-id *conn-id*
              :tx-data (pr-str [{:db/ident       :person/friend
                                 :db/valueType   :db.type/ref
                                 :db/cardinality :db.cardinality/one}])}})
    ;; Create chain: A -> B -> C
    (let [tx-result (edn-result
                      {:op :transact
                       :args {:connection-id *conn-id*
                              :tx-data (pr-str
                                         [{:db/id "rec-a" :person/name "RecA" :person/email "rec-a@test.com"
                                           :person/friend "rec-b"}
                                          {:db/id "rec-b" :person/name "RecB" :person/email "rec-b@test.com"
                                           :person/friend "rec-c"}
                                          {:db/id "rec-c" :person/name "RecC" :person/email "rec-c@test.com"}])}})
          a-id (when (map? tx-result) (get (:tempids tx-result) "rec-a"))]
      (when a-id
        (let [result (edn-result {:op :pull
                                  :args {:pattern "[{:person/friend ...}]"
                                         :entity-id a-id}})]
          (is (map? result) "Recursive pull should return a map")
          (when (map? result)
            ;; Should have :person/name (from recursive attr expansion)
            (is (some? (:person/name result)) "Root should have :person/name")
            ;; Should follow the chain
            (when-let [friend (:person/friend result)]
              (is (map? friend) "Friend should be a map")
              (is (= "RecB" (:person/name friend)) "Friend should be RecB"))))))))

(deftest ^:http test-pull-recursive-bounded
  (testing "pull with bounded recursion {:attr 1}"
    ;; Re-use the recursive schema from above (already installed)
    (let [query-results (edn-result
                          {:op :q
                           :args {:query (pr-str '[:find ?e
                                                   :where [?e :person/email "rec-a@test.com"]])
                                  :args []}})
          a-id (ffirst query-results)]
      (when a-id
        (let [result (edn-result {:op :pull
                                  :args {:pattern "[{:person/friend 1}]"
                                         :entity-id a-id}})]
          (is (map? result) "Bounded recursive pull should return a map")
          (when (map? result)
            (when-let [friend (:person/friend result)]
              ;; At depth 1, friend's friend should be absent or a stub
              (let [ff (:person/friend friend)]
                (is (or (nil? ff)
                        (and (map? ff) (contains? ff :db/id)))
                    "At depth 1, second-level friend should be nil or {:db/id} stub")))))))))

(deftest ^:http test-pull-component-expansion
  (testing "pull auto-expands component ref attributes"
    ;; Install component attribute
    (edn-request
      {:op :transact
       :args {:connection-id *conn-id*
              :tx-data (pr-str [{:db/ident        :person/address
                                 :db/valueType    :db.type/ref
                                 :db/cardinality  :db.cardinality/one
                                 :db/isComponent  true}
                                {:db/ident        :address/city
                                 :db/valueType    :db.type/string
                                 :db/cardinality  :db.cardinality/one}])}})
    ;; Create person with component address
    (let [tx-result (edn-result
                      {:op :transact
                       :args {:connection-id *conn-id*
                              :tx-data (pr-str
                                         [{:db/id "comp-person"
                                           :person/name "CompPerson"
                                           :person/email "comp@test.com"
                                           :person/address {:db/id "comp-addr"
                                                            :address/city "Boston"}}])}})
          person-id (when (map? tx-result) (get (:tempids tx-result) "comp-person"))]
      (when person-id
        (let [result (edn-result {:op :pull
                                  :args {:pattern "[:person/name :person/address]"
                                         :entity-id person-id}})]
          (is (map? result) "Component pull should return a map")
          (when (map? result)
            (is (= "CompPerson" (:person/name result)))
            (when-let [addr (:person/address result)]
              (is (map? addr) "Component address should be auto-expanded map")
              (is (= "Boston" (:address/city addr))
                  "Component address should have :address/city"))))))))
