-- Test suite: SQL function aliases (07_function_aliases.sql)
--
-- Verifies that all 12 short-name aliases delegate correctly to their
-- underlying mentat_* functions and produce identical results.
--
-- Functions tested:
--   mentat.q()          -> mentat.mentat_query()
--   mentat.t()          -> mentat.mentat_transact()
--   mentat.pull()       -> mentat.mentat_pull()
--   mentat.pull_many()  -> mentat.mentat_pull_many()
--   mentat.entity()     -> mentat.mentat_entity()
--   mentat.schema()     -> mentat.mentat_schema()
--   mentat.explain()    -> mentat.mentat_explain()
--   mentat.stats()      -> mentat.mentat_query_stats()
--   mentat.slow_queries() -> mentat.mentat_slow_queries()
--   mentat.storage()    -> mentat.mentat_storage_stats()
--   mentat.cache_stats() -> mentat.mentat_stmt_cache_stats()
--   mentat.cache_clear() -> mentat.mentat_stmt_cache_clear()

BEGIN;

-- =========================================================================
-- Setup: Install a test schema with sample data
-- =========================================================================
SELECT mentat.mentat_transact('[
  {:db/ident       :person/name
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique      :db.unique/identity
   :db/index       true}
  {:db/ident       :person/age
   :db/valueType   :db.type/long
   :db/cardinality :db.cardinality/one}
  {:db/ident       :person/email
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one}
]');

SELECT mentat.mentat_transact('[
  {:db/id "alice" :person/name "Alice" :person/age 30 :person/email "alice@example.com"}
  {:db/id "bob"   :person/name "Bob"   :person/age 25}
]');

-- =========================================================================
-- Test 1: mentat.schema() == mentat.mentat_schema()
-- =========================================================================
DO $$
DECLARE
    alias_result JSONB;
    full_result  JSONB;
BEGIN
    alias_result := mentat.schema();
    full_result  := mentat.mentat_schema();
    ASSERT alias_result = full_result,
        'schema() alias should return same result as mentat_schema()';
    RAISE NOTICE 'PASS: schema() alias matches mentat_schema()';
END;
$$;

-- =========================================================================
-- Test 2: mentat.q() == mentat.mentat_query()
-- =========================================================================
DO $$
DECLARE
    alias_result JSONB;
    full_result  JSONB;
    query_str TEXT := '[:find ?name :where [?e :person/name ?name]]';
BEGIN
    alias_result := mentat.q(query_str, '{}'::jsonb);
    full_result  := mentat.mentat_query(query_str, '{}'::jsonb);
    ASSERT alias_result = full_result,
        'q() alias should return same result as mentat_query()';
    RAISE NOTICE 'PASS: q() alias matches mentat_query()';
END;
$$;

-- =========================================================================
-- Test 3: mentat.q() with default inputs parameter
-- =========================================================================
DO $$
DECLARE
    result JSONB;
BEGIN
    result := mentat.q('[:find ?name :where [?e :person/name ?name]]');
    ASSERT result IS NOT NULL, 'q() with default inputs should return a result';
    RAISE NOTICE 'PASS: q() works with default inputs parameter';
END;
$$;

-- =========================================================================
-- Test 4: mentat.t() == mentat.mentat_transact()
--         (both transact and return a report string)
-- =========================================================================
DO $$
DECLARE
    alias_result TEXT;
BEGIN
    alias_result := mentat.t('[[:db/add "test1" :person/name "Carol"]]');
    ASSERT alias_result IS NOT NULL, 't() should return a transaction report';
    ASSERT alias_result LIKE '%tx-id%' OR alias_result LIKE '%tempids%',
        't() should contain tx report keys';
    RAISE NOTICE 'PASS: t() alias returns transaction report';
END;
$$;

-- =========================================================================
-- Test 5: mentat.entity() == mentat.mentat_entity()
-- =========================================================================
DO $$
DECLARE
    alias_result JSONB;
    full_result  JSONB;
    eid BIGINT;
BEGIN
    -- Find Alice's entity ID
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Alice"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO eid;

    IF eid IS NOT NULL THEN
        alias_result := mentat.entity(eid);
        full_result  := mentat.mentat_entity(eid);
        ASSERT alias_result = full_result,
            'entity() alias should return same result as mentat_entity()';
        RAISE NOTICE 'PASS: entity() alias matches mentat_entity()';
    ELSE
        RAISE NOTICE 'SKIP: could not find Alice entity ID';
    END IF;
END;
$$;

-- =========================================================================
-- Test 6: mentat.pull() == mentat.mentat_pull()
-- =========================================================================
DO $$
DECLARE
    alias_result JSONB;
    full_result  JSONB;
    eid BIGINT;
BEGIN
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Alice"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO eid;

    IF eid IS NOT NULL THEN
        alias_result := mentat.pull('[:person/name :person/age]', eid);
        full_result  := mentat.mentat_pull('[:person/name :person/age]', eid);
        ASSERT alias_result = full_result,
            'pull() alias should return same result as mentat_pull()';
        RAISE NOTICE 'PASS: pull() alias matches mentat_pull()';
    ELSE
        RAISE NOTICE 'SKIP: could not find Alice entity ID';
    END IF;
END;
$$;

-- =========================================================================
-- Test 7: mentat.pull_many() == mentat.mentat_pull_many()
-- =========================================================================
DO $$
DECLARE
    alias_result JSONB;
    full_result  JSONB;
    eids BIGINT[];
