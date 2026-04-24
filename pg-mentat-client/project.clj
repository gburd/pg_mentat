(defproject pg-mentat-client "0.1.0-SNAPSHOT"
  :description "Clojure client library for pg_mentat Datalog database"
  :url "https://codeberg.org/gregburd/pg_mentat"
  :license {:name "Apache License 2.0"
            :url "http://www.apache.org/licenses/LICENSE-2.0"}
  :dependencies [[org.clojure/clojure "1.11.1"]
                 [clj-http "3.12.3"]
                 [cheshire "5.11.0"]  ; JSON parsing
                 [org.clojure/data.edn "0.1.1"]]
  :profiles {:dev {:dependencies [[midje "1.10.9"]]}}
  :repl-options {:init-ns pg-mentat.client}
  :main nil
  :target-path "target/%s"
  :clean-targets [:target-path])