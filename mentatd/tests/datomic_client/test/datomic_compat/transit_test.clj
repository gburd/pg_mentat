(ns datomic-compat.transit-test
  "Transit wire format compatibility tests for mentatd.

   Tests that Transit+JSON and Transit+MessagePack encoding/decoding
   works correctly at the HTTP wire level.  Each test sends a raw
   Transit-encoded request body with the appropriate Content-Type and
   Accept headers, then verifies the response is valid Transit.

   Test categories:
     1. Transit+JSON request/response round-trip
     2. Transit+MessagePack request/response round-trip
     3. Content-Type negotiation (mixed input/output formats)
     4. Transit value type fidelity (keywords, symbols, UUIDs, etc.)"
  (:require [clojure.test :refer [deftest is testing]]
            [cognitect.transit :as transit])
  (:import [java.io ByteArrayOutputStream ByteArrayInputStream]
           [java.net URI HttpURLConnection URL]))

;; ---------------------------------------------------------------------------
;; Configuration
;; ---------------------------------------------------------------------------

(def mentatd-url
  "Base URL for the mentatd server."
  (or (System/getenv "MENTATD_URL") "http://localhost:8080"))

;; ---------------------------------------------------------------------------
;; Transit encoding / decoding helpers
;; ---------------------------------------------------------------------------

(defn encode-transit-json
  "Encode a Clojure value as a Transit+JSON byte array."
  [value]
  (let [baos (ByteArrayOutputStream.)
        writer (transit/writer baos :json)]
    (transit/write writer value)
    (.toByteArray baos)))

(defn decode-transit-json
  "Decode a Transit+JSON byte array into a Clojure value."
  [^bytes bs]
  (let [bais (ByteArrayInputStream. bs)
        reader (transit/reader bais :json)]
    (transit/read reader)))

(defn encode-transit-msgpack
  "Encode a Clojure value as a Transit+MessagePack byte array."
  [value]
  (let [baos (ByteArrayOutputStream.)
        writer (transit/writer baos :msgpack)]
    (transit/write writer value)
    (.toByteArray baos)))

(defn decode-transit-msgpack
  "Decode a Transit+MessagePack byte array into a Clojure value."
  [^bytes bs]
  (let [bais (ByteArrayInputStream. bs)
        reader (transit/reader bais :msgpack)]
    (transit/read reader)))

;; ---------------------------------------------------------------------------
;; HTTP helpers
;; ---------------------------------------------------------------------------

(defn post-raw
  "Send a raw HTTP POST request to mentatd.
   Returns {:status int :body bytes :content-type string}."
  [path ^bytes body content-type accept]
  (let [url (URL. (str mentatd-url path))
        ^HttpURLConnection conn (.openConnection url)]
    (.setRequestMethod conn "POST")
    (.setDoOutput conn true)
    (.setRequestProperty conn "Content-Type" content-type)
    (.setRequestProperty conn "Accept" accept)
    (.setConnectTimeout conn 5000)
    (.setReadTimeout conn 5000)
    (with-open [os (.getOutputStream conn)]
      (.write os body))
    (let [status (.getResponseCode conn)
          ct (.getHeaderField conn "Content-Type")
          is (if (< status 400)
               (.getInputStream conn)
               (.getErrorStream conn))
          response-bytes (when is (.readAllBytes is))]
      {:status status
       :body response-bytes
       :content-type ct})))

;; ===================================================================
;; 1. Transit+JSON request/response round-trip
;; ===================================================================

(deftest test-transit-json-health
  (testing "Health check via Transit+JSON"
    (let [request-data {:op :health}
          body (encode-transit-json request-data)
          response (post-raw "/" body
                             "application/transit+json"
                             "application/transit+json")]
      (is (= 200 (:status response))
          "Health check should return 200")
      (is (some? (:body response))
          "Response body should not be nil")
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          (is (map? result) "Response should decode to a map")
          (is (contains? result :result)
              "Response should contain :result key"))))))

(deftest test-transit-json-list-dbs
  (testing "List databases via Transit+JSON"
    (let [request-data {:op :list-dbs}
          body (encode-transit-json request-data)
          response (post-raw "/" body
                             "application/transit+json"
                             "application/transit+json")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          (is (map? result))
          (is (contains? result :result))
          (is (sequential? (:result result))
              ":result should be a list of database names"))))))

