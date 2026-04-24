(ns pg-mentat.client-cached-test
  (:require [clojure.test :refer :all]
            [pg-mentat.client-cached :as mentat]))

(def test-conn (atom nil))

(defn test-fixture
  "Setup and teardown for tests."
  [f]
  (reset! test-conn (mentat/connect "http://localhost:8080"))
  (f)
  (reset! test-conn nil))

(use-fixtures :once test-fixture)

(deftest test-cached-db-value
  (testing "Database value caching"
    (let [conn @test-conn
          db1 (mentat/db conn)
          db2 (mentat/db conn)]
      ;; Check if we get DatabaseValue with caching support
      (when (instance? pg_mentat.client_cached.DatabaseValue db1)
        (testing "Cached db values have db-id"
          (is (some? (:db-id db1))))

        (testing "Cached db values have basis-t"
          (is (some? (:basis-t db1))))

        (testing "Same db snapshot is reused from cache"
          (let [cached-dbs (mentat/cached-dbs conn)]
            (is (= (:current cached-dbs) db1))))))))

(deftest test-query-with-cached-db
  (testing "Queries work with cached db value"
    (let [conn @test-conn
          db (mentat/db conn)]
      ;; This should work whether caching is supported or not
      (let [result (mentat/q '[:find ?e :where [?e :db/ident]] db)]
        (is (vector? result))
        (is (every? vector? result))))))

(deftest test-transact-updates-cache
  (testing "Transaction updates cached db value"
    (let [conn @test-conn]
      ;; Initial db
      (let [db-before (mentat/db conn)]
        ;; Transact
        (let [tx-result (mentat/transact conn
                          [{:db/id "temp1"
                            :test/name "Cache Test"
                            :test/value 42}])]
          (is (contains? tx-result :tx))

          ;; Check if we got db-after with caching
          (when (contains? tx-result :db-after)
            (testing "Transaction returns db-after"
              (is (some? (:db-after tx-result))))

            (testing "Cached current db is updated"
              (let [db-after (mentat/db conn)
                    cached-dbs (mentat/cached-dbs conn)]
                (is (= (:current cached-dbs) db-after))
                (when (instance? pg_mentat.client_cached.DatabaseValue db-after)
                  (is (not= (:db-id db-before) (:db-id db-after))))))))))))

(deftest test-batch-queries
  (testing "Batch query execution"
    (let [conn @test-conn
          db (mentat/db conn)]
      (when (instance? pg_mentat.client_cached.DatabaseValue db)
        ;; Only test if we have caching support
        (comment  ; Requires server support for :q-batch
          (let [queries [{:query '[:find ?e :where [?e :db/ident]]
                         :args []}
                        {:query '[:find ?e :in $ ?attr :where [?e ?attr]]
                         :args [:db/ident]}]
                results (mentat/q-batch db queries)]
            (is (vector? results))
            (is (= 2 (count results)))
            (is (every? vector? (first results)))
            (is (every? vector? (second results)))))))))

(deftest test-batch-pulls
  (testing "Batch pull execution"
    (let [conn @test-conn
          db (mentat/db conn)]
      (when (instance? pg_mentat.client_cached.DatabaseValue db)
        ;; Only test if we have caching support
        (comment  ; Requires server support for :pull-batch
          (let [pulls [{:pattern [:db/ident] :eid 1}
                      {:pattern '[*] :eid 2}]
                results (mentat/pull-batch db pulls)]
            (is (vector? results))
            (is (= 2 (count results)))
            (is (every? map? results))))))))

(deftest test-temporal-with-caching
  (testing "Temporal queries create cached db values"
    (let [conn @test-conn
          db (mentat/db conn)]
      ;; Test as-of
      (let [as-of-db (mentat/as-of db 1000)]
        (when (instance? pg_mentat.client_cached.DatabaseValue as-of-db)
          (is (some? (:db-id as-of-db)))
          (is (not= (:db-id db) (:db-id as-of-db)))))

      ;; Test since
      (let [since-db (mentat/since db 1000)]
        (when (instance? pg_mentat.client_cached.DatabaseValue since-db)
          (is (some? (:db-id since-db)))
          (is (not= (:db-id db) (:db-id since-db)))))

      ;; Test history
      (let [history-db (mentat/history db)]
        (when (instance? pg_mentat.client_cached.DatabaseValue history-db)
          (is (some? (:db-id history-db)))
          (is (not= (:db-id db) (:db-id history-db))))))))

(deftest test-with-db-helper
  (testing "with-db ensures consistent snapshot"
    (let [conn @test-conn]
      (let [result (mentat/with-db conn
                     (fn [db]
                       ;; All queries in here use the same db snapshot
                       {:q1 (mentat/q '[:find ?e :where [?e :db/ident]] db)
                        :q2 (mentat/q '[:find (count ?e) . :where [?e :db/ident]] db)}))]
        (is (map? result))
        (is (contains? result :q1))
        (is (contains? result :q2))
        (is (vector? (:q1 result)))
        (is (number? (:q2 result)))))))

(deftest test-cache-management
  (testing "Cache management functions"
    (let [conn @test-conn]
      ;; Create some cached values
      (mentat/db conn)
      (mentat/db conn {:refresh true})

      (testing "cached-dbs returns cache contents"
        (let [cache (mentat/cached-dbs conn)]
          (is (map? cache))
          (when (contains? cache :current)
            (is (some? (:current cache))))))

      (testing "clear-cache! removes cached values"
        (mentat/clear-cache! conn)
        (let [cache (mentat/cached-dbs conn)]
          (is (= {} cache)))))))

(deftest test-backward-compatibility
  (testing "Client works with non-caching servers"
    (let [conn @test-conn
          db (mentat/db conn)]
      ;; These should work regardless of caching support
      (is (some? db))

      ;; Query should work
      (is (vector? (mentat/q '[:find ?e :where [?e :db/ident]] db)))

      ;; Pull should work
      (comment  ; Requires entity to exist
        (is (map? (mentat/pull db [:db/ident] 1))))

      ;; Transact should work
      (let [tx-result (mentat/transact conn
                        [{:db/id "temp1" :test/compat "test"}])]
        (is (contains? tx-result :tx))
        (is (contains? tx-result :tempids))))))

(deftest test-connection-pool
  (testing "Connection pooling"
    (let [pool (mentat/connection-pool "http://localhost:8080" 5)]
      (is (vector? pool))
      (is (= 5 (count pool)))
      (is (every? #(instance? pg_mentat.client_cached.Connection %) pool))

      (testing "with-connection executes with pool connection"
        (mentat/with-connection pool
          (fn [conn]
            (is (instance? pg_mentat.client_cached.Connection conn))
            ;; Should be able to use the connection
            (let [db (mentat/db conn)]
              (is (some? db)))))))))

(deftest test-basis-t-with-caching
  (testing "basis-t returns cached value when available"
    (let [conn @test-conn
          db (mentat/db conn)]
      (when (instance? pg_mentat.client_cached.DatabaseValue db)
        (let [basis (mentat/basis-t db)]
          (is (number? basis))
          ;; Should match cached basis-t
          (is (= basis (:basis-t db))))))))

(deftest test-snapshot-isolation
  (testing "Snapshot isolation with cached db values"
    (let [conn @test-conn]
      ;; Get initial snapshot
      (let [db1 (mentat/db conn)]
        ;; Transact some data
        (mentat/transact conn
          [{:db/id "temp1" :test/isolation "value1"}])

        ;; Get new snapshot
        (let [db2 (mentat/db conn)]
          (when (and (instance? pg_mentat.client_cached.DatabaseValue db1)
                     (instance? pg_mentat.client_cached.DatabaseValue db2))
            ;; Different snapshots should have different db-ids
            (is (not= (:db-id db1) (:db-id db2)))

            ;; Queries on db1 shouldn't see the new data
            ;; (This would require actual server support to test properly)
            (comment
              (let [q1 (mentat/q '[:find ?e :where [?e :test/isolation "value1"]] db1)
                    q2 (mentat/q '[:find ?e :where [?e :test/isolation "value1"]] db2)]
                (is (empty? q1))  ; db1 shouldn't see new data
                (is (not-empty q2))))))))))  ; db2 should see new data

(deftest test-integration-with-caching
  (testing "Full integration workflow with caching"
    (let [conn @test-conn]
      ;; Define schema (if needed)
      (comment  ; Full integration test
        (mentat/transact conn
          [{:db/ident :person/name
            :db/valueType :db.type/string
            :db/cardinality :db.cardinality/one}
           {:db/ident :person/email
            :db/valueType :db.type/string
            :db/cardinality :db.cardinality/one
            :db/unique :db.unique/identity}])

        ;; Get initial db snapshot
        (let [db-before (mentat/db conn)]
          ;; Create entities
          (let [tx-result (mentat/transact conn
                            [{:db/id "alice"
                              :person/name "Alice"
                              :person/email "alice@example.com"}
                             {:db/id "bob"
                              :person/name "Bob"
                              :person/email "bob@example.com"}])]
            ;; Use db-after if available
            (let [db-after (or (:db-after tx-result) (mentat/db conn))]
              ;; Batch queries on same snapshot
              (when (instance? pg_mentat.client_cached.DatabaseValue db-after)
                (let [batch-results (mentat/q-batch db-after
                                      [{:query '[:find ?name :where [?e :person/name ?name]]
                                        :args []}
                                       {:query '[:find ?e :in $ ?email
                                                :where [?e :person/email ?email]]
                                        :args ["alice@example.com"]}])]
                  (is (= 2 (count batch-results)))))

              ;; Verify snapshot isolation
              (let [names-before (mentat/q '[:find ?name :where [?e :person/name ?name]] db-before)
                    names-after (mentat/q '[:find ?name :where [?e :person/name ?name]] db-after)]
                (is (< (count names-before) (count names-after))))))))))))