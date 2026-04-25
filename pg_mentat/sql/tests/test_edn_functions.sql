-- Test suite: EDN function suite
--
-- Tests the 10 EDN accessor, introspection, and conversion functions
-- defined in edn_functions.rs, plus the operators from operators.rs.
--
-- Functions tested:
--   mentat.edn_get_key()       - map key lookup (keyword/text)
--   mentat.edn_get_idx()       - vector/list index access
--   mentat.edn_array_elements() - SRF: vector/list/set elements
--   mentat.edn_map_keys()      - SRF: map keys
--   mentat.edn_each()          - SRF: map key-value pairs
--   mentat.edn_typeof()        - type name introspection
--   mentat.edn_exists()        - map key existence check
--   mentat.edn_array_length()  - collection length
--   mentat.edn_to_jsonb()      - EDN -> JSONB conversion
--   mentat.jsonb_to_edn()      - JSONB -> EDN conversion
--
-- Also tested (from operators.rs):
--   edn_get()      - generic map get by EDN key
--   edn_nth()      - vector index access
--   edn_count()    - collection element count
--   edn_is_nil(), edn_is_boolean(), edn_is_integer(), edn_is_float(),
--   edn_is_text(), edn_is_keyword(), edn_is_vector(), edn_is_list(),
--   edn_is_set(), edn_is_map()
--   edn_contains() - collection membership
--   edn_keys()     - map keys as vector
--   edn_values()   - map values as vector

BEGIN;

-- =========================================================================
-- edn_get_key: Map key lookup
-- =========================================================================

-- Test 1: Get by keyword with leading colon
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.edn_get_key('{:name "Alice"}'::mentat.edn, ':name')::TEXT;
    ASSERT result IS NOT NULL, 'edn_get_key should find :name in map';
    RAISE NOTICE 'PASS: edn_get_key with colon prefix';
END;
$$;

-- Test 2: Get by keyword without leading colon
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.edn_get_key('{:name "Alice"}'::mentat.edn, 'name')::TEXT;
    ASSERT result IS NOT NULL, 'edn_get_key should find name without colon';
    RAISE NOTICE 'PASS: edn_get_key without colon prefix';
END;
$$;

-- Test 3: Get by namespaced keyword
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.edn_get_key('{:person/name "Alice"}'::mentat.edn, ':person/name')::TEXT;
    ASSERT result IS NOT NULL, 'edn_get_key should find :person/name';
    RAISE NOTICE 'PASS: edn_get_key with namespaced keyword';
END;
$$;

-- Test 4: Get missing key returns NULL
DO $$
DECLARE
    result mentat.edn;
BEGIN
    result := mentat.edn_get_key('{:name "Alice"}'::mentat.edn, ':missing');
    ASSERT result IS NULL, 'edn_get_key should return NULL for missing key';
    RAISE NOTICE 'PASS: edn_get_key returns NULL for missing key';
END;
$$;

-- Test 5: Get on non-map returns NULL
DO $$
DECLARE
    result mentat.edn;
BEGIN
    result := mentat.edn_get_key('[1 2 3]'::mentat.edn, ':name');
    ASSERT result IS NULL, 'edn_get_key should return NULL for non-map input';
    RAISE NOTICE 'PASS: edn_get_key returns NULL for non-map';
END;
$$;

-- =========================================================================
-- edn_get_idx: Vector/list index access
-- =========================================================================

-- Test 6: Get by index from vector
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.edn_get_idx('[10 20 30]'::mentat.edn, 1)::TEXT;
    ASSERT result = '20', 'edn_get_idx(1) should return 20, got: ' || COALESCE(result, 'NULL');
    RAISE NOTICE 'PASS: edn_get_idx vector access';
END;
$$;

-- Test 7: Get first element
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.edn_get_idx('[10 20 30]'::mentat.edn, 0)::TEXT;
    ASSERT result = '10', 'edn_get_idx(0) should return 10';
    RAISE NOTICE 'PASS: edn_get_idx first element';
END;
$$;