(deftest test-transit-json-connect
  (testing "Connect operation via Transit+JSON"
    (let [request-data {:op :connect
                        :args {:db-name "postgres"}}
          body (encode-transit-json request-data)
          response (post-raw "/" body
                             "application/transit+json"
                             "application/transit+json")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          (is (map? result)))))))

(deftest test-transit-json-query
  (testing "Query operation via Transit+JSON"
    (let [request-data {:op :q
                        :args {:query "[:find ?e :where [?e :name]]"
                               :args []}}
          body (encode-transit-json request-data)
          response (post-raw "/" body
                             "application/transit+json"
                             "application/transit+json")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          (is (map? result)))))))

(deftest test-transit-json-invalid-op
  (testing "Invalid operation returns error via Transit+JSON"
    (let [request-data {:op :nonexistent-op}
          body (encode-transit-json request-data)
          response (post-raw "/" body
                             "application/transit+json"
                             "application/transit+json")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          (is (map? result))
          (is (contains? result :error)
              "Invalid operation should return :error"))))))

;; ===================================================================
;; 2. Transit+MessagePack request/response round-trip
;; ===================================================================

(deftest test-transit-msgpack-health
  (testing "Health check via Transit+MessagePack"
    (let [request-data {:op :health}
          body (encode-transit-msgpack request-data)
          response (post-raw "/" body
                             "application/transit+msgpack"
                             "application/transit+msgpack")]
      (is (= 200 (:status response))
          "Health check should return 200")
      (when (:body response)
        (let [result (decode-transit-msgpack (:body response))]
          (is (map? result) "Response should decode to a map")
          (is (contains? result :result)
              "Response should contain :result key"))))))

(deftest test-transit-msgpack-list-dbs
  (testing "List databases via Transit+MessagePack"
    (let [request-data {:op :list-dbs}
          body (encode-transit-msgpack request-data)
          response (post-raw "/" body
                             "application/transit+msgpack"
                             "application/transit+msgpack")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-msgpack (:body response))]
          (is (map? result))
          (is (contains? result :result)))))))

(deftest test-transit-msgpack-connect
  (testing "Connect operation via Transit+MessagePack"
    (let [request-data {:op :connect
                        :args {:db-name "postgres"}}
          body (encode-transit-msgpack request-data)
          response (post-raw "/" body
                             "application/transit+msgpack"
                             "application/transit+msgpack")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-msgpack (:body response))]
          (is (map? result)))))))

(deftest test-transit-msgpack-query
  (testing "Query operation via Transit+MessagePack"
    (let [request-data {:op :q
                        :args {:query "[:find ?e :where [?e :name]]"
                               :args []}}
          body (encode-transit-msgpack request-data)
          response (post-raw "/" body
                             "application/transit+msgpack"
                             "application/transit+msgpack")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-msgpack (:body response))]
          (is (map? result)))))))

;; ===================================================================
;; 3. Content-Type negotiation (mixed formats)
;; ===================================================================

(deftest test-transit-json-request-edn-response
  (testing "Transit+JSON request with EDN response (Accept: application/edn)"
    (let [request-data {:op :health}
          body (encode-transit-json request-data)
          response (post-raw "/" body
                             "application/transit+json"
                             "application/edn")]
      (is (= 200 (:status response)))
      (is (= "application/edn" (:content-type response))
          "Response Content-Type should be application/edn")
      (when (:body response)
        (let [body-str (String. ^bytes (:body response) "UTF-8")]
          (is (.contains body-str ":result")
              "EDN response should contain :result"))))))

(deftest test-transit-json-request-msgpack-response
  (testing "Transit+JSON request with Transit+MessagePack response"
    (let [request-data {:op :health}
          body (encode-transit-json request-data)
          response (post-raw "/" body
                             "application/transit+json"
                             "application/transit+msgpack")]
      (is (= 200 (:status response)))
      (is (= "application/transit+msgpack" (:content-type response))
          "Response Content-Type should be application/transit+msgpack")
      (when (:body response)
        (let [result (decode-transit-msgpack (:body response))]
          (is (map? result))
          (is (contains? result :result)))))))

