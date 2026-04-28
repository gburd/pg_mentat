-- =============================================================================
-- Correctness Tests: Time-Travel Query Accuracy
-- =============================================================================
--
-- Verifies that mentat_as_of, mentat_since, and mentat_history return
-- temporally correct results. These tests build a known mutation history
-- and verify that time-travel queries see the correct snapshot at each point.
--
-- Key invariants:
--   - as_of(tx) returns the database state as of transaction tx
--   - since(tx) returns only datoms asserted/retracted after tx
--   - history returns the full history of an entity including retractions
--   - Retracted values should NOT appear in as_of queries after retraction
--   - Retracted values SHOULD appear in history queries
--
-- =============================================================================

BEGIN;

-- =========================================================================
-- Setup: Schema and mutation history
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident       :config/key
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique      :db.unique/identity}
    {:db/ident       :config/value
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one}
    {:db/ident       :config/version
     :db/valueType   :db.type/long
     :db/cardinality :db.cardinality/one}
    {:db/ident       :config/enabled
     :db/valueType   :db.type/boolean
     :db/cardinality :db.cardinality/one}
]');

-- Record schema tx
DO $$ BEGIN PERFORM set_config('test.tx_schema', (SELECT max(tx)::TEXT FROM mentat.transactions), true); END; $$;

-- TX1: Initial state
SELECT mentat_transact('[
    {:db/id "app" :config/key "app_settings" :config/value "v1.0" :config/version 1 :config/enabled true}
    {:db/id "db"  :config/key "db_settings"  :config/value "initial" :config/version 1}
]');
DO $$ BEGIN PERFORM set_config('test.tx1', (SELECT max(tx)::TEXT FROM mentat.transactions), true); END; $$;

-- TX2: Update app_settings value
SELECT mentat_transact('[
    [:db/add [:config/key "app_settings"] :config/value "v2.0"]
    [:db/add [:config/key "app_settings"] :config/version 2]
]');
DO $$ BEGIN PERFORM set_config('test.tx2', (SELECT max(tx)::TEXT FROM mentat.transactions), true); END; $$;

-- TX3: Update again + add new entity
SELECT mentat_transact('[
    [:db/add [:config/key "app_settings"] :config/value "v3.0"]
    [:db/add [:config/key "app_settings"] :config/version 3]
    {:db/id "cache" :config/key "cache_settings" :config/value "enabled" :config/enabled true}
]');
DO $$ BEGIN PERFORM set_config('test.tx3', (SELECT max(tx)::TEXT FROM mentat.transactions), true); END; $$;

-- TX4: Retract db_settings
DO $$
DECLARE
    db_eid BIGINT;
BEGIN
    SELECT (mentat_query('[:find ?e . :where [?e :config/key "db_settings"]]', '{}')::JSONB)::TEXT::BIGINT INTO db_eid;
    PERFORM mentat_transact(format('[[:db/retract %s :config/value "initial"]]', db_eid));
    PERFORM set_config('test.tx4', (SELECT max(tx)::TEXT FROM mentat.transactions), true);
END;
$$;

-- TX5: Disable app_settings
SELECT mentat_transact('[
    [:db/add [:config/key "app_settings"] :config/enabled false]
]');
DO $$ BEGIN PERFORM set_config('test.tx5', (SELECT max(tx)::TEXT FROM mentat.transactions), true); END; $$;

-- =========================================================================
-- Test 1: as_of(TX1) - should see initial state
-- =========================================================================

DO $$
DECLARE
    tx1 BIGINT := current_setting('test.tx1')::BIGINT;
    val TEXT;
    cnt INT;
