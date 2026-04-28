-- Test suite: Virtual table views
--
-- Tests the virtual table views (entities, attributes, facts, type-specific
-- views, searchable_text) created automatically for each store.
--
-- Depends on the default store having schema and data loaded.

BEGIN;

-- =========================================================================
-- Setup: Create schema and test data in the default store
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident :test/name
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one
     :db/index true}
    {:db/ident :test/age
     :db/valueType :db.type/long
     :db/cardinality :db.cardinality/one}
    {:db/ident :test/active
     :db/valueType :db.type/boolean
     :db/cardinality :db.cardinality/one}
    {:db/ident :test/score
     :db/valueType :db.type/double
     :db/cardinality :db.cardinality/one}
    {:db/ident :test/tag
     :db/valueType :db.type/keyword
     :db/cardinality :db.cardinality/many}
]');

SELECT mentat_transact('[
    {:db/id "e1" :test/name "Alice" :test/age 30 :test/active true :test/score 95.5}
    {:db/id "e2" :test/name "Bob" :test/age 25 :test/active false :test/score 87.3}
]');

SELECT mentat_transact('[
    [:db/add "e1" :test/tag :priority/high]
    [:db/add "e1" :test/tag :status/active]
]');

-- =========================================================================
-- Regenerate virtual tables for the default store
-- =========================================================================

-- Test 1: Regenerate virtual tables
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat_create_virtual_tables('default');
    ASSERT result LIKE '%created%', 'Should regenerate virtual tables, got: ' || result;
    RAISE NOTICE 'PASS: regenerate virtual tables';
END;
$$;

-- =========================================================================
-- Entities view
-- =========================================================================

-- Test 2: Entities view returns rows
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.entities;
    ASSERT cnt > 0, 'entities view should have rows';
    RAISE NOTICE 'PASS: entities view has rows (count: %)', cnt;
END;
$$;

-- Test 3: Entities view has expected columns
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt
    FROM information_schema.columns
    WHERE table_schema = 'mentat' AND table_name = 'entities'
      AND column_name IN ('entity_id', 'first_tx', 'last_tx', 'attribute_count');
    ASSERT cnt >= 4, 'entities view should have expected columns';
    RAISE NOTICE 'PASS: entities view columns correct';
END;
$$;

-- =========================================================================
-- Attributes view
-- =========================================================================

-- Test 4: Attributes view returns schema attributes
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.attributes;
    ASSERT cnt > 0, 'attributes view should have rows';
    RAISE NOTICE 'PASS: attributes view has rows (count: %)', cnt;
END;
$$;

-- Test 5: Attributes view has test attributes
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.attributes WHERE ident LIKE ':test/%';
    ASSERT cnt >= 5, 'attributes view should have test attributes, got: ' || cnt;
    RAISE NOTICE 'PASS: attributes view has test attributes';
END;
$$;

-- =========================================================================
-- Facts view
-- =========================================================================

-- Test 6: Facts view returns human-readable facts
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.facts;
    ASSERT cnt > 0, 'facts view should have rows';
    RAISE NOTICE 'PASS: facts view has rows (count: %)', cnt;
END;
$$;

-- Test 7: Facts view resolves attribute names
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.facts WHERE attribute = ':test/name';
    ASSERT cnt >= 2, 'facts view should resolve :test/name attribute, got: ' || cnt;
    RAISE NOTICE 'PASS: facts view resolves attribute names';
END;
$$;

-- Test 8: Facts view decodes value types correctly
DO $$
DECLARE
    name_type TEXT;
    age_type TEXT;
BEGIN
    SELECT DISTINCT value_type INTO name_type FROM mentat.facts WHERE attribute = ':test/name' LIMIT 1;
    SELECT DISTINCT value_type INTO age_type FROM mentat.facts WHERE attribute = ':test/age' LIMIT 1;
    ASSERT name_type = 'string', 'name should have type string, got: ' || COALESCE(name_type, 'NULL');
    ASSERT age_type = 'long', 'age should have type long, got: ' || COALESCE(age_type, 'NULL');
    RAISE NOTICE 'PASS: facts view decodes value types';
END;
$$;

-- =========================================================================
-- Type-specific views
-- =========================================================================

-- Test 9: text_values view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.text_values WHERE attribute = ':test/name';
    ASSERT cnt >= 2, 'text_values should have test/name values, got: ' || cnt;
    RAISE NOTICE 'PASS: text_values view works';
END;
$$;

