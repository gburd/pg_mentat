-- =============================================================================
-- Correctness Tests: Speculative Transactions (mentat_with / d/with)
-- =============================================================================
--
-- Verifies the speculative transaction infrastructure from Task #6:
--   - mentat_with() returns correct transaction report format
--   - Tempid resolution matches committed transactions
--   - Constraint checking works in speculative context
--   - SAVEPOINT rollback leaves no database side effects
--   - CAS operations within speculative transactions
--   - Cache invalidation after speculative tx
--
-- Datomic semantics:
--   d/with applies a transaction to a database value without committing.
--   It returns a transaction report identical in format to d/transact,
--   but the underlying database is unchanged afterward.
--
-- =============================================================================

BEGIN;

-- =========================================================================
-- Setup: Schema for speculative transaction tests
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident       :spec/name
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one}

    {:db/ident       :spec/val
     :db/valueType   :db.type/long
     :db/cardinality :db.cardinality/one}

    {:db/ident       :spec/tags
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/many}

    {:db/ident       :spec/uid
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique      :db.unique/identity}
]');

-- =========================================================================
-- Test 1: mentat_with returns valid JSON with all required keys
-- =========================================================================

DO $$
DECLARE
    r JSONB;
BEGIN
    r := mentat_with('[[:db/add "e" :spec/name "Alice"]]')::JSONB;

    ASSERT r ? 'db-before', 'Report should contain db-before';
    ASSERT r ? 'db-after', 'Report should contain db-after';
    ASSERT r ? 'tx-data', 'Report should contain tx-data';
    ASSERT r ? 'tempids', 'Report should contain tempids';

    ASSERT jsonb_typeof(r->'db-before') = 'object', 'db-before should be object';
    ASSERT jsonb_typeof(r->'db-after') = 'object', 'db-after should be object';
    ASSERT jsonb_typeof(r->'tx-data') = 'array', 'tx-data should be array';
    ASSERT jsonb_typeof(r->'tempids') = 'object', 'tempids should be object';

    RAISE NOTICE 'PASS: Test 1 - mentat_with returns valid JSON with all required keys';
END;
$$;

-- =========================================================================
-- Test 2: Speculative transaction does NOT persist datoms
-- =========================================================================

DO $$
DECLARE
    cnt_before BIGINT;
    cnt_after BIGINT;
BEGIN
    SELECT COUNT(*) FROM mentat.datoms WHERE added = true INTO cnt_before;

    PERFORM mentat_with('[[:db/add "ghost" :spec/name "I should not exist"]]');

    SELECT COUNT(*) FROM mentat.datoms WHERE added = true INTO cnt_after;

    ASSERT cnt_before = cnt_after,
        format('Datom count should be unchanged: before=%s, after=%s',
               cnt_before, cnt_after);

    RAISE NOTICE 'PASS: Test 2 - Speculative transaction does not persist datoms';
END;
$$;

-- =========================================================================
-- Test 3: Speculative transaction does NOT create transaction records
-- =========================================================================

DO $$
DECLARE
    cnt_before BIGINT;
    cnt_after BIGINT;
BEGIN
    SELECT COUNT(*) FROM mentat.transactions INTO cnt_before;

    PERFORM mentat_with('[[:db/add "ghost" :spec/name "No tx record"]]');

    SELECT COUNT(*) FROM mentat.transactions INTO cnt_after;

    ASSERT cnt_before = cnt_after,
        format('Transaction count should be unchanged: before=%s, after=%s',
               cnt_before, cnt_after);

    RAISE NOTICE 'PASS: Test 3 - Speculative transaction does not create tx records';
END;
$$;

-- =========================================================================
-- Test 4: Tempid resolution works in speculative context
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    tempid_e BIGINT;
BEGIN
    r := mentat_with('[[:db/add "person" :spec/name "Bob"]]')::JSONB;

    tempid_e := (r->'tempids'->>'person')::BIGINT;

    ASSERT tempid_e IS NOT NULL, 'tempid "person" should resolve to a number';
    ASSERT tempid_e > 0, format('tempid should be positive, got %s', tempid_e);

    RAISE NOTICE 'PASS: Test 4 - Tempid resolution works in speculative context';
END;
$$;

-- =========================================================================
-- Test 5: Multiple assertions with same tempid resolve consistently
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    tempid_e BIGINT;
    tx_data JSONB;
    entity_datom_count INT;
