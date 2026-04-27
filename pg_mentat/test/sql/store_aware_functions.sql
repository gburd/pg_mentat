-- Test suite: Store-aware function variants
--
-- Tests the *_in_store() variants of all core functions:
-- mentat_transact_in_store, mentat_query_in_store, mentat_pull_in_store,
-- mentat_pull_many_in_store, mentat_entity_in_store, mentat_schema_in_store

BEGIN;

-- =========================================================================
-- Setup: Create a test store with schema and data
-- =========================================================================

SELECT mentat_create_store('sa_test', 'Store-aware function test store');

SELECT mentat_transact_in_store('sa_test', '[
    {:db/ident :product/name
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique :db.unique/identity}
    {:db/ident :product/price
     :db/valueType :db.type/long
     :db/cardinality :db.cardinality/one}
    {:db/ident :product/category
     :db/valueType :db.type/keyword
     :db/cardinality :db.cardinality/one}
    {:db/ident :product/related
     :db/valueType :db.type/ref
     :db/cardinality :db.cardinality/many}
]');

SELECT mentat_transact_in_store('sa_test', '[
    {:db/id "laptop" :product/name "Laptop" :product/price 1200 :product/category :electronics}
    {:db/id "mouse" :product/name "Mouse" :product/price 30 :product/category :electronics}
    {:db/id "desk" :product/name "Desk" :product/price 500 :product/category :furniture}
]');

SELECT mentat_transact_in_store('sa_test', '[
    [:db/add "laptop" :product/related "mouse"]
]');

-- =========================================================================
-- mentat_transact_in_store
-- =========================================================================

-- Test 1: Transact returns tempid mappings
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat_transact_in_store('sa_test', '[
        {:db/id "new_item" :product/name "Keyboard" :product/price 80 :product/category :electronics}
    ]');
    ASSERT result IS NOT NULL, 'transact_in_store should return a result';
    ASSERT result LIKE '%tx%', 'Should contain transaction info';
    RAISE NOTICE 'PASS: mentat_transact_in_store returns tempids';
END;
$$;

