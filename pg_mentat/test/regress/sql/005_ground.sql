-- pg_mentat regression: ground where-function (Phase 3 feature)

-- Ground integer: bind constant to variable
SELECT mentat_query(
  '[:find ?name :where [(ground 30) ?age] [?e :person/age ?age] [?e :person/name ?name]]',
  '{}'::jsonb
);

-- Ground string: bind string constant
SELECT mentat_query(
  '[:find ?e ?age :where [(ground "Alice") ?name] [?e :person/name ?name] [?e :person/age ?age]]',
  '{}'::jsonb
);

-- Ground in :find only (variable not in pattern value position)
SELECT mentat_query(
  '[:find ?name ?label :where [(ground "senior") ?label] [?e :person/name ?name] [?e :person/age ?age] [(>= ?age 30)] :order (asc ?name)]',
  '{}'::jsonb
);
