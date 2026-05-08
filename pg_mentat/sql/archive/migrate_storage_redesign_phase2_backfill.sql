-- Storage Redesign Phase 2: Backfill Data
--
-- This script migrates existing datoms from the partitioned wide-row tables
-- to the new type-specific tables with store_id support.
--
-- IMPORTANT: Run this AFTER Phase 1 is complete and tested.
--
-- Execution time depends on dataset size:
-- - 1M datoms: ~30 seconds
-- - 10M datoms: ~5 minutes
-- - 100M datoms: ~1 hour
--
-- Monitor progress with:
-- SELECT phase, description, started_at, completed_at
-- FROM mentat.storage_migration_status;

-- Mark Phase 2 as started
UPDATE mentat.storage_migration_status
SET started_at = NOW()
WHERE phase = 2 AND started_at IS NULL;

-- --------------------------------------------------------------------------
-- Helper function to count progress
-- --------------------------------------------------------------------------

CREATE OR REPLACE FUNCTION mentat.show_migration_progress()
RETURNS TABLE (
    table_name TEXT,
    old_count BIGINT,
    new_count BIGINT,
    pct_complete NUMERIC
) AS $$
BEGIN
    RETURN QUERY
    SELECT 'datoms_ref'::TEXT,
           (SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 0),
           (SELECT COUNT(*) FROM mentat.datoms_ref_new),
           (SELECT ROUND(100.0 * COUNT(*) / NULLIF((SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 0), 0), 2)
            FROM mentat.datoms_ref_new)
    UNION ALL
    SELECT 'datoms_long'::TEXT,
           (SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 2),
           (SELECT COUNT(*) FROM mentat.datoms_long_new),
           (SELECT ROUND(100.0 * COUNT(*) / NULLIF((SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 2), 0), 2)
            FROM mentat.datoms_long_new)
    UNION ALL
    SELECT 'datoms_text'::TEXT,
           (SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 7),
           (SELECT COUNT(*) FROM mentat.datoms_text_new),
           (SELECT ROUND(100.0 * COUNT(*) / NULLIF((SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 7), 0), 2)
            FROM mentat.datoms_text_new)
    UNION ALL
    SELECT 'datoms_double'::TEXT,
           (SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 3),
           (SELECT COUNT(*) FROM mentat.datoms_double_new),
           (SELECT ROUND(100.0 * COUNT(*) / NULLIF((SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 3), 0), 2)
            FROM mentat.datoms_double_new)
    UNION ALL
    SELECT 'datoms_instant'::TEXT,
           (SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 4),
           (SELECT COUNT(*) FROM mentat.datoms_instant_new),
           (SELECT ROUND(100.0 * COUNT(*) / NULLIF((SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 4), 0), 2)
            FROM mentat.datoms_instant_new)
    UNION ALL
    SELECT 'datoms_keyword'::TEXT,
           (SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 8),
           (SELECT COUNT(*) FROM mentat.datoms_keyword_new),
           (SELECT ROUND(100.0 * COUNT(*) / NULLIF((SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 8), 0), 2)
            FROM mentat.datoms_keyword_new)
    UNION ALL
    SELECT 'datoms_uuid'::TEXT,
           (SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 10),
           (SELECT COUNT(*) FROM mentat.datoms_uuid_new),
           (SELECT ROUND(100.0 * COUNT(*) / NULLIF((SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 10), 0), 2)
            FROM mentat.datoms_uuid_new)
    UNION ALL
    SELECT 'datoms_bytes'::TEXT,
           (SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 11),
           (SELECT COUNT(*) FROM mentat.datoms_bytes_new),
           (SELECT ROUND(100.0 * COUNT(*) / NULLIF((SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 11), 0), 2)
            FROM mentat.datoms_bytes_new)
    UNION ALL
    SELECT 'datoms_boolean'::TEXT,
           (SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 1),
           (SELECT COUNT(*) FROM mentat.datoms_boolean_new),
           (SELECT ROUND(100.0 * COUNT(*) / NULLIF((SELECT COUNT(*) FROM mentat.datoms WHERE value_type_tag = 1), 0), 2)
            FROM mentat.datoms_boolean_new);
END;
$$ LANGUAGE plpgsql;

-- --------------------------------------------------------------------------
-- Backfill data from old to new tables
-- --------------------------------------------------------------------------

DO $$
DECLARE
    start_time TIMESTAMPTZ;
    row_count BIGINT;
