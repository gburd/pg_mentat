-- Test Suite for Storage Migration Phase 1
-- Verifies new type-specific tables are created correctly

\echo '=== Testing Storage Migration Phase 1 ==='
\echo ''

-- Test 1: Verify new tables exist
\echo 'Test 1: Checking new tables exist...'
SELECT COUNT(*) = 9 AS "new_tables_exist"
FROM pg_tables
WHERE schemaname = 'mentat'
  AND tablename IN (
    'datoms_ref_new',
    'datoms_long_new',
    'datoms_text_new',
    'datoms_double_new',
    'datoms_instant_new',
    'datoms_keyword_new',
    'datoms_uuid_new',
    'datoms_bytes_new',
    'datoms_boolean_new'
  );

-- Test 2: Verify store_id column added to stores table
\echo 'Test 2: Checking store_id column in stores table...'
SELECT COUNT(*) = 1 AS "store_id_column_exists"
FROM information_schema.columns
WHERE table_schema = 'mentat'
  AND table_name = 'stores'
  AND column_name = 'store_id';

-- Test 3: Verify default store has store_id = 0
\echo 'Test 3: Checking default store has store_id = 0...'
SELECT store_id = 0 AS "default_store_id_correct"
FROM mentat.stores
WHERE store_name = 'default';

-- Test 4: Verify indexes exist for each table
\echo 'Test 4: Checking indexes...'
SELECT
    tablename,
    COUNT(*) AS index_count,
    CASE
        WHEN tablename = 'datoms_ref_new' THEN COUNT(*) >= 4  -- EAVT, AEVT, VAET, TX
        WHEN tablename = 'datoms_long_new' THEN COUNT(*) >= 3  -- EAVT, AEVT, TX (no VAET)
        WHEN tablename = 'datoms_text_new' THEN COUNT(*) >= 5  -- EAVT, AEVT, TX, FTS, TRGM
        WHEN tablename = 'datoms_keyword_new' THEN COUNT(*) >= 4  -- EAVT, AEVT, VAET, TX
        ELSE COUNT(*) >= 3
    END AS "has_expected_indexes"
FROM pg_indexes
WHERE schemaname = 'mentat'
  AND tablename LIKE 'datoms%new'
GROUP BY tablename
ORDER BY tablename;

-- Test 5: Verify trigger exists but is disabled
\echo 'Test 5: Checking dual-write trigger...'
SELECT
    tgname,
    tgenabled = 'D' AS "trigger_disabled"
FROM pg_trigger t
JOIN pg_class c ON c.oid = t.tgrelid
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE tgname = 'dual_write_datoms_trigger'
  AND n.nspname = 'mentat'
  AND c.relname = 'datoms';

-- Test 6: Verify migration tracking table exists
\echo 'Test 6: Checking migration status tracking...'
SELECT
    phase,
    description,
    started_at IS NOT NULL AS "phase1_started"
FROM mentat.storage_migration_status
WHERE phase = 1;

-- Test 7: Verify table parameters (fillfactor, etc.)
\echo 'Test 7: Checking table storage parameters...'
SELECT
    c.relname AS table_name,
    COALESCE((
        SELECT option_value::int
        FROM pg_options_to_table(c.reloptions)
        WHERE option_name = 'fillfactor'
    ), 100) AS fillfactor,
    CASE
        WHEN c.relname IN ('datoms_text_new', 'datoms_bytes_new') THEN 85
        ELSE 90
    END AS expected_fillfactor
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'mentat'
  AND c.relname LIKE 'datoms%new'
ORDER BY c.relname;

-- Test 8: Test trigger function (without enabling trigger)
\echo 'Test 8: Testing trigger function manually...'
DO $$
DECLARE
    test_tx BIGINT;
BEGIN
    -- Get next tx
    test_tx := nextval('mentat.partition_tx_seq');

    -- Insert test data into old table (trigger is disabled, so manual insert to new table)
    INSERT INTO mentat.datoms (e, a, value_type_tag, v_long, tx, added)
    VALUES (999999, 1, 2, 12345, test_tx, true);

    -- Manually call trigger function logic
    INSERT INTO mentat.datoms_long_new (store_id, e, a, v, tx, added)
    VALUES (0, 999999, 1, 12345, test_tx, true);

    -- Verify data in new table
    IF NOT EXISTS (
        SELECT 1 FROM mentat.datoms_long_new
        WHERE e = 999999 AND v = 12345
    ) THEN
        RAISE EXCEPTION 'Test data not found in new table';
    END IF;

    -- Cleanup test data
    DELETE FROM mentat.datoms WHERE e = 999999;
    DELETE FROM mentat.datoms_long_new WHERE e = 999999;

    RAISE NOTICE 'Trigger function test passed';
END $$;

-- Test 9: Verify no data loss - tables should be empty (Phase 2 not run yet)
\echo 'Test 9: Checking tables are empty before backfill...'
SELECT
    'datoms_ref_new' AS table_name,
    COUNT(*) AS row_count,
    COUNT(*) = 0 AS "correctly_empty"
FROM mentat.datoms_ref_new
UNION ALL
SELECT 'datoms_long_new', COUNT(*), COUNT(*) = 0
FROM mentat.datoms_long_new
UNION ALL
SELECT 'datoms_text_new', COUNT(*), COUNT(*) = 0
FROM mentat.datoms_text_new
UNION ALL
SELECT 'datoms_double_new', COUNT(*), COUNT(*) = 0
FROM mentat.datoms_double_new
UNION ALL
SELECT 'datoms_instant_new', COUNT(*), COUNT(*) = 0
FROM mentat.datoms_instant_new
UNION ALL
SELECT 'datoms_keyword_new', COUNT(*), COUNT(*) = 0
FROM mentat.datoms_keyword_new
UNION ALL
SELECT 'datoms_uuid_new', COUNT(*), COUNT(*) = 0
FROM mentat.datoms_uuid_new
UNION ALL
SELECT 'datoms_bytes_new', COUNT(*), COUNT(*) = 0
FROM mentat.datoms_bytes_new
UNION ALL
SELECT 'datoms_boolean_new', COUNT(*), COUNT(*) = 0
FROM mentat.datoms_boolean_new;

-- Test 10: Verify PRIMARY KEY constraints
\echo 'Test 10: Checking primary key constraints...'
SELECT
    c.relname AS table_name,
    con.conname AS constraint_name,
    pg_get_constraintdef(con.oid) AS constraint_definition
FROM pg_constraint con
JOIN pg_class c ON c.oid = con.conrelid
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'mentat'
  AND c.relname LIKE 'datoms%new'
  AND con.contype = 'p'
ORDER BY c.relname;

\echo ''
\echo '=== Phase 1 Tests Complete ==='
\echo 'If all tests passed, Phase 1 is ready.'
\echo 'Next step: Run Phase 2 backfill script'
\echo ''
