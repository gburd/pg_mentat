-- pg_mentat regression: get-else where-function
-- Returns a default value when an attribute is missing for an entity

-- Setup: schema with optional attribute
\echo Setup: schema for get-else tests

SELECT mentat_transact('[
  {:db/ident :ge/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :ge/nickname
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
  {:ge/name "Alice" :ge/nickname "Ali"}
  {:ge/name "Bob"}
]');

-- Test: get-else with present attribute returns actual value
\echo Test: get-else returns actual value when present

SELECT mentat_query(
  '[:find ?name ?nick :where [?e :ge/name ?name] [(get-else $ ?e :ge/nickname "N/A") ?nick] :order (asc ?name)]',
  '{}'::jsonb
);

-- Test: get-else with missing attribute returns default
\echo Test: get-else returns default when attribute missing

-- Bob has no :ge/nickname, should get "N/A"
SELECT mentat_query(
  '[:find ?name ?nick :where [?e :ge/name ?name] [(get-else $ ?e :ge/nickname "unknown") ?nick] [(= ?name "Bob")]]',
  '{}'::jsonb
);
