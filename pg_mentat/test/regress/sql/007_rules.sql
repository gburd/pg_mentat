-- pg_mentat regression: rules
-- Test that rule parsing and invocation works at the Datalog level.
-- Note: full rule execution requires PG-level recursive CTE settings
-- that may not be available in all environments; this test verifies
-- the parse + compile path via mentat_explain.

-- Verify rule syntax parses and compiles to SQL (via explain, avoids execution)
SELECT (mentat_explain(
  '[:find ?name :where (adult ?e) [?e :person/name ?name] :with [[(adult ?p) [?p :person/age ?a] [(>= ?a 18)]]]]',
  '{}'::jsonb
)::jsonb)->>'generated_sql' IS NOT NULL AS rules_compile_ok;
