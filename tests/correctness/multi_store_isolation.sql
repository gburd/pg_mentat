-- =============================================================================
-- Correctness Tests: Multi-Store Isolation
-- =============================================================================
--
-- Verifies that multiple stores are fully isolated from each other:
--   - Data in store A is invisible to queries on store B
--   - Schema changes in one store don't affect another
--   - Entity IDs are store-scoped (no cross-store collisions)
--   - Virtual tables are store-specific
--   - Dropping a store does not affect other stores
--
-- =============================================================================

BEGIN;

-- =========================================================================
-- Setup: Create two isolated stores
-- =========================================================================

SELECT mentat_create_store('alpha', 'Alpha store');
SELECT mentat_create_store('beta', 'Beta store');

-- Define different schemas in each store

SELECT mentat_transact_in_store('alpha', '[
    {:db/ident       :product/name
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique      :db.unique/identity}
    {:db/ident       :product/price
     :db/valueType   :db.type/double
     :db/cardinality :db.cardinality/one}
    {:db/ident       :product/category
     :db/valueType   :db.type/keyword
     :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact_in_store('beta', '[
    {:db/ident       :employee/name
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique      :db.unique/identity}
    {:db/ident       :employee/dept
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one}
    {:db/ident       :employee/salary
     :db/valueType   :db.type/long
     :db/cardinality :db.cardinality/one}
]');

-- Insert data into each store

SELECT mentat_transact_in_store('alpha', '[
    {:db/id "p1" :product/name "Widget"  :product/price 9.99  :product/category :cat/hardware}
    {:db/id "p2" :product/name "Gadget"  :product/price 19.99 :product/category :cat/electronics}
    {:db/id "p3" :product/name "Doohickey" :product/price 4.99 :product/category :cat/hardware}
]');

SELECT mentat_transact_in_store('beta', '[
    {:db/id "e1" :employee/name "Alice" :employee/dept "Engineering" :employee/salary 120000}
    {:db/id "e2" :employee/name "Bob"   :employee/dept "Marketing"   :employee/salary 95000}
]');

-- =========================================================================
-- Test 1: Data isolation - store A data is not visible in store B
-- =========================================================================

DO $$
DECLARE
    cnt INT;
BEGIN
    -- Query alpha for products - should find 3
    SELECT (mentat_query_in_store('alpha',
        '[:find (count ?e) . :where [?e :product/name _]]', '{}'
    )::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt = 3, format('Alpha should have 3 products, got: %s', cnt);

    -- Query beta for products - should find 0 (attribute doesn't exist in beta)
    BEGIN
        SELECT (mentat_query_in_store('beta',
            '[:find (count ?e) . :where [?e :product/name _]]', '{}'
        )::JSONB)::TEXT::INT INTO cnt;
        -- If the attribute doesn't exist, this should either return 0 or error
        ASSERT cnt = 0, format('Beta should have 0 products, got: %s', cnt);
    EXCEPTION
        WHEN OTHERS THEN
            -- Expected: attribute doesn't exist in beta's schema
            NULL;
    END;

    -- Query beta for employees - should find 2
    SELECT (mentat_query_in_store('beta',
        '[:find (count ?e) . :where [?e :employee/name _]]', '{}'
    )::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt = 2, format('Beta should have 2 employees, got: %s', cnt);

    RAISE NOTICE 'PASS: Test 1 - Data isolation between stores';
END;
$$;

-- =========================================================================
-- Test 2: Schema isolation - attributes are store-specific
-- =========================================================================

DO $$
DECLARE
    alpha_attrs INT;
    beta_attrs  INT;
BEGIN
    -- Count user-defined attributes in each store
    SELECT (mentat_query_in_store('alpha',
        '[:find (count ?e) . :where [?e :db/ident _] [?e :db/valueType _]]', '{}'
    )::JSONB)::TEXT::INT INTO alpha_attrs;

    SELECT (mentat_query_in_store('beta',
        '[:find (count ?e) . :where [?e :db/ident _] [?e :db/valueType _]]', '{}'
    )::JSONB)::TEXT::INT INTO beta_attrs;

    -- Both should have their respective schema attributes
    ASSERT alpha_attrs >= 3, format('Alpha should have >= 3 schema attrs, got: %s', alpha_attrs);
    ASSERT beta_attrs >= 3, format('Beta should have >= 3 schema attrs, got: %s', beta_attrs);

    RAISE NOTICE 'PASS: Test 2 - Schema isolation (alpha=% attrs, beta=% attrs)', alpha_attrs, beta_attrs;
END;
$$;

-- =========================================================================
-- Test 3: Virtual tables are store-specific
-- =========================================================================

DO $$
DECLARE
    alpha_facts INT;
    beta_facts  INT;
BEGIN
    -- Create virtual tables for both stores
    PERFORM mentat_create_virtual_tables('alpha');
    PERFORM mentat_create_virtual_tables('beta');

    -- Alpha's facts view should only show alpha data
    SELECT COUNT(*) INTO alpha_facts FROM mentat_alpha.facts;
    ASSERT alpha_facts > 0, 'Alpha facts view should have data';

    -- Beta's facts view should only show beta data
    SELECT COUNT(*) INTO beta_facts FROM mentat_beta.facts;
    ASSERT beta_facts > 0, 'Beta facts view should have data';

    -- They should have different counts (different data)
    ASSERT alpha_facts != beta_facts, format('Stores should have different fact counts: alpha=%s, beta=%s', alpha_facts, beta_facts);

    RAISE NOTICE 'PASS: Test 3 - Virtual tables scoped to store (alpha=% facts, beta=% facts)', alpha_facts, beta_facts;
END;
$$;

-- =========================================================================
-- Test 4: Transactions in one store don't affect another
-- =========================================================================

DO $$
DECLARE
    beta_cnt_before INT;
    beta_cnt_after  INT;
BEGIN
    -- Count beta employees before
    SELECT (mentat_query_in_store('beta',
        '[:find (count ?e) . :where [?e :employee/name _]]', '{}'
    )::JSONB)::TEXT::INT INTO beta_cnt_before;

    -- Transact into alpha
    PERFORM mentat_transact_in_store('alpha', '[
        {:db/id "p4" :product/name "Thingamajig" :product/price 29.99 :product/category :cat/misc}
    ]');

    -- Count beta employees after - should be unchanged
    SELECT (mentat_query_in_store('beta',
        '[:find (count ?e) . :where [?e :employee/name _]]', '{}'
    )::JSONB)::TEXT::INT INTO beta_cnt_after;

    ASSERT beta_cnt_before = beta_cnt_after, format('Beta should be unaffected: before=%s, after=%s', beta_cnt_before, beta_cnt_after);

    RAISE NOTICE 'PASS: Test 4 - Alpha transaction does not affect beta';
END;
$$;

-- =========================================================================
-- Test 5: Store-level type-specific table isolation via store_id
-- =========================================================================

DO $$
DECLARE
    alpha_sid INT;
    beta_sid  INT;
    alpha_text_cnt INT;
    beta_text_cnt  INT;
    cross_cnt      INT;
BEGIN
    SELECT store_id INTO alpha_sid FROM mentat.stores WHERE store_name = 'alpha';
    SELECT store_id INTO beta_sid  FROM mentat.stores WHERE store_name = 'beta';

    -- Count text datoms for each store in the shared table
    SELECT COUNT(*) INTO alpha_text_cnt FROM mentat.datoms_text_new WHERE store_id = alpha_sid AND added = true;
    SELECT COUNT(*) INTO beta_text_cnt  FROM mentat.datoms_text_new WHERE store_id = beta_sid  AND added = true;

    -- Verify no rows have mismatched store_id (sanity check)
    SELECT COUNT(*) INTO cross_cnt
    FROM mentat.datoms_text_new
    WHERE store_id NOT IN (alpha_sid, beta_sid)
      AND store_id != (SELECT store_id FROM mentat.stores WHERE store_name = 'default');

    ASSERT cross_cnt = 0, format('No datoms should have unknown store_id, found: %s', cross_cnt);

    RAISE NOTICE 'PASS: Test 5 - Type-specific tables use store_id isolation (alpha=%, beta=% text datoms)', alpha_text_cnt, beta_text_cnt;
END;
$$;

-- =========================================================================
-- Test 6: Upsert is store-scoped
-- =========================================================================

DO $$
DECLARE
    alpha_cnt INT;
    beta_cnt  INT;
BEGIN
    -- Create an employee attribute in alpha too (same ident, different store)
    PERFORM mentat_transact_in_store('alpha', '[
        {:db/ident       :employee/name
         :db/valueType   :db.type/string
         :db/cardinality :db.cardinality/one
         :db/unique      :db.unique/identity}
    ]');

    -- Add an employee named "Alice" to alpha
    PERFORM mentat_transact_in_store('alpha', '[
        {:employee/name "Alice" :product/price 0.0}
    ]');

    -- Beta's "Alice" should be unaffected
    SELECT (mentat_query_in_store('beta',
        '[:find (count ?e) . :where [?e :employee/name "Alice"]]', '{}'
    )::JSONB)::TEXT::INT INTO beta_cnt;
    ASSERT beta_cnt = 1, format('Beta should still have exactly 1 Alice, got: %s', beta_cnt);

    -- Alpha should also have an Alice (separate entity)
    SELECT (mentat_query_in_store('alpha',
        '[:find (count ?e) . :where [?e :employee/name "Alice"]]', '{}'
    )::JSONB)::TEXT::INT INTO alpha_cnt;
    ASSERT alpha_cnt = 1, format('Alpha should have 1 Alice, got: %s', alpha_cnt);

    RAISE NOTICE 'PASS: Test 6 - Upsert is store-scoped (same identity, different stores)';
END;
$$;

-- =========================================================================
-- Test 7: Dropping one store does not affect another
-- =========================================================================

DO $$
DECLARE
    beta_cnt INT;
BEGIN
    -- Drop alpha
    PERFORM mentat_drop_store('alpha');

    -- Verify beta is still intact
    SELECT (mentat_query_in_store('beta',
        '[:find (count ?e) . :where [?e :employee/name _]]', '{}'
    )::JSONB)::TEXT::INT INTO beta_cnt;
    ASSERT beta_cnt = 2, format('Beta should be unaffected after alpha drop, got: %s', beta_cnt);

    RAISE NOTICE 'PASS: Test 7 - Dropping alpha does not affect beta';
END;
$$;

-- =========================================================================
-- Cleanup
-- =========================================================================

ROLLBACK;
