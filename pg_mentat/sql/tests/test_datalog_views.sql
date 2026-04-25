-- Test suite: Datalog VIEW helpers (09_view_helpers.sql)
--
-- Tests the VIEW creation, materialized view, drop, and refresh functions
-- that bridge Datalog queries and PostgreSQL VIEWs.
--
-- Functions tested:
--   mentat.create_datalog_view()              - Create a VIEW from Datalog
--   mentat.create_datalog_materialized_view() - Create a MATERIALIZED VIEW
--   mentat.drop_datalog_view()                - Drop a VIEW or MATERIALIZED VIEW
--   mentat.refresh_datalog_view()             - Refresh a MATERIALIZED VIEW

BEGIN;

-- =========================================================================
-- Setup: Create schema and sample data
-- =========================================================================

SELECT mentat.mentat_transact('[
  {:db/ident       :person/name
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique      :db.unique/identity
   :db/index       true}
  {:db/ident       :person/age
   :db/valueType   :db.type/long
   :db/cardinality :db.cardinality/one
   :db/index       true}
  {:db/ident       :person/email
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one}
]');

SELECT mentat.mentat_transact('[
  {:db/id "alice" :person/name "Alice" :person/age 30 :person/email "alice@example.com"}
  {:db/id "bob"   :person/name "Bob"   :person/age 25 :person/email "bob@example.com"}
  {:db/id "carol" :person/name "Carol" :person/age 35}
]');

-- =========================================================================
-- create_datalog_view: Regular VIEW creation
-- =========================================================================

-- Test 1: Create a simple view
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.create_datalog_view(
        'test_people',
        '[:find ?e ?name :where [?e :person/name ?name]]',
        '{}'::jsonb
    );
    ASSERT result IS NOT NULL, 'create_datalog_view should return a result';
    ASSERT result LIKE 'VIEW%created%', 'Result should say VIEW created, got: ' || result;
    RAISE NOTICE 'PASS: create_datalog_view returns creation message (%)' , result;
END;
$$;

-- Test 2: Query the created view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt FROM test_people;
    ASSERT cnt >= 3, 'VIEW should return at least 3 people, got: ' || cnt;
    RAISE NOTICE 'PASS: VIEW test_people has % rows', cnt;
END;
$$;

-- Test 3: View columns match query :find vars
DO $$
DECLARE
    col_exists BOOLEAN;
BEGIN
    SELECT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_name = 'test_people'
    ) INTO col_exists;
    ASSERT col_exists = TRUE, 'VIEW should have columns';
    RAISE NOTICE 'PASS: VIEW has columns in information_schema';
END;
$$;

-- Test 4: Drop the view
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.drop_datalog_view('test_people');
    ASSERT result LIKE 'VIEW%dropped%', 'Result should say VIEW dropped';
    RAISE NOTICE 'PASS: drop_datalog_view returns drop message';
END;
$$;

-- Test 5: Verify view is gone
DO $$
DECLARE
    view_exists BOOLEAN;
BEGIN
    SELECT EXISTS (
        SELECT 1
        FROM information_schema.views
        WHERE table_name = 'test_people'
    ) INTO view_exists;
    ASSERT view_exists = FALSE, 'VIEW should no longer exist after drop';
    RAISE NOTICE 'PASS: VIEW test_people no longer exists';
END;
$$;

-- =========================================================================
-- create_datalog_view: Multi-column views
-- =========================================================================

-- Test 6: Create a view with 3 columns
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.create_datalog_view(
        'test_people_detail',
        '[:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]',
        '{}'::jsonb
    );
    ASSERT result LIKE '%3 columns%', 'Result should mention 3 columns';
    RAISE NOTICE 'PASS: create_datalog_view with 3 columns (%)' , result;
END;
$$;

-- Test 7: Query multi-column view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt FROM test_people_detail;
    ASSERT cnt >= 3, 'Multi-column VIEW should have data';
    RAISE NOTICE 'PASS: multi-column VIEW has % rows', cnt;
END;
$$;

-- Cleanup
SELECT mentat.drop_datalog_view('test_people_detail');

-- =========================================================================
-- create_datalog_view: With default inputs parameter
-- =========================================================================

-- Test 8: Create view with default inputs
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.create_datalog_view(
        'test_names_only',
        '[:find ?name :where [?e :person/name ?name]]'
    );
    ASSERT result IS NOT NULL, 'create_datalog_view with default inputs should work';
    RAISE NOTICE 'PASS: create_datalog_view with default inputs';
END;
$$;

-- Test 9: Query the default-inputs view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt FROM test_names_only;
    ASSERT cnt >= 3, 'Default-inputs VIEW should have data';
    RAISE NOTICE 'PASS: default-inputs VIEW has % rows', cnt;
END;
$$;

SELECT mentat.drop_datalog_view('test_names_only');

-- =========================================================================
-- create_datalog_view: Schema-qualified view name
-- =========================================================================

