(ns pg-mentat.examples.migration-example
  "Datomic-to-pg_mentat migration example.

   This file shows a realistic Datomic application and its pg_mentat
   equivalent side by side. The goal is to demonstrate that most code
   needs only a namespace change to work with pg_mentat.

   Structure:
     1. Original Datomic code (commented out, for reference)
     2. Equivalent pg_mentat code (runnable)
     3. Handling migration differences

   Prerequisites:
     - mentatd running at ws://localhost:8080/ws
     - PostgreSQL with pg_mentat extension

   Run:
     clj -M -m pg-mentat.examples.migration-example"
  (:require [pg-mentat.client :as d]))

;; ===========================================================================
;; PART 1: Schema -- Identical in Both Systems
;; ===========================================================================

;; This schema works in both Datomic and pg_mentat without modification.

(def movie-schema
  [{:db/ident       :movie/title
    :db/valueType   :db.type/string
    :db/cardinality :db.cardinality/one
    :db/doc         "The title of the movie"}

   {:db/ident       :movie/year
    :db/valueType   :db.type/long
    :db/cardinality :db.cardinality/one
    :db/doc         "Year the movie was released"}

   {:db/ident       :movie/genre
    :db/valueType   :db.type/keyword
    :db/cardinality :db.cardinality/many
    :db/doc         "Set of genre keywords"}

   {:db/ident       :movie/director
    :db/valueType   :db.type/ref
    :db/cardinality :db.cardinality/one
    :db/doc         "Reference to the director entity"}

   {:db/ident       :movie/rating
    :db/valueType   :db.type/double
    :db/cardinality :db.cardinality/one}

   {:db/ident       :person/name
    :db/valueType   :db.type/string
    :db/cardinality :db.cardinality/one}

   {:db/ident       :person/born
    :db/valueType   :db.type/instant
    :db/cardinality :db.cardinality/one}

   ;; Enum values
   {:db/ident :genre/sci-fi}
   {:db/ident :genre/drama}
   {:db/ident :genre/action}
   {:db/ident :genre/comedy}
   {:db/ident :genre/thriller}])

;; ===========================================================================
;; PART 2: Connection Setup -- This Is Where Migration Starts
;; ===========================================================================