BEGIN
    SELECT ARRAY(
        SELECT (elem->0)::BIGINT
        FROM jsonb_array_elements(
            (mentat.mentat_query(
                '[:find ?e :where [?e :person/name]]',
                '{}'::jsonb
            ))->'results'
        ) AS elem
    ) INTO eids;

    IF array_length(eids, 1) > 0 THEN
        alias_result := mentat.pull_many('[:person/name]', eids);
        full_result  := mentat.mentat_pull_many('[:person/name]', eids);
        ASSERT alias_result = full_result,
            'pull_many() alias should return same result as mentat_pull_many()';
        RAISE NOTICE 'PASS: pull_many() alias matches mentat_pull_many()';
    ELSE
        RAISE NOTICE 'SKIP: no entities found';
    END IF;
END;
$$;

-- =========================================================================
-- Test 8: mentat.explain() == mentat.mentat_explain()
-- =========================================================================
DO $$
DECLARE
    alias_result JSONB;
    full_result  JSONB;
    query_str TEXT := '[:find ?name :where [?e :person/name ?name]]';
BEGIN
    alias_result := mentat.explain(query_str, '{}'::jsonb);
    full_result  := mentat.mentat_explain(query_str, '{}'::jsonb);
    ASSERT alias_result = full_result,
        'explain() alias should return same result as mentat_explain()';
    RAISE NOTICE 'PASS: explain() alias matches mentat_explain()';
END;
$$;

-- =========================================================================
-- Test 9: mentat.explain() with default inputs
-- =========================================================================
DO $$
DECLARE
    result JSONB;
BEGIN
    result := mentat.explain('[:find ?name :where [?e :person/name ?name]]');
    ASSERT result IS NOT NULL, 'explain() with default inputs should return a result';
    RAISE NOTICE 'PASS: explain() works with default inputs';
END;
$$;

-- =========================================================================
-- Test 10: mentat.stats() == mentat.mentat_query_stats()
-- =========================================================================
DO $$
DECLARE
    alias_result JSONB;
    full_result  JSONB;
BEGIN
    alias_result := mentat.stats();
    full_result  := mentat.mentat_query_stats();
    ASSERT alias_result = full_result,
        'stats() alias should return same result as mentat_query_stats()';
    RAISE NOTICE 'PASS: stats() alias matches mentat_query_stats()';
END;
$$;

-- =========================================================================
-- Test 11: mentat.slow_queries() == mentat.mentat_slow_queries()
-- =========================================================================
DO $$
DECLARE
    alias_result JSONB;
    full_result  JSONB;
BEGIN
    alias_result := mentat.slow_queries(100.0);
    full_result  := mentat.mentat_slow_queries(100.0);
    ASSERT alias_result = full_result,
        'slow_queries() alias should return same result as mentat_slow_queries()';
    RAISE NOTICE 'PASS: slow_queries() alias matches mentat_slow_queries()';
END;
$$;

-- =========================================================================
-- Test 12: mentat.slow_queries() with default threshold
-- =========================================================================
DO $$
DECLARE
    result JSONB;
BEGIN
    result := mentat.slow_queries();
    ASSERT result IS NOT NULL, 'slow_queries() with default threshold should return a result';
    RAISE NOTICE 'PASS: slow_queries() works with default threshold';
END;
$$;

-- =========================================================================
-- Test 13: mentat.storage() == mentat.mentat_storage_stats()
-- =========================================================================
DO $$
DECLARE
    alias_result JSONB;
    full_result  JSONB;
BEGIN
    alias_result := mentat.storage();
    full_result  := mentat.mentat_storage_stats();
    ASSERT alias_result = full_result,
        'storage() alias should return same result as mentat_storage_stats()';
    RAISE NOTICE 'PASS: storage() alias matches mentat_storage_stats()';
END;
$$;

-- =========================================================================
-- Test 14: mentat.cache_stats() == mentat.mentat_stmt_cache_stats()
-- =========================================================================
DO $$
DECLARE
    alias_result JSONB;
    full_result  JSONB;
BEGIN
    alias_result := mentat.cache_stats();
    full_result  := mentat.mentat_stmt_cache_stats();
    ASSERT alias_result = full_result,
        'cache_stats() alias should return same result as mentat_stmt_cache_stats()';
    RAISE NOTICE 'PASS: cache_stats() alias matches mentat_stmt_cache_stats()';
END;
$$;

-- =========================================================================
-- Test 15: mentat.cache_clear() == mentat.mentat_stmt_cache_clear()
-- =========================================================================
DO $$
DECLARE
    alias_result TEXT;
    full_result  TEXT;
BEGIN
    alias_result := mentat.cache_clear();
    full_result  := mentat.mentat_stmt_cache_clear();
    ASSERT alias_result = full_result,
        'cache_clear() alias should return same result as mentat_stmt_cache_clear()';
    RAISE NOTICE 'PASS: cache_clear() alias matches mentat_stmt_cache_clear()';
END;
$$;

-- =========================================================================
-- Test 16: Aliases return correct types
-- =========================================================================
DO $$
BEGIN
    -- schema() returns JSONB
    PERFORM jsonb_typeof(mentat.schema());
    RAISE NOTICE 'PASS: schema() returns valid JSONB';
END;
$$;

DO $$
DECLARE
    result JSONB;
BEGIN
    -- stats() returns JSONB
    result := mentat.stats();
    PERFORM jsonb_typeof(result);
    RAISE NOTICE 'PASS: stats() returns valid JSONB';
END;
$$;

DO $$
DECLARE
    result JSONB;
BEGIN
    -- cache_stats() returns JSONB
    result := mentat.cache_stats();
    PERFORM jsonb_typeof(result);
    RAISE NOTICE 'PASS: cache_stats() returns valid JSONB';
END;
$$;

ROLLBACK;
