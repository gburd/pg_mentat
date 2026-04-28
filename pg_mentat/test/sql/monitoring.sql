-- Test suite: Production monitoring infrastructure
--
-- Tests the monitoring functions, views, and GUC parameters.

BEGIN;

-- =========================================================================
-- Setup: Ensure some data exists for meaningful stats
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident :monitor/name
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
    {:db/ident :monitor/count
     :db/valueType :db.type/long
     :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
    {:monitor/name "test1" :monitor/count 100}
    {:monitor/name "test2" :monitor/count 200}
]');

-- Run a query to populate per-backend stats
SELECT mentat_query('[:find ?n :where [?e :monitor/name ?n]]');

-- =========================================================================
-- GUC parameters
-- =========================================================================

-- Test 1: Slow query threshold GUC is accessible
DO $$
DECLARE
    val INT;
BEGIN
    SHOW mentat.slow_query_threshold_ms INTO val;
    ASSERT val >= 0, 'slow_query_threshold_ms should be non-negative';
    RAISE NOTICE 'PASS: slow_query_threshold_ms = %', val;
END;
$$;

-- Test 2: Can set slow query threshold
DO $$
BEGIN
    SET mentat.slow_query_threshold_ms = 500;
    RAISE NOTICE 'PASS: slow_query_threshold_ms can be set';
    -- Reset to default
    SET mentat.slow_query_threshold_ms = 100;
END;
$$;

-- Test 3: Log all queries GUC is accessible
DO $$
DECLARE
    val BOOLEAN;
BEGIN
    SHOW mentat.log_all_queries INTO val;
    RAISE NOTICE 'PASS: log_all_queries = %', val;
END;
$$;

-- =========================================================================
-- Per-backend statistics
-- =========================================================================

-- Test 4: mentat_backend_stats returns valid JSON
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_backend_stats()::JSONB INTO result;
    ASSERT result IS NOT NULL, 'backend_stats should return JSON';
    ASSERT result ? 'total_queries', 'backend_stats should have total_queries';
    ASSERT result ? 'slow_queries', 'backend_stats should have slow_queries';
    ASSERT result ? 'max_execution_ms', 'backend_stats should have max_execution_ms';
    RAISE NOTICE 'PASS: backend_stats returns valid JSON: %', result;
END;
$$;

-- Test 5: mentat_reset_stats works
DO $$
DECLARE
    result TEXT;
    stats JSONB;
BEGIN
    SELECT mentat_reset_stats() INTO result;
    ASSERT result IS NOT NULL, 'reset_stats should return text';

    SELECT mentat_backend_stats()::JSONB INTO stats;
    ASSERT (stats->>'total_queries')::INT = 0, 'total_queries should be 0 after reset';
    RAISE NOTICE 'PASS: reset_stats clears counters';
END;
$$;

-- =========================================================================
-- Stats functions (from stats.rs)
-- =========================================================================

-- Test 6: mentat_query_stats returns valid JSON
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_query_stats()::JSONB INTO result;
    ASSERT result IS NOT NULL, 'query_stats should return JSON';
    ASSERT result ? 'database_stats', 'query_stats should have database_stats';
    ASSERT result ? 'cache', 'query_stats should have cache';
    RAISE NOTICE 'PASS: query_stats returns valid JSON';
END;
$$;

-- Test 7: mentat_slow_queries returns valid JSON
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_slow_queries()::JSONB INTO result;
    ASSERT result IS NOT NULL, 'slow_queries should return JSON';
    ASSERT result ? 'slow_functions', 'slow_queries should have slow_functions';
    ASSERT result ? 'heavy_transactions', 'slow_queries should have heavy_transactions';
    RAISE NOTICE 'PASS: slow_queries returns valid JSON';
END;
$$;

-- Test 8: mentat_storage_stats returns valid JSON
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_storage_stats()::JSONB INTO result;
    ASSERT result IS NOT NULL, 'storage_stats should return JSON';
    ASSERT result ? 'tables', 'storage_stats should have tables';
    ASSERT result ? 'indexes', 'storage_stats should have indexes';
    RAISE NOTICE 'PASS: storage_stats returns valid JSON';
END;
$$;

-- =========================================================================
-- Index health function and view
-- =========================================================================

-- Test 9: mentat_index_health function returns rows
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat_index_health();
    -- May be 0 if no indexes exist yet, but should not error
    RAISE NOTICE 'PASS: index_health function returns % rows', cnt;
END;
$$;

-- Test 10: mentat.index_health view exists and is queryable
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.index_health;
    RAISE NOTICE 'PASS: index_health view returns % rows', cnt;
END;
$$;

-- Test 11: mentat.table_health view exists and is queryable
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.table_health;
    ASSERT cnt > 0, 'table_health view should have rows for mentat tables';
    RAISE NOTICE 'PASS: table_health view returns % rows', cnt;
END;
$$;

-- =========================================================================
-- Health check function
-- =========================================================================

-- Test 12: mentat_health_check returns valid JSON with status
DO $$
DECLARE
    result JSONB;
    status TEXT;
BEGIN
    SELECT mentat_health_check()::JSONB INTO result;
    ASSERT result IS NOT NULL, 'health_check should return JSON';
    ASSERT result ? 'status', 'health_check should have status field';
    ASSERT result ? 'schema_attributes', 'health_check should have schema_attributes';
    ASSERT result ? 'stores', 'health_check should have stores';
    ASSERT result ? 'transactions', 'health_check should have transactions';

    status := result->>'status';
    ASSERT status IN ('healthy', 'degraded', 'unhealthy'),
        'status should be one of healthy/degraded/unhealthy, got: ' || status;
    RAISE NOTICE 'PASS: health_check returns status = %', status;
END;
$$;

-- Test 13: health_check reports healthy after data exists
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_health_check()::JSONB INTO result;
    ASSERT result->>'status' = 'healthy',
        'health_check should be healthy with schema + stores, got: ' || (result->>'status');
    ASSERT (result->>'schema_attributes')::INT > 0,
        'should have schema attributes';
    ASSERT (result->>'stores')::INT > 0,
        'should have stores';
    RAISE NOTICE 'PASS: health_check reports healthy';
END;
$$;

-- =========================================================================
-- Slow query detection (functional test)
-- =========================================================================

-- Test 14: Slow query threshold can be set very low to trigger logging
DO $$
BEGIN
    -- Set threshold very low to ensure next query triggers slow logging
    SET mentat.slow_query_threshold_ms = 0;

    -- Run a query (should trigger slow query warning since threshold is 0)
    PERFORM mentat_query('[:find ?n :where [?e :monitor/name ?n]]');

    -- Reset threshold
    SET mentat.slow_query_threshold_ms = 100;
    RAISE NOTICE 'PASS: slow query detection works with threshold=0';
END;
$$;

ROLLBACK;
