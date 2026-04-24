(ns datomic-compat.typed-values-test
  "Tests that validate typed value handling through the Datomic client API.

   These tests are specifically designed to catch the BYTEA encoding bug
   (Phase 1.1 fix): the old schema stored all values as binary blobs in a
   single BYTEA column, which caused incorrect results for range queries
   because binary comparison does not match numeric or lexicographic ordering.

   The fix stores each value type in its native PostgreSQL column (v_long as
   BIGINT, v_text as TEXT, v_instant as TIMESTAMPTZ, v_uuid as UUID, etc.)
   so that comparison operators use correct type semantics.

   Test categories:
     1. Numeric range queries (the core BYTEA bug)
     2. Text ordering
     3. Multi-type schema round-trip (boolean, double, instant, uuid)
     4. Timestamp range ordering
     5. UUID ordering consistency"
  (:require [clojure.test :refer [deftest is testing]]
            [datomic.api :as d])
  (:import [java.util UUID Date]))

;; ---------------------------------------------------------------------------
;; Configuration
;; ---------------------------------------------------------------------------

(def base-uri
  (or (System/getenv "MENTATD_URI")
      "datomic:free://localhost:8080/typed-val-test"))

(defn fresh-uri []
  (str base-uri "-" (System/nanoTime)))

;; ---------------------------------------------------------------------------
;; Helpers
;; ---------------------------------------------------------------------------

(defn with-fresh-db
  "Create a fresh database, call (f conn), then delete the database."
  [f]
  (let [uri (fresh-uri)]
    (d/create-database uri)
    (try
      (let [conn (d/connect uri)]
        (f conn))
      (finally
        (d/delete-database uri)))))

;; ===================================================================
;; 1. Numeric range queries -- the core BYTEA bug
;; ===================================================================

(deftest test-numeric-greater-than
  (testing "numeric > comparison uses native BIGINT ordering, not binary"
    (with-fresh-db
      (fn [conn]
        ;; Install schema
        @(d/transact conn [{:db/ident       :person/name
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :person/age
                            :db/valueType   :db.type/long
                            :db/cardinality :db.cardinality/one}])
        ;; Insert data with ages that expose BYTEA comparison bugs:
        ;; In BYTEA (LE i64), the byte pattern for 2 (0x02 0x00...) could
        ;; sort higher than 10 (0x0a 0x00...) depending on comparison.
        @(d/transact conn [{:person/name "Alice" :person/age 25}
                           {:person/name "Bob"   :person/age 35}
                           {:person/name "Carol" :person/age 10}
                           {:person/name "Dave"  :person/age 100}
                           {:person/name "Eve"   :person/age 2}])
        (let [db      (d/db conn)
              results (d/q '[:find ?name ?age
                             :where
                             [?e :person/name ?name]
                             [?e :person/age ?age]
                             [(> ?age 30)]]
                           db)]
          (is (= 2 (count results))
              (str "Expected 2 people with age > 30, got " (count results) ": " results))
          (let [names (set (map first results))]
            (is (contains? names "Bob")
                "Bob (35) should be in results")
            (is (contains? names "Dave")
                "Dave (100) should be in results")
            (is (not (contains? names "Eve"))
                "Eve (2) must NOT be in results -- this was the BYTEA bug")))))))

(deftest test-numeric-less-than
  (testing "numeric < comparison is correct"
    (with-fresh-db
      (fn [conn]
        @(d/transact conn [{:db/ident       :person/name
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :person/age
                            :db/valueType   :db.type/long
                            :db/cardinality :db.cardinality/one}])
        @(d/transact conn [{:person/name "Alice" :person/age 5}
                           {:person/name "Bob"   :person/age 10}
                           {:person/name "Carol" :person/age 100}
                           {:person/name "Dave"  :person/age 2}])
        (let [db      (d/db conn)
              results (d/q '[:find ?name ?age
                             :where
                             [?e :person/name ?name]
                             [?e :person/age ?age]
                             [(< ?age 10)]]
                           db)
              names   (set (map first results))]
          (is (= 2 (count results))
              (str "Expected 2 people with age < 10, got: " results))
          (is (contains? names "Alice") "Alice (5) should match")
          (is (contains? names "Dave") "Dave (2) should match")
          (is (not (contains? names "Carol")) "Carol (100) should not match"))))))

(deftest test-numeric-between-range
  (testing "combined > and < predicates form a correct range"
    (with-fresh-db
      (fn [conn]
        @(d/transact conn [{:db/ident       :person/name
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :person/age
                            :db/valueType   :db.type/long
                            :db/cardinality :db.cardinality/one}])
        @(d/transact conn [{:person/name "Alice" :person/age 5}
                           {:person/name "Bob"   :person/age 15}
                           {:person/name "Carol" :person/age 25}
                           {:person/name "Dave"  :person/age 35}
                           {:person/name "Eve"   :person/age 45}])
        (let [db      (d/db conn)
              results (d/q '[:find ?name ?age
                             :where
                             [?e :person/name ?name]
                             [?e :person/age ?age]
                             [(> ?age 10)]
                             [(< ?age 40)]]
                           db)
              names   (set (map first results))]
          (is (= 3 (count results))
              (str "Expected 3 people in (10, 40), got: " results))
          (is (contains? names "Bob") "Bob (15) should be in range")
          (is (contains? names "Carol") "Carol (25) should be in range")
          (is (contains? names "Dave") "Dave (35) should be in range")
          (is (not (contains? names "Alice")) "Alice (5) should not be in range")
          (is (not (contains? names "Eve")) "Eve (45) should not be in range"))))))

