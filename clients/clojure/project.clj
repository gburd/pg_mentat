(defproject com.pg-mentat/pg-mentat-client "0.1.0-SNAPSHOT"
  :description "Datomic-compatible Clojure peer library for pg_mentat.
                Direct PostgreSQL connection via next.jdbc -- no HTTP daemon required."
  :url "https://github.com/anthropics/pg_mentat"
  :license {:name "Apache-2.0"
            :url "https://www.apache.org/licenses/LICENSE-2.0"}

  :dependencies [[org.clojure/clojure "1.11.1"]
                 [com.github.seancorfield/next.jdbc "1.3.894"]
                 [org.postgresql/postgresql "42.6.0"]]

  :profiles {:dev {:dependencies [[lambdaisland/kaocha "1.87.1366"]
                                  [org.clojure/test.check "1.1.1"]
                                  [com.zaxxer/HikariCP "5.0.1"]]
                   :source-paths ["test" "examples"]}}

  :source-paths ["src"]
  :test-paths ["test"]

  :repl-options {:init-ns pg-mentat.client}

  :deploy-repositories [["releases" {:url "https://repo.clojars.org"
                                     :sign-releases false}]
                        ["snapshots" {:url "https://repo.clojars.org"
                                      :sign-releases false}]])
