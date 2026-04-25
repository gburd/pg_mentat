(defproject datomic-compat-tests "0.1.0"
  :description "Datomic compatibility tests for mentatd"
  :license {:name "Apache-2.0"
            :url "https://www.apache.org/licenses/LICENSE-2.0"}
  :dependencies [[org.clojure/clojure "1.11.1"]
                 [com.datomic/datomic-free "0.9.5697"]
                 [com.cognitect/transit-clj "1.0.333"]
                 [clj-http "3.12.3"]
                 [cheshire "5.11.0"]]
  :test-paths ["test"]
  :profiles {:test {:jvm-opts ["-Xmx512m"]}}

  ;; Test selectors allow running subsets of the test suite:
  ;;   lein test :http       - HTTP-based tests only (Transit wire + EDN integration)
  ;;   lein test :peer       - Datomic peer API tests (requires peer protocol support)
  ;;   lein test             - All tests (default)
  :test-selectors {:http    (fn [m] (or (:http m)
                                        (= (:ns m) 'datomic-compat.transit-test)
                                        (= (:ns m) 'datomic-compat.http-integration-test)))
                   :peer    (fn [m] (or (:peer m)
                                        (= (:ns m) 'datomic-compat.core-test)
                                        (= (:ns m) 'datomic-compat.real-client-test)
                                        (= (:ns m) 'datomic-compat.typed-values-test)))
                   :default (constantly true)})
