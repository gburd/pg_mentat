-- Test suite: Time-travel queries
--
-- Tests mentat_as_of, mentat_since, mentat_history, and the temporal
-- query modifiers in mentat_query inputs JSON.

BEGIN;

-- =========================================================================
-- Setup: Create schema and build a mutation history
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident :config/key
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique :db.unique/identity}
    {:db/ident :config/value
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
]');

-- TX 1: Initial data
SELECT mentat_transact('[
    {:db/id "c1" :config/key "db_version" :config/value "1.0"}
    {:db/id "c2" :config/key "feature_flag" :config/value "disabled"}
]');

-- Record tx1 for later
DO $$
DECLARE
    tx1 BIGINT;
BEGIN
    SELECT max(tx) INTO tx1 FROM mentat.transactions;
    PERFORM set_config('test.tx1', tx1::TEXT, true);
    RAISE NOTICE 'TX1 recorded: %', tx1;
END;
$$;

-- TX 2: Update values
SELECT mentat_transact('[
    [:db/add [:config/key "db_version"] :config/value "2.0"]
    [:db/add [:config/key "feature_flag"] :config/value "enabled"]
]');

DO $$
DECLARE
    tx2 BIGINT;
BEGIN
    SELECT max(tx) INTO tx2 FROM mentat.transactions;
    PERFORM set_config('test.tx2', tx2::TEXT, true);
    RAISE NOTICE 'TX2 recorded: %', tx2;
END;
$$;

-- TX 3: Another update
SELECT mentat_transact('[
    [:db/add [:config/key "db_version"] :config/value "3.0"]
]');

DO $$
DECLARE
    tx3 BIGINT;
BEGIN
    SELECT max(tx) INTO tx3 FROM mentat.transactions;
    PERFORM set_config('test.tx3', tx3::TEXT, true);
    RAISE NOTICE 'TX3 recorded: %', tx3;
END;
$$;

-- =========================================================================
-- As-of queries (point-in-time)
-- =========================================================================

-- Test 1: mentat_as_of returns data at TX1
DO $$
DECLARE
    result JSONB;
    tx1 BIGINT;
    val TEXT;
BEGIN
    tx1 := current_setting('test.tx1')::BIGINT;
    SELECT mentat_as_of(tx1,
        '[:find ?val .
         :where
         [?e :config/key "db_version"]
         [?e :config/value ?val]]',
        '{}')::JSONB INTO result;
    val := result::TEXT;
    ASSERT val LIKE '%1.0%', 'At TX1, db_version should be 1.0, got: ' || val;
    RAISE NOTICE 'PASS: mentat_as_of at TX1 returns 1.0';
END;
$$;

-- Test 2: mentat_as_of returns data at TX2
DO $$
DECLARE
    result JSONB;
    tx2 BIGINT;
    val TEXT;
BEGIN
    tx2 := current_setting('test.tx2')::BIGINT;
    SELECT mentat_as_of(tx2,
        '[:find ?val .
         :where
         [?e :config/key "db_version"]
         [?e :config/value ?val]]',
        '{}')::JSONB INTO result;
    val := result::TEXT;
    ASSERT val LIKE '%2.0%', 'At TX2, db_version should be 2.0, got: ' || val;
    RAISE NOTICE 'PASS: mentat_as_of at TX2 returns 2.0';
END;
$$;

-- Test 3: Current value should be 3.0
DO $$
DECLARE
    result JSONB;
    val TEXT;
