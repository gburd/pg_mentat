-- pg_mentat regression: NOT clauses
-- NOT with predicates (Phase 3 feature)

-- NOT: find people NOT younger than 30
SELECT mentat_query(
  '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] (not [?e :person/age ?a] [(< ?a 30)]) :order (asc ?name)]',
  '{}'::jsonb
);

-- NOT: exclude a specific name
SELECT mentat_query(
  '[:find ?name :where [?e :person/name ?name] (not [?e :person/name "Alice"]) :order (asc ?name)]',
  '{}'::jsonb
);