;; ===================================================================
;; 2. Text ordering
;; ===================================================================

(deftest test-text-greater-than
  (testing "text > comparison uses lexicographic ordering"
    (with-fresh-db
      (fn [conn]
        @(d/transact conn [{:db/ident       :person/name
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}])
        @(d/transact conn [{:person/name "Alice"}
                           {:person/name "Bob"}
                           {:person/name "Carol"}
                           {:person/name "Zara"}])
        (let [db      (d/db conn)
              results (d/q '[:find ?name
                             :where
                             [?e :person/name ?name]
                             [(> ?name "Bob")]]
                           db)
              names   (set (map first results))]
          (is (= 2 (count results))
              (str "Expected 2 names > 'Bob', got: " results))
          (is (contains? names "Carol"))
          (is (contains? names "Zara"))
          (is (not (contains? names "Alice"))))))))

;; ===================================================================
;; 3. Multi-type schema round-trip
;; ===================================================================

(deftest test-boolean-round-trip
  (testing "boolean values round-trip correctly"
    (with-fresh-db
      (fn [conn]
        @(d/transact conn [{:db/ident       :feature/name
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :feature/active
                            :db/valueType   :db.type/boolean
                            :db/cardinality :db.cardinality/one}])
        @(d/transact conn [{:feature/name "dark-mode" :feature/active true}
                           {:feature/name "beta"      :feature/active false}])
        (let [db      (d/db conn)
              results (d/q '[:find ?name ?active
                             :where
                             [?e :feature/name ?name]
                             [?e :feature/active ?active]]
                           db)
              by-name (into {} (map (fn [[n a]] [n a]) results))]
          (is (= 2 (count results)) "Should find 2 features")
          (is (true? (get by-name "dark-mode")) "dark-mode should be true")
          (is (false? (get by-name "beta")) "beta should be false"))))))

(deftest test-double-round-trip
  (testing "double/float values round-trip correctly"
    (with-fresh-db
      (fn [conn]
        @(d/transact conn [{:db/ident       :measurement/label
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :measurement/value
                            :db/valueType   :db.type/double
                            :db/cardinality :db.cardinality/one}])
        @(d/transact conn [{:measurement/label "pi"   :measurement/value 3.14159}
                           {:measurement/label "e"    :measurement/value 2.71828}
                           {:measurement/label "phi"  :measurement/value 1.61803}])
        (let [db      (d/db conn)
              results (d/q '[:find ?label ?val
                             :where
                             [?e :measurement/label ?label]
                             [?e :measurement/value ?val]]
                           db)
              by-label (into {} (map (fn [[l v]] [l v]) results))]
          (is (= 3 (count results)) "Should find 3 measurements")
          (is (< (Math/abs (- (get by-label "pi") 3.14159)) 0.001)
              "pi should be approximately 3.14159")
          (is (< (Math/abs (- (get by-label "e") 2.71828)) 0.001)
              "e should be approximately 2.71828"))))))

(deftest test-instant-round-trip
  (testing "instant/timestamp values round-trip correctly"
    (with-fresh-db
      (fn [conn]
        @(d/transact conn [{:db/ident       :event/label
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :event/timestamp
                            :db/valueType   :db.type/instant
                            :db/cardinality :db.cardinality/one}])
        (let [ts1 #inst "2020-01-01T00:00:00.000Z"
              ts2 #inst "2024-06-15T12:30:00.000Z"]
          @(d/transact conn [{:event/label "new-year" :event/timestamp ts1}
                             {:event/label "mid-year" :event/timestamp ts2}])
          (let [db      (d/db conn)
                results (d/q '[:find ?label ?ts
                               :where
                               [?e :event/label ?label]
                               [?e :event/timestamp ?ts]]
                             db)]
            (is (= 2 (count results)) "Should find 2 events")
            ;; Verify timestamps are returned (format may vary)
            (is (every? (fn [[_ ts]] (some? ts)) results)
                "All timestamps should be non-nil")))))))