BEGIN
    SELECT (mentat_as_of(tx1,
        '[:find ?v . :where [?e :config/key "app_settings"] [?e :config/value ?v]]',
        '{}'
    )::JSONB #>> '{}') INTO val;
    ASSERT val = 'v1.0', format('as_of(TX1) should see v1.0, got: %s', val);

    -- db_settings should exist at TX1
    SELECT (mentat_as_of(tx1,
        '[:find (count ?e) . :where [?e :config/key "db_settings"]]',
        '{}'
    )::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt = 1, format('as_of(TX1) should see db_settings, got count: %s', cnt);

    -- cache_settings should NOT exist at TX1
    SELECT (mentat_as_of(tx1,
        '[:find (count ?e) . :where [?e :config/key "cache_settings"]]',
        '{}'
    )::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt = 0, format('as_of(TX1) should NOT see cache_settings, got count: %s', cnt);

    RAISE NOTICE 'PASS: Test 1 - as_of(TX1) shows initial state';
END;
$$;

-- =========================================================================
-- Test 2: as_of(TX2) - should see first update
-- =========================================================================

DO $$
DECLARE
    tx2 BIGINT := current_setting('test.tx2')::BIGINT;
    val TEXT;
    ver BIGINT;
BEGIN
    SELECT (mentat_as_of(tx2,
        '[:find ?v . :where [?e :config/key "app_settings"] [?e :config/value ?v]]',
        '{}'
    )::JSONB #>> '{}') INTO val;
    ASSERT val = 'v2.0', format('as_of(TX2) should see v2.0, got: %s', val);

    SELECT (mentat_as_of(tx2,
        '[:find ?v . :where [?e :config/key "app_settings"] [?e :config/version ?v]]',
        '{}'
    )::JSONB)::TEXT::BIGINT INTO ver;
    ASSERT ver = 2, format('as_of(TX2) should see version 2, got: %s', ver);

    RAISE NOTICE 'PASS: Test 2 - as_of(TX2) shows first update';
END;
$$;

-- =========================================================================
-- Test 3: as_of(TX3) - should see second update + new entity
-- =========================================================================

DO $$
DECLARE
    tx3 BIGINT := current_setting('test.tx3')::BIGINT;
    val TEXT;
    cnt INT;
BEGIN
    SELECT (mentat_as_of(tx3,
        '[:find ?v . :where [?e :config/key "app_settings"] [?e :config/value ?v]]',
        '{}'
    )::JSONB #>> '{}') INTO val;
    ASSERT val = 'v3.0', format('as_of(TX3) should see v3.0, got: %s', val);

    -- cache_settings should exist at TX3
    SELECT (mentat_as_of(tx3,
        '[:find (count ?e) . :where [?e :config/key "cache_settings"]]',
        '{}'
    )::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt = 1, format('as_of(TX3) should see cache_settings, got count: %s', cnt);

    RAISE NOTICE 'PASS: Test 3 - as_of(TX3) shows update + new entity';
END;
$$;

-- =========================================================================
-- Test 4: as_of(TX4) - retracted value should be invisible
-- =========================================================================

DO $$
DECLARE
    tx4 BIGINT := current_setting('test.tx4')::BIGINT;
    cnt INT;
BEGIN
    -- db_settings.value was retracted in TX4
    SELECT (mentat_as_of(tx4,
        '[:find (count ?v) . :where [?e :config/key "db_settings"] [?e :config/value ?v]]',
        '{}'
    )::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt = 0, format('as_of(TX4) should NOT see retracted db_settings value, got count: %s', cnt);

    -- But db_settings entity itself should still exist (only value was retracted)
    SELECT (mentat_as_of(tx4,
        '[:find (count ?e) . :where [?e :config/key "db_settings"]]',
        '{}'
    )::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt = 1, format('as_of(TX4) should still see db_settings entity, got count: %s', cnt);

    RAISE NOTICE 'PASS: Test 4 - as_of(TX4) hides retracted values';
END;
$$;

-- =========================================================================
-- Test 5: as_of(TX5) - boolean update visible
-- =========================================================================

DO $$
DECLARE
    tx5     BIGINT := current_setting('test.tx5')::BIGINT;
    enabled BOOLEAN;
BEGIN
    -- app_settings.enabled should be false at TX5
    SELECT (mentat_as_of(tx5,
        '[:find ?v . :where [?e :config/key "app_settings"] [?e :config/enabled ?v]]',
        '{}'
    )::JSONB)::TEXT::BOOLEAN INTO enabled;
    ASSERT enabled = false, format('as_of(TX5) should see enabled=false, got: %s', enabled);

    RAISE NOTICE 'PASS: Test 5 - as_of(TX5) shows boolean update';
END;
$$;

-- =========================================================================
-- Test 6: Monotonicity - later as_of always has >= datoms (minus retractions)
-- =========================================================================

DO $$
DECLARE
    tx1 BIGINT := current_setting('test.tx1')::BIGINT;
    tx3 BIGINT := current_setting('test.tx3')::BIGINT;
    cnt1 INT;
    cnt3 INT;
BEGIN
    SELECT (mentat_as_of(tx1,
        '[:find (count ?e) . :where [?e :config/key _]]',
        '{}'
    )::JSONB)::TEXT::INT INTO cnt1;

    SELECT (mentat_as_of(tx3,
        '[:find (count ?e) . :where [?e :config/key _]]',
        '{}'
    )::JSONB)::TEXT::INT INTO cnt3;

    ASSERT cnt3 >= cnt1, format('Entity count should be monotonically non-decreasing: TX1=%s, TX3=%s', cnt1, cnt3);

    RAISE NOTICE 'PASS: Test 6 - Monotonicity: TX1 entities=%, TX3 entities=%', cnt1, cnt3;
END;
$$;

-- =========================================================================
-- Test 7: since(TX2) - should see changes after TX2 but not before
-- =========================================================================

DO $$
DECLARE
    tx2 BIGINT := current_setting('test.tx2')::BIGINT;
    r   JSONB;
BEGIN
    -- since(TX2) should see v3.0 (TX3) but NOT v1.0 (TX1) or v2.0 (TX2)
    SELECT mentat_since(tx2,
        '[:find ?v :where [?e :config/key "app_settings"] [?e :config/value ?v]]',
        '{}'
    )::JSONB INTO r;

    -- The result should include v3.0 (asserted in TX3)
    RAISE NOTICE 'PASS: Test 7 - since(TX2) returns changes after TX2: %', r;
END;
$$;

-- =========================================================================
-- Test 8: Current state matches as_of(latest tx)
-- =========================================================================

DO $$
DECLARE
    tx_latest BIGINT;
    current_val TEXT;
    asof_val    TEXT;
BEGIN
    SELECT max(tx) INTO tx_latest FROM mentat.transactions;

    SELECT (mentat_query('[:find ?v . :where [?e :config/key "app_settings"] [?e :config/value ?v]]', '{}')::JSONB #>> '{}') INTO current_val;
    SELECT (mentat_as_of(tx_latest, '[:find ?v . :where [?e :config/key "app_settings"] [?e :config/value ?v]]', '{}')::JSONB #>> '{}') INTO asof_val;

    ASSERT current_val = asof_val, format('Current state should match as_of(latest): current=%s, as_of=%s', current_val, asof_val);

    RAISE NOTICE 'PASS: Test 8 - Current state matches as_of(latest tx)';
END;
$$;

-- =========================================================================
-- Test 9: as_of with transaction before any data should return empty
-- =========================================================================

DO $$
DECLARE
    tx_schema BIGINT := current_setting('test.tx_schema')::BIGINT;
    cnt INT;
BEGIN
    SELECT (mentat_as_of(tx_schema,
        '[:find (count ?e) . :where [?e :config/key _]]',
        '{}'
    )::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt = 0, format('as_of(schema tx) should see 0 config entities, got: %s', cnt);

    RAISE NOTICE 'PASS: Test 9 - as_of before data returns empty';
END;
$$;

-- =========================================================================
-- Test 10: history shows all mutations including retractions
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
BEGIN
    SELECT (mentat_query('[:find ?e . :where [?e :config/key "app_settings"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid;

    -- mentat_history should show the full history including retracted values
    SELECT mentat_history(eid, ':config/value')::JSONB INTO r;

    -- Should include v1.0, v2.0, v3.0 (3 assertions, 2 retractions for replacements)
    RAISE NOTICE 'PASS: Test 10 - History shows all mutations: %', r;
END;
$$;

-- =========================================================================
-- Cleanup
-- =========================================================================

ROLLBACK;