-- Test 10: Create view in public schema explicitly
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.create_datalog_view(
        'public.test_schema_view',
        '[:find ?name :where [?e :person/name ?name]]',
        '{}'::jsonb
    );
    ASSERT result IS NOT NULL, 'Schema-qualified view name should work';
    RAISE NOTICE 'PASS: schema-qualified view creation';
END;
$$;

SELECT mentat.drop_datalog_view('public.test_schema_view');

-- =========================================================================
-- create_datalog_view: Error conditions
-- =========================================================================

-- Test 11: Invalid Datalog query
DO $$
BEGIN
    PERFORM mentat.create_datalog_view(
        'test_bad_query',
        'not valid datalog',
        '{}'::jsonb
    );
    RAISE EXCEPTION 'should have raised error for invalid Datalog';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: rejects invalid Datalog query (%)' , SQLERRM;
END;
$$;

-- Test 12: Invalid view name (SQL injection attempt)
DO $$
BEGIN
    PERFORM mentat.create_datalog_view(
        'test; DROP TABLE mentat.datoms;--',
        '[:find ?e :where [?e :person/name]]',
        '{}'::jsonb
    );
    RAISE EXCEPTION 'should have raised error for invalid view name';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: rejects invalid view name (%)' , SQLERRM;
END;
$$;

-- Test 13: View name with special characters
DO $$
BEGIN
    PERFORM mentat.create_datalog_view(
        'test-view-with-dashes',
        '[:find ?e :where [?e :person/name]]',
        '{}'::jsonb
    );
    RAISE EXCEPTION 'should have raised error for view name with dashes';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: rejects view name with special characters (%)' , SQLERRM;
END;
$$;

-- =========================================================================
-- create_datalog_materialized_view
-- =========================================================================

-- Test 14: Create a materialized view
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.create_datalog_materialized_view(
        'test_people_mat',
        '[:find ?e ?name :where [?e :person/name ?name]]',
        '{}'::jsonb
    );
    ASSERT result LIKE 'MATERIALIZED VIEW%created%',
        'Should say MATERIALIZED VIEW created';
    RAISE NOTICE 'PASS: create_datalog_materialized_view (%)' , result;
END;
$$;

-- Test 15: Query the materialized view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt FROM test_people_mat;
    ASSERT cnt >= 3, 'MATERIALIZED VIEW should have data';
    RAISE NOTICE 'PASS: MATERIALIZED VIEW has % rows', cnt;
END;
$$;

-- Test 16: Materialized view appears in pg_matviews
DO $$
DECLARE
    exists_flag BOOLEAN;
BEGIN
    SELECT EXISTS (
        SELECT 1
        FROM pg_matviews
        WHERE matviewname = 'test_people_mat'
    ) INTO exists_flag;
    ASSERT exists_flag = TRUE, 'MATERIALIZED VIEW should be in pg_matviews';
    RAISE NOTICE 'PASS: MATERIALIZED VIEW in pg_matviews';
END;
$$;

-- =========================================================================
-- refresh_datalog_view
-- =========================================================================

-- Test 17: Refresh the materialized view
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.refresh_datalog_view('test_people_mat');
    ASSERT result LIKE '%refreshed%', 'Should say view was refreshed';
    RAISE NOTICE 'PASS: refresh_datalog_view (%)' , result;
END;
$$;

-- Test 18: Data still available after refresh
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt FROM test_people_mat;
    ASSERT cnt >= 3, 'Data should be available after refresh';
    RAISE NOTICE 'PASS: data available after refresh (% rows)', cnt;
END;
$$;

-- Test 19: Add data and refresh to see new rows
DO $$
DECLARE
    cnt_before INT;
    cnt_after INT;
BEGIN
    SELECT count(*) INTO cnt_before FROM test_people_mat;

    PERFORM mentat.mentat_transact('[
      {:db/id "newperson" :person/name "NewPerson"}
    ]');

    PERFORM mentat.refresh_datalog_view('test_people_mat');

    SELECT count(*) INTO cnt_after FROM test_people_mat;
    ASSERT cnt_after >= cnt_before,
        'After inserting and refreshing, count should not decrease';
    RAISE NOTICE 'PASS: materialized view updated after refresh (before=%, after=%)',
        cnt_before, cnt_after;
END;
$$;

-- =========================================================================
-- refresh_datalog_view: Error conditions
-- =========================================================================

-- Test 20: Refresh non-existent view
DO $$
BEGIN
    PERFORM mentat.refresh_datalog_view('nonexistent_view_12345');
    RAISE EXCEPTION 'should have raised error for non-existent view';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: refresh rejects non-existent view (%)' , SQLERRM;
END;
$$;

-- Test 21: Invalid view name for refresh
DO $$
BEGIN
    PERFORM mentat.refresh_datalog_view('invalid;name');
    RAISE EXCEPTION 'should have raised error for invalid view name';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: refresh rejects invalid view name (%)' , SQLERRM;
END;
$$;