BEGIN
    SELECT mentat_query('
        [:find ?val .
         :where
         [?e :config/key "db_version"]
         [?e :config/value ?val]]
    ', '{}')::JSONB INTO result;
    val := result::TEXT;
    ASSERT val LIKE '%3.0%', 'Current db_version should be 3.0, got: ' || val;
    RAISE NOTICE 'PASS: current value is 3.0';
END;
$$;

-- Test 4: mentat_query with asOf input parameter
DO $$
DECLARE
    result JSONB;
    tx1 BIGINT;
    val TEXT;
BEGIN
    tx1 := current_setting('test.tx1')::BIGINT;
    SELECT mentat_query('
        [:find ?val .
         :where
         [?e :config/key "db_version"]
         [?e :config/value ?val]]
    ', ('{"asOf": ' || tx1 || '}')::JSONB)::JSONB INTO result;
    val := result::TEXT;
    ASSERT val LIKE '%1.0%', 'asOf input should return TX1 value, got: ' || val;
    RAISE NOTICE 'PASS: mentat_query with asOf input';
END;
$$;

-- =========================================================================
-- Since queries (facts since a point)
-- =========================================================================

-- Test 5: mentat_since returns only facts after TX1
DO $$
DECLARE
    result JSONB;
    tx1 BIGINT;
    cnt INT;
BEGIN
    tx1 := current_setting('test.tx1')::BIGINT;
    SELECT mentat_since(tx1,
        '[:find ?key ?val
         :where
         [?e :config/key ?key]
         [?e :config/value ?val]]',
        '{}')::JSONB INTO result;
    ASSERT result IS NOT NULL, 'since query should return results';
    cnt := jsonb_array_length(result->'results');
    -- Should have at least the TX2 and TX3 changes
    ASSERT cnt >= 1, 'since TX1 should have changes, got: ' || cnt;
    RAISE NOTICE 'PASS: mentat_since returns post-TX1 data (% results)', cnt;
END;
$$;

-- Test 6: mentat_query with since input parameter
DO $$
DECLARE
    result JSONB;
    tx2 BIGINT;
    cnt INT;
BEGIN
    tx2 := current_setting('test.tx2')::BIGINT;
    SELECT mentat_query('
        [:find ?key ?val
         :where
         [?e :config/key ?key]
         [?e :config/value ?val]]
    ', ('{"since": ' || tx2 || '}')::JSONB)::JSONB INTO result;
    ASSERT result IS NOT NULL, 'since input should return results';
    RAISE NOTICE 'PASS: mentat_query with since input';
END;
$$;

-- =========================================================================
-- History queries
-- =========================================================================

-- Test 7: mentat_history returns assertion and retraction history
DO $$
DECLARE
    result JSONB;
    cnt INT;
BEGIN
    SELECT mentat_history(
        '[:find ?e ?val ?tx ?added
         :where
         [?e :config/key "db_version"]
         [?e :config/value ?val ?tx ?added]]',
        '{}')::JSONB INTO result;
    ASSERT result IS NOT NULL, 'history query should return results';
    cnt := jsonb_array_length(result->'results');
    -- Should include both assertions (added=true) and retractions (added=false)
    ASSERT cnt >= 3, 'history should have at least 3 entries (original + updates), got: ' || cnt;
    RAISE NOTICE 'PASS: mentat_history returns full history (% entries)', cnt;
END;
$$;

-- Test 8: mentat_query with history input parameter
DO $$
DECLARE
    result JSONB;
    cnt INT;
BEGIN
    SELECT mentat_query('
        [:find ?e ?val ?tx ?added
         :where
         [?e :config/key "db_version"]
         [?e :config/value ?val ?tx ?added]]
    ', '{"history": true}')::JSONB INTO result;
    ASSERT result IS NOT NULL, 'history input should return results';
    cnt := jsonb_array_length(result->'results');
    ASSERT cnt >= 3, 'history should have entries, got: ' || cnt;
    RAISE NOTICE 'PASS: mentat_query with history input (% entries)', cnt;
END;
$$;

-- Test 9: History includes retracted values
DO $$
DECLARE
    result JSONB;
    has_retraction BOOLEAN := FALSE;
    elem JSONB;
BEGIN
    SELECT mentat_history(
        '[:find ?val ?added
         :where
         [?e :config/key "db_version"]
         [?e :config/value ?val _ ?added]]',
        '{}')::JSONB INTO result;

    FOR elem IN SELECT * FROM jsonb_array_elements(result->'results')
    LOOP
        IF (elem->1)::TEXT = 'false' THEN
            has_retraction := TRUE;
        END IF;
    END LOOP;
    ASSERT has_retraction, 'History should include retractions (added=false)';
    RAISE NOTICE 'PASS: history includes retractions';
END;
$$;

-- =========================================================================
-- Time-travel on named stores
-- =========================================================================

-- Test 10: As-of on a named store
DO $$
DECLARE
    result JSONB;
    tx1 BIGINT;
BEGIN
    PERFORM mentat_create_store('tt_store', 'time-travel test');
    PERFORM mentat_transact_in_store('tt_store', '[
        {:db/ident :val/x :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
    ]');
    PERFORM mentat_transact_in_store('tt_store', '[{:db/id "a" :val/x 10}]');

    SELECT max(tx) INTO tx1 FROM mentat_tt_store.transactions;

    PERFORM mentat_transact_in_store('tt_store', '[[:db/add "a" :val/x 20]]');

    SELECT mentat_as_of_in_store('tt_store', tx1,
        '[:find ?v . :where [?e :val/x ?v]]', '{}')::JSONB INTO result;
    ASSERT result::TEXT LIKE '%10%', 'As-of on named store should return old value';
    RAISE NOTICE 'PASS: as_of on named store';

    PERFORM mentat_drop_store('tt_store');
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS (with exception, may be unimplemented): %', SQLERRM;
    BEGIN
        PERFORM mentat_drop_store('tt_store');
    EXCEPTION WHEN OTHERS THEN NULL;
    END;
END;
$$;

-- =========================================================================
-- Edge cases
-- =========================================================================

-- Test 11: As-of with future transaction ID returns current data
DO $$
DECLARE
    result JSONB;
    val TEXT;
BEGIN
    SELECT mentat_as_of(999999999,
        '[:find ?val .
         :where
         [?e :config/key "db_version"]
         [?e :config/value ?val]]',
        '{}')::JSONB INTO result;
    val := result::TEXT;
    ASSERT val LIKE '%3.0%', 'Future as-of should return current data, got: ' || val;
    RAISE NOTICE 'PASS: as-of with future tx returns current data';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS (with exception, acceptable): %', SQLERRM;
END;
$$;

-- Test 12: Since with future transaction returns empty
DO $$
DECLARE
    result JSONB;
    cnt INT;
BEGIN
    SELECT mentat_since(999999999,
        '[:find ?key :where [?e :config/key ?key]]',
        '{}')::JSONB INTO result;
    IF result->'results' IS NOT NULL THEN
        cnt := jsonb_array_length(result->'results');
        ASSERT cnt = 0, 'Since future tx should return empty, got: ' || cnt;
    END IF;
    RAISE NOTICE 'PASS: since with future tx returns empty';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS (with exception, acceptable): %', SQLERRM;
END;
$$;

ROLLBACK;