-- Test 8: Out-of-bounds index returns NULL
DO $$
DECLARE
    result mentat.edn;
BEGIN
    result := mentat.edn_get_idx('[10 20 30]'::mentat.edn, 5);
    ASSERT result IS NULL, 'edn_get_idx out-of-bounds should return NULL';
    RAISE NOTICE 'PASS: edn_get_idx out-of-bounds returns NULL';
END;
$$;

-- Test 9: Negative index returns NULL
DO $$
DECLARE
    result mentat.edn;
BEGIN
    result := mentat.edn_get_idx('[10 20 30]'::mentat.edn, -1);
    ASSERT result IS NULL, 'edn_get_idx negative index should return NULL';
    RAISE NOTICE 'PASS: edn_get_idx negative index returns NULL';
END;
$$;

-- Test 10: Get from list
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.edn_get_idx('(10 20 30)'::mentat.edn, 2)::TEXT;
    ASSERT result = '30', 'edn_get_idx on list should work';
    RAISE NOTICE 'PASS: edn_get_idx list access';
END;
$$;

-- Test 11: Get from non-collection returns NULL
DO $$
DECLARE
    result mentat.edn;
BEGIN
    result := mentat.edn_get_idx('42'::mentat.edn, 0);
    ASSERT result IS NULL, 'edn_get_idx on scalar should return NULL';
    RAISE NOTICE 'PASS: edn_get_idx on scalar returns NULL';
END;
$$;

-- =========================================================================
-- edn_array_elements: Set-returning function
-- =========================================================================

-- Test 12: Vector elements
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.edn_array_elements('[1 2 3]'::mentat.edn);
    ASSERT cnt = 3, 'edn_array_elements should return 3 rows for [1 2 3]';
    RAISE NOTICE 'PASS: edn_array_elements vector';
END;
$$;

-- Test 13: Empty vector
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.edn_array_elements('[]'::mentat.edn);
    ASSERT cnt = 0, 'edn_array_elements should return 0 rows for []';
    RAISE NOTICE 'PASS: edn_array_elements empty vector';
END;
$$;

-- Test 14: Set elements
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.edn_array_elements('#{:a :b :c}'::mentat.edn);
    ASSERT cnt = 3, 'edn_array_elements should return 3 rows for set';
    RAISE NOTICE 'PASS: edn_array_elements set';
END;
$$;

-- Test 15: Non-collection returns no rows
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.edn_array_elements('42'::mentat.edn);
    ASSERT cnt = 0, 'edn_array_elements on scalar should return 0 rows';
    RAISE NOTICE 'PASS: edn_array_elements on scalar';
END;
$$;

-- =========================================================================
-- edn_map_keys: Map key enumeration
-- =========================================================================

-- Test 16: Map keys
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.edn_map_keys('{:a 1 :b 2 :c 3}'::mentat.edn);
    ASSERT cnt = 3, 'edn_map_keys should return 3 keys';
    RAISE NOTICE 'PASS: edn_map_keys';
END;
$$;

-- Test 17: Empty map keys
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.edn_map_keys('{}'::mentat.edn);
    ASSERT cnt = 0, 'edn_map_keys on empty map should return 0';
    RAISE NOTICE 'PASS: edn_map_keys empty map';
END;
$$;

-- Test 18: Non-map returns no rows
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.edn_map_keys('[1 2 3]'::mentat.edn);
    ASSERT cnt = 0, 'edn_map_keys on non-map should return 0';
    RAISE NOTICE 'PASS: edn_map_keys on non-map';
END;
$$;

-- =========================================================================
-- edn_each: Map key-value pairs
-- =========================================================================

-- Test 19: Map each
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.edn_each('{:a 1 :b 2}'::mentat.edn);
    ASSERT cnt = 2, 'edn_each should return 2 pairs';
    RAISE NOTICE 'PASS: edn_each';
END;
$$;

-- Test 20: Non-map returns no rows
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.edn_each('[1 2]'::mentat.edn);
    ASSERT cnt = 0, 'edn_each on non-map should return 0';
    RAISE NOTICE 'PASS: edn_each on non-map';
