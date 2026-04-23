(defproject datomic-compat-tests "0.1.0"
  :description "Datomic compatibility tests for mentatd"
  :license {:name "Apache-2.0"
            :url "https://www.apache.org/licenses/LICENSE-2.0"}
  :dependencies [[org.clojure/clojure "1.11.1"]
                 [com.datomic/datomic-free "0.9.5697"]
                 [com.cognitect/transit-clj "1.0.333"]]
  :test-paths ["test"]
  :profiles {:test {:jvm-opts ["-Xmx512m"]}})
