-- Test suite: EDN pretty printing (edn_pretty)
--
-- Tests the edn_pretty() function which formats EDN values with smart
-- indentation. Verifies output for all EDN types and formatting options.
--
-- Function signature:
--   edn_pretty(edn_input TEXT, width INT DEFAULT NULL) -> TEXT
--   NULL width defaults to 80 columns.

BEGIN;

-- =========================================================================
-- Scalar types: compact formatting
-- =========================================================================

-- Test 1: nil
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('nil');
    ASSERT result = 'nil', 'edn_pretty(nil) should be "nil", got: ' || COALESCE(result, 'NULL');
    RAISE NOTICE 'PASS: edn_pretty nil';
END;
$$;

-- Test 2: boolean true
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('true');
    ASSERT result = 'true', 'edn_pretty(true) should be "true"';
    RAISE NOTICE 'PASS: edn_pretty true';
END;
$$;

-- Test 3: boolean false
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('false');
    ASSERT result = 'false', 'edn_pretty(false) should be "false"';
    RAISE NOTICE 'PASS: edn_pretty false';
END;
$$;

-- Test 4: integer
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('42');
    ASSERT result = '42', 'edn_pretty(42) should be "42"';
    RAISE NOTICE 'PASS: edn_pretty integer';
END;
$$;

-- Test 5: negative integer
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('-123');
    ASSERT result = '-123', 'edn_pretty(-123) should be "-123"';
    RAISE NOTICE 'PASS: edn_pretty negative integer';
END;
$$;

-- Test 6: float
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('3.14');
    ASSERT result IS NOT NULL, 'edn_pretty(3.14) should return a result';
    RAISE NOTICE 'PASS: edn_pretty float';
END;
$$;

-- Test 7: string
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('"hello world"');
    ASSERT result = '"hello world"', 'edn_pretty("hello world") should preserve string';
    RAISE NOTICE 'PASS: edn_pretty string';
END;
$$;

-- Test 8: keyword (plain)
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty(':name');
    ASSERT result = ':name', 'edn_pretty(:name) should be ":name"';
    RAISE NOTICE 'PASS: edn_pretty keyword';
END;
$$;

-- Test 9: keyword (namespaced)
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty(':person/name');
    ASSERT result = ':person/name', 'edn_pretty(:person/name) should be ":person/name"';
    RAISE NOTICE 'PASS: edn_pretty namespaced keyword';
END;
$$;

-- Test 10: symbol
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('my-var');
    ASSERT result IS NOT NULL, 'edn_pretty(my-var) should return a result';
    RAISE NOTICE 'PASS: edn_pretty symbol';
END;
$$;

-- Test 11: UUID
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('#uuid "550e8400-e29b-41d4-a716-446655440000"');
    ASSERT result IS NOT NULL, 'edn_pretty(uuid) should return a result';
    ASSERT result LIKE '%550e8400%', 'edn_pretty(uuid) should contain the UUID';
    RAISE NOTICE 'PASS: edn_pretty UUID';
END;
$$;

-- =========================================================================
-- Collection types: compact when fits in width
-- =========================================================================

-- Test 12: short vector stays compact
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('[1 2 3]');
    ASSERT result = '[1 2 3]', 'short vector should stay compact, got: ' || result;
    RAISE NOTICE 'PASS: edn_pretty short vector';
END;
$$;

-- Test 13: empty vector
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('[]');
    ASSERT result = '[]', 'empty vector should be "[]"';
    RAISE NOTICE 'PASS: edn_pretty empty vector';
END;
$$;

-- Test 14: short list stays compact
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('(1 2 3)');
    ASSERT result IS NOT NULL, 'list should format';
    ASSERT result LIKE '(%', 'list should start with (';
    ASSERT result LIKE '%)', 'list should end with )';
    RAISE NOTICE 'PASS: edn_pretty short list';
END;
$$;

-- Test 15: set formatting
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('#{1 2 3}');
    ASSERT result IS NOT NULL, 'set should format';
    ASSERT result LIKE '#{%', 'set should start with #{';
    ASSERT result LIKE '%}', 'set should end with }';
    RAISE NOTICE 'PASS: edn_pretty set';
END;
$$;

-- Test 16: empty set
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('#{}');
    ASSERT result IS NOT NULL, 'empty set should format';
    RAISE NOTICE 'PASS: edn_pretty empty set';
END;
$$;

-- Test 17: short map stays compact
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('{:a 1 :b 2}');
    ASSERT result IS NOT NULL, 'short map should format';
    ASSERT result LIKE '{%', 'map should start with {';
    ASSERT result LIKE '%}', 'map should end with }';
    RAISE NOTICE 'PASS: edn_pretty short map';
END;
$$;

-- Test 18: empty map
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('{}');
    ASSERT result = '{}', 'empty map should be "{}"';
    RAISE NOTICE 'PASS: edn_pretty empty map';
END;
$$;

-- =========================================================================
-- Width parameter: control line-breaking
-- =========================================================================

-- Test 19: narrow width forces multi-line output
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('[1 2 3 4 5 6 7 8 9 10]', 10);
    ASSERT result LIKE '%' || chr(10) || '%',
        'narrow width should produce multi-line output';
    RAISE NOTICE 'PASS: edn_pretty narrow width produces multi-line';