END;
$$;

-- =========================================================================
-- edn_typeof: Type introspection
-- =========================================================================

-- Test 21: Type of nil
DO $$
BEGIN
    ASSERT mentat.edn_typeof('nil'::mentat.edn) = 'nil',
        'typeof(nil) should be "nil"';
    RAISE NOTICE 'PASS: edn_typeof nil';
END;
$$;

-- Test 22: Type of boolean
DO $$
BEGIN
    ASSERT mentat.edn_typeof('true'::mentat.edn) = 'boolean',
        'typeof(true) should be "boolean"';
    RAISE NOTICE 'PASS: edn_typeof boolean';
END;
$$;

-- Test 23: Type of integer
DO $$
BEGIN
    ASSERT mentat.edn_typeof('42'::mentat.edn) = 'integer',
        'typeof(42) should be "integer"';
    RAISE NOTICE 'PASS: edn_typeof integer';
END;
$$;

-- Test 24: Type of float
DO $$
BEGIN
    ASSERT mentat.edn_typeof('3.14'::mentat.edn) = 'float',
        'typeof(3.14) should be "float"';
    RAISE NOTICE 'PASS: edn_typeof float';
END;
$$;

-- Test 25: Type of text
DO $$
BEGIN
    ASSERT mentat.edn_typeof('"hello"'::mentat.edn) = 'text',
        'typeof("hello") should be "text"';
    RAISE NOTICE 'PASS: edn_typeof text';
END;
$$;

-- Test 26: Type of keyword
DO $$
BEGIN
    ASSERT mentat.edn_typeof(':name'::mentat.edn) = 'keyword',
        'typeof(:name) should be "keyword"';
    RAISE NOTICE 'PASS: edn_typeof keyword';
END;
$$;

-- Test 27: Type of vector
DO $$
BEGIN
    ASSERT mentat.edn_typeof('[1 2 3]'::mentat.edn) = 'vector',
        'typeof([1 2 3]) should be "vector"';
    RAISE NOTICE 'PASS: edn_typeof vector';
END;
$$;

-- Test 28: Type of list
DO $$
BEGIN
    ASSERT mentat.edn_typeof('(1 2 3)'::mentat.edn) = 'list',
        'typeof((1 2 3)) should be "list"';
    RAISE NOTICE 'PASS: edn_typeof list';
END;
$$;

-- Test 29: Type of set
DO $$
BEGIN
    ASSERT mentat.edn_typeof('#{1 2 3}'::mentat.edn) = 'set',
        'typeof(#{1 2 3}) should be "set"';
    RAISE NOTICE 'PASS: edn_typeof set';
END;
$$;

-- Test 30: Type of map
DO $$
BEGIN
    ASSERT mentat.edn_typeof('{:a 1}'::mentat.edn) = 'map',
        'typeof({:a 1}) should be "map"';
    RAISE NOTICE 'PASS: edn_typeof map';
END;
$$;

-- Test 31: Type of uuid
DO $$
BEGIN
    ASSERT mentat.edn_typeof('#uuid "550e8400-e29b-41d4-a716-446655440000"'::mentat.edn) = 'uuid',
        'typeof(uuid) should be "uuid"';
    RAISE NOTICE 'PASS: edn_typeof uuid';
END;
$$;

-- Test 32: Type of symbol
DO $$
BEGIN
    ASSERT mentat.edn_typeof('my-symbol'::mentat.edn) = 'symbol',
        'typeof(my-symbol) should be "symbol"';
    RAISE NOTICE 'PASS: edn_typeof symbol';
END;
$$;

-- =========================================================================
-- edn_exists: Key existence check
-- =========================================================================

-- Test 33: Key exists
DO $$
BEGIN
    ASSERT mentat.edn_exists('{:name "Alice"}'::mentat.edn, ':name') = TRUE,
        'edn_exists should find :name';
    RAISE NOTICE 'PASS: edn_exists found key';
END;
$$;

