-- pg_mentat regression: speculative transactions (mentat_with)
-- Speculative transactions compute results without persisting changes

-- Setup: baseline data
\echo Setup: schema for speculative transaction tests

SELECT mentat_transact('[
  {:db/ident :spec/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :spec/count
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
  {:spec/name "baseline" :spec/count 10}
]');

-- Verify baseline exists
SELECT mentat_query(
  '[:find ?name ?c :where [?e :spec/name ?name] [?e :spec/count ?c]]',
  '{}'::jsonb
);

-- Test: speculative transaction returns result but doesn't persist
\echo Test: mentat_with does not persist

SELECT mentat_with('[
  {:spec/name "speculative" :spec/count 99}
]');

-- Verify the speculative entity was NOT actually stored
SELECT mentat_query(
  '[:find ?name :where [?e :spec/name ?name] :order (asc ?name)]',
  '{}'::jsonb
);

-- Only "baseline" should exist, not "speculative"
SELECT mentat_query(
  '[:find (count ?e) :where [?e :spec/name _]]',
  '{}'::jsonb
);

-- Test: speculative retraction doesn't persist either
\echo Test: speculative retraction does not persist

SELECT mentat_with('[
  [:db/retract [:spec/name "baseline"] :spec/count 10]
]');

-- Original data should still be intact
SELECT mentat_query(
  '[:find ?name ?c :where [?e :spec/name ?name] [?e :spec/count ?c]]',
  '{}'::jsonb
);