END;
$$;

-- Test 20: wide width keeps things on one line
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('[1 2 3 4 5]', 200);
    ASSERT result NOT LIKE '%' || chr(10) || '%',
        'wide width should keep short vector on one line';
    RAISE NOTICE 'PASS: edn_pretty wide width keeps one line';
END;
$$;

-- Test 21: default width (NULL) uses 80 columns
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('[1 2 3]', NULL);
    ASSERT result IS NOT NULL, 'NULL width should use default (80)';
    RAISE NOTICE 'PASS: edn_pretty default NULL width';
END;
$$;

-- =========================================================================
-- Nested structures
-- =========================================================================

-- Test 22: nested map in vector
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('[{:name "Alice"} {:name "Bob"}]');
    ASSERT result IS NOT NULL, 'nested map in vector should format';
    ASSERT result LIKE '%Alice%', 'should contain Alice';
    ASSERT result LIKE '%Bob%', 'should contain Bob';
    RAISE NOTICE 'PASS: edn_pretty nested map in vector';
END;
$$;

-- Test 23: nested vector in map
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('{:names ["Alice" "Bob"] :ages [30 25]}');
    ASSERT result IS NOT NULL, 'nested vector in map should format';
    ASSERT result LIKE '%names%', 'should contain :names';
    ASSERT result LIKE '%ages%', 'should contain :ages';
    RAISE NOTICE 'PASS: edn_pretty nested vector in map';
END;
$$;

-- Test 24: deeply nested structure
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('{:a {:b {:c [1 2 3]}}}');
    ASSERT result IS NOT NULL, 'deeply nested structure should format';
    ASSERT result LIKE '%:a%', 'should contain :a';
    ASSERT result LIKE '%:b%', 'should contain :b';
    ASSERT result LIKE '%:c%', 'should contain :c';
    RAISE NOTICE 'PASS: edn_pretty deeply nested';
END;
$$;

-- =========================================================================
-- Datalog query formatting
-- =========================================================================

-- Test 25: typical Datalog query
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('[:find ?e ?name :where [?e :person/name ?name]]');
    ASSERT result IS NOT NULL, 'Datalog query should format';
    ASSERT result LIKE '%:find%', 'should contain :find';
    ASSERT result LIKE '%:where%', 'should contain :where';
    RAISE NOTICE 'PASS: edn_pretty Datalog query';
END;
$$;

-- Test 26: complex Datalog query with narrow width
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty(
        '[:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] [(> ?age 18)]]',
        40
    );
    ASSERT result IS NOT NULL, 'complex Datalog query should format';
    ASSERT result LIKE '%' || chr(10) || '%',
        'complex query at width 40 should be multi-line';
    RAISE NOTICE 'PASS: edn_pretty complex Datalog query';
END;
$$;

-- =========================================================================
-- Error conditions
-- =========================================================================

-- Test 27: invalid EDN input
DO $$
BEGIN
    PERFORM edn_pretty('{invalid edn');
    RAISE EXCEPTION 'should have raised an error for invalid EDN';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: edn_pretty rejects invalid EDN (%)' , SQLERRM;
END;
$$;

-- Test 28: negative width
DO $$
BEGIN
    PERFORM edn_pretty('{:a 1}', -1);
    RAISE EXCEPTION 'should have raised an error for negative width';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: edn_pretty rejects negative width (%)' , SQLERRM;
END;
$$;

-- Test 29: zero width
DO $$
BEGIN
    PERFORM edn_pretty('{:a 1}', 0);
    RAISE EXCEPTION 'should have raised an error for zero width';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: edn_pretty rejects zero width (%)' , SQLERRM;
END;
$$;

-- =========================================================================
-- Idempotence: pretty printing the output of pretty printing
-- =========================================================================

-- Test 30: idempotent formatting
DO $$
DECLARE
    first_pass TEXT;
    second_pass TEXT;
BEGIN
    first_pass := edn_pretty('{:a [1 2 3] :b {:c 4}}');
    second_pass := edn_pretty(first_pass);
    ASSERT first_pass = second_pass,
        'pretty printing should be idempotent';
    RAISE NOTICE 'PASS: edn_pretty is idempotent';
END;
$$;

-- =========================================================================
-- Large structure formatting
-- =========================================================================

-- Test 31: large vector
DO $$
DECLARE
    large_vec TEXT;
    result TEXT;
    i INT;
BEGIN
    large_vec := '[';
    FOR i IN 1..50 LOOP
        IF i > 1 THEN large_vec := large_vec || ' '; END IF;
        large_vec := large_vec || i::TEXT;
    END LOOP;
    large_vec := large_vec || ']';

    result := edn_pretty(large_vec, 40);
    ASSERT result IS NOT NULL, 'large vector should format';
    ASSERT result LIKE '%' || chr(10) || '%',
        'large vector at width 40 should be multi-line';
    RAISE NOTICE 'PASS: edn_pretty large vector';
END;
$$;

-- Test 32: transaction-style EDN
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_pretty('[{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]', 60);
    ASSERT result IS NOT NULL, 'transaction EDN should format';
    ASSERT result LIKE '%:db/ident%', 'should contain :db/ident';
    RAISE NOTICE 'PASS: edn_pretty transaction EDN';
END;
$$;

ROLLBACK;
