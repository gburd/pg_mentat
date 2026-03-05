-- Bootstrap SQL for pg_mentat extension
-- This file orchestrates the complete schema initialization

-- EDN Type is automatically created by pgrx with the following functions:
-- - mentat.edn_in(text) -> EdnValue
-- - mentat.edn_out(EdnValue) -> text
-- - mentat.edn_send(EdnValue) -> bytea
-- - mentat.edn_recv(bytea) -> EdnValue

-- Load schema components in order
\i 01_types.sql
\i 02_tables.sql
\i 03_indexes.sql
\i 04_constraints.sql
\i 05_functions.sql
\i 06_bootstrap_data.sql

-- Grant usage on schema to public
GRANT USAGE ON SCHEMA mentat TO PUBLIC;

-- Grant permissions on tables
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA mentat TO PUBLIC;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA mentat TO PUBLIC;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA mentat TO PUBLIC;
