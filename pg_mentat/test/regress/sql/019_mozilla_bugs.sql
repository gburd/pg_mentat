-- pg_mentat regression: Mozilla Mentat bug regressions
-- Tests for specific bugs from the upstream Mozilla Mentat tracker

-- Bug #670: Complex upserts with same :db.unique/identity value collapse
-- to a single entity (not duplicate insertions)
\echo Bug #670: upsert identity collapse

SELECT mentat_transact('[
  {:db/ident :bug670/id
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
  {:db/ident :bug670/val
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
]');

-- Two assertions with the same identity value should merge into one entity
SELECT mentat_transact('[
  {:bug670/id "x" :bug670/val 1}
  {:bug670/id "x" :bug670/val 2}
]');

-- Should find exactly ONE entity with :bug670/id "x" (last-write-wins for val)
SELECT mentat_query(
  '[:find (count ?e) :where [?e :bug670/id "x"]]',
  '{}'::jsonb
);

-- Bug #684: COUNT aggregate over 0 rows returns [[0]] not crash
\echo Bug #684: COUNT over empty result set

SELECT mentat_transact('[
  {:db/ident :bug684/phantom
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- No entities exist with :bug684/phantom, so count should be 0
SELECT mentat_query(
  '[:find (count ?e) :where [?e :bug684/phantom _]]',
  '{}'::jsonb
);

-- Bug #520: numeric comparison on string variable returns empty set (no crash)
\echo Bug #520: type-mismatch predicate safety

SELECT mentat_transact('[
  {:db/ident :bug520/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
  {:bug520/name "hello"}
  {:bug520/name "world"}
]');

-- [(> ?name 0)] where ?name is a string should return empty set, not crash
SELECT mentat_query(
  '[:find ?name :where [?e :bug520/name ?name] [(> ?name 0)]]',
  '{}'::jsonb
);

-- Bug #654: Unused :in variable doesn't panic
\echo Bug #654: unused input variable

SELECT mentat_query(
  '[:find ?name :in ?unused :where [?e :person/name ?name]]',
  '{"?unused": 42}'::jsonb
);

-- Bug #813: Escape sequences in strings are preserved
\echo Bug #813: string escape sequences

SELECT mentat_transact('[
  {:db/ident :bug813/text
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
  {:bug813/text "line1\nline2\ttab"}
]');

SELECT mentat_query(
  '[:find ?t :where [?e :bug813/text ?t]]',
  '{}'::jsonb
);