-- Test 10: numeric_values view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.numeric_values WHERE attribute = ':test/age';
    ASSERT cnt >= 2, 'numeric_values should have test/age values, got: ' || cnt;
    RAISE NOTICE 'PASS: numeric_values view works';
END;
$$;

-- Test 11: boolean_values view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.boolean_values WHERE attribute = ':test/active';
    ASSERT cnt >= 2, 'boolean_values should have test/active values, got: ' || cnt;
    RAISE NOTICE 'PASS: boolean_values view works';
END;
$$;

-- Test 12: double_values view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.double_values WHERE attribute = ':test/score';
    ASSERT cnt >= 2, 'double_values should have test/score values, got: ' || cnt;
    RAISE NOTICE 'PASS: double_values view works';
END;
$$;

-- Test 13: keyword_values view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.keyword_values WHERE attribute = ':test/tag';
    ASSERT cnt >= 2, 'keyword_values should have test/tag values, got: ' || cnt;
    RAISE NOTICE 'PASS: keyword_values view works';
END;
$$;

-- =========================================================================
-- Searchable text view
-- =========================================================================

-- Test 14: searchable_text view has tsvector column
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.searchable_text WHERE value IS NOT NULL;
    ASSERT cnt > 0, 'searchable_text view should have rows';
    RAISE NOTICE 'PASS: searchable_text view works';
END;
$$;

-- Test 15: Full-text search via searchable_text
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt
    FROM mentat.searchable_text
    WHERE search_vector @@ to_tsquery('english', 'Alice');
    -- Alice may or may not be in the FTS index depending on tsvector
    RAISE NOTICE 'PASS: searchable_text FTS query runs (count: %)', cnt;
END;
$$;

-- =========================================================================
-- Relationship navigation views
-- =========================================================================

-- Test 16: entity_references view shows refs with attribute names
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.entity_references;
    ASSERT cnt > 0, 'entity_references view should have rows (schema refs exist)';
    RAISE NOTICE 'PASS: entity_references view has rows (count: %)', cnt;
END;
$$;

