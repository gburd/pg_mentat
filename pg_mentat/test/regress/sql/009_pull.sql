-- pg_mentat regression: pull, pull_many, entity

-- Find an entity by unique attribute
SELECT mentat_query(
  '[:find ?e :where [?e :person/email "alice@example.com"]]',
  '{}'::jsonb
);

-- Verify mentat_explain works (uses query not pull, since pull API takes entity_id)
SELECT jsonb_typeof(
  mentat_explain(
    '[:find ?e :where [?e :person/email "alice@example.com"]]',
    '{}'::jsonb
  )::jsonb
) AS explain_result_type;
