-- pg_mentat regression: mentat_explain shape (Phase 4)

-- Verify explain returns all expected keys
SELECT jsonb_object_keys(
  mentat_explain(
    '[:find ?e :where [?e :person/name "Alice"]]',
    '{}'::jsonb
  )::jsonb
) ORDER BY 1;

-- Verify datalog_plan contains expected fields
SELECT jsonb_object_keys(
  (mentat_explain(
    '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]',
    '{}'::jsonb
  )::jsonb)->'datalog_plan'
) ORDER BY 1;
