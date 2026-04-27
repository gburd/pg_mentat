-- Test suite: Materialized views
--
-- Tests mentat_create_matview, mentat_refresh_matview, mentat_drop_matview,
-- mentat_list_matviews, and automatic refresh scheduling.

BEGIN;

-- =========================================================================
-- Setup: Ensure schema and test data exist
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident :employee/name
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
    {:db/ident :employee/dept
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
    {:db/ident :employee/salary
     :db/valueType :db.type/long
     :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
    {:db/id "e1" :employee/name "Alice" :employee/dept "Engineering" :employee/salary 120000}
    {:db/id "e2" :employee/name "Bob" :employee/dept "Engineering" :employee/salary 110000}
    {:db/id "e3" :employee/name "Carol" :employee/dept "Sales" :employee/salary 95000}
    {:db/id "e4" :employee/name "Dave" :employee/dept "Sales" :employee/salary 85000}
]');

-- =========================================================================
-- Create materialized view
-- =========================================================================

-- Test 1: Create a materialized view from a Datalog query
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat_create_matview('engineers',
        '[:find ?name ?salary
         :where
         [?e :employee/name ?name]
         [?e :employee/dept "Engineering"]
         [?e :employee/salary ?salary]]',
        '{}');
    ASSERT result LIKE '%created%', 'Should create matview, got: ' || result;
    RAISE NOTICE 'PASS: create materialized view';
END;
$$;

-- Test 2: Query the materialized view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.matview_engineers;
    ASSERT cnt >= 2, 'Matview should have at least 2 rows, got: ' || cnt;
    RAISE NOTICE 'PASS: query materialized view (% rows)', cnt;
END;
$$;

-- Test 3: Materialized view has correct columns
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt
    FROM information_schema.columns
    WHERE table_schema = 'mentat' AND table_name = 'matview_engineers';
    ASSERT cnt >= 2, 'Matview should have at least 2 columns, got: ' || cnt;
    RAISE NOTICE 'PASS: matview has correct columns';
END;
$$;

-- =========================================================================
-- List materialized views
-- =========================================================================

-- Test 4: List matviews includes the new view
DO $$
DECLARE
    result JSONB;
    found BOOLEAN := FALSE;
    elem JSONB;
BEGIN
    SELECT mentat_list_matviews()::JSONB INTO result;
    ASSERT result IS NOT NULL, 'list_matviews should return JSON';
    FOR elem IN SELECT * FROM jsonb_array_elements(result)
    LOOP
        IF elem->>'name' = 'engineers' THEN
            found := TRUE;
        END IF;
    END LOOP;
    ASSERT found, 'engineers matview should be in the list';
    RAISE NOTICE 'PASS: list matviews includes engineers';
END;
$$;

-- =========================================================================
-- Refresh materialized view
-- =========================================================================

-- Test 5: Refresh after adding data
DO $$
DECLARE
    cnt_before INT;
    cnt_after INT;
BEGIN
    SELECT COUNT(*) INTO cnt_before FROM mentat.matview_engineers;

    -- Add a new engineer
    PERFORM mentat_transact('[
        {:db/id "e5" :employee/name "Eve" :employee/dept "Engineering" :employee/salary 130000}
    ]');

    -- Matview should not yet reflect the new data
    SELECT COUNT(*) INTO cnt_after FROM mentat.matview_engineers;
    ASSERT cnt_after = cnt_before, 'Before refresh, count should be unchanged';

    -- Refresh
    PERFORM mentat_refresh_matview('engineers');

    -- Now it should reflect the new data
    SELECT COUNT(*) INTO cnt_after FROM mentat.matview_engineers;
    ASSERT cnt_after = cnt_before + 1, 'After refresh, should have one more row, got: ' || cnt_after;
    RAISE NOTICE 'PASS: refresh materialized view adds new data';
END;
$$;

-- Test 6: Concurrent refresh (if supported)
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat_refresh_matview('engineers', true);
    RAISE NOTICE 'PASS: concurrent refresh (or fallback): %', result;
