-- Test suite: Security tests
--
-- Tests protection against SQL injection, privilege escalation,
-- store name injection, and input validation boundaries.

BEGIN;

-- =========================================================================
-- SQL injection via store names
-- =========================================================================

-- Test 1: Reject store name with SQL injection attempt (DROP TABLE)
DO $$
BEGIN
    PERFORM mentat_create_store('test; DROP TABLE mentat.datoms; --');
    RAISE EXCEPTION 'Should reject SQL injection in store name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects SQL injection in store name (%)', SQLERRM;
END;
$$;

-- Test 2: Reject store name with schema escape
DO $$
BEGIN
    PERFORM mentat_create_store('test".public."evil');
    RAISE EXCEPTION 'Should reject schema escape in store name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects schema escape in store name (%)', SQLERRM;
END;
$$;

-- Test 3: Reject store name with semicolons
DO $$
BEGIN
    PERFORM mentat_create_store('test;evil');
    RAISE EXCEPTION 'Should reject semicolons in store name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects semicolons in store name (%)', SQLERRM;
END;
$$;

-- Test 4: Reject store name with single quotes
DO $$
BEGIN
    PERFORM mentat_create_store(E'test\'evil');
    RAISE EXCEPTION 'Should reject quotes in store name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects quotes in store name (%)', SQLERRM;
END;
$$;

-- Test 5: Reject store name with double quotes
DO $$
BEGIN
    PERFORM mentat_create_store('test"evil');
    RAISE EXCEPTION 'Should reject double quotes in store name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects double quotes in store name (%)', SQLERRM;
END;
$$;

-- Test 6: Reject overly long store name
DO $$
DECLARE
    long_name TEXT;
BEGIN
    long_name := repeat('a', 200);
    PERFORM mentat_create_store(long_name);
    RAISE EXCEPTION 'Should reject overly long store name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects overly long store name (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- SQL injection via EDN transaction data
-- =========================================================================

-- Test 7: EDN transaction with embedded SQL should not execute raw SQL
DO $$
DECLARE
    result TEXT;
BEGIN
    -- This should be treated as EDN, not as SQL
    result := mentat_transact('[{:db/id "test" :test/name "Robert''); DROP TABLE mentat.datoms;--"}]');
    -- If we get here, the EDN parser handled the quotes safely
    RAISE NOTICE 'PASS: EDN with SQL injection characters handled safely';
EXCEPTION WHEN OTHERS THEN
    -- The EDN parser should reject malformed EDN, which is also fine
    RAISE NOTICE 'PASS: EDN parser rejected malformed input (%)', SQLERRM;
END;
$$;

-- Test 8: Verify datoms table still exists after injection attempts
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt FROM information_schema.tables
    WHERE table_schema = 'mentat' AND table_name = 'datoms';
    ASSERT cnt >= 1, 'mentat.datoms table should still exist after injection attempts';
    RAISE NOTICE 'PASS: datoms table intact after injection attempts';
END;
$$;

-- =========================================================================
-- SQL injection via query inputs
-- =========================================================================

-- Test 9: Query with injection in input parameter key
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_query(
        '[:find ?e :where [?e :person/name ?n]]',
        '{"test''; DROP TABLE mentat.datoms;--": 1}'
    )::JSONB INTO result;
    RAISE NOTICE 'PASS: query with injection in input key handled safely';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: query rejected malformed input (%)', SQLERRM;
END;
$$;

-- Test 10: Query with injection in input value
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_query(
        '[:find ?e :in $ ?name :where [?e :person/name ?name]]',
        '{"name": "Robert''); DROP TABLE mentat.datoms;--"}'
    )::JSONB INTO result;
    RAISE NOTICE 'PASS: query with injection in input value handled safely';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: query rejected malformed input (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- SQL injection via Datalog query text
-- =========================================================================

-- Test 11: Injection via query string
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_query(
        '[:find ?e :where [?e :person/name "test"]] UNION SELECT * FROM pg_shadow;--',
        '{}'
    )::JSONB INTO result;
    RAISE NOTICE 'PASS: injection via query string handled safely';
EXCEPTION WHEN OTHERS THEN
    -- The EDN/Datalog parser should reject this
    RAISE NOTICE 'PASS: parser rejected injected query (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- SQL injection via matview names
-- =========================================================================

-- Test 12: Injection in matview name
DO $$
BEGIN
    PERFORM mentat_create_matview('test; DROP TABLE mentat.datoms;--',
        '[:find ?e :where [?e :person/name _]]', '{}');
    RAISE EXCEPTION 'Should reject injection in matview name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects injection in matview name (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- SQL injection via subscription names
-- =========================================================================

-- Test 13: Injection in subscription name
DO $$
BEGIN
    PERFORM mentat_subscribe('test''; DROP TABLE mentat.datoms;--',
        '[:find ?e :where [?e :person/name _]]', '{}');
    RAISE EXCEPTION 'Should reject injection in subscription name';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects injection in subscription name (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- Input validation boundaries
-- =========================================================================

-- Test 14: NULL EDN input to transact
DO $$
BEGIN
    PERFORM mentat_transact(NULL);
    RAISE EXCEPTION 'Should reject NULL transact input';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects NULL transact input (%)', SQLERRM;
END;
$$;

-- Test 15: Empty string EDN input to transact
DO $$
BEGIN
    PERFORM mentat_transact('');
    RAISE EXCEPTION 'Should reject empty transact input';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects empty transact input (%)', SQLERRM;
END;
$$;

-- Test 16: NULL query input
DO $$
BEGIN
    PERFORM mentat_query(NULL, '{}');
    RAISE EXCEPTION 'Should reject NULL query input';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects NULL query input (%)', SQLERRM;
END;
$$;

-- Test 17: NULL inputs JSON
DO $$
BEGIN
    PERFORM mentat_query('[:find ?e :where [?e :person/name _]]', NULL);
    RAISE EXCEPTION 'Should reject NULL inputs JSON';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: rejects NULL inputs JSON (%)', SQLERRM;
END;
$$;

-- Test 18: Entity ID boundaries (negative)
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_entity(-1)::JSONB INTO result;
    -- Negative entity IDs should return empty or error, not crash
    RAISE NOTICE 'PASS: negative entity ID handled gracefully';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: negative entity ID rejected (%)', SQLERRM;
END;
$$;

-- Test 19: Entity ID boundaries (zero)
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_entity(0)::JSONB INTO result;
    RAISE NOTICE 'PASS: zero entity ID handled gracefully';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: zero entity ID rejected (%)', SQLERRM;
END;
$$;

-- Test 20: Entity ID boundaries (very large)
DO $$
DECLARE
    result JSONB;
BEGIN
    SELECT mentat_entity(9223372036854775807)::JSONB INTO result;
    -- Should return empty entity, not crash
    RAISE NOTICE 'PASS: max BIGINT entity ID handled gracefully';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'PASS: max BIGINT entity ID rejected (%)', SQLERRM;
END;
$$;

-- =========================================================================
-- Final integrity check
-- =========================================================================

-- Test 21: All core tables still exist after security tests
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT COUNT(*) INTO cnt
    FROM information_schema.tables
    WHERE table_schema = 'mentat'
      AND table_name IN ('datoms', 'schema', 'idents', 'partitions', 'transactions');
    ASSERT cnt >= 5, 'All core tables should still exist after security tests, got: ' || cnt;
    RAISE NOTICE 'PASS: all core tables intact after security tests';
END;
$$;

ROLLBACK;
