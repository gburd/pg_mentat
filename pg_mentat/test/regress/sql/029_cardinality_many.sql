-- pg_mentat regression: cardinality/many set semantics
-- Multiple values per entity, no duplicates, retract one leaves others

-- Setup: cardinality/many attribute
\echo Setup: cardinality many attribute

SELECT mentat_transact('[
  {:db/ident :cm/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :cm/tag
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/many}
]');

-- Add entity with multiple tags
\echo Test: add multiple values to cardinality/many

SELECT mentat_transact('[
  {:cm/name "article" :cm/tag "rust"}
]');

SELECT mentat_transact('[
  [:db/add [:cm/name "article"] :cm/tag "postgres"]
  [:db/add [:cm/name "article"] :cm/tag "extension"]
  [:db/add [:cm/name "article"] :cm/tag "mentat"]
]');

-- Verify all tags exist
SELECT mentat_query(
  '[:find ?tag :where [?e :cm/name "article"] [?e :cm/tag ?tag] :order (asc ?tag)]',
  '{}'::jsonb
);

-- Test: set semantics — adding duplicate value is idempotent
\echo Test: duplicate add is idempotent

SELECT mentat_transact('[
  [:db/add [:cm/name "article"] :cm/tag "rust"]
]');

-- Count should still be 4, not 5
SELECT mentat_query(
  '[:find (count ?tag) :where [?e :cm/name "article"] [?e :cm/tag ?tag]]',
  '{}'::jsonb
);

-- Test: retract one value leaves others intact
\echo Test: retract one value preserves others

SELECT mentat_transact('[
  [:db/retract [:cm/name "article"] :cm/tag "mentat"]
]');

-- Should now have 3 tags
SELECT mentat_query(
  '[:find ?tag :where [?e :cm/name "article"] [?e :cm/tag ?tag] :order (asc ?tag)]',
  '{}'::jsonb
);

-- Test: multiple entities can have the same cardinality/many values
\echo Test: same values on different entities

SELECT mentat_transact('[
  {:cm/name "article2" :cm/tag "rust"}
]');

-- Both entities should appear when querying for tag "rust"
SELECT mentat_query(
  '[:find ?name :where [?e :cm/name ?name] [?e :cm/tag "rust"] :order (asc ?name)]',
  '{}'::jsonb
);