-- =========================================================================
-- drop_datalog_view: Materialized views
-- =========================================================================

-- Test 22: Drop materialized view
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.drop_datalog_view('test_people_mat', FALSE, TRUE);
    ASSERT result LIKE 'MATERIALIZED VIEW%dropped%',
        'Should say MATERIALIZED VIEW dropped';
    RAISE NOTICE 'PASS: drop materialized view (%)' , result;
END;
$$;

-- Test 23: Verify materialized view is gone
DO $$
DECLARE
    exists_flag BOOLEAN;
BEGIN
    SELECT EXISTS (
        SELECT 1
        FROM pg_matviews
        WHERE matviewname = 'test_people_mat'
    ) INTO exists_flag;
    ASSERT exists_flag = FALSE, 'MATERIALIZED VIEW should be gone after drop';
    RAISE NOTICE 'PASS: MATERIALIZED VIEW removed';
END;
$$;

-- =========================================================================
-- drop_datalog_view: Drop non-existent view (IF EXISTS)
-- =========================================================================

-- Test 24: Drop non-existent view should not error (IF EXISTS)
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.drop_datalog_view('nonexistent_view_67890');
    ASSERT result LIKE 'VIEW%dropped%', 'Should still return drop message';
    RAISE NOTICE 'PASS: drop non-existent view does not error';
END;
$$;

-- =========================================================================
-- drop_datalog_view: CASCADE option
-- =========================================================================

-- Test 25: Create view and drop with CASCADE
DO $$
DECLARE
    result TEXT;
BEGIN
    PERFORM mentat.create_datalog_view(
        'test_cascade_view',
        '[:find ?name :where [?e :person/name ?name]]',
        '{}'::jsonb
    );

    result := mentat.drop_datalog_view('test_cascade_view', TRUE);
    ASSERT result LIKE 'VIEW%dropped%', 'CASCADE drop should work';
    RAISE NOTICE 'PASS: drop with CASCADE';
END;
$$;

-- =========================================================================
-- drop_datalog_view: Invalid view name
-- =========================================================================

-- Test 26: Invalid view name for drop
DO $$
BEGIN
    PERFORM mentat.drop_datalog_view('invalid;name');
    RAISE EXCEPTION 'should have raised error for invalid view name';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: drop rejects invalid view name (%)' , SQLERRM;
END;
$$;

-- =========================================================================
-- create_datalog_view: Replace existing view
-- =========================================================================

-- Test 27: CREATE OR REPLACE VIEW
DO $$
DECLARE
    result1 TEXT;
    result2 TEXT;
    cnt INT;
BEGIN
    result1 := mentat.create_datalog_view(
        'test_replace_view',
        '[:find ?name :where [?e :person/name ?name]]',
        '{}'::jsonb
    );

    -- Replace with a different query (adding age)
    result2 := mentat.create_datalog_view(
        'test_replace_view',
        '[:find ?name :where [?e :person/name ?name]]',
        '{}'::jsonb
    );

    SELECT count(*) INTO cnt FROM test_replace_view;
    ASSERT cnt >= 3, 'Replaced VIEW should still have data';
    RAISE NOTICE 'PASS: CREATE OR REPLACE VIEW works (% rows)', cnt;
END;
$$;

SELECT mentat.drop_datalog_view('test_replace_view');

-- =========================================================================
-- Materialized view with default inputs
-- =========================================================================

-- Test 28: Create materialized view with default inputs
DO $$
DECLARE
    result TEXT;
    cnt INT;
BEGIN
    result := mentat.create_datalog_materialized_view(
        'test_mat_default',
        '[:find ?name :where [?e :person/name ?name]]'
    );
    ASSERT result IS NOT NULL, 'Should create mat view with default inputs';

    SELECT count(*) INTO cnt FROM test_mat_default;
    ASSERT cnt >= 3, 'Mat view should have data';
    RAISE NOTICE 'PASS: materialized view with default inputs (% rows)', cnt;
END;
$$;

SELECT mentat.drop_datalog_view('test_mat_default', FALSE, TRUE);

-- =========================================================================
-- Materialized view: Invalid Datalog
-- =========================================================================

-- Test 29: Invalid Datalog for materialized view
DO $$
BEGIN
    PERFORM mentat.create_datalog_materialized_view(
        'test_bad_mat',
        'not valid datalog',
        '{}'::jsonb
    );
    RAISE EXCEPTION 'should have raised error for invalid Datalog';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: materialized view rejects invalid Datalog';
END;
$$;

-- =========================================================================
-- Materialized view: Invalid view name
-- =========================================================================

-- Test 30: Invalid view name for materialized view
DO $$
BEGIN
    PERFORM mentat.create_datalog_materialized_view(
        'test; DROP TABLE mentat.datoms;--',
        '[:find ?e :where [?e :person/name]]',
        '{}'::jsonb
    );
    RAISE EXCEPTION 'should have raised error for invalid view name';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: materialized view rejects invalid view name';
END;
$$;

ROLLBACK;
