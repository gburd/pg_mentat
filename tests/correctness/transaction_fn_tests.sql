-- =============================================================================
-- Correctness Tests: Transaction Functions
-- =============================================================================
--
-- Verifies the built-in transaction function infrastructure from Task #5:
--   - :db.fn/cas with all namespace variants (:db/cas, :db.fn/cas)
--   - :db.fn/retractEntity with namespace variants
--   - mentat.transaction_fns() discovery API returns correct JSON
--   - CAS with retry logic integration
--   - Transaction functions in speculative context
--
-- Datomic semantics:
--   Transaction functions are special forms within a transaction that
--   execute logic atomically. The two built-in functions are:
--   - :db.fn/cas (compare-and-swap): atomically update if current = expected
--   - :db.fn/retractEntity: retract all datoms for an entity
--
-- =============================================================================

BEGIN;

-- =========================================================================
-- Setup: Schema for transaction function tests
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident       :txfn/name
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one}

    {:db/ident       :txfn/counter
     :db/valueType   :db.type/long
     :db/cardinality :db.cardinality/one}

    {:db/ident       :txfn/label
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one}

    {:db/ident       :txfn/tags
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/many}

    {:db/ident       :txfn/uid
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique      :db.unique/identity}
]');

-- =========================================================================
-- Test 1: :db.fn/cas basic success
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    val BIGINT;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/counter 10]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- CAS 10 -> 20
    PERFORM mentat_transact(format(
        '[[:db.fn/cas %s :txfn/counter 10 20]]', eid
    ));

    val := (mentat_query(
        format('[:find ?v . :where [%s :txfn/counter ?v]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    ASSERT val = 20, format('CAS should update value to 20, got %s', val);

    RAISE NOTICE 'PASS: Test 1 - :db.fn/cas basic success';
END;
$$;

-- =========================================================================
-- Test 2: :db/cas short namespace variant works identically
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    val BIGINT;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/counter 5]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Use :db/cas (short namespace) instead of :db.fn/cas
    PERFORM mentat_transact(format(
        '[[:db/cas %s :txfn/counter 5 15]]', eid
    ));

    val := (mentat_query(
        format('[:find ?v . :where [%s :txfn/counter ?v]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    ASSERT val = 15, format(':db/cas should update value to 15, got %s', val);

    RAISE NOTICE 'PASS: Test 2 - :db/cas short namespace works identically';
END;
$$;

-- =========================================================================
-- Test 3: CAS from nil (attribute not yet set)
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    val BIGINT;
BEGIN
    -- Create entity with name but no counter
    r := mentat_transact('[[:db/add "e" :txfn/name "nil-cas"]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- CAS from nil to 42
    PERFORM mentat_transact(format(
        '[[:db.fn/cas %s :txfn/counter nil 42]]', eid
    ));

    val := (mentat_query(
        format('[:find ?v . :where [%s :txfn/counter ?v]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    ASSERT val = 42, format('CAS from nil should set value to 42, got %s', val);

    RAISE NOTICE 'PASS: Test 3 - CAS from nil succeeds';
END;
$$;

-- =========================================================================
-- Test 4: CAS failure with wrong old value
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    val BIGINT;
    cas_failed BOOLEAN := FALSE;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/counter 100]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- CAS with wrong old value
    BEGIN
        PERFORM mentat_transact(format(
            '[[:db.fn/cas %s :txfn/counter 999 200]]', eid
        ));
    EXCEPTION WHEN OTHERS THEN
        cas_failed := TRUE;
    END;

    ASSERT cas_failed, 'CAS with wrong old value should fail';

    -- Value should be unchanged
    val := (mentat_query(
        format('[:find ?v . :where [%s :txfn/counter ?v]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    ASSERT val = 100, format('Value should remain 100 after failed CAS, got %s', val);

    RAISE NOTICE 'PASS: Test 4 - CAS with wrong old value fails correctly';
END;
$$;

-- =========================================================================
-- Test 5: CAS produces correct tx-data (retract + assert)
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    cas_r JSONB;
    tx_data JSONB;
    retract_count INT := 0;
    assert_count INT := 0;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/counter 10]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    cas_r := mentat_transact(format(
        '[[:db.fn/cas %s :txfn/counter 10 20]]', eid
    ))::JSONB;

    tx_data := cas_r->'tx-data';

    -- Count retractions and assertions for the entity (skip txInstant)
    FOR i IN 0..jsonb_array_length(tx_data) - 1 LOOP
        IF (tx_data->i->>0)::BIGINT = eid THEN
            IF (tx_data->i->>4)::BOOLEAN = false THEN
                retract_count := retract_count + 1;
            ELSE
                assert_count := assert_count + 1;
            END IF;
        END IF;
    END LOOP;

    ASSERT retract_count = 1,
        format('CAS should produce exactly 1 retraction for entity, got %s', retract_count);
    ASSERT assert_count = 1,
        format('CAS should produce exactly 1 assertion for entity, got %s', assert_count);

    RAISE NOTICE 'PASS: Test 5 - CAS produces correct retract + assert tx-data';
END;
$$;

-- =========================================================================
-- Test 6: :db.fn/retractEntity basic functionality
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    result_val TEXT;
BEGIN
    -- Create entity with multiple attributes
    r := mentat_transact('[
        [:db/add "e" :txfn/name "will-be-retracted"]
        [:db/add "e" :txfn/counter 99]
        [:db/add "e" :txfn/label "doomed"]
    ]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Retract the entity
    PERFORM mentat_transact(format(
        '[[:db.fn/retractEntity %s]]', eid
    ));

    -- All attributes should be gone
    result_val := mentat_query(
        format('[:find ?n . :where [%s :txfn/name ?n]]', eid),
        '{}'
    )::JSONB->>'result';

    ASSERT result_val IS NULL,
        format('Name should be null after retractEntity, got "%s"', result_val);

    result_val := mentat_query(
        format('[:find ?v . :where [%s :txfn/counter ?v]]', eid),
        '{}'
    )::JSONB->>'result';

    ASSERT result_val IS NULL,
        format('Counter should be null after retractEntity, got "%s"', result_val);

    RAISE NOTICE 'PASS: Test 6 - :db.fn/retractEntity removes all attributes';
END;
$$;

-- =========================================================================
-- Test 7: :db/retractEntity original namespace still works
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    result_val TEXT;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/name "retract-original"]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Use the original :db/retractEntity namespace
    PERFORM mentat_transact(format(
        '[[:db/retractEntity %s]]', eid
    ));

    result_val := mentat_query(
        format('[:find ?n . :where [%s :txfn/name ?n]]', eid),
        '{}'
    )::JSONB->>'result';

    ASSERT result_val IS NULL,
        ':db/retractEntity should retract the entity';

    RAISE NOTICE 'PASS: Test 7 - :db/retractEntity original namespace works';
END;
$$;

-- =========================================================================
-- Test 8: retractEntity produces retraction datoms in tx-data
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    retract_r JSONB;
    tx_data JSONB;
    retraction_count INT := 0;
BEGIN
    r := mentat_transact('[
        [:db/add "e" :txfn/name "retract-datoms"]
        [:db/add "e" :txfn/counter 7]
    ]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    retract_r := mentat_transact(format(
        '[[:db.fn/retractEntity %s]]', eid
    ))::JSONB;

    tx_data := retract_r->'tx-data';

    -- Count retraction datoms for the entity
    FOR i IN 0..jsonb_array_length(tx_data) - 1 LOOP
        IF (tx_data->i->>0)::BIGINT = eid AND (tx_data->i->>4)::BOOLEAN = false THEN
            retraction_count := retraction_count + 1;
        END IF;
    END LOOP;

    ASSERT retraction_count >= 2,
        format('retractEntity should produce at least 2 retractions (name + counter), got %s',
               retraction_count);

    RAISE NOTICE 'PASS: Test 8 - retractEntity produces retraction datoms in tx-data';
END;
$$;

-- =========================================================================
-- Test 9: retractEntity on cardinality-many retracts all values
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    tag_count BIGINT;
BEGIN
    -- Create entity with multiple tags
    r := mentat_transact('[
        [:db/add "e" :txfn/tags "alpha"]
        [:db/add "e" :txfn/tags "beta"]
        [:db/add "e" :txfn/tags "gamma"]
    ]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Verify tags exist
    tag_count := (mentat_query(
        format('[:find (count ?t) . :where [%s :txfn/tags ?t]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    ASSERT tag_count = 3,
        format('Should have 3 tags before retract, got %s', tag_count);

    -- Retract entity
    PERFORM mentat_transact(format('[[:db.fn/retractEntity %s]]', eid));

    -- All tags should be gone
    tag_count := (mentat_query(
        format('[:find (count ?t) . :where [%s :txfn/tags ?t]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    -- count returns NULL when there are no matches
    ASSERT tag_count IS NULL OR tag_count = 0,
        format('All tags should be retracted, got count=%s', tag_count);

    RAISE NOTICE 'PASS: Test 9 - retractEntity retracts all cardinality-many values';
END;
$$;

-- =========================================================================
-- Test 10: mentat.transaction_fns() returns valid JSON array
-- =========================================================================

DO $$
DECLARE
    result JSONB;
    arr_len INT;
BEGIN
    result := mentat.transaction_fns()::JSONB;

    ASSERT jsonb_typeof(result) = 'array',
        'transaction_fns() should return a JSON array';

    arr_len := jsonb_array_length(result);
    ASSERT arr_len = 2,
        format('Should list 2 built-in functions, got %s', arr_len);

    RAISE NOTICE 'PASS: Test 10 - transaction_fns() returns valid JSON array with 2 entries';
END;
$$;

-- =========================================================================
-- Test 11: transaction_fns() lists :db.fn/cas with correct metadata
-- =========================================================================

DO $$
DECLARE
    result JSONB;
    cas_fn JSONB := NULL;
    fn_name TEXT;
    fn_args TEXT;
    fn_desc TEXT;
BEGIN
    result := mentat.transaction_fns()::JSONB;

    -- Find :db.fn/cas
    FOR i IN 0..jsonb_array_length(result) - 1 LOOP
        IF result->i->>'name' = ':db.fn/cas' THEN
            cas_fn := result->i;
        END IF;
    END LOOP;

    ASSERT cas_fn IS NOT NULL, ':db.fn/cas should be listed';

    fn_name := cas_fn->>'name';
    fn_args := cas_fn->>'args';
    fn_desc := cas_fn->>'description';

    ASSERT fn_name = ':db.fn/cas', format('Name should be :db.fn/cas, got %s', fn_name);
    ASSERT fn_args LIKE '%old-value%', format('Args should mention old-value, got %s', fn_args);
    ASSERT fn_desc LIKE '%Compare-and-swap%',
        format('Description should mention Compare-and-swap, got %s', fn_desc);

    RAISE NOTICE 'PASS: Test 11 - transaction_fns() lists :db.fn/cas with correct metadata';
END;
$$;

-- =========================================================================
-- Test 12: transaction_fns() lists :db.fn/retractEntity
-- =========================================================================

DO $$
DECLARE
    result JSONB;
    retract_fn JSONB := NULL;
BEGIN
    result := mentat.transaction_fns()::JSONB;

    FOR i IN 0..jsonb_array_length(result) - 1 LOOP
        IF result->i->>'name' = ':db.fn/retractEntity' THEN
            retract_fn := result->i;
        END IF;
    END LOOP;

    ASSERT retract_fn IS NOT NULL, ':db.fn/retractEntity should be listed';
    ASSERT retract_fn ? 'args', 'retractEntity entry should have args';
    ASSERT retract_fn ? 'description', 'retractEntity entry should have description';

    RAISE NOTICE 'PASS: Test 12 - transaction_fns() lists :db.fn/retractEntity';
END;
$$;

-- =========================================================================
-- Test 13: CAS in speculative context via :db.fn/cas
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    with_r JSONB;
    committed_val BIGINT;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/counter 50]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Speculative CAS
    with_r := mentat_with(format('[[:db.fn/cas %s :txfn/counter 50 75]]', eid))::JSONB;

    -- Should succeed and return tx-data
    ASSERT jsonb_array_length(with_r->'tx-data') >= 3,
        'Speculative CAS should produce tx-data';

    -- Committed value unchanged
    committed_val := (mentat_query(
        format('[:find ?v . :where [%s :txfn/counter ?v]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    ASSERT committed_val = 50,
        format('Committed value should remain 50 after speculative CAS, got %s', committed_val);

    RAISE NOTICE 'PASS: Test 13 - CAS works in speculative context via :db.fn/cas';
END;
$$;

-- =========================================================================
-- Test 14: CAS in speculative context via :db/cas (short namespace)
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    with_r JSONB;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/counter 30]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Speculative CAS with short namespace
    with_r := mentat_with(format('[[:db/cas %s :txfn/counter 30 60]]', eid))::JSONB;

    ASSERT jsonb_array_length(with_r->'tx-data') >= 3,
        ':db/cas should work in speculative context';

    RAISE NOTICE 'PASS: Test 14 - CAS works in speculative context via :db/cas';
END;
$$;

-- =========================================================================
-- Test 15: retractEntity in speculative context
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    with_r JSONB;
    retraction_count INT := 0;
    tx_data JSONB;
    committed_name TEXT;
BEGIN
    r := mentat_transact('[
        [:db/add "e" :txfn/name "speculative-retract"]
        [:db/add "e" :txfn/counter 7]
    ]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Speculative retractEntity
    with_r := mentat_with(format('[[:db.fn/retractEntity %s]]', eid))::JSONB;
    tx_data := with_r->'tx-data';

    FOR i IN 0..jsonb_array_length(tx_data) - 1 LOOP
        IF (tx_data->i->>4)::BOOLEAN = false THEN
            retraction_count := retraction_count + 1;
        END IF;
    END LOOP;

    ASSERT retraction_count >= 2,
        format('Speculative retractEntity should produce retractions, got %s', retraction_count);

    -- Entity should still exist
    committed_name := mentat_query(
        format('[:find ?n . :where [%s :txfn/name ?n]]', eid),
        '{}'
    )::JSONB->>'result';

    ASSERT committed_name = 'speculative-retract',
        'Entity should still exist after speculative retractEntity';

    RAISE NOTICE 'PASS: Test 15 - retractEntity works in speculative context';
END;
$$;

-- =========================================================================
-- Test 16: CAS chain -- sequential committed CAS operations
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    val BIGINT;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/counter 0]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Chain of CAS: 0->1, 1->2, 2->3, 3->4, 4->5
    FOR i IN 0..4 LOOP
        PERFORM mentat_transact(format(
            '[[:db.fn/cas %s :txfn/counter %s %s]]', eid, i, i + 1
        ));
    END LOOP;

    val := (mentat_query(
        format('[:find ?v . :where [%s :txfn/counter ?v]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    ASSERT val = 5, format('Counter should be 5 after CAS chain, got %s', val);

    RAISE NOTICE 'PASS: Test 16 - CAS chain works correctly (0 -> 5)';
END;
$$;

-- =========================================================================
-- Test 17: CAS and regular assertions in the same transaction
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    val BIGINT;
    name_val TEXT;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/counter 10] [:db/add "e" :txfn/name "mixed"]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Mix CAS with regular assertion in same tx
    PERFORM mentat_transact(format('[
        [:db.fn/cas %s :txfn/counter 10 20]
        [:db/add %s :txfn/label "updated"]
    ]', eid, eid));

    val := (mentat_query(
        format('[:find ?v . :where [%s :txfn/counter ?v]]', eid),
        '{}'
    )::JSONB->>'result')::BIGINT;

    name_val := mentat_query(
        format('[:find ?l . :where [%s :txfn/label ?l]]', eid),
        '{}'
    )::JSONB->>'result';

    ASSERT val = 20, format('Counter should be 20 after CAS, got %s', val);
    ASSERT name_val = 'updated', format('Label should be "updated", got "%s"', name_val);

    RAISE NOTICE 'PASS: Test 17 - CAS and regular assertions work in same transaction';
END;
$$;

-- =========================================================================
-- Test 18: CAS on string attribute (not just long)
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    val TEXT;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/name "original"]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- CAS on a string attribute
    PERFORM mentat_transact(format(
        '[[:db.fn/cas %s :txfn/name "original" "updated"]]', eid
    ));

    val := mentat_query(
        format('[:find ?n . :where [%s :txfn/name ?n]]', eid),
        '{}'
    )::JSONB->>'result';

    ASSERT val = 'updated', format('Name should be "updated" after CAS, got "%s"', val);

    RAISE NOTICE 'PASS: Test 18 - CAS works on string attributes';
END;
$$;

-- =========================================================================
-- Test 19: retractEntity then re-assert creates fresh entity
-- =========================================================================

DO $$
DECLARE
    r JSONB;
    eid BIGINT;
    new_r JSONB;
    new_eid BIGINT;
    val TEXT;
BEGIN
    r := mentat_transact('[[:db/add "e" :txfn/uid "reborn"] [:db/add "e" :txfn/name "original"]]')::JSONB;
    eid := (r->'tempids'->>'e')::BIGINT;

    -- Retract the entity
    PERFORM mentat_transact(format('[[:db.fn/retractEntity %s]]', eid));

    -- Re-assert with same unique value should create new entity (or upsert to same ID)
    new_r := mentat_transact('[[:db/add "f" :txfn/uid "reborn"] [:db/add "f" :txfn/name "reborn-name"]]')::JSONB;
    new_eid := (new_r->'tempids'->>'f')::BIGINT;

    val := mentat_query(
        format('[:find ?n . :where [%s :txfn/name ?n]]', new_eid),
        '{}'
    )::JSONB->>'result';

    ASSERT val = 'reborn-name',
        format('Re-asserted entity should have name "reborn-name", got "%s"', val);

    RAISE NOTICE 'PASS: Test 19 - retractEntity followed by re-assertion works';
END;
$$;

-- =========================================================================
-- Test 20: Transaction function discovery API is idempotent
-- =========================================================================

DO $$
DECLARE
    r1 JSONB;
    r2 JSONB;
BEGIN
    r1 := mentat.transaction_fns()::JSONB;
    r2 := mentat.transaction_fns()::JSONB;

    ASSERT r1 = r2,
        'transaction_fns() should return identical results on repeated calls';

    RAISE NOTICE 'PASS: Test 20 - transaction_fns() is idempotent';
END;
$$;

COMMIT;

\echo '============================================'
\echo 'All transaction function tests passed!'
\echo '============================================'