-- Test 34: Key does not exist
DO $$
BEGIN
    ASSERT mentat.edn_exists('{:name "Alice"}'::mentat.edn, ':age') = FALSE,
        'edn_exists should not find :age';
    RAISE NOTICE 'PASS: edn_exists key not found';
END;
$$;

-- Test 35: Namespaced key exists
DO $$
BEGIN
    ASSERT mentat.edn_exists('{:person/name "Alice"}'::mentat.edn, ':person/name') = TRUE,
        'edn_exists should find :person/name';
    RAISE NOTICE 'PASS: edn_exists namespaced key';
END;
$$;

-- Test 36: Non-map always returns false
DO $$
BEGIN
    ASSERT mentat.edn_exists('[1 2 3]'::mentat.edn, ':name') = FALSE,
        'edn_exists on non-map should return false';
    RAISE NOTICE 'PASS: edn_exists on non-map';
END;
$$;

-- =========================================================================
-- edn_array_length: Collection length
-- =========================================================================

-- Test 37: Vector length
DO $$
BEGIN
    ASSERT mentat.edn_array_length('[1 2 3]'::mentat.edn) = 3,
        'edn_array_length of [1 2 3] should be 3';
    RAISE NOTICE 'PASS: edn_array_length vector';
END;
$$;

-- Test 38: Map length
DO $$
BEGIN
    ASSERT mentat.edn_array_length('{:a 1 :b 2}'::mentat.edn) = 2,
        'edn_array_length of {:a 1 :b 2} should be 2';
    RAISE NOTICE 'PASS: edn_array_length map';
END;
$$;

-- Test 39: Set length
DO $$
BEGIN
    ASSERT mentat.edn_array_length('#{:a :b :c :d}'::mentat.edn) = 4,
        'edn_array_length of #{:a :b :c :d} should be 4';
    RAISE NOTICE 'PASS: edn_array_length set';
END;
$$;

-- Test 40: Empty collection
DO $$
BEGIN
    ASSERT mentat.edn_array_length('[]'::mentat.edn) = 0,
        'edn_array_length of [] should be 0';
    RAISE NOTICE 'PASS: edn_array_length empty';
END;
$$;

-- Test 41: Non-collection returns 0
DO $$
BEGIN
    ASSERT mentat.edn_array_length('42'::mentat.edn) = 0,
        'edn_array_length of scalar should be 0';
    RAISE NOTICE 'PASS: edn_array_length scalar';
END;
$$;

-- =========================================================================
-- edn_to_jsonb: EDN -> JSONB conversion
-- =========================================================================

-- Test 42: Integer conversion
DO $$
DECLARE
    result JSONB;
BEGIN
    result := mentat.edn_to_jsonb('42'::mentat.edn);
    ASSERT result = '42'::jsonb, 'edn_to_jsonb(42) should produce JSON 42';
    RAISE NOTICE 'PASS: edn_to_jsonb integer';
END;
$$;

-- Test 43: Boolean conversion
DO $$
DECLARE
    result JSONB;
BEGIN
    result := mentat.edn_to_jsonb('true'::mentat.edn);
    ASSERT result = 'true'::jsonb, 'edn_to_jsonb(true) should produce JSON true';
    RAISE NOTICE 'PASS: edn_to_jsonb boolean';
END;
$$;

-- Test 44: Nil to null conversion
DO $$
DECLARE
    result JSONB;
BEGIN
    result := mentat.edn_to_jsonb('nil'::mentat.edn);
    ASSERT result = 'null'::jsonb, 'edn_to_jsonb(nil) should produce JSON null';
    RAISE NOTICE 'PASS: edn_to_jsonb nil';
END;
$$;

-- Test 45: Text conversion
DO $$
DECLARE
    result JSONB;
BEGIN
    result := mentat.edn_to_jsonb('"hello"'::mentat.edn);
    ASSERT result = '"hello"'::jsonb, 'edn_to_jsonb("hello") should produce JSON "hello"';
    RAISE NOTICE 'PASS: edn_to_jsonb text';
END;
$$;

-- Test 46: Vector to array conversion
DO $$
DECLARE
    result JSONB;