;; ---------- DATOMIC (commented out for reference) ----------
;;
;; (require '[datomic.client.api :as d])
;;
;; ;; Datomic Cloud
;; (def client (d/client {:server-type :cloud
;;                        :region "us-east-1"
;;                        :system "my-system"
;;                        :endpoint "https://..."
;;                        :proxy-port 8182}))
;;
;; ;; Datomic On-Prem (Peer)
;; ;; (def conn (d/connect "datomic:free://localhost:4334/movies"))
;;
;; (def conn (d/connect client {:db-name "movies"}))

;; ---------- PG_MENTAT ----------

(defn setup-connection
  "Connect to pg_mentat. The ONLY change from Datomic: different config map."
  []
  (let [client (d/client {:server-type :pg-mentat
                           :endpoint    "ws://localhost:8080/ws"})]
    (d/connect client {:db-name "movies"})))

;; ===========================================================================
;; PART 3: Transactions -- Minor Differences
;; ===========================================================================

(defn seed-data
  "Seed the database with schema and sample data.

   Key differences from Datomic:
   - No @(d/transact ...) -- pg_mentat returns directly, no deref needed
   - String tempids instead of (d/tempid :db.part/user)
   - No #db/id reader literal"
  [conn]
  ;; Schema transaction (identical to Datomic)
  (d/transact conn {:tx-data movie-schema})

  ;; ---------- DATOMIC ----------
  ;; @(d/transact conn
  ;;   {:tx-data [{:db/id (d/tempid :db.part/user)
  ;;               :person/name "Christopher Nolan"
  ;;               :person/born #inst "1970-07-30"}]})

  ;; ---------- PG_MENTAT ----------
  ;; String tempids instead of (d/tempid ...)
  ;; Direct return instead of @(deref)
  (d/transact conn
    {:tx-data [{:db/id       "nolan"
                :person/name "Christopher Nolan"
                :person/born #inst "1970-07-30"}

               {:db/id        "spielberg"
                :person/name  "Steven Spielberg"
                :person/born  #inst "1946-12-18"}

               {:db/id        "kubrick"
                :person/name  "Stanley Kubrick"
                :person/born  #inst "1928-07-26"}]})

  ;; Movies with director references
  (d/transact conn
    {:tx-data [{:movie/title    "Inception"
                :movie/year     2010
                :movie/genre    [:genre/sci-fi :genre/action :genre/thriller]
                :movie/director [:person/name "Christopher Nolan"]
                :movie/rating   8.8}

               {:movie/title    "Interstellar"
                :movie/year     2014
                :movie/genre    [:genre/sci-fi :genre/drama]
                :movie/director [:person/name "Christopher Nolan"]
                :movie/rating   8.6}

               {:movie/title    "The Dark Knight"
                :movie/year     2008
                :movie/genre    [:genre/action :genre/drama :genre/thriller]
                :movie/director [:person/name "Christopher Nolan"]
                :movie/rating   9.0}

               {:movie/title    "Schindler's List"
                :movie/year     1993
                :movie/genre    [:genre/drama]
                :movie/director [:person/name "Steven Spielberg"]
                :movie/rating   9.0}

               {:movie/title    "2001: A Space Odyssey"
                :movie/year     1968
                :movie/genre    [:genre/sci-fi]
                :movie/director [:person/name "Stanley Kubrick"]
                :movie/rating   8.3}]}))

;; ===========================================================================
;; PART 4: Queries -- Mostly Identical
;; ===========================================================================

;; All of these queries work in both Datomic and pg_mentat with zero changes.

(defn query-examples
  "Query examples that work identically in Datomic and pg_mentat."
  [conn]
  (let [db (d/db conn)]

    ;; Simple query
    (println "\n--- Movies released after 2000 ---")
    (let [results (d/q '[:find ?title ?year
                         :where
                         [?m :movie/title ?title]
                         [?m :movie/year ?year]
                         [(> ?year 2000)]]
                       db)]
      (doseq [[title year] (sort-by second results)]
        (println (format "  %s (%d)" title year))))

    ;; Query with input binding
    (println "\n--- Movies by director ---")
    (let [results (d/q '[:find ?title ?year
                         :in $ ?director-name
                         :where
                         [?d :person/name ?director-name]
                         [?m :movie/director ?d]
                         [?m :movie/title ?title]
                         [?m :movie/year ?year]]
                       db "Christopher Nolan")]
      (doseq [[title year] (sort-by second results)]
        (println (format "  %s (%d)" title year))))

    ;; Aggregation
    (println "\n--- Director statistics ---")
    (let [results (d/q '[:find ?director-name (count ?m) (avg ?rating)
                         :where
                         [?m :movie/director ?d]
                         [?d :person/name ?director-name]
                         [?m :movie/rating ?rating]]
                       db)]
      (doseq [[name cnt avg-rating] results]
        (println (format "  %s: %d movies, avg rating %.1f" name cnt (double avg-rating)))))

    ;; Genre search with OR
    (println "\n--- Sci-fi or thriller movies ---")
    (let [results (d/q '[:find ?title
                         :where
                         [?m :movie/title ?title]
                         (or [?m :movie/genre :genre/sci-fi]
                             [?m :movie/genre :genre/thriller])]
                       db)]
      (doseq [[title] results]
        (println "  -" title)))

    ;; NOT clause: movies that are NOT dramas
    (println "\n--- Non-drama movies ---")
    (let [results (d/q '[:find ?title
                         :where
                         [?m :movie/title ?title]
                         (not [?m :movie/genre :genre/drama])]
                       db)]
      (doseq [[title] results]
        (println "  -" title)))))

;; ===========================================================================
;; PART 5: Pull API -- Identical
;; ===========================================================================

(defn pull-examples
  "Pull examples that work identically in both systems."
  [conn]
  (let [db (d/db conn)]

    (println "\n--- Pull: Inception details ---")
    (let [inception-id (d/q '[:find ?e .
                              :where [?e :movie/title "Inception"]]
                            db)]
      (when inception-id
        ;; All attributes
        (println "  All:" (d/pull db '[*] inception-id))

        ;; Specific attrs with nested ref
        (println "  With director:"
                 (d/pull db '[:movie/title
                               :movie/year
                               :movie/rating
                               {:movie/director [:person/name :person/born]}]
                         inception-id))))

    ;; Reverse lookup: find all movies by a director
    (println "\n--- Pull: Nolan's filmography (reverse lookup) ---")
    (let [nolan-id (d/q '[:find ?e .
                          :where [?e :person/name "Christopher Nolan"]]
                        db)]
      (when nolan-id
        (println "  "
                 (d/pull db '[:person/name
                               {:movie/_director [:movie/title :movie/year]}]
                         nolan-id))))))

;; ===========================================================================
;; PART 6: Time Travel -- Identical API
;; ===========================================================================

(defn time-travel-example
  "Demonstrate as-of, since, and history queries."
  [conn]
  (println "\n--- Time Travel ---")
  (let [db (d/db conn)
        t-before (:t db)]

    ;; Update a rating
    (let [inception-id (d/q '[:find ?e .
                              :where [?e :movie/title "Inception"]]
                            db)]
      (when inception-id
        (println "Original rating:"
                 (d/q '[:find ?r .
                        :where [?e :movie/title "Inception"]
                               [?e :movie/rating ?r]]
                      db))

        ;; Change the rating
        (d/transact conn {:tx-data [[:db/add inception-id :movie/rating 9.0]]})

        (let [new-db (d/db conn)]
          (println "New rating:"
                   (d/q '[:find ?r .
                          :where [?e :movie/title "Inception"]
                                 [?e :movie/rating ?r]]
                        new-db))

          ;; as-of: see the old rating
          (println "Rating as-of t" (str t-before ":")
                   (d/q '[:find ?r .
                          :where [?e :movie/title "Inception"]
                                 [?e :movie/rating ?r]]
                        (d/as-of new-db t-before)))

          ;; history: see all ratings over time
          (println "Rating history:")
          (let [hist-results (d/q '[:find ?r ?tx ?added
                                    :where
                                    [?e :movie/title "Inception"]
                                    [?e :movie/rating ?r ?tx ?added]]
                                  (d/history new-db))]
            (doseq [[rating tx added] (sort-by second hist-results)]
              (println (format "  t=%d rating=%.1f %s"
                               tx (double rating)
                               (if added "asserted" "retracted"))))))))))

