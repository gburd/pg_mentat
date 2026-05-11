-- pg_mentat regression: component cascade retraction
-- When a parent entity is retracted, :db/isComponent children are retracted too

-- Define component attribute
\echo Setup: component attribute schema

SELECT mentat_transact('[
  {:db/ident :comp/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :comp/child
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many
   :db/isComponent true}
  {:db/ident :comp/label
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Create parent -> child chain
\echo Test: create parent with component children

SELECT mentat_transact('[
  {:comp/name "parent"
   :comp/child [{:comp/label "child-1"}
                {:comp/label "child-2"}]}
]');

-- Verify parent and children exist
SELECT mentat_query(
  '[:find ?name :where [?e :comp/name ?name]]',
  '{}'::jsonb
);

SELECT mentat_query(
  '[:find ?label :where [?e :comp/label ?label] :order (asc ?label)]',
  '{}'::jsonb
);

-- Retract parent entity
\echo Test: retract parent cascades to children

SELECT mentat_transact('[
  [:db.fn/retractEntity [:comp/name "parent"]]
]');

-- Verify parent is gone
SELECT mentat_query(
  '[:find ?name :where [?e :comp/name ?name]]',
  '{}'::jsonb
);

-- Verify children are also retracted (component cascade)
SELECT mentat_query(
  '[:find ?label :where [?e :comp/label ?label]]',
  '{}'::jsonb
);
