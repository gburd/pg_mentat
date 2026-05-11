-- pg_mentat regression: setup
-- Create extension and set search path

CREATE EXTENSION IF NOT EXISTS pg_mentat;

SELECT 'extension_created' AS status;