;; ===========================================================================
;; PART 7: Handling Migration Differences
;; ===========================================================================

(defn migration-gotchas
  "Demonstrate patterns that differ between Datomic and pg_mentat."
  [conn]
  (println "\n--- Migration Gotchas ---")
  (let [db (d/db conn)]

    ;; 1. Tempids: use strings, not (d/tempid ...)
    (println "\n1. String tempids:")
    (let [result (d/transact conn
                   {:tx-data [{:db/id "new-director"
                               :person/name "Denis Villeneuve"}
                              {:movie/title "Blade Runner 2049"
                               :movie/year 2017
                               :movie/director "new-director"
                               :movie/genre [:genre/sci-fi :genre/drama]
                               :movie/rating 8.0}]})]
      (println "  Tempid resolved:" (get-in result [:tempids "new-director"])))

    ;; 2. No transaction functions -- use application logic
    (println "\n2. Application-level logic (instead of tx functions):")
    (let [inception-id (d/q '[:find ?e .
                              :where [?e :movie/title "Inception"]]
                            (d/db conn))
          current-rating (d/q '[:find ?r .
                                :where [?e :movie/title "Inception"]
                                       [?e :movie/rating ?r]]
                              (d/db conn))]
      ;; In Datomic, you might use a transaction function to atomically
      ;; read-modify-write. In pg_mentat, use CAS:
      (when (and inception-id current-rating)
        (d/transact conn {:tx-data [[:db.fn/cas inception-id
                                      :movie/rating current-rating
                                      (+ current-rating 0.1)]]})
        (println "  Rating bumped from" current-rating "to"
                 (d/q '[:find ?r .
                        :where [?e :movie/title "Inception"]
                               [?e :movie/rating ?r]]
                      (d/db conn)))))

    ;; 3. No (fulltext $) -- query differently
    (println "\n3. Finding movies by text (without fulltext predicate):")
    ;; In Datomic: [(fulltext $ :movie/title "dark") [[?e ?title]]]
    ;; In pg_mentat: filter in the query itself, or use find_text SQL
    (let [results (d/q '[:find ?title
                         :where
                         [?e :movie/title ?title]]
                       (d/db conn))]
      (println "  All titles (filter in app):"
               (filter #(re-find #"(?i)dark" (first %)) results)))

    ;; 4. Retract entity
    (println "\n4. Entity retraction (same as Datomic):")
    (let [villeneuve-id (d/q '[:find ?e .
                               :where [?e :person/name "Denis Villeneuve"]]
                             (d/db conn))]
      ;; This works identically in both systems
      (when villeneuve-id
        (d/transact conn {:tx-data [[:db/retractEntity villeneuve-id]]})
        (println "  Denis Villeneuve retracted")))))

;; ===========================================================================
;; Main
;; ===========================================================================

(defn -main [& args]
  (println "=== Datomic to pg_mentat Migration Example ===")
  (println "This demonstrates migrating a movie database application.\n")

  (try
    (let [conn (setup-connection)]
      (seed-data conn)
      (query-examples conn)
      (pull-examples conn)
      (time-travel-example conn)
      (migration-gotchas conn)
      (d/release conn))
    (catch Exception e
      (println "\nError:" (.getMessage e))
      (when-let [data (ex-data e)]
        (println "Details:" (pr-str data)))))

  (println "\n=== Migration Example Complete ===")
  (println "Key takeaways:")
  (println "  1. Change require: [datomic.client.api :as d] -> [pg-mentat.client :as d]")
  (println "  2. Update connection config to point to mentatd WebSocket")
  (println "  3. Use string tempids instead of (d/tempid ...)")
  (println "  4. Remove @ (deref) from transaction calls")
  (println "  5. Replace (fulltext $) with application-level filtering or SQL")
  (println "  6. Move transaction functions to application logic with CAS")
  (shutdown-agents))
