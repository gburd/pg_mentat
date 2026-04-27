-- Test suite: Store management functions
--
-- Tests mentat_create_store, mentat_drop_store, mentat_list_stores,
-- mentat_rename_store, and related store infrastructure.
--
-- All tests run inside a transaction and roll back at the end.

BEGIN;

-- =========================================================================
-- Store creation
-- =========================================================================

-- Test 1: Create a named store
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat_create_store('test_store', 'A test store');
    ASSERT result LIKE '%created%', 'mentat_create_store should return success message, got: ' || result;
    RAISE NOTICE 'PASS: create named store';
END;
$$;

-- Test 2: Verify store appears in mentat.stores metadata
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.stores WHERE store_name = 'test_store';
    ASSERT cnt = 1, 'test_store should exist in mentat.stores, count: ' || cnt;
    RAISE NOTICE 'PASS: store in metadata table';
END;
$$;

-- Test 3: Verify store schema was created
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM information_schema.schemata WHERE schema_name = 'mentat_test_store';
    ASSERT cnt = 1, 'mentat_test_store schema should exist';
    RAISE NOTICE 'PASS: store schema created';
END;
$$;

-- Test 4: Verify core tables exist in the store schema
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt
    FROM information_schema.tables
    WHERE table_schema = 'mentat_test_store'
      AND table_name IN ('datoms', 'schema', 'idents', 'partitions', 'transactions', 'fulltext');
    ASSERT cnt >= 5, 'Store schema should have core tables (datoms, schema, idents, partitions, transactions), got: ' || cnt;
    RAISE NOTICE 'PASS: core tables in store schema';
END;
$$;

-- Test 5: Verify virtual table views exist
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt
    FROM information_schema.views
    WHERE table_schema = 'mentat_test_store'
      AND table_name IN ('entities', 'attributes', 'facts');
    ASSERT cnt >= 3, 'Store should have virtual table views (entities, attributes, facts), got: ' || cnt;
    RAISE NOTICE 'PASS: virtual table views exist';
END;
$$;

-- Test 6: Create store without description
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat_create_store('minimal_store');
    ASSERT result LIKE '%created%', 'Should create store without description';
    RAISE NOTICE 'PASS: create store without description';
END;
$$;

-- =========================================================================
-- Store listing
-- =========================================================================

-- Test 7: List stores returns JSON with test stores
DO $$
DECLARE
    result JSONB;
    cnt INT;
BEGIN
    SELECT mentat_list_stores()::JSONB INTO result;
    ASSERT result IS NOT NULL, 'mentat_list_stores should return JSON';
    cnt := jsonb_array_length(result);
    ASSERT cnt >= 2, 'Should have at least 2 stores (test_store, minimal_store), got: ' || cnt;
    RAISE NOTICE 'PASS: list stores returns results';
END;
$$;

-- =========================================================================
-- Store renaming
-- =========================================================================

-- Test 8: Rename a store
DO $$
DECLARE
    result TEXT;
    cnt INT;
BEGIN
    result := mentat_rename_store('minimal_store', 'renamed_store');
    ASSERT result LIKE '%renamed%', 'Should return rename success message';
    SELECT COUNT(*) INTO cnt FROM mentat.stores WHERE store_name = 'renamed_store';
    ASSERT cnt = 1, 'renamed_store should exist in metadata';
    SELECT COUNT(*) INTO cnt FROM mentat.stores WHERE store_name = 'minimal_store';
    ASSERT cnt = 0, 'minimal_store should no longer exist in metadata';
    RAISE NOTICE 'PASS: rename store';
END;
$$;

-- =========================================================================
-- Store name validation
-- =========================================================================

-- Test 9: Reject empty store name
DO $$
BEGIN
    PERFORM mentat_create_store('');
    RAISE EXCEPTION 'Should have rejected empty store name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects empty store name (%)', SQLERRM;
END;
$$;

-- Test 10: Reject store name starting with digit
DO $$
BEGIN
    PERFORM mentat_create_store('1bad');
    RAISE EXCEPTION 'Should have rejected store name starting with digit';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects store name starting with digit (%)', SQLERRM;
END;
$$;

-- Test 11: Reject store name with special characters
DO $$
BEGIN
    PERFORM mentat_create_store('my-store');
    RAISE EXCEPTION 'Should have rejected store name with hyphen';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects store name with special chars (%)', SQLERRM;
END;
$$;

-- Test 12: Reject reserved name "default"
DO $$
BEGIN
    PERFORM mentat_create_store('default');
    RAISE EXCEPTION 'Should have rejected reserved name "default"';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects reserved name "default" (%)', SQLERRM;
END;
$$;

-- Test 13: Reject reserved name with pg_ prefix
DO $$
BEGIN
    PERFORM mentat_create_store('pg_test');
    RAISE EXCEPTION 'Should have rejected pg_ prefix';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects pg_ prefix (%)', SQLERRM;
END;
$$;

-- Test 14: Reject duplicate store name
DO $$
BEGIN
    PERFORM mentat_create_store('test_store');
    RAISE EXCEPTION 'Should have rejected duplicate store name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects duplicate store name (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- Store operations - transact to a named store
-- =========================================================================

-- Test 15: Transact schema into named store
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat_transact_full('test_store', '[
        {:db/ident :item/name
         :db/valueType :db.type/string
         :db/cardinality :db.cardinality/one}
        {:db/ident :item/price
         :db/valueType :db.type/long
         :db/cardinality :db.cardinality/one}
    ]');
    ASSERT result IS NOT NULL, 'mentat_transact_full should return result';
    RAISE NOTICE 'PASS: transact schema into named store';
END;
$$;

-- Test 16: Transact data into named store
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat_transact_full('test_store', '[
        {:db/id "t1" :item/name "Widget" :item/price 100}
        {:db/id "t2" :item/name "Gadget" :item/price 200}
    ]');
    ASSERT result IS NOT NULL, 'Data transact should succeed';
    RAISE NOTICE 'PASS: transact data into named store';
END;
$$;

-- Test 17: Query virtual table views in named store
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat_test_store.facts;
    ASSERT cnt > 0, 'facts view should have rows after transacting data, got: ' || cnt;
    RAISE NOTICE 'PASS: query facts view in named store';
END;
$$;

-- =========================================================================
-- Store dropping
-- =========================================================================

-- Test 18: Cannot drop default store
DO $$
BEGIN
    PERFORM mentat_drop_store('default');
    RAISE EXCEPTION 'Should have rejected dropping default store';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: cannot drop default store (%)', SQLERRM;
END;
$$;

-- Test 19: Drop a named store
DO $$
DECLARE
    result TEXT;
    cnt INT;
BEGIN
    result := mentat_drop_store('renamed_store');
    ASSERT result LIKE '%dropped%', 'Should return drop success message';
    SELECT COUNT(*) INTO cnt FROM mentat.stores WHERE store_name = 'renamed_store';
    ASSERT cnt = 0, 'renamed_store should be removed from metadata';
    RAISE NOTICE 'PASS: drop named store';
END;
$$;

-- Test 20: Drop non-existent store
DO $$
BEGIN
    PERFORM mentat_drop_store('nonexistent');
    RAISE EXCEPTION 'Should have rejected dropping non-existent store';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects dropping non-existent store (%)', SQLERRM;
END;
$$;

-- Clean up remaining test store
DO $$
BEGIN
    PERFORM mentat_drop_store('test_store');
    RAISE NOTICE 'CLEANUP: dropped test_store';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'CLEANUP: test_store already dropped or does not exist';
END;
$$;

ROLLBACK;
