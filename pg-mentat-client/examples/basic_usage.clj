#!/usr/bin/env clojure
;; Basic usage example for pg-mentat-client
;;
;; Run with:
;;   clojure -Sdeps '{:deps {pg-mentat-client/pg-mentat-client {:local/root "."}}}' examples/basic_usage.clj

(require '[pg-mentat.client :as mentat])

(println "Connecting to pg_mentat via mentatd...")
(def conn (mentat/connect "http://localhost:8080"))
(println "Connected!")

;; Define schema
(println "\nDefining schema...")
(mentat/transact conn
  [{:db/ident :person/name
    :db/valueType :db.type/string
    :db/cardinality :db.cardinality/one
    :db/doc "A person's full name"}
   {:db/ident :person/email
    :db/valueType :db.type/string
    :db/cardinality :db.cardinality/one
    :db/unique :db.unique/identity
    :db/doc "A person's email address (unique)"}
   {:db/ident :person/age
    :db/valueType :db.type/long
    :db/cardinality :db.cardinality/one
    :db/doc "A person's age in years"}
   {:db/ident :person/friends
    :db/valueType :db.type/ref
    :db/cardinality :db.cardinality/many
    :db/doc "References to a person's friends"}
   {:db/ident :person/tags
    :db/valueType :db.type/string
    :db/cardinality :db.cardinality/many
    :db/doc "Tags associated with a person"}])
(println "Schema defined!")

;; Add some data
(println "\nAdding people...")
(let [tx-result (mentat/transact conn
                  [{:db/id "alice"
                    :person/name "Alice Johnson"
                    :person/email "alice@example.com"
                    :person/age 30
                    :person/tags ["developer" "team-lead"]}
                   {:db/id "bob"
                    :person/name "Bob Smith"
                    :person/email "bob@example.com"
                    :person/age 25
                    :person/tags ["developer" "junior"]
                    :person/friends "alice"}
                   {:db/id "charlie"
                    :person/name "Charlie Brown"
                    :person/email "charlie@example.com"
                    :person/age 35
                    :person/tags ["manager" "scrum-master"]
                    :person/friends ["alice" "bob"]}])]
  (println "Transaction complete!")
  (println "Tempid mappings:" (:tempids tx-result)))

;; Get fresh database value
(def db (mentat/db conn))

;; Simple queries
(println "\n=== Simple Queries ===")

(println "\nAll people:")
(let [people (mentat/q '[:find ?name ?age
                        :where [?e :person/name ?name]
                               [?e :person/age ?age]]
                      db)]
  (doseq [[name age] people]
    (println (format "  %s (age %d)" name age))))

(println "\nPeople over 25:")
(let [adults (mentat/q '[:find ?name
                        :where [?e :person/name ?name]
                               [?e :person/age ?age]
                               [(> ?age 25)]]
                      db)]
  (doseq [[name] adults]
    (println (str "  " name))))

(println "\nDevelopers:")
(let [devs (mentat/q '[:find ?name ?tag
                      :where [?e :person/name ?name]
                             [?e :person/tags ?tag]
                             [(= ?tag "developer")]]
                    db)]
  (doseq [[name tag] devs]
    (println (format "  %s - %s" name tag))))

;; Pull API examples
(println "\n=== Pull API ===")

(println "\nPull Alice's data (by lookup ref):")
(let [alice (mentat/pull db
              [:person/name
               :person/email
               :person/age
               :person/tags]
              [:person/email "alice@example.com"])]
  (println "  Name:" (:person/name alice))
  (println "  Email:" (:person/email alice))
  (println "  Age:" (:person/age alice))
  (println "  Tags:" (pr-str (:person/tags alice))))

(println "\nPull Charlie with friends:")
(let [charlie (mentat/pull db
                [:person/name
                 {:person/friends [:person/name :person/email]}]
                [:person/email "charlie@example.com"])]
  (println "  Name:" (:person/name charlie))
  (println "  Friends:")
  (doseq [friend (:person/friends charlie)]
    (println (format "    - %s (%s)"
                    (:person/name friend)
                    (:person/email friend)))))

;; Pull many
(println "\nPull multiple people at once:")
(let [people (mentat/pull-many db
               [:person/name :person/age]
               [[:person/email "alice@example.com"]
                [:person/email "bob@example.com"]])]
  (doseq [person people]
    (println (format "  %s (age %d)"
                    (:person/name person)
                    (:person/age person)))))

;; Update data
(println "\n=== Updates ===")

(println "\nUpdating Alice's age to 31...")
(mentat/transact conn
  [[:db/add [:person/email "alice@example.com"] :person/age 31]])

(println "\nAdding a new tag to Bob...")
(mentat/transact conn
  [[:db/add [:person/email "bob@example.com"] :person/tags "golang"]])

;; Query after updates
(let [db2 (mentat/db conn)]
  (println "\nAlice's new age:")
  (let [alice-age (mentat/q '[:find ?age .
                             :in $ ?email
                             :where [?e :person/email ?email]
                                    [?e :person/age ?age]]
                           db2 "alice@example.com")]
    (println (str "  " alice-age)))

  (println "\nBob's tags:")
  (let [bob-tags (mentat/q '[:find [?tag ...]
                            :in $ ?email
                            :where [?e :person/email ?email]
                                   [?e :person/tags ?tag]]
                          db2 "bob@example.com")]
    (println (str "  " (pr-str bob-tags)))))

;; Aggregates
(println "\n=== Aggregates ===")

(println "\nAverage age:")
(let [avg-age (mentat/q '[:find (avg ?age) .
                         :where [?e :person/age ?age]]
                       (mentat/db conn))]
  (println (format "  %.1f years" (double avg-age))))

(println "\nTotal number of people:")
(let [count (mentat/q '[:find (count ?e) .
                       :where [?e :person/name]]
                     (mentat/db conn))]
  (println (str "  " count)))

(println "\nNumber of tags per person:")
(let [tag-counts (mentat/q '[:find ?name (count ?tag)
                            :where [?e :person/name ?name]
                                   [?e :person/tags ?tag]]
                          (mentat/db conn))]
  (doseq [[name count] tag-counts]
    (println (format "  %s: %d tags" name count))))

;; Speculative transactions
(println "\n=== Speculative Transactions ===")

(println "\nTrying a what-if scenario (not committed):")
(let [what-if (mentat/with (mentat/db conn)
                [{:db/id "diana"
                  :person/name "Diana Prince"
                  :person/email "diana@example.com"
                  :person/age 28}])]
  (println "  What-if result would include Diana")
  (println "  (but she's not actually in the database)"))

;; Verify Diana is not in the actual database
(let [diana (mentat/q '[:find ?name .
                       :in $ ?email
                       :where [?e :person/email ?email]
                              [?e :person/name ?name]]
                     (mentat/db conn) "diana@example.com")]
  (println (format "  Diana in actual database: %s"
                  (if diana diana "Not found"))))

(println "\n=== Complete! ===")
(println "Successfully demonstrated pg-mentat-client functionality!")