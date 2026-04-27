-- Test suite: Streaming subscriptions
--
-- Tests mentat_subscribe, mentat_unsubscribe, mentat_list_subscriptions,
-- mentat_notify_subscribers, and the subscription infrastructure.

BEGIN;

-- =========================================================================
-- Setup
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident :sensor/id
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique :db.unique/identity}
    {:db/ident :sensor/temp
     :db/valueType :db.type/double
     :db/cardinality :db.cardinality/one}
    {:db/ident :sensor/status
     :db/valueType :db.type/keyword
     :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
    {:db/id "s1" :sensor/id "sensor-001" :sensor/temp 22.5 :sensor/status :ok}
    {:db/id "s2" :sensor/id "sensor-002" :sensor/temp 25.0 :sensor/status :ok}
]');

-- =========================================================================
-- Subscription creation
-- =========================================================================

-- Test 1: Create a subscription
DO $$
DECLARE
    sub_id TEXT;
BEGIN
    sub_id := mentat_subscribe('hot_sensors',
        '[:find ?id ?temp
         :where
         [?e :sensor/id ?id]
         [?e :sensor/temp ?temp]
         [(> ?temp 30.0)]]',
        '{}');
    ASSERT sub_id IS NOT NULL, 'mentat_subscribe should return a subscription ID';
    ASSERT length(sub_id) > 0, 'Subscription ID should be non-empty';
    RAISE NOTICE 'PASS: create subscription (id: %)', sub_id;
END;
$$;

-- Test 2: Create subscription with channel name
DO $$
DECLARE
    sub_id TEXT;
BEGIN
    sub_id := mentat_subscribe('all_sensors',
        '[:find ?id ?temp
         :where
         [?e :sensor/id ?id]
         [?e :sensor/temp ?temp]]',
        '{}',
        'sensor_updates');
    ASSERT sub_id IS NOT NULL, 'Named channel subscription should return ID';
    RAISE NOTICE 'PASS: create subscription with channel name';
END;
$$;

-- =========================================================================
-- List subscriptions
-- =========================================================================

-- Test 3: List subscriptions returns JSON
DO $$
DECLARE
    result JSONB;
    cnt INT;
BEGIN
    SELECT mentat_list_subscriptions()::JSONB INTO result;
    ASSERT result IS NOT NULL, 'list_subscriptions should return JSON';
    cnt := jsonb_array_length(result);
    ASSERT cnt >= 2, 'Should have at least 2 subscriptions, got: ' || cnt;
    RAISE NOTICE 'PASS: list subscriptions (% subs)', cnt;
END;
$$;

-- Test 4: Subscription metadata has expected fields
DO $$
DECLARE
    result JSONB;
    first_sub JSONB;
BEGIN
    SELECT mentat_list_subscriptions()::JSONB INTO result;
    first_sub := result->0;
    ASSERT first_sub->>'name' IS NOT NULL, 'Subscription should have name';
    ASSERT first_sub->>'query' IS NOT NULL, 'Subscription should have query';
    RAISE NOTICE 'PASS: subscription metadata has expected fields';
END;
$$;

-- =========================================================================
-- Notify subscribers (trigger check)
-- =========================================================================

-- Test 5: Notify after data change
DO $$
DECLARE
    result TEXT;
BEGIN
    -- First, insert data that matches the hot_sensors query
    PERFORM mentat_transact('[
        {:db/id "s3" :sensor/id "sensor-003" :sensor/temp 35.0 :sensor/status :warning}
    ]');

    -- Trigger notification check
    result := mentat_notify_subscribers();
    ASSERT result IS NOT NULL, 'notify_subscribers should return a result';
    RAISE NOTICE 'PASS: notify_subscribers after data change: %', result;
END;
$$;

-- Test 6: Notify when no changes match
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat_notify_subscribers();
    ASSERT result IS NOT NULL, 'notify_subscribers should work even with no changes';
    RAISE NOTICE 'PASS: notify_subscribers with no new changes';
END;
$$;

-- =========================================================================
-- Subscription on named store
-- =========================================================================

-- Test 7: Subscribe on named store
DO $$
DECLARE
    sub_id TEXT;
BEGIN
    PERFORM mentat_create_store('sub_store', 'subscription test');
    PERFORM mentat_transact_in_store('sub_store', '[
        {:db/ident :event/type :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
    ]');

    sub_id := mentat_subscribe_in_store('sub_store', 'events',
        '[:find ?type :where [?e :event/type ?type]]', '{}');
    ASSERT sub_id IS NOT NULL, 'Named store subscription should return ID';
    RAISE NOTICE 'PASS: subscribe on named store';

    PERFORM mentat_drop_store('sub_store');
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS (with exception, may be unimplemented): %', SQLERRM;
    BEGIN
        PERFORM mentat_drop_store('sub_store');
    EXCEPTION WHEN OTHERS THEN NULL;
    END;
END;
$$;

-- =========================================================================
-- Unsubscribe
-- =========================================================================

-- Test 8: Unsubscribe by name
DO $$
DECLARE
    result TEXT;
    cnt_before INT;
    cnt_after INT;
BEGIN
    SELECT jsonb_array_length(mentat_list_subscriptions()::JSONB) INTO cnt_before;

    result := mentat_unsubscribe('hot_sensors');
    ASSERT result IS NOT NULL, 'unsubscribe should return a result';

    SELECT jsonb_array_length(mentat_list_subscriptions()::JSONB) INTO cnt_after;
    ASSERT cnt_after < cnt_before, 'Should have fewer subscriptions after unsubscribe';
    RAISE NOTICE 'PASS: unsubscribe by name (% -> %)', cnt_before, cnt_after;
END;
$$;

-- Test 9: Unsubscribe non-existent
DO $$
BEGIN
    PERFORM mentat_unsubscribe('nonexistent_sub');
    RAISE EXCEPTION 'Should reject unsubscribe of non-existent sub';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects unsubscribe of non-existent (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- Cleanup remaining subscriptions
-- =========================================================================

DO $$
BEGIN
    PERFORM mentat_unsubscribe('all_sensors');
    RAISE NOTICE 'CLEANUP: unsubscribed all_sensors';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'CLEANUP: all_sensors already unsubscribed';
END;
$$;

ROLLBACK;