(deftest test-edn-request-transit-json-response
  (testing "EDN request with Transit+JSON response (Accept: application/transit+json)"
    (let [body (.getBytes "{:op :health}" "UTF-8")
          response (post-raw "/" body
                             "application/edn"
                             "application/transit+json")]
      (is (= 200 (:status response)))
      (is (= "application/transit+json" (:content-type response))
          "Response Content-Type should be application/transit+json")
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          (is (map? result))
          (is (contains? result :result)))))))

(deftest test-msgpack-request-json-response
  (testing "Transit+MessagePack request with Transit+JSON response"
    (let [request-data {:op :health}
          body (encode-transit-msgpack request-data)
          response (post-raw "/" body
                             "application/transit+msgpack"
                             "application/transit+json")]
      (is (= 200 (:status response)))
      (is (= "application/transit+json" (:content-type response))
          "Response Content-Type should be application/transit+json")
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          (is (map? result))
          (is (contains? result :result)))))))

;; ===================================================================
;; 4. Transit value type fidelity
;; ===================================================================

(deftest test-transit-json-keyword-fidelity
  (testing "Keywords survive Transit+JSON round-trip"
    (let [request-data {:op :health}
          body (encode-transit-json request-data)
          response (post-raw "/" body
                             "application/transit+json"
                             "application/transit+json")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          ;; The response map keys should be keywords, not strings
          (is (every? keyword? (keys result))
              "Map keys in Transit+JSON response should be keywords"))))))

(deftest test-transit-msgpack-keyword-fidelity
  (testing "Keywords survive Transit+MessagePack round-trip"
    (let [request-data {:op :health}
          body (encode-transit-msgpack request-data)
          response (post-raw "/" body
                             "application/transit+msgpack"
                             "application/transit+msgpack")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-msgpack (:body response))]
          (is (every? keyword? (keys result))
              "Map keys in Transit+MessagePack response should be keywords"))))))

(deftest test-transit-json-string-value
  (testing "String values survive Transit+JSON round-trip"
    (let [request-data {:op :health}
          body (encode-transit-json request-data)
          response (post-raw "/" body
                             "application/transit+json"
                             "application/transit+json")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          (is (string? (:result result))
              "Health result should be a string"))))))

(deftest test-transit-msgpack-list-dbs-returns-vector
  (testing "List-dbs returns a vector of strings via Transit+MessagePack"
    (let [request-data {:op :list-dbs}
          body (encode-transit-msgpack request-data)
          response (post-raw "/" body
                             "application/transit+msgpack"
                             "application/transit+msgpack")]
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-msgpack (:body response))]
          (is (sequential? (:result result))
              ":result should be a sequence")
          (is (every? string? (:result result))
              "Each database name should be a string"))))))

;; ===================================================================
;; 5. Error cases
;; ===================================================================

(deftest test-transit-json-malformed-body
  (testing "Malformed Transit+JSON body returns an error"
    (let [body (.getBytes "this is not transit json" "UTF-8")
          response (post-raw "/" body
                             "application/transit+json"
                             "application/transit+json")]
      (is (= 200 (:status response))
          "Should still return 200 with error in body")
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          (is (contains? result :error)
              "Malformed Transit+JSON should produce an :error response"))))))

(deftest test-transit-msgpack-malformed-body
  (testing "Malformed Transit+MessagePack body returns an error"
    (let [body (byte-array [0xff 0xfe 0xfd 0xfc])
          response (post-raw "/" body
                             "application/transit+msgpack"
                             "application/transit+json")]
      ;; The response format should be transit+json since that's what we asked for
      (is (= 200 (:status response)))
      (when (:body response)
        (let [result (decode-transit-json (:body response))]
          (is (contains? result :error)
              "Malformed Transit+MessagePack should produce an :error response"))))))

;; ===================================================================
;; 6. Transit encoding of transaction reports
;; ===================================================================

