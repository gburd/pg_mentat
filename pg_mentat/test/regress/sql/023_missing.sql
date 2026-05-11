-- pg_mentat regression: missing? where-function
-- Filters entities that lack a specific attribute

-- Setup: entities with optional attributes
\echo Setup: schema for missing? tests

SELECT mentat_transact('[
  {:db/ident :ms/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :ms/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
  {:ms/name "Alice" :ms/email "alice@example.com"}
  {:ms/name "Bob"}
  {:ms/name "Carol" :ms/email "carol@example.com"}
  {:ms/name "Dave"}
]');

-- Test: find entities missing :ms/email
\echo Test: missing? filters entities without attribute

SELECT mentat_query(
  '[:find ?name :where [?e :ms/name ?name] [(missing? $ ?e :ms/email)] :order (asc ?name)]',
  '{}'::jsonb
);

-- Test: find entities that have :ms/email (inverse of missing?)
\echo Test: entities that DO have the attribute

SELECT mentat_query(
  '[:find ?name :where [?e :ms/name ?name] [?e :ms/email _] :order (asc ?name)]',
  '{}'::jsonb
);
