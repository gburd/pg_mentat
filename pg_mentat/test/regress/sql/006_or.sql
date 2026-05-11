-- pg_mentat regression: OR branches with arithmetic (Phase 3 feature)

-- OR: find people aged 25 OR 35
SELECT mentat_query(
  '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] (or [?e :person/age 25] [?e :person/age 35]) :order (asc ?name)]',
  '{}'::jsonb
);

-- OR with predicates: age < 26 OR age > 34
SELECT mentat_query(
  '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] (or (and [?e :person/age ?a1] [(< ?a1 26)]) (and [?e :person/age ?a2] [(> ?a2 34)])) :order (asc ?name)]',
  '{}'::jsonb
);