(deftest test-transit-json-transact-report-structure
  (testing "Transaction report via Transit+JSON contains all Datomic fields"
    ;; First create a database and connect
    (let [create-body (encode-transit-json {:op :create-db
                                            :args {:db-name "transit-tx-test"}})
          _ (post-raw "/" create-body
                      "application/transit+json"
                      "application/transit+json")
          conn-body (encode-transit-json {:op :connect
                                          :args {:db-name "transit-tx-test"}})
          conn-resp (post-raw "/" conn-body
                              "application/transit+json"
                              "application/transit+json")]
      (when (and (= 200 (:status conn-resp)) (:body conn-resp))
        (let [conn-result (decode-transit-json (:body conn-resp))
              conn-id (get-in conn-result [:result :connection-id])]
          (when conn-id
            ;; Install schema
            (let [schema-body (encode-transit-json
                                {:op :transact
                                 :args {:connection-id (str conn-id)
                                        :tx-data "[{:db/ident :test/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]"}})
                  _ (post-raw "/" schema-body
                              "application/transit+json"
                              "application/transit+json")
                  ;; Now transact data and check the report
                  tx-body (encode-transit-json
                            {:op :transact
                             :args {:connection-id (str conn-id)
                                    :tx-data "[{:db/id \"temp\" :test/name \"Hello\"}]"}})
                  tx-resp (post-raw "/" tx-body
                                    "application/transit+json"
                                    "application/transit+json")]
              (when (and (= 200 (:status tx-resp)) (:body tx-resp))
                (let [tx-result (decode-transit-json (:body tx-resp))
                      report (:result tx-result)]
                  (is (map? report) "Transaction result should be a map")
                  (when (map? report)
                    (is (contains? report :db-before) "Report should contain :db-before")
                    (is (contains? report :db-after) "Report should contain :db-after")
                    (is (contains? report :tx-data) "Report should contain :tx-data")
                    (is (contains? report :tempids) "Report should contain :tempids")
                    ;; Verify tempids have string keys via Transit
                    (when-let [tempids (:tempids report)]
                      (is (map? tempids) ":tempids should be a map")
                      (when (map? tempids)
                        (is (every? string? (keys tempids))
                            (str "tempid keys should be strings, got: "
                                 (mapv type (keys tempids)))))))))
              ;; Cleanup
              (let [del-body (encode-transit-json {:op :delete-db
                                                    :args {:db-name "transit-tx-test"}})]
                (post-raw "/" del-body
                          "application/transit+json"
                          "application/transit+json")))))))))

(deftest test-transit-msgpack-transact-report-structure
  (testing "Transaction report via Transit+MessagePack contains all Datomic fields"
    (let [create-body (encode-transit-msgpack {:op :create-db
                                               :args {:db-name "transit-mp-tx-test"}})
          _ (post-raw "/" create-body
                      "application/transit+msgpack"
                      "application/transit+msgpack")
          conn-body (encode-transit-msgpack {:op :connect
                                             :args {:db-name "transit-mp-tx-test"}})
          conn-resp (post-raw "/" conn-body
                              "application/transit+msgpack"
                              "application/transit+msgpack")]
      (when (and (= 200 (:status conn-resp)) (:body conn-resp))
        (let [conn-result (decode-transit-msgpack (:body conn-resp))
              conn-id (get-in conn-result [:result :connection-id])]
          (when conn-id
            (let [schema-body (encode-transit-msgpack
                                {:op :transact
                                 :args {:connection-id (str conn-id)
                                        :tx-data "[{:db/ident :test/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]"}})
                  _ (post-raw "/" schema-body
                              "application/transit+msgpack"
                              "application/transit+msgpack")
                  tx-body (encode-transit-msgpack
                            {:op :transact
                             :args {:connection-id (str conn-id)
                                    :tx-data "[{:db/id \"temp\" :test/name \"World\"}]"}})
                  tx-resp (post-raw "/" tx-body
                                    "application/transit+msgpack"
                                    "application/transit+msgpack")]
              (when (and (= 200 (:status tx-resp)) (:body tx-resp))
                (let [tx-result (decode-transit-msgpack (:body tx-resp))
                      report (:result tx-result)]
                  (is (map? report) "Transaction result should be a map")
                  (when (map? report)
                    (is (contains? report :db-before) "Report should contain :db-before")
                    (is (contains? report :db-after) "Report should contain :db-after")
                    (is (contains? report :tx-data) "Report should contain :tx-data")
                    (is (contains? report :tempids) "Report should contain :tempids"))))
              (let [del-body (encode-transit-msgpack {:op :delete-db
                                                      :args {:db-name "transit-mp-tx-test"}})]
                (post-raw "/" del-body
                          "application/transit+msgpack"
                          "application/transit+msgpack")))))))))