BEGIN
    RAISE NOTICE 'Starting data backfill...';
    start_time := clock_timestamp();

    -- Ref values
    RAISE NOTICE 'Migrating ref values...';
    INSERT INTO mentat.datoms_ref_new (store_id, e, a, v, tx, added)
    SELECT 0, e, a, v_ref, tx, added
    FROM mentat.datoms
    WHERE value_type_tag = 0
    ON CONFLICT (store_id, e, a, tx) DO NOTHING;
    GET DIAGNOSTICS row_count = ROW_COUNT;
    RAISE NOTICE 'Migrated % ref datoms in %', row_count, clock_timestamp() - start_time;

    -- Boolean values
    RAISE NOTICE 'Migrating boolean values...';
    start_time := clock_timestamp();
    INSERT INTO mentat.datoms_boolean_new (store_id, e, a, v, tx, added)
    SELECT 0, e, a, v_bool, tx, added
    FROM mentat.datoms
    WHERE value_type_tag = 1
    ON CONFLICT (store_id, e, a, tx) DO NOTHING;
    GET DIAGNOSTICS row_count = ROW_COUNT;
    RAISE NOTICE 'Migrated % boolean datoms in %', row_count, clock_timestamp() - start_time;

    -- Long values
    RAISE NOTICE 'Migrating long values...';
    start_time := clock_timestamp();
    INSERT INTO mentat.datoms_long_new (store_id, e, a, v, tx, added)
    SELECT 0, e, a, v_long, tx, added
    FROM mentat.datoms
    WHERE value_type_tag = 2
    ON CONFLICT (store_id, e, a, tx) DO NOTHING;
    GET DIAGNOSTICS row_count = ROW_COUNT;
    RAISE NOTICE 'Migrated % long datoms in %', row_count, clock_timestamp() - start_time;

    -- Double values
    RAISE NOTICE 'Migrating double values...';
    start_time := clock_timestamp();
    INSERT INTO mentat.datoms_double_new (store_id, e, a, v, tx, added)
    SELECT 0, e, a, v_double, tx, added
    FROM mentat.datoms
    WHERE value_type_tag = 3
    ON CONFLICT (store_id, e, a, tx) DO NOTHING;
    GET DIAGNOSTICS row_count = ROW_COUNT;
    RAISE NOTICE 'Migrated % double datoms in %', row_count, clock_timestamp() - start_time;

    -- Instant values
    RAISE NOTICE 'Migrating instant values...';
    start_time := clock_timestamp();
    INSERT INTO mentat.datoms_instant_new (store_id, e, a, v, tx, added)
    SELECT 0, e, a, v_instant, tx, added
    FROM mentat.datoms
    WHERE value_type_tag = 4
    ON CONFLICT (store_id, e, a, tx) DO NOTHING;
    GET DIAGNOSTICS row_count = ROW_COUNT;
    RAISE NOTICE 'Migrated % instant datoms in %', row_count, clock_timestamp() - start_time;

    -- Text values
    RAISE NOTICE 'Migrating text values...';
    start_time := clock_timestamp();
    INSERT INTO mentat.datoms_text_new (store_id, e, a, v, tx, added)
    SELECT 0, e, a, v_text, tx, added
    FROM mentat.datoms
    WHERE value_type_tag = 7
    ON CONFLICT (store_id, e, a, tx) DO NOTHING;
    GET DIAGNOSTICS row_count = ROW_COUNT;
    RAISE NOTICE 'Migrated % text datoms in %', row_count, clock_timestamp() - start_time;

    -- Keyword values
    RAISE NOTICE 'Migrating keyword values...';
    start_time := clock_timestamp();
    INSERT INTO mentat.datoms_keyword_new (store_id, e, a, v, tx, added)
    SELECT 0, e, a, v_keyword, tx, added
    FROM mentat.datoms
    WHERE value_type_tag = 8
    ON CONFLICT (store_id, e, a, tx) DO NOTHING;
    GET DIAGNOSTICS row_count = ROW_COUNT;
    RAISE NOTICE 'Migrated % keyword datoms in %', row_count, clock_timestamp() - start_time;

    -- UUID values
    RAISE NOTICE 'Migrating UUID values...';
    start_time := clock_timestamp();
    INSERT INTO mentat.datoms_uuid_new (store_id, e, a, v, tx, added)
    SELECT 0, e, a, v_uuid, tx, added
    FROM mentat.datoms
    WHERE value_type_tag = 10
    ON CONFLICT (store_id, e, a, tx) DO NOTHING;
    GET DIAGNOSTICS row_count = ROW_COUNT;
    RAISE NOTICE 'Migrated % UUID datoms in %', row_count, clock_timestamp() - start_time;

    -- Bytes values
    RAISE NOTICE 'Migrating bytes values...';
    start_time := clock_timestamp();
    INSERT INTO mentat.datoms_bytes_new (store_id, e, a, v, tx, added)
    SELECT 0, e, a, v_bytes, tx, added
    FROM mentat.datoms
    WHERE value_type_tag = 11
    ON CONFLICT (store_id, e, a, tx) DO NOTHING;
    GET DIAGNOSTICS row_count = ROW_COUNT;
    RAISE NOTICE 'Migrated % bytes datoms in %', row_count, clock_timestamp() - start_time;

    RAISE NOTICE 'Backfill complete!';
END $$;

-- Verify counts match
SELECT * FROM mentat.show_migration_progress();

-- Mark Phase 2 as complete
UPDATE mentat.storage_migration_status
SET completed_at = NOW()
WHERE phase = 2;

-- Update statistics for query planner
ANALYZE mentat.datoms_ref_new;
ANALYZE mentat.datoms_boolean_new;
ANALYZE mentat.datoms_long_new;
ANALYZE mentat.datoms_double_new;
ANALYZE mentat.datoms_instant_new;
ANALYZE mentat.datoms_text_new;
ANALYZE mentat.datoms_keyword_new;
ANALYZE mentat.datoms_uuid_new;
ANALYZE mentat.datoms_bytes_new;

-- Show final statistics
SELECT
    schemaname,
    tablename,
    pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) AS total_size,
    pg_size_pretty(pg_relation_size(schemaname||'.'||tablename)) AS table_size,
    pg_size_pretty(pg_indexes_size(schemaname||'.'||tablename)) AS index_size
FROM pg_tables
WHERE schemaname = 'mentat'
  AND tablename LIKE 'datoms%new'
ORDER BY pg_total_relation_size(schemaname||'.'||tablename) DESC;
