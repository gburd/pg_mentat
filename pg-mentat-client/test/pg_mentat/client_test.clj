(ns pg-mentat.client-test
  (:require [clojure.test :refer :all]
            [pg-mentat.client :as mentat]))

(def test-conn (atom nil))

(defn test-fixture
  "Setup and teardown for tests."
  [f]
  (reset! test-conn (mentat/connect "http://localhost:8080"))
  (f)
  (reset! test-conn nil))

(use-fixtures :once test-fixture)

(deftest test-connect
  (testing "Connection object creation"
    (let [conn (mentat/connect "http://localhost:8080")]
      (is (instance? pg_mentat.client.Connection conn))
      (is (= "http://localhost:8080" (:uri conn))))))

(deftest test-db
  (testing "Database value retrieval"
    (let [db (mentat/db @test-conn)]
      (is (not (nil? db)))
      (is (= @test-conn db)))))

(deftest test-query
  (testing "Basic query execution"
    (let [db (mentat/db @test-conn)]
      ;; Test finding all entities with :db/ident
      (let [result (mentat/q '[:find ?e :where [?e :db/ident]] db)]
        (is (vector? result))
        (is (every? vector? result)))

      ;; Test query with inputs
      (let [result (mentat/q '[:find ?e
                              :in $ ?attr
                              :where [?e ?attr]]
                            db :db/ident)]
        (is (vector? result)))))

  (testing "Query with multiple inputs"
    (let [db (mentat/db @test-conn)]
      ;; This will only work if there's data in the database
      (comment
        (let [result (mentat/q '[:find ?e ?v
                                :in $ ?attr ?value
                                :where [?e ?attr ?v]
                                       [(= ?v ?value)]]
                              db :person/name "Alice")]
          (is (vector? result)))))))

(deftest test-transact
  (testing "Transaction execution"
    ;; Test with entity map
    (let [tx-result (mentat/transact @test-conn
                      [{:db/id "tempid1"
                        :test/name "Test User"
                        :test/email "test@example.com"}])]
      (is (contains? tx-result :tx))
      (is (contains? tx-result :tempids))
      (is (map? (:tempids tx-result))))

    ;; Test with :db/add form
    (comment  ; This requires an existing entity
      (let [tx-result (mentat/transact @test-conn
                        [[:db/add 10001 :test/age 30]])]
        (is (contains? tx-result :tx))))

    ;; Test with multiple operations
    (let [tx-result (mentat/transact @test-conn
                      [{:db/id "temp1" :test/name "User 1"}
                       {:db/id "temp2" :test/name "User 2"}])]
      (is (contains? tx-result :tempids))
      (is (>= (count (:tempids tx-result)) 2)))))

