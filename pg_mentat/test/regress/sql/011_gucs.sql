-- pg_mentat regression: GUC settings (Phase 4)
-- Force library load so GUCs are registered
LOAD 'pg_mentat';

SHOW mentat.explain_format;
SET mentat.explain_format = 'json';
SHOW mentat.explain_format;

SET mentat.query_timeout_ms = 5000;
SHOW mentat.query_timeout_ms;

SET mentat.max_result_rows = 500;
SHOW mentat.max_result_rows;

-- Reset to defaults
RESET mentat.explain_format;
SHOW mentat.explain_format;