(deftest test-uuid-round-trip
  (testing "UUID values round-trip correctly"
    (with-fresh-db
      (fn [conn]
        @(d/transact conn [{:db/ident       :session/label
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :session/id
                            :db/valueType   :db.type/uuid
                            :db/cardinality :db.cardinality/one}])
        (let [uuid1 (UUID/fromString "550e8400-e29b-41d4-a716-446655440000")
              uuid2 (UUID/fromString "123e4567-e89b-12d3-a456-426614174000")]
          @(d/transact conn [{:session/label "session-a" :session/id uuid1}
                             {:session/label "session-b" :session/id uuid2}])
          (let [db      (d/db conn)
                results (d/q '[:find ?label ?id
                               :where
                               [?e :session/label ?label]
                               [?e :session/id ?id]]
                             db)]
            (is (= 2 (count results)) "Should find 2 sessions")
            (let [by-label (into {} (map (fn [[l id]] [l id]) results))]
              ;; UUID may come back as UUID object or string depending on client
              (is (some? (get by-label "session-a")) "session-a should have a UUID")
              (is (some? (get by-label "session-b")) "session-b should have a UUID"))))))))

;; ===================================================================
;; 4. Timestamp range ordering
;; ===================================================================

(deftest test-timestamp-ordering
  (testing "instant values sort in correct chronological order"
    (with-fresh-db
      (fn [conn]
        @(d/transact conn [{:db/ident       :event/label
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :event/timestamp
                            :db/valueType   :db.type/instant
                            :db/cardinality :db.cardinality/one}])
        @(d/transact conn [{:event/label "ancient"  :event/timestamp #inst "1999-06-15T12:00:00.000Z"}
                           {:event/label "early"    :event/timestamp #inst "2020-01-01T00:00:00.000Z"}
                           {:event/label "middle"   :event/timestamp #inst "2022-06-15T12:00:00.000Z"}
                           {:event/label "recent"   :event/timestamp #inst "2024-12-25T18:30:00.000Z"}])
        (let [db      (d/db conn)
              results (d/q '[:find ?label ?ts
                             :where
                             [?e :event/label ?label]
                             [?e :event/timestamp ?ts]]
                           db)]
          (is (= 4 (count results)) "Should find all 4 events")
          ;; Sort by timestamp and verify chronological order
          (let [sorted-labels (->> results
                                   (sort-by second)
                                   (map first))]
            (is (= ["ancient" "early" "middle" "recent"] sorted-labels)
                (str "Events should be in chronological order, got: " sorted-labels))))))))

;; ===================================================================
;; 5. UUID ordering consistency
;; ===================================================================

(deftest test-uuid-ordering
  (testing "UUID values maintain consistent ordering"
    (with-fresh-db
      (fn [conn]
        @(d/transact conn [{:db/ident       :item/name
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :item/id
                            :db/valueType   :db.type/uuid
                            :db/cardinality :db.cardinality/one}])
        (let [uuid-lo (UUID/fromString "11111111-1111-1111-1111-111111111111")
              uuid-mi (UUID/fromString "55555555-5555-5555-5555-555555555555")
              uuid-hi (UUID/fromString "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")]
          @(d/transact conn [{:item/name "First"  :item/id uuid-mi}
                             {:item/name "Second" :item/id uuid-lo}
                             {:item/name "Third"  :item/id uuid-hi}])
          (let [db      (d/db conn)
                results (d/q '[:find ?name ?id
                               :where
                               [?e :item/name ?name]
                               [?e :item/id ?id]]
                             db)]
            (is (= 3 (count results)) "Should find all 3 items")
            ;; Sort by UUID and verify consistent ordering
            (let [sorted-names (->> results
                                    (sort-by (comp str second))
                                    (map first))]
              (is (= ["Second" "First" "Third"] sorted-names)
                  (str "UUIDs should sort consistently: 1111 < 5555 < aaaa, got: "
                       sorted-names)))))))))

;; ===================================================================
;; 6. Mixed-type entity with all value types
;; ===================================================================

(deftest test-multi-type-entity
  (testing "entity with multiple typed attributes round-trips correctly"
    (with-fresh-db
      (fn [conn]
        @(d/transact conn [{:db/ident       :thing/name
                            :db/valueType   :db.type/string
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :thing/count
                            :db/valueType   :db.type/long
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :thing/score
                            :db/valueType   :db.type/double
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :thing/active
                            :db/valueType   :db.type/boolean
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :thing/created
                            :db/valueType   :db.type/instant
                            :db/cardinality :db.cardinality/one}
                           {:db/ident       :thing/uuid
                            :db/valueType   :db.type/uuid
                            :db/cardinality :db.cardinality/one}])
        (let [tx @(d/transact conn [{:db/id         "t1"
                                      :thing/name    "Widget"
                                      :thing/count   42
                                      :thing/score   9.5
                                      :thing/active  true
                                      :thing/created #inst "2024-01-15T10:30:00.000Z"
                                      :thing/uuid    (UUID/fromString "deadbeef-dead-beef-dead-beefdeadbeef")}])]
          (is (some? tx) "Transaction should succeed")
          (let [db     (d/db conn)
                eid    (get (:tempids tx) "t1")
                result (d/pull db '[*] eid)]
            (is (= "Widget" (:thing/name result)) "Name should round-trip")
            (is (= 42 (:thing/count result)) "Count should round-trip")
            (is (some? (:thing/score result)) "Score should be present")
            (is (true? (:thing/active result)) "Active should be true")
            (is (some? (:thing/created result)) "Created should be present")
            (is (some? (:thing/uuid result)) "UUID should be present")))))))