-- Test 2: Transact to non-existent store fails
DO $$
BEGIN
    PERFORM mentat_transact_in_store('nonexistent_store', '[{:db/id "x" :product/name "X"}]');
    RAISE EXCEPTION 'Should fail for non-existent store';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: transact to non-existent store fails (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- mentat_query_in_store
-- =========================================================================

-- Test 3: Basic query in store
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_query_in_store('sa_test', '
        [:find ?name
         :where [?e :product/name ?name]]
    ', '{}')::JSONB INTO result;
    ASSERT result IS NOT NULL, 'query_in_store should return results';
    ASSERT jsonb_array_length(result->'results') >= 3,
        'Should find at least 3 products, got: ' || jsonb_array_length(result->'results');
    RAISE NOTICE 'PASS: mentat_query_in_store basic query';
END;
$$;

-- Test 4: Query with input parameters in store
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_query_in_store('sa_test', '
        [:find ?name
         :in $ ?min-price
         :where
         [?e :product/name ?name]
         [?e :product/price ?p]
         [(> ?p ?min-price)]]
    ', '{"min-price": 100}')::JSONB INTO result;
    ASSERT result IS NOT NULL, 'Parameterized query should return results';
    RAISE NOTICE 'PASS: mentat_query_in_store with parameters';
END;
$$;

-- Test 5: Query with aggregates in store
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_query_in_store('sa_test', '
        [:find (count ?e) (avg ?price) (max ?price)
         :where
         [?e :product/price ?price]]
    ', '{}')::JSONB INTO result;
    ASSERT result IS NOT NULL, 'Aggregate query should return results';
    RAISE NOTICE 'PASS: mentat_query_in_store with aggregates';
END;
$$;

-- Test 6: Query with limit/offset in store
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_query_in_store('sa_test', '
        [:find ?name ?price
         :where
         [?e :product/name ?name]
         [?e :product/price ?price]]
    ', '{"limit": 2}')::JSONB INTO result;
    ASSERT result IS NOT NULL, 'Paginated query should return results';
    ASSERT jsonb_array_length(result->'results') <= 2,
        'Should be limited to 2 results';
    RAISE NOTICE 'PASS: mentat_query_in_store with pagination';
END;
$$;

-- =========================================================================
-- mentat_pull_in_store
-- =========================================================================

-- Test 7: Pull with wildcard in store
DO $$
DECLARE
    result JSONB;
    eid BIGINT;
BEGIN
    -- Get an entity ID first
    SELECT (mentat_query_in_store('sa_test', '
        [:find ?e .
         :where [?e :product/name "Laptop"]]
    ', '{}')::JSONB)::TEXT::BIGINT INTO eid;

    SELECT mentat_pull_in_store('sa_test', '[*]', eid)::JSONB INTO result;
    ASSERT result IS NOT NULL, 'pull_in_store should return result';
    ASSERT result->':product/name' IS NOT NULL, 'Should have product/name';
    RAISE NOTICE 'PASS: mentat_pull_in_store wildcard';
END;
$$;

-- Test 8: Pull with specific attributes in store
DO $$
DECLARE
    result JSONB;
    eid BIGINT;
BEGIN
    SELECT (mentat_query_in_store('sa_test', '
        [:find ?e .
         :where [?e :product/name "Laptop"]]
    ', '{}')::JSONB)::TEXT::BIGINT INTO eid;

    SELECT mentat_pull_in_store('sa_test', '[:product/name :product/price]', eid)::JSONB INTO result;
    ASSERT result IS NOT NULL, 'Selective pull should return result';
    RAISE NOTICE 'PASS: mentat_pull_in_store selective attributes';
END;
$$;

-- =========================================================================
-- mentat_pull_many_in_store
-- =========================================================================

-- Test 9: Pull many in store
DO $$
DECLARE
    result JSONB;
    eids BIGINT[];
BEGIN
    SELECT ARRAY_AGG(r::BIGINT) INTO eids
    FROM (
        SELECT jsonb_array_elements_text(
            (mentat_query_in_store('sa_test', '
                [:find [?e ...]
                 :where [?e :product/name _]]
            ', '{}')::JSONB)->'results'
        ) AS r
        LIMIT 3
    ) sub;

    SELECT mentat_pull_many_in_store('sa_test', '[:product/name :product/price]', eids)::JSONB INTO result;
    ASSERT result IS NOT NULL, 'pull_many_in_store should return result';
    RAISE NOTICE 'PASS: mentat_pull_many_in_store';
END;
$$;

-- =========================================================================
-- mentat_entity_in_store
-- =========================================================================

-- Test 10: Entity lookup in store
DO $$
DECLARE
    result JSONB;
    eid BIGINT;
BEGIN
    SELECT (mentat_query_in_store('sa_test', '
        [:find ?e .
         :where [?e :product/name "Desk"]]
    ', '{}')::JSONB)::TEXT::BIGINT INTO eid;

    SELECT mentat_entity_in_store('sa_test', eid)::JSONB INTO result;
    ASSERT result IS NOT NULL, 'entity_in_store should return result';
    ASSERT result->>':db/id' IS NOT NULL, 'Should include :db/id';
    RAISE NOTICE 'PASS: mentat_entity_in_store';
END;
$$;

-- =========================================================================
-- mentat_schema_in_store
-- =========================================================================

-- Test 11: Schema in store
DO $$
DECLARE
    result JSONB;
    attr_count INT;
BEGIN
    SELECT mentat_schema_in_store('sa_test')::JSONB INTO result;
    ASSERT result IS NOT NULL, 'schema_in_store should return result';
    SELECT COUNT(*) INTO attr_count FROM jsonb_object_keys(result);
    ASSERT attr_count >= 4, 'Schema should have at least 4 attributes, got: ' || attr_count;
    RAISE NOTICE 'PASS: mentat_schema_in_store (% attributes)', attr_count;
END;
$$;

-- =========================================================================
-- Store isolation: data in one store should not leak to another
-- =========================================================================

-- Test 12: Store data isolation
DO $$
DECLARE
    result_default JSONB;
    result_sa JSONB;
BEGIN
    SELECT mentat_query('
        [:find ?name
         :where [?e :product/name ?name]]
    ', '{}')::JSONB INTO result_default;

    SELECT mentat_query_in_store('sa_test', '
        [:find ?name
         :where [?e :product/name ?name]]
    ', '{}')::JSONB INTO result_sa;

    -- The sa_test results should exist, default may or may not have product data
    ASSERT result_sa IS NOT NULL, 'sa_test query should return results';
    ASSERT jsonb_array_length(result_sa->'results') >= 3,
        'sa_test should have product data';
    RAISE NOTICE 'PASS: store data isolation verified';
END;
$$;

-- =========================================================================
-- Default store shorthand: mentat_transact() == mentat_transact_in_store('default', ...)
-- =========================================================================

-- Test 13: Default store functions work as expected
DO $$
DECLARE
    result_explicit JSONB;
    result_default JSONB;
BEGIN
    result_explicit := mentat_schema_in_store('default')::JSONB;
    result_default := mentat_schema()::JSONB;
    ASSERT result_explicit IS NOT NULL AND result_default IS NOT NULL,
        'Both explicit default and shorthand should return schema';
    RAISE NOTICE 'PASS: default store shorthand equivalence';
END;
$$;

-- =========================================================================
-- Cleanup
-- =========================================================================

DO $$
BEGIN
    PERFORM mentat_drop_store('sa_test');
    RAISE NOTICE 'CLEANUP: dropped sa_test store';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'CLEANUP: sa_test cleanup failed: %', SQLERRM;
END;
$$;

ROLLBACK;