BEGIN
    result := mentat.edn_to_jsonb('[1 2 3]'::mentat.edn);
    ASSERT result = '[1, 2, 3]'::jsonb, 'edn_to_jsonb([1 2 3]) should produce JSON array';
    RAISE NOTICE 'PASS: edn_to_jsonb vector';
END;
$$;

-- Test 47: Map to object conversion
DO $$
DECLARE
    result JSONB;
BEGIN
    result := mentat.edn_to_jsonb('{:name "Alice"}'::mentat.edn);
    ASSERT result ? ':name', 'edn_to_jsonb map should have :name key';
    ASSERT result->>':name' = 'Alice', 'edn_to_jsonb map :name should be Alice';
    RAISE NOTICE 'PASS: edn_to_jsonb map';
END;
$$;

-- =========================================================================
-- jsonb_to_edn: JSONB -> EDN conversion
-- =========================================================================

-- Test 48: JSON null to EDN nil
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.jsonb_to_edn('null'::jsonb)::TEXT;
    ASSERT result = 'nil', 'jsonb_to_edn(null) should produce nil, got: ' || result;
    RAISE NOTICE 'PASS: jsonb_to_edn null';
END;
$$;

-- Test 49: JSON boolean to EDN boolean
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.jsonb_to_edn('true'::jsonb)::TEXT;
    ASSERT result = 'true', 'jsonb_to_edn(true) should produce true';
    RAISE NOTICE 'PASS: jsonb_to_edn boolean';
END;
$$;

-- Test 50: JSON integer to EDN integer
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.jsonb_to_edn('42'::jsonb)::TEXT;
    ASSERT result = '42', 'jsonb_to_edn(42) should produce 42';
    RAISE NOTICE 'PASS: jsonb_to_edn integer';
END;
$$;

-- Test 51: JSON array to EDN vector
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.jsonb_to_edn('[1, 2, 3]'::jsonb)::TEXT;
    ASSERT result = '[1 2 3]', 'jsonb_to_edn([1,2,3]) should produce [1 2 3], got: ' || result;
    RAISE NOTICE 'PASS: jsonb_to_edn array';
END;
$$;

-- Test 52: JSON string starting with ":" becomes keyword
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.jsonb_to_edn('":name"'::jsonb)::TEXT;
    ASSERT result = ':name', 'jsonb_to_edn(":name") should produce :name, got: ' || result;
    RAISE NOTICE 'PASS: jsonb_to_edn keyword string';
END;
$$;

-- Test 53: JSON string without ":" stays text
DO $$
DECLARE
    result TEXT;
BEGIN
    result := mentat.jsonb_to_edn('"hello"'::jsonb)::TEXT;
    ASSERT result = '"hello"', 'jsonb_to_edn("hello") should produce "hello"';
    RAISE NOTICE 'PASS: jsonb_to_edn plain string';
END;
$$;

-- =========================================================================
-- Round-trip: EDN -> JSONB -> EDN
-- =========================================================================

-- Test 54: Integer round-trip
DO $$
DECLARE
    original mentat.edn;
    roundtrip mentat.edn;
BEGIN
    original := '42'::mentat.edn;
    roundtrip := mentat.jsonb_to_edn(mentat.edn_to_jsonb(original));
    ASSERT original::TEXT = roundtrip::TEXT,
        'Integer round-trip should preserve value';
    RAISE NOTICE 'PASS: integer round-trip';
END;
$$;

-- Test 55: Vector round-trip
DO $$
DECLARE
    original mentat.edn;
    roundtrip mentat.edn;
BEGIN
    original := '[1 2 3]'::mentat.edn;
    roundtrip := mentat.jsonb_to_edn(mentat.edn_to_jsonb(original));
    ASSERT original::TEXT = roundtrip::TEXT,
        'Vector round-trip should preserve value';
    RAISE NOTICE 'PASS: vector round-trip';
END;
$$;

-- =========================================================================
-- Operators (from operators.rs)
-- =========================================================================

-- Test 56: edn_get
DO $$
DECLARE
    result mentat.edn;