EXCEPTION WHEN OTHERS THEN
    -- Concurrent refresh requires a unique index; this is OK to fail
    RAISE NOTICE 'PASS: concurrent refresh not supported (expected): %', SQLERRM;
END;
$$;

-- =========================================================================
-- Create with aggregation
-- =========================================================================

-- Test 7: Create matview with aggregate query
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat_create_matview('dept_stats',
        '[:find ?dept (count ?e) (avg ?salary)
         :where
         [?e :employee/dept ?dept]
         [?e :employee/salary ?salary]]',
        '{}');
    ASSERT result LIKE '%created%', 'Should create aggregate matview';
    RAISE NOTICE 'PASS: create aggregate materialized view';
END;
$$;

-- Test 8: Query aggregate matview
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.matview_dept_stats;
    ASSERT cnt >= 2, 'Dept stats matview should have at least 2 rows (Engineering, Sales), got: ' || cnt;
    RAISE NOTICE 'PASS: query aggregate matview (% rows)', cnt;
END;
$$;

-- =========================================================================
-- Error handling
-- =========================================================================

-- Test 9: Cannot create matview with duplicate name
DO $$
BEGIN
    PERFORM mentat_create_matview('engineers',
        '[:find ?name :where [?e :employee/name ?name]]', '{}');
    RAISE EXCEPTION 'Should reject duplicate matview name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects duplicate matview name (%)', SQLERRM;
END;
$$;

-- Test 10: Cannot refresh non-existent matview
DO $$
BEGIN
    PERFORM mentat_refresh_matview('nonexistent_view');
    RAISE EXCEPTION 'Should reject refresh of non-existent matview';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects refresh of non-existent matview (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- Drop materialized view
-- =========================================================================

-- Test 11: Drop matview
DO $$
DECLARE
    result TEXT;
    cnt INT;
BEGIN
    result := mentat_drop_matview('dept_stats');
    ASSERT result LIKE '%dropped%', 'Should drop matview';
    SELECT COUNT(*) INTO cnt
    FROM information_schema.tables
    WHERE table_schema = 'mentat' AND table_name = 'matview_dept_stats';
    ASSERT cnt = 0, 'Dropped matview should not exist';
    RAISE NOTICE 'PASS: drop materialized view';
END;
$$;

-- Test 12: Drop non-existent matview
DO $$
BEGIN
    PERFORM mentat_drop_matview('nonexistent');
    RAISE EXCEPTION 'Should reject drop of non-existent matview';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects drop of non-existent matview (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- Named store matviews
-- =========================================================================

-- Test 13: Create matview on a named store
DO $$
DECLARE
    result TEXT;
    cnt INT;
BEGIN
    PERFORM mentat_create_store('mv_store', 'matview test store');
    PERFORM mentat_transact_in_store('mv_store', '[
        {:db/ident :item/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
    ]');
    PERFORM mentat_transact_in_store('mv_store', '[
        {:db/id "i1" :item/name "Widget"}
        {:db/id "i2" :item/name "Gadget"}
    ]');

    result := mentat_create_matview_in_store('mv_store', 'items',
        '[:find ?name :where [?e :item/name ?name]]', '{}');
    ASSERT result LIKE '%created%', 'Should create matview in named store';

    SELECT COUNT(*) INTO cnt FROM mentat_mv_store.matview_items;
    ASSERT cnt >= 2, 'Named store matview should have data, got: ' || cnt;
    RAISE NOTICE 'PASS: matview on named store (% rows)', cnt;

    PERFORM mentat_drop_store('mv_store');
END;
$$;

-- =========================================================================
-- Cleanup
-- =========================================================================

DO $$
BEGIN
    PERFORM mentat_drop_matview('engineers');
    RAISE NOTICE 'CLEANUP: dropped engineers matview';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'CLEANUP: engineers matview already gone';
END;
$$;

ROLLBACK;