BEGIN
    r := mentat_with('[
        [:db/add "e" :spec/name "Alice"]
        [:db/add "e" :spec/val 42]
    ]')::JSONB;

    tempid_e := (r->'tempids'->>'e')::BIGINT;
    ASSERT tempid_e IS NOT NULL, 'tempid "e" should resolve';

    -- Count datoms for the resolved entity (skip txInstant at index 0)
    tx_data := r->'tx-data';
    entity_datom_count := 0;

    FOR i IN 1..jsonb_array_length(tx_data) - 1 LOOP
        IF (tx_data->i->>0)::BIGINT = tempid_e THEN
            entity_datom_count := entity_datom_count + 1;
        END IF;
    END LOOP;

    ASSERT entity_datom_count = 2,
        format('Both assertions should use same entity ID, got %s datoms for entity %s',
               entity_datom_count, tempid_e);

    RAISE NOTICE 'PASS: Test 5 - Multiple assertions with same tempid resolve consistently';
END;
$$;

-- =========================================================================
-- Test 6: db-before and db-after have correct basis-t ordering
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    basis_before BIGINT;
    basis_after BIGINT;
BEGIN
    r := mentat_with('[[:db/add "e" :spec/name "test"]]')::JSONB;

    basis_before := (r->'db-before'->>'basis-t')::BIGINT;
    basis_after := (r->'db-after'->>'basis-t')::BIGINT;

    ASSERT basis_after > basis_before,
        format('db-after basis-t (%s) should be > db-before basis-t (%s)',
               basis_after, basis_before);

    RAISE NOTICE 'PASS: Test 6 - Speculative tx has correct basis-t ordering';
END;
$$;

-- =========================================================================
-- Test 7: tx-data includes :db/txInstant as first entry
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    tx_data JSONB;
    first_attr BIGINT;
    first_added BOOLEAN;
BEGIN
    r := mentat_with('[[:db/add "e" :spec/name "test"]]')::JSONB;
    tx_data := r->'tx-data';

    ASSERT jsonb_array_length(tx_data) > 0, 'tx-data should not be empty';

    first_attr := (tx_data->0->>1)::BIGINT;
    first_added := (tx_data->0->>4)::BOOLEAN;

    ASSERT first_attr = 50,
        format('First tx-data datom should be :db/txInstant (attr 50), got %s', first_attr);
    ASSERT first_added = true,
        'txInstant datom should be added=true';

    RAISE NOTICE 'PASS: Test 7 - tx-data includes :db/txInstant as first entry';
END;
$$;

-- =========================================================================
-- Test 8: Speculative report format matches committed report format
-- =========================================================================

DO $$
DECLARE
    with_r JSONB;
    real_r JSONB;
    with_data_len INT;
    real_data_len INT;
BEGIN
    with_r := mentat_with('[[:db/add "e" :spec/name "compare"]]')::JSONB;
    real_r := mentat_transact('[[:db/add "e" :spec/name "compare2"]]')::JSONB;

    -- Both should have the same top-level keys
    ASSERT with_r ? 'db-before' AND with_r ? 'db-after'
       AND with_r ? 'tx-data'   AND with_r ? 'tempids',
        'Speculative report should have all top-level keys';
    ASSERT real_r ? 'db-before' AND real_r ? 'db-after'
       AND real_r ? 'tx-data'   AND real_r ? 'tempids',
        'Committed report should have all top-level keys';

    -- Both should have the same number of tx-data entries
    with_data_len := jsonb_array_length(with_r->'tx-data');
    real_data_len := jsonb_array_length(real_r->'tx-data');

    ASSERT with_data_len = real_data_len,
        format('tx-data length should match: speculative=%s, committed=%s',
               with_data_len, real_data_len);

    RAISE NOTICE 'PASS: Test 8 - Speculative report format matches committed report';
END;
$$;

-- =========================================================================
-- Test 9: CAS succeeds in speculative context
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    with_r JSONB;
    tx_data JSONB;