BEGIN
    result := edn_get('{:a 1}'::mentat.edn, ':a'::mentat.edn);
    ASSERT result IS NOT NULL, 'edn_get should find :a';
    RAISE NOTICE 'PASS: edn_get';
END;
$$;

-- Test 57: edn_nth
DO $$
DECLARE
    result TEXT;
BEGIN
    result := edn_nth('[10 20 30]'::mentat.edn, 1)::TEXT;
    ASSERT result = '20', 'edn_nth(1) should return 20';
    RAISE NOTICE 'PASS: edn_nth';
END;
$$;

-- Test 58: edn_count
DO $$
BEGIN
    ASSERT edn_count('[1 2 3 4]'::mentat.edn) = 4, 'edn_count should be 4';
    ASSERT edn_count('{:a 1 :b 2}'::mentat.edn) = 2, 'edn_count of map should be 2';
    ASSERT edn_count('42'::mentat.edn) = 0, 'edn_count of scalar should be 0';
    RAISE NOTICE 'PASS: edn_count';
END;
$$;

-- Test 59: edn_is_* type predicates
DO $$
BEGIN
    ASSERT edn_is_nil('nil'::mentat.edn) = TRUE, 'nil should be nil';
    ASSERT edn_is_nil('42'::mentat.edn) = FALSE, '42 should not be nil';

    ASSERT edn_is_boolean('true'::mentat.edn) = TRUE, 'true should be boolean';
    ASSERT edn_is_boolean('42'::mentat.edn) = FALSE, '42 should not be boolean';

    ASSERT edn_is_integer('42'::mentat.edn) = TRUE, '42 should be integer';
    ASSERT edn_is_integer('"hi"'::mentat.edn) = FALSE, '"hi" should not be integer';

    ASSERT edn_is_float('3.14'::mentat.edn) = TRUE, '3.14 should be float';
    ASSERT edn_is_float('42'::mentat.edn) = FALSE, '42 should not be float';

    ASSERT edn_is_text('"hello"'::mentat.edn) = TRUE, '"hello" should be text';
    ASSERT edn_is_text(':kw'::mentat.edn) = FALSE, ':kw should not be text';

    ASSERT edn_is_keyword(':kw'::mentat.edn) = TRUE, ':kw should be keyword';
    ASSERT edn_is_keyword('"hi"'::mentat.edn) = FALSE, '"hi" should not be keyword';

    ASSERT edn_is_vector('[1 2]'::mentat.edn) = TRUE, '[1 2] should be vector';
    ASSERT edn_is_vector('(1 2)'::mentat.edn) = FALSE, '(1 2) should not be vector';

    ASSERT edn_is_list('(1 2)'::mentat.edn) = TRUE, '(1 2) should be list';
    ASSERT edn_is_list('[1 2]'::mentat.edn) = FALSE, '[1 2] should not be list';

    ASSERT edn_is_set('#{1 2}'::mentat.edn) = TRUE, '#{1 2} should be set';
    ASSERT edn_is_set('[1 2]'::mentat.edn) = FALSE, '[1 2] should not be set';

    ASSERT edn_is_map('{:a 1}'::mentat.edn) = TRUE, '{:a 1} should be map';
    ASSERT edn_is_map('[1 2]'::mentat.edn) = FALSE, '[1 2] should not be map';

    RAISE NOTICE 'PASS: all edn_is_* type predicates';
END;
$$;

-- Test 60: edn_contains
DO $$
BEGIN
    -- Vector contains
    ASSERT edn_contains('[1 2 3]'::mentat.edn, '2'::mentat.edn) = TRUE,
        'vector should contain 2';
    ASSERT edn_contains('[1 2 3]'::mentat.edn, '5'::mentat.edn) = FALSE,
        'vector should not contain 5';

    -- Set contains
    ASSERT edn_contains('#{:a :b :c}'::mentat.edn, ':b'::mentat.edn) = TRUE,
        'set should contain :b';
    ASSERT edn_contains('#{:a :b :c}'::mentat.edn, ':d'::mentat.edn) = FALSE,
        'set should not contain :d';

    -- Map contains key
    ASSERT edn_contains('{:a 1 :b 2}'::mentat.edn, ':a'::mentat.edn) = TRUE,
        'map should contain key :a';

    -- Scalar returns false
    ASSERT edn_contains('42'::mentat.edn, '42'::mentat.edn) = FALSE,
        'scalar contains should return false';

    RAISE NOTICE 'PASS: edn_contains';
END;
$$;

-- Test 61: edn_keys and edn_values
DO $$
DECLARE
    keys_result mentat.edn;
    vals_result mentat.edn;
BEGIN
    keys_result := edn_keys('{:a 1 :b 2}'::mentat.edn);
    ASSERT keys_result IS NOT NULL, 'edn_keys should return a result';
    ASSERT edn_is_vector(keys_result) = TRUE, 'edn_keys should return a vector';
    ASSERT edn_count(keys_result) = 2, 'edn_keys should have 2 elements';

    vals_result := edn_values('{:a 1 :b 2}'::mentat.edn);
    ASSERT vals_result IS NOT NULL, 'edn_values should return a result';
    ASSERT edn_is_vector(vals_result) = TRUE, 'edn_values should return a vector';
    ASSERT edn_count(vals_result) = 2, 'edn_values should have 2 elements';

    -- Non-map returns NULL
    ASSERT edn_keys('[1 2 3]'::mentat.edn) IS NULL, 'edn_keys on non-map should be NULL';
    ASSERT edn_values('[1 2 3]'::mentat.edn) IS NULL, 'edn_values on non-map should be NULL';

    RAISE NOTICE 'PASS: edn_keys and edn_values';
END;
$$;

-- =========================================================================
-- EDN equality (from PostgresEq derive)
-- =========================================================================

-- Test 62: EDN equality
DO $$
BEGIN
    ASSERT ('42'::mentat.edn = '42'::mentat.edn) = TRUE,
        'same integer should be equal';
    ASSERT ('[1 2 3]'::mentat.edn = '[1 2 3]'::mentat.edn) = TRUE,
        'same vector should be equal';
    ASSERT ('42'::mentat.edn = '43'::mentat.edn) = FALSE,
        'different integers should not be equal';
    ASSERT ('[1 2]'::mentat.edn = '[1 2 3]'::mentat.edn) = FALSE,
        'different vectors should not be equal';
    RAISE NOTICE 'PASS: EDN equality';
END;
$$;

-- =========================================================================
-- EDN type in table context
-- =========================================================================

-- Test 63: Store and retrieve EDN values in a table
CREATE TEMPORARY TABLE edn_store (
    id SERIAL PRIMARY KEY,
    data mentat.edn
);

INSERT INTO edn_store (data) VALUES
    ('nil'::mentat.edn),
    ('42'::mentat.edn),
    ('"hello"'::mentat.edn),
    (':keyword'::mentat.edn),
    ('[1 2 3]'::mentat.edn),
    ('{:a 1}'::mentat.edn);

DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt FROM edn_store;
    ASSERT cnt = 6, 'should have 6 rows in edn_store';

    -- Verify type of each row
    ASSERT (SELECT mentat.edn_typeof(data) FROM edn_store WHERE id = 1) = 'nil', 'row 1 type';
    ASSERT (SELECT mentat.edn_typeof(data) FROM edn_store WHERE id = 2) = 'integer', 'row 2 type';
    ASSERT (SELECT mentat.edn_typeof(data) FROM edn_store WHERE id = 3) = 'text', 'row 3 type';
    ASSERT (SELECT mentat.edn_typeof(data) FROM edn_store WHERE id = 4) = 'keyword', 'row 4 type';
    ASSERT (SELECT mentat.edn_typeof(data) FROM edn_store WHERE id = 5) = 'vector', 'row 5 type';
    ASSERT (SELECT mentat.edn_typeof(data) FROM edn_store WHERE id = 6) = 'map', 'row 6 type';

    RAISE NOTICE 'PASS: EDN in table context';
END;
$$;

DROP TABLE edn_store;

ROLLBACK;
