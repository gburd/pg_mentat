-- pg_mentat regression: unique constraints
-- Tests for :db.unique/identity and :db.unique/value

-- Setup: schema with both unique types
\echo Setup: unique identity and unique value attributes

SELECT mentat_transact('[
  {:db/ident :uniq/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
  {:db/ident :uniq/ssn
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/value}
  {:db/ident :uniq/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Insert initial entities
SELECT mentat_transact('[
  {:uniq/email "alice@test.com" :uniq/name "Alice" :uniq/ssn "111-11-1111"}
  {:uniq/email "bob@test.com" :uniq/name "Bob" :uniq/ssn "222-22-2222"}
]');

-- Test: :db.unique/identity — upserting with same email merges into existing entity
\echo Test: unique identity upsert merges entities

SELECT mentat_transact('[
  {:uniq/email "alice@test.com" :uniq/name "Alice Updated"}
]');

-- Should still be only 2 entities total (alice was upserted, not duplicated)
SELECT mentat_query(
  '[:find (count ?e) :where [?e :uniq/email _]]',
  '{}'::jsonb
);

-- Alice's name should be updated
SELECT mentat_query(
  '[:find ?name :where [?e :uniq/email "alice@test.com"] [?e :uniq/name ?name]]',
  '{}'::jsonb
);

-- Test: :db.unique/value — inserting duplicate value fails
\echo Test: unique value rejects duplicates

DO $$
BEGIN
  PERFORM mentat_transact('[
    {:uniq/email "carol@test.com" :uniq/name "Carol" :uniq/ssn "111-11-1111"}
  ]');
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'Unique value violation as expected: %', SQLERRM;
END $$;

-- Verify Carol was NOT inserted (constraint violation rolled back)
SELECT mentat_query(
  '[:find ?name :where [?e :uniq/email "carol@test.com"] [?e :uniq/name ?name]]',
  '{}'::jsonb
);

-- Test: identity lookup ref syntax [:attr value]
\echo Test: lookup ref with unique identity

SELECT mentat_transact('[
  [:db/add [:uniq/email "bob@test.com"] :uniq/name "Bob Updated"]
]');

SELECT mentat_query(
  '[:find ?name :where [?e :uniq/email "bob@test.com"] [?e :uniq/name ?name]]',
  '{}'::jsonb
);

-- Test: unique value with different entities succeeds
\echo Test: unique value with distinct values succeeds

SELECT mentat_transact('[
  {:uniq/email "carol@test.com" :uniq/name "Carol" :uniq/ssn "333-33-3333"}
]');

SELECT mentat_query(
  '[:find (count ?e) :where [?e :uniq/email _]]',
  '{}'::jsonb
);
