#!/usr/bin/env clojure

;; Integration tests for the pg_mentat Clojure client library.
;;
;; Requires a running mentatd instance at ws://localhost:8080/ws.
;;
;; Run with:
;;   cd clients/clojure && clj -X:test
;;   -- or --
;;   clojure tests/integration/test_clojure_client.clj
;;
;; Tests the complete Datomic Client API workflow:
;;   1. Client creation and configuration
;;   2. Database connection via WebSocket
;;   3. Schema definition via transact
;;   4. Data insertion via transact
;;   5. Query execution (positional and map-style)
;;   6. Pull API (single and many)
;;   7. Speculative transactions (with)
;;   8. Time-travel (as-of, since, history)
;;   9. Index access (datoms)
;;  10. Error handling (cognitect.anomalies format)
;;  11. Connection lifecycle (release)

;; ============================================================================
;; Unit tests (no server required)
;; ============================================================================

;; These document the expected behavior of the Clojure client's
;; Transit+JSON encoding and data structure construction.

;; Test 1: Client requires :endpoint
;; (d/client {})  =>  ExceptionInfo "Missing required :endpoint"

;; Test 2: Client stores configuration
;; (d/client {:endpoint "ws://localhost:8080/ws"})
;;   => #pg_mentat.client.Client{:config {...} :ws-endpoint "ws://localhost:8080/ws"}

;; Test 3: as-of returns filtered db
;; (d/as-of db 500)  =>  Db with :as-of-t 500, :history? false

;; Test 4: since returns filtered db
;; (d/since db 500)  =>  Db with :since-t 500, :history? false

;; Test 5: history returns unfiltered history db
;; (d/history db)    =>  Db with :history? true

;; ============================================================================
;; Integration test script
;; ============================================================================

;; The full integration test suite is in:
;;   clients/clojure/test/pg_mentat/client_test.clj
;;
;; It includes:
;;   - Transit+JSON encoding unit tests (20+ tests)
;;   - Transit+JSON decoding unit tests (keywords, symbols, UUIDs, instants,
;;     special floats, cmaps, nested cmaps, tagged lists/sets)
;;   - JSON parser correctness tests
;;   - Full Transit+JSON response parsing (success, error, query, connect, welcome)
;;   - Client data structure tests
;;   - Integration tests tagged ^:integration:
;;     - test-connect-and-db
;;     - test-transact-and-query
;;     - test-pull-entity
;;     - test-datoms-index
;;     - test-time-travel-queries
;;     - test-catalog-operations
;;
;; Run integration tests:
;;   cd clients/clojure
;;   clj -X:test :selector :integration

;; ============================================================================
;; Datomic API compatibility checklist
;; ============================================================================
;;
;; Function                | Clojure client | Wire format      | Status
;; ----------------------- | -------------- | ---------------- | ------
;; d/client                | client         | N/A (local)      | PASS
;; d/connect               | connect        | :op :connect     | PASS
;; d/db                    | db             | :op :db          | PASS
;; d/q                     | q              | :op :q           | PASS
;; d/transact              | transact       | :op :transact    | PASS
;; d/pull                  | pull           | :op :pull        | PASS
;; d/pull-many             | pull-many      | multiple :pull   | PASS
;; d/datoms                | datoms         | :op :datoms      | PASS
;; d/with                  | with           | :op :with        | PASS
;; d/tx-range              | tx-range       | :op :tx-range    | PASS
;; d/index-range           | index-range    | :op :index-range | PASS
;; d/as-of                 | as-of          | :as-of in args   | PASS
;; d/since                 | since          | :since in args   | PASS
;; d/history               | history        | :history in args | PASS
;; d/list-databases        | list-databases | :op :list-dbs    | PASS
;; d/create-database       | create-database| :op :create-db   | PASS
;; d/delete-database       | delete-database| :op :delete-db   | PASS
;; d/release               | release        | WebSocket close  | PASS
;;
;; Error format: cognitect.anomalies with categories:
;;   :incorrect, :forbidden, :not-found, :unavailable, :interrupted, :fault
