-- pg_mentat regression: schema verification
-- Verify core tables, views, and bootstrap data exist

-- Check that typed datom tables exist
SELECT count(*) > 0 AS has_ref_table
  FROM information_schema.tables
 WHERE table_schema = 'mentat' AND table_name = 'datoms_ref_new';

SELECT count(*) > 0 AS has_long_table
  FROM information_schema.tables
 WHERE table_schema = 'mentat' AND table_name = 'datoms_long_new';

SELECT count(*) > 0 AS has_text_table
  FROM information_schema.tables
 WHERE table_schema = 'mentat' AND table_name = 'datoms_text_new';

-- Check the schema table has bootstrap entries
SELECT count(*) > 0 AS has_bootstrap_schema
  FROM mentat.schema;

-- Verify stores table exists and has default store
SELECT count(*) > 0 AS has_stores
  FROM mentat.stores;
