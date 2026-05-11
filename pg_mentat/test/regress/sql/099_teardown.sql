-- pg_mentat regression: teardown
DROP EXTENSION IF EXISTS pg_mentat CASCADE;
SELECT 'extension_dropped' AS status;