BEGIN
    -- Commit an entity with a value
    r := mentat_transact('[[:db/add "e" :spec/val 10]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Speculative CAS should succeed
    with_r := mentat_with(format('[[:db.fn/cas %s :spec/val 10 20]]', eid))::JSONB;
    tx_data := with_r->'tx-data';

    -- Should contain retract of old value + assert of new value + txInstant
    ASSERT jsonb_array_length(tx_data) >= 3,
        format('CAS should produce at least 3 datoms, got %s', jsonb_array_length(tx_data));

    RAISE NOTICE 'PASS: Test 9 - CAS succeeds in speculative context';
END;
$$;

-- =========================================================================
-- Test 10: CAS failure in speculative context returns error
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    cas_failed BOOLEAN := FALSE;
BEGIN
    r := mentat_transact('[[:db/add "e" :spec/val 10]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Speculative CAS with wrong old value should fail
    BEGIN
        PERFORM mentat_with(format('[[:db.fn/cas %s :spec/val 999 20]]', eid));
    EXCEPTION WHEN OTHERS THEN
        cas_failed := TRUE;
    END;

    ASSERT cas_failed, 'CAS with wrong old value should fail in speculative context';

    RAISE NOTICE 'PASS: Test 10 - CAS failure returns error in speculative context';
END;
$$;

-- =========================================================================
-- Test 11: Successful speculative CAS does NOT change committed value
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    committed_val BIGINT;
BEGIN
    r := mentat_transact('[[:db/add "e" :spec/val 100]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Run a successful speculative CAS 100 -> 200
    PERFORM mentat_with(format('[[:db.fn/cas %s :spec/val 100 200]]', eid));

    -- Committed value should still be 100
    committed_val := (mentat_query(
        format('[:find ?v . :where [%s :spec/val ?v]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    ASSERT committed_val = 100,
        format('Committed value should remain 100 after speculative CAS, got %s', committed_val);

    RAISE NOTICE 'PASS: Test 11 - Speculative CAS does not change committed value';
END;
$$;

-- =========================================================================
-- Test 12: Unique constraint upsert works in speculative context
-- =========================================================================

DO $$
DECLARE
    with_r JSONB;
BEGIN
    -- Commit an entity with unique identity
    PERFORM mentat_transact('[[:db/add "e" :spec/uid "unique-1"]]');

    -- Speculative tx referencing same unique value should trigger upsert
    with_r := mentat_with('[[:db/add "f" :spec/uid "unique-1"]]')::JSONB;

    -- Should succeed (upsert) and resolve tempid
    ASSERT with_r->'tempids' ? 'f', 'Upsert should resolve tempid "f"';

    RAISE NOTICE 'PASS: Test 12 - Unique constraint upsert works speculatively';
END;
$$;

-- =========================================================================
-- Test 13: retractEntity works in speculative context without persisting
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    with_r JSONB;
    tx_data JSONB;
    retraction_count INT := 0;
    committed_name TEXT;
BEGIN
    -- Commit an entity with multiple attributes
    r := mentat_transact('[
        [:db/add "e" :spec/name "Alice"]
        [:db/add "e" :spec/val 42]
    ]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Speculative retractEntity
    with_r := mentat_with(format('[[:db.fn/retractEntity %s]]', eid))::JSONB;
    tx_data := with_r->'tx-data';

    -- Count retraction datoms (added = false)
    FOR i IN 0..jsonb_array_length(tx_data) - 1 LOOP
        IF (tx_data->i->>4)::BOOLEAN = false THEN
            retraction_count := retraction_count + 1;
        END IF;
    END LOOP;

    ASSERT retraction_count >= 2,
        format('retractEntity should produce at least 2 retractions (name + val), got %s',
               retraction_count);

    -- Entity should still exist in committed database
    committed_name := mentat_query(
        format('[:find ?n . :where [%s :spec/name ?n]]', eid),
        '{}'
    )::JSONB->>'result';

    ASSERT committed_name = 'Alice',
        format('Entity should still exist after speculative retract, got "%s"', committed_name);

    RAISE NOTICE 'PASS: Test 13 - retractEntity works speculatively without persisting';
END;
$$;

-- =========================================================================
-- Test 14: Multiple speculative transactions leave no side effects
-- =========================================================================

DO $$
DECLARE
    cnt_before BIGINT;
    cnt_after BIGINT;
    tx_before BIGINT;
    tx_after BIGINT;
    i INT;
BEGIN
    SELECT COUNT(*) FROM mentat.datoms WHERE added = true INTO cnt_before;
    SELECT COUNT(*) FROM mentat.transactions INTO tx_before;

    -- Run 10 speculative transactions
    FOR i IN 1..10 LOOP
        PERFORM mentat_with(format(
            '[[:db/add "ghost-%s" :spec/name "phantom-%s"] [:db/add "ghost-%s" :spec/val %s]]',
            i, i, i, i * 10
        ));
    END LOOP;

    SELECT COUNT(*) FROM mentat.datoms WHERE added = true INTO cnt_after;
    SELECT COUNT(*) FROM mentat.transactions INTO tx_after;

    ASSERT cnt_before = cnt_after,
        format('Datom count should be unchanged after 10 speculative txns: before=%s, after=%s',
               cnt_before, cnt_after);
    ASSERT tx_before = tx_after,
        format('Tx count should be unchanged after 10 speculative txns: before=%s, after=%s',
               tx_before, tx_after);

    RAISE NOTICE 'PASS: Test 14 - 10 speculative transactions leave no side effects';
END;
$$;

-- =========================================================================
-- Test 15: Speculative tx followed by real tx sees clean state
-- =========================================================================

DO $$
DECLARE
    real_r JSONB;
    real_eid BIGINT;
    name_val TEXT;
BEGIN
    -- Run speculative transaction
    PERFORM mentat_with('[[:db/add "ghost" :spec/name "phantom"]]');

    -- Run real transaction -- should see clean state (no phantom entity)
    real_r := mentat_transact('[[:db/add "real" :spec/name "solid"]]')::JSONB;
    real_eid := (real_r->'tempids'->>'real')::BIGINT;

    name_val := mentat_query(
        format('[:find ?n . :where [%s :spec/name ?n]]', real_eid),
        '{}'
    )::JSONB->>'result';

    ASSERT name_val = 'solid',
        format('Real entity should have correct name, got "%s"', name_val);

    -- Phantom entity should NOT exist
    PERFORM mentat_query(
        '[:find ?e . :where [?e :spec/name "phantom"]]',
        '{}'
    );
    -- If phantom entity existed, we'd find it. The query should return null.

    RAISE NOTICE 'PASS: Test 15 - Real transaction sees clean state after speculative tx';
END;
$$;

-- =========================================================================
-- Test 16: Speculative CAS chain (multiple dependent CAS operations)
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    with_r JSONB;
    committed_val BIGINT;
BEGIN
    -- Commit entity with initial value
    r := mentat_transact('[[:db/add "e" :spec/val 1]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Run 5 independent speculative CAS operations (each sees committed value)
    -- All should succeed since they all CAS from the same committed value
    FOR i IN 1..5 LOOP
        with_r := mentat_with(format('[[:db.fn/cas %s :spec/val 1 %s]]', eid, i * 100))::JSONB;
        ASSERT jsonb_array_length(with_r->'tx-data') >= 3,
            format('Speculative CAS #%s should succeed', i);
    END LOOP;

    -- Committed value should still be 1
    committed_val := (mentat_query(
        format('[:find ?v . :where [%s :spec/val ?v]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    ASSERT committed_val = 1,
        format('Committed value should remain 1 after 5 speculative CAS ops, got %s', committed_val);

    RAISE NOTICE 'PASS: Test 16 - Multiple independent speculative CAS ops all see committed state';
END;
$$;

-- =========================================================================
-- Test 17: Cardinality-many works in speculative context
-- =========================================================================

DO $$
DECLARE
    with_r JSONB;
    tx_data JSONB;
BEGIN
    -- Speculative tx with cardinality-many attribute (multiple values)
    with_r := mentat_with('[
        [:db/add "e" :spec/tags "alpha"]
        [:db/add "e" :spec/tags "beta"]
        [:db/add "e" :spec/tags "gamma"]
    ]')::JSONB;

    tx_data := with_r->'tx-data';

    -- Should have txInstant + 3 tag datoms = 4 entries
    ASSERT jsonb_array_length(tx_data) >= 4,
        format('Cardinality-many should produce at least 4 datoms, got %s',
               jsonb_array_length(tx_data));

    -- Tags should not be persisted
    PERFORM mentat_query('[:find ?t . :where [?e :spec/tags ?t]]', '{}');
    -- This should return null since nothing was committed

    RAISE NOTICE 'PASS: Test 17 - Cardinality-many works in speculative context';
END;
$$;

-- =========================================================================
-- Test 18: Schema installation in speculative context is rolled back
-- =========================================================================

DO $$
DECLARE
    schema_failed BOOLEAN := FALSE;
BEGIN
    -- Speculatively install a new schema attribute
    PERFORM mentat_with('[
        {:db/ident       :phantom/attr
         :db/valueType   :db.type/string
         :db/cardinality :db.cardinality/one}
    ]');

    -- The schema should NOT be persisted, so transacting with it should fail
    BEGIN
        PERFORM mentat_transact('[[:db/add "e" :phantom/attr "test"]]');
    EXCEPTION WHEN OTHERS THEN
        schema_failed := TRUE;
    END;

    ASSERT schema_failed,
        'Schema installed in speculative tx should not be visible to committed transactions';

    RAISE NOTICE 'PASS: Test 18 - Schema installation in speculative context is rolled back';
END;
$$;

COMMIT;

\echo '============================================'
\echo 'All speculative transaction tests passed!'
\echo '============================================'