(deftest test-pull
  (testing "Pull API"
    (let [db (mentat/db @test-conn)]
      ;; Test pulling schema attribute
      (let [result (mentat/pull db [:db/ident] 1)]
        (is (map? result)))

      ;; Test pull with wildcard
      (comment  ; Requires entity to exist
        (let [result (mentat/pull db '[*] 10001)]
          (is (map? result))))

      ;; Test pull with specific attributes
      (comment  ; Requires entity to exist
        (let [result (mentat/pull db [:person/name :person/email] 10001)]
          (is (map? result)))))))

(deftest test-pull-many
  (testing "Pull-many API"
    (let [db (mentat/db @test-conn)]
      (comment  ; Requires entities to exist
        (let [result (mentat/pull-many db [:db/ident] [1 2 3])]
          (is (vector? result))
          (is (every? map? result)))))))

(deftest test-entity
  (testing "Entity retrieval"
    (let [db (mentat/db @test-conn)]
      (comment  ; Requires entity to exist
        (let [entity (mentat/entity db 1)]
          (is (map? entity)))))))

(deftest test-datoms
  (testing "Datoms index access"
    (let [db (mentat/db @test-conn)]
      ;; Test EAVT index
      (comment  ; Requires entity to exist
        (let [datoms (mentat/datoms db :eavt [10001])]
          (is (vector? datoms))
          (is (every? vector? datoms))))

      ;; Test AVET index
      (comment  ; Requires attribute and value to exist
        (let [datoms (mentat/datoms db :avet [:person/name "Alice"])]
          (is (vector? datoms)))))))

(deftest test-temporal-functions
  (testing "Temporal database functions"
    (let [db (mentat/db @test-conn)]
      ;; Test as-of
      (let [as-of-db (mentat/as-of db 1000)]
        (is (contains? as-of-db :as-of))
        (is (= 1000 (:as-of as-of-db))))

      ;; Test since
      (let [since-db (mentat/since db 1000)]
        (is (contains? since-db :since))
        (is (= 1000 (:since since-db))))

      ;; Test history
      (let [history-db (mentat/history db)]
        (is (contains? history-db :history))
        (is (true? (:history history-db)))))))

(deftest test-basis-t
  (testing "Basis-t retrieval"
    (let [db (mentat/db @test-conn)]
      (comment  ; Requires mentatd to implement basis-t
        (let [basis (mentat/basis-t db)]
          (is (number? basis)))))))

(deftest test-with
  (testing "Speculative transaction"
    (let [db (mentat/db @test-conn)]
      (comment  ; Requires mentatd to implement with
        (let [result (mentat/with db [{:db/id "temp1" :test/name "Speculative"}])]
          (is (map? result))
          (is (contains? result :tx-data)))))))

(deftest test-schema
  (testing "Schema retrieval"
    (let [db (mentat/db @test-conn)]
      (comment  ; Requires mentatd to implement schema
        (let [schema (mentat/schema db)]
          (is (map? schema)))))))

(deftest test-retraction-helpers
  (testing "Retraction helper functions"
    ;; Test retract with value
    (let [retraction (mentat/retract 10001 :person/name "Old Name")]
      (is (vector? retraction))
      (is (= :db/retract (first retraction)))
      (is (= 10001 (second retraction)))
      (is (= :person/name (nth retraction 2)))
      (is (= "Old Name" (nth retraction 3))))

    ;; Test retract attribute
    (let [retraction (mentat/retract 10001 :person/email)]
      (is (vector? retraction))
      (is (= :db/retractAttribute (first retraction))))

    ;; Test retract entity
    (let [retraction (mentat/retract-entity 10001)]
      (is (vector? retraction))
      (is (= :db/retractEntity (first retraction)))
      (is (= 10001 (second retraction))))))

(deftest test-lookup-ref
  (testing "Lookup ref utilities"
    ;; Test lookup-ref? predicate
    (is (true? (mentat/lookup-ref? [:person/email "alice@example.com"])))
    (is (false? (mentat/lookup-ref? "not-a-ref")))
    (is (false? (mentat/lookup-ref? [:too :many :elements])))
    (is (false? (mentat/lookup-ref? ["string" "not-keyword"])))

    ;; Test resolve-lookup-ref
    (let [db (mentat/db @test-conn)]
      (comment  ; Requires data to exist
        (let [eid (mentat/resolve-lookup-ref db [:person/email "alice@example.com"])]
          (is (or (nil? eid) (number? eid))))))))

(deftest test-tempid
  (testing "Tempid generation"
    ;; Test tempid without ID
    (let [tid (mentat/tempid :db.part/user)]
      (is (string? tid))
      (is (.startsWith tid "tempid-")))

    ;; Test tempid with ID
    (let [tid (mentat/tempid :db.part/user 42)]
      (is (string? tid))
      (is (.contains tid "42")))

    ;; Test tempid? predicate
    (is (true? (mentat/tempid? "tempid-123")))
    (is (true? (mentat/tempid? "temp-foo")))
    (is (false? (mentat/tempid? "not-a-tempid")))
    (is (false? (mentat/tempid? 123)))))

(deftest test-error-handling
  (testing "Error handling in requests"
    ;; Test connection to non-existent server
    (let [bad-conn (mentat/connect "http://localhost:9999")]
      (is (thrown? Exception
                   (mentat/q '[:find ?e :where [?e :db/ident]] (mentat/db bad-conn)))))

    ;; Test malformed query
    (let [db (mentat/db @test-conn)]
      (comment  ; Depends on mentatd error handling
        (is (thrown? Exception
                     (mentat/q "not-a-query" db)))))))

(deftest test-integration
  (testing "Full integration workflow"
    (let [conn @test-conn
          db (mentat/db conn)]
      ;; Create schema
      (comment  ; Full integration test
        (mentat/transact conn
          [{:db/ident :person/name
            :db/valueType :db.type/string
            :db/cardinality :db.cardinality/one}
           {:db/ident :person/email
            :db/valueType :db.type/string
            :db/cardinality :db.cardinality/one
            :db/unique :db.unique/identity}
           {:db/ident :person/age
            :db/valueType :db.type/long
            :db/cardinality :db.cardinality/one}
           {:db/ident :person/friends
            :db/valueType :db.type/ref
            :db/cardinality :db.cardinality/many}])

        ;; Create entities
        (let [tx-result (mentat/transact conn
                          [{:db/id "alice"
                            :person/name "Alice"
                            :person/email "alice@example.com"
                            :person/age 30}
                           {:db/id "bob"
                            :person/name "Bob"
                            :person/email "bob@example.com"
                            :person/age 25
                            :person/friends "alice"}])]
          (is (contains? (:tempids tx-result) "alice"))
          (is (contains? (:tempids tx-result) "bob"))

          ;; Query data
          (let [db2 (mentat/db conn)
                people (mentat/q '[:find ?e ?name
                                  :where [?e :person/name ?name]]
                                db2)]
            (is (>= (count people) 2)))

          ;; Pull data
          (let [alice-id (get (:tempids tx-result) "alice")
                alice-data (mentat/pull db2
                             [:person/name
                              :person/email
                              :person/age]
                             alice-id)]
            (is (= "Alice" (:person/name alice-data)))
            (is (= "alice@example.com" (:person/email alice-data)))
            (is (= 30 (:person/age alice-data))))

          ;; Update data
          (let [alice-id (get (:tempids tx-result) "alice")]
            (mentat/transact conn
              [[:db/add alice-id :person/age 31]]))

          ;; Query with lookup ref
          (let [db3 (mentat/db conn)
                alice (mentat/pull db3 '[*] [:person/email "alice@example.com"])]
            (is (= 31 (:person/age alice)))))))))