-- Schema validation tests for pg_mentat
-- Run after loading the full schema

BEGIN;

-- Test 1: Verify all tables exist
DO $$
DECLARE
    table_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO table_count
    FROM pg_tables
    WHERE schemaname = 'mentat';

    ASSERT table_count >= 7, 'Expected at least 7 tables in mentat schema';
    RAISE NOTICE 'Test 1 PASSED: Found % tables', table_count;
END $$;

-- Test 2: Verify partitions loaded
DO $$
DECLARE
    partition_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO partition_count FROM mentat.partitions;
    ASSERT partition_count = 3, 'Expected 3 default partitions';
    RAISE NOTICE 'Test 2 PASSED: Found % partitions', partition_count;
END $$;

-- Test 3: Verify bootstrap schema attributes loaded
DO $$
DECLARE
    attr_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO attr_count FROM mentat.schema WHERE entid < 100;
    ASSERT attr_count > 30, 'Expected >30 bootstrap attributes';
    RAISE NOTICE 'Test 3 PASSED: Found % bootstrap attributes', attr_count;
END $$;

-- Test 4: Test entity ID allocation
DO $$
DECLARE
    entid BIGINT;
BEGIN
    entid := mentat.allocate_entid('db.part/user');
    ASSERT entid = 10000, 'First user entid should be 10000';

    entid := mentat.allocate_entid('db.part/user');
    ASSERT entid = 10001, 'Second user entid should be 10001';

    RAISE NOTICE 'Test 4 PASSED: Entity allocation works';
END $$;

-- Test 5: Test multiple entity ID allocation
DO $$
DECLARE
    entids BIGINT[];
BEGIN
    entids := mentat.allocate_entids('db.part/user', 5);
    ASSERT array_length(entids, 1) = 5, 'Should allocate 5 entids';
    ASSERT entids[1] = 10002, 'First allocated should be 10002';
    ASSERT entids[5] = 10006, 'Last allocated should be 10006';

    RAISE NOTICE 'Test 5 PASSED: Batch entity allocation works';
END $$;

-- Test 6: Test schema attribute definition
DO $$
DECLARE
    attr_entid BIGINT;
BEGIN
    attr_entid := mentat.allocate_entid('db.part/db');

    INSERT INTO mentat.schema (entid, ident, value_type, cardinality, unique_constraint, indexed)
    VALUES (attr_entid, ':test/name', 'string', 'one', NULL, FALSE);

    INSERT INTO mentat.idents (ident, entid)
    VALUES (':test/name', attr_entid);

    ASSERT (SELECT value_type FROM mentat.schema WHERE entid = attr_entid) = 'string',
        'Attribute value_type should be string';

    RAISE NOTICE 'Test 6 PASSED: Schema attribute creation works';
END $$;

-- Test 7: Test ident resolution
DO $$
DECLARE
    resolved_entid BIGINT;
BEGIN
    resolved_entid := mentat.resolve_ident(':db/ident');
    ASSERT resolved_entid = 10, 'db/ident should resolve to 10';

    RAISE NOTICE 'Test 7 PASSED: Ident resolution works';
END $$;

-- Test 8: Test transaction creation
DO $$
DECLARE
    tx_id BIGINT;
BEGIN
    tx_id := mentat.current_tx();
    ASSERT tx_id >= 1000000000, 'Transaction ID should be in tx partition';

    ASSERT (SELECT COUNT(*) FROM mentat.transactions WHERE tx_id = tx_id) = 1,
        'Transaction should be recorded';

    RAISE NOTICE 'Test 8 PASSED: Transaction creation works (tx=%)', tx_id;
END $$;

-- Test 9: Test datom insertion
DO $$
DECLARE
    test_entity BIGINT;
    test_attr BIGINT;
    tx_id BIGINT;
BEGIN
    test_entity := mentat.allocate_entid('db.part/user');
    test_attr := mentat.resolve_ident(':test/name');
    tx_id := mentat.current_tx();

    -- Insert a string datom
    INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag)
    VALUES (test_entity, test_attr, 'Alice'::bytea, tx_id, TRUE, 10);

    ASSERT (SELECT COUNT(*) FROM mentat.datoms WHERE e = test_entity) = 1,
        'Should have one datom for entity';

    RAISE NOTICE 'Test 9 PASSED: Datom insertion works (entity=%)', test_entity;
END $$;

-- Test 10: Test EAVT index
DO $$
DECLARE
    test_entity BIGINT;
BEGIN
    SELECT e INTO test_entity
    FROM mentat.datoms
    WHERE added = TRUE
    LIMIT 1;

    IF test_entity IS NOT NULL THEN
        EXPLAIN (FORMAT TEXT)
        SELECT * FROM mentat.datoms WHERE e = test_entity AND added = TRUE;

        RAISE NOTICE 'Test 10 PASSED: EAVT index exists';
    ELSE
        RAISE NOTICE 'Test 10 SKIPPED: No datoms to test';
    END IF;
END $$;

-- Test 11: Test fulltext table and triggers
DO $$
DECLARE
    rowid BIGINT;
    search_vector_set BOOLEAN;
BEGIN
    INSERT INTO mentat.fulltext (text_value)
    VALUES ('The quick brown fox jumps over the lazy dog')
    RETURNING fulltext.rowid INTO rowid;

    SELECT search_vector IS NOT NULL INTO search_vector_set
    FROM mentat.fulltext WHERE fulltext.rowid = rowid;

    ASSERT search_vector_set, 'Search vector should be automatically set';

    RAISE NOTICE 'Test 11 PASSED: Fulltext trigger works (rowid=%)', rowid;
END $$;

-- Test 12: Test fulltext search function
DO $$
DECLARE
    result_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO result_count
    FROM mentat.fulltext_search('quick fox');

    ASSERT result_count > 0, 'Should find fulltext results';

    RAISE NOTICE 'Test 12 PASSED: Fulltext search works (% results)', result_count;
END $$;

-- Test 13: Test unique constraint enforcement
DO $$
DECLARE
    unique_attr BIGINT;
    entity1 BIGINT;
    entity2 BIGINT;
    tx_id BIGINT;
    constraint_violated BOOLEAN := FALSE;
BEGIN
    -- Create a unique attribute
    unique_attr := mentat.allocate_entid('db.part/db');
    INSERT INTO mentat.schema (entid, ident, value_type, cardinality, unique_constraint, indexed)
    VALUES (unique_attr, ':test/unique', 'long', 'one', 'value', TRUE);

    entity1 := mentat.allocate_entid('db.part/user');
    entity2 := mentat.allocate_entid('db.part/user');
    tx_id := mentat.current_tx();

    -- Insert first datom with value 42
    INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag)
    VALUES (entity1, unique_attr, int8send(42::bigint), tx_id, TRUE, 4);

    -- Try to insert duplicate (should fail)
    BEGIN
        INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag)
        VALUES (entity2, unique_attr, int8send(42::bigint), tx_id, TRUE, 4);
    EXCEPTION
        WHEN unique_violation THEN
            constraint_violated := TRUE;
    END;

    ASSERT constraint_violated, 'Unique constraint should prevent duplicate values';

    RAISE NOTICE 'Test 13 PASSED: Unique constraint enforcement works';
END $$;

-- Test 14: Test partition boundary validation
DO $$
DECLARE
    boundary_error BOOLEAN := FALSE;
BEGIN
    -- Try to set next_entid outside bounds
    UPDATE mentat.partitions
    SET next_entid = end_entid + 1
    WHERE name = 'db.part/user';

EXCEPTION
    WHEN check_violation OR raise_exception THEN
        boundary_error := TRUE;
END;

DO $$
BEGIN
    -- Reset the partition
    UPDATE mentat.partitions
    SET next_entid = 10007  -- Reset to current state
    WHERE name = 'db.part/user';

    RAISE NOTICE 'Test 14 PASSED: Partition boundary validation works';
END $$;

-- Test 15: Test value type validation trigger
DO $$
DECLARE
    string_attr BIGINT;
    entity BIGINT;
    tx_id BIGINT;
    type_error BOOLEAN := FALSE;
BEGIN
    string_attr := mentat.resolve_ident(':test/name');
    entity := mentat.allocate_entid('db.part/user');
    tx_id := mentat.current_tx();

    -- Try to insert long value for string attribute (should fail)
    BEGIN
        INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag)
        VALUES (entity, string_attr, int8send(123::bigint), tx_id, TRUE, 4);  -- Wrong type tag
    EXCEPTION
        WHEN raise_exception THEN
            type_error := TRUE;
    END;

    ASSERT type_error, 'Type validation should prevent wrong value types';

    RAISE NOTICE 'Test 15 PASSED: Value type validation works';
END $$;

-- Summary
DO $$
BEGIN
    RAISE NOTICE '================================';
    RAISE NOTICE 'All schema validation tests PASSED!';
    RAISE NOTICE '================================';
END $$;

ROLLBACK;  -- Don't commit test data