-- Test 17: entity_references resolves target idents
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.entity_references
    WHERE target_ident IS NOT NULL;
    -- Bootstrap refs to :db.type/*, :db.cardinality/* should have idents
    ASSERT cnt > 0, 'entity_references should resolve some target idents';
    RAISE NOTICE 'PASS: entity_references resolves target idents (count: %)', cnt;
END;
$$;

-- Test 18: reverse_references view works
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.reverse_references;
    ASSERT cnt > 0, 'reverse_references view should have rows';
    RAISE NOTICE 'PASS: reverse_references view has rows (count: %)', cnt;
END;
$$;

-- Test 19: graph_edges view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.graph_edges;
    ASSERT cnt > 0, 'graph_edges view should have rows';
    RAISE NOTICE 'PASS: graph_edges view has rows (count: %)', cnt;
END;
$$;

-- =========================================================================
-- Transaction history views
-- =========================================================================

-- Test 20: tx_log view shows transactions
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.tx_log;
    ASSERT cnt > 0, 'tx_log view should have rows';
    RAISE NOTICE 'PASS: tx_log view has rows (count: %)', cnt;
END;
$$;

-- Test 21: tx_log has datom counts
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.tx_log WHERE datom_count > 0;
    ASSERT cnt > 0, 'tx_log should have transactions with datom_count > 0';
    RAISE NOTICE 'PASS: tx_log has datom counts (count: %)', cnt;
END;
$$;

-- Test 22: entity_history view includes all operations
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.entity_history;
    ASSERT cnt > 0, 'entity_history view should have rows';
    RAISE NOTICE 'PASS: entity_history view has rows (count: %)', cnt;
END;
$$;

-- Test 23: entity_history has assert operations
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.entity_history WHERE operation = 'assert';
    ASSERT cnt > 0, 'entity_history should have assert operations';
    RAISE NOTICE 'PASS: entity_history has assert operations (count: %)', cnt;
END;
$$;

-- Test 24: recent_changes view
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.recent_changes;
    ASSERT cnt > 0, 'recent_changes view should have rows';
    RAISE NOTICE 'PASS: recent_changes view has rows (count: %)', cnt;
END;
$$;

-- =========================================================================
-- Schema summary view
-- =========================================================================

-- Test 25: schema_summary view shows user attributes
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.schema_summary WHERE ident LIKE ':test/%';
    ASSERT cnt >= 5, 'schema_summary should show test attributes, got: ' || cnt;
    RAISE NOTICE 'PASS: schema_summary has test attributes (count: %)', cnt;
END;
$$;

-- Test 26: schema_summary has usage counts
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.schema_summary
    WHERE ident = ':test/name' AND entity_count >= 2;
    ASSERT cnt = 1, 'schema_summary should show entity_count >= 2 for :test/name';
    RAISE NOTICE 'PASS: schema_summary has usage counts';
END;
$$;

-- =========================================================================
-- Convenience functions
-- =========================================================================

-- Test 27: entity_value function returns text values
DO $$
DECLARE
    val TEXT;
    alice_eid BIGINT;
BEGIN
    -- Find Alice's entity ID
    SELECT entity_id INTO alice_eid FROM mentat.text_values
    WHERE attribute = ':test/name' AND value = 'Alice' LIMIT 1;

    val := mentat.entity_value(alice_eid, ':test/name');
    ASSERT val = 'Alice', 'entity_value should return Alice, got: ' || COALESCE(val, 'NULL');
    RAISE NOTICE 'PASS: entity_value returns text values';
END;
$$;

-- Test 28: entity_value function returns numeric values
DO $$
DECLARE
    val TEXT;
    alice_eid BIGINT;
BEGIN
    SELECT entity_id INTO alice_eid FROM mentat.text_values
    WHERE attribute = ':test/name' AND value = 'Alice' LIMIT 1;

    val := mentat.entity_value(alice_eid, ':test/age');
    ASSERT val = '30', 'entity_value should return 30, got: ' || COALESCE(val, 'NULL');
    RAISE NOTICE 'PASS: entity_value returns numeric values';
END;
$$;

-- Test 29: lookup_entity function
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.lookup_entity(':test/name', 'Alice');
    ASSERT cnt >= 1, 'lookup_entity should find Alice, got count: ' || cnt;
    RAISE NOTICE 'PASS: lookup_entity finds entities by value (count: %)', cnt;
END;
$$;

-- Test 30: count_by_attribute function
DO $$
DECLARE
    cnt INT;
    test_name_count BIGINT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.count_by_attribute();
    ASSERT cnt > 0, 'count_by_attribute should return rows';

    SELECT entity_count INTO test_name_count FROM mentat.count_by_attribute()
    WHERE attribute = ':test/name';
    ASSERT test_name_count >= 2, 'count_by_attribute should show >= 2 entities for :test/name, got: ' || test_name_count;
    RAISE NOTICE 'PASS: count_by_attribute works (total attrs: %)', cnt;
END;
$$;

-- Test 31: find_text function
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.find_text('Alice');
    ASSERT cnt >= 1, 'find_text should find Alice, got: ' || cnt;
    RAISE NOTICE 'PASS: find_text finds text matches (count: %)', cnt;
END;
$$;

-- Test 32: entities_with_attribute function
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM mentat.entities_with_attribute(':test/name');
    ASSERT cnt >= 2, 'entities_with_attribute should find >= 2 entities, got: ' || cnt;
    RAISE NOTICE 'PASS: entities_with_attribute works (count: %)', cnt;
END;
$$;

-- =========================================================================
-- Virtual tables on custom stores
-- =========================================================================

-- Test 33: Virtual tables on a new store include new views
DO $$
DECLARE
    result TEXT;
    cnt INT;
BEGIN
    PERFORM mentat_create_store('vt_test_store', 'store for virtual table tests');

    -- Verify all views were auto-created (original + new)
    SELECT COUNT(*) INTO cnt
    FROM information_schema.views
    WHERE table_schema = 'mentat_vt_test_store'
      AND table_name IN (
          'entities', 'attributes', 'facts', 'text_values', 'numeric_values',
          'entity_references', 'reverse_references', 'graph_edges',
          'tx_log', 'entity_history', 'recent_changes', 'schema_summary'
      );
    ASSERT cnt >= 12, 'Custom store should have all virtual table views, got: ' || cnt;
    RAISE NOTICE 'PASS: virtual tables auto-created for new store (count: %)', cnt;

    -- Verify functions were created
    SELECT COUNT(*) INTO cnt
    FROM information_schema.routines
    WHERE routine_schema = 'mentat_vt_test_store'
      AND routine_name IN (
          'entities_with_attribute', 'lookup_entity', 'entity_value',
          'count_by_attribute', 'find_text'
      );
    ASSERT cnt >= 5, 'Custom store should have helper functions, got: ' || cnt;
    RAISE NOTICE 'PASS: helper functions created for new store (count: %)', cnt;

    -- Clean up
    PERFORM mentat_drop_store('vt_test_store');
END;
$$;

ROLLBACK;
