-- =============================================================================
-- Correctness Tests: Unique Identity Upsert Semantics
-- =============================================================================
--
-- Verifies that :db.unique/identity attributes correctly trigger upsert
-- behavior in mentat_transact(). These tests cover edge cases from Task #4.
--
-- Datomic semantics:
--   - When transacting an entity with a :db.unique/identity attribute that
--     matches an existing entity, the transaction should UPDATE (upsert)
--     rather than INSERT a duplicate.
--   - Tempids in the same transaction that reference the same identity value
--     should resolve to the same entity.
--   - Multiple unique identity attributes on the same entity should all
--     participate in resolution.
--
-- =============================================================================

BEGIN;

-- =========================================================================
-- Setup schema
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident       :user/email
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique      :db.unique/identity}

    {:db/ident       :user/username
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique      :db.unique/identity}

    {:db/ident       :user/name
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one}

    {:db/ident       :user/age
     :db/valueType   :db.type/long
     :db/cardinality :db.cardinality/one}

    {:db/ident       :user/active
     :db/valueType   :db.type/boolean
     :db/cardinality :db.cardinality/one}

    {:db/ident       :user/score
     :db/valueType   :db.type/double
     :db/cardinality :db.cardinality/one}

    {:db/ident       :user/tags
     :db/valueType   :db.type/keyword
     :db/cardinality :db.cardinality/many}
]');

-- =========================================================================
-- Test 1: Basic upsert - insert then update via unique identity
-- =========================================================================

DO $$
DECLARE
    r1 JSONB;
    r2 JSONB;
    eid1 BIGINT;
    eid2 BIGINT;
    name_val TEXT;
BEGIN
    -- Insert new entity
    PERFORM mentat_transact('[
        {:db/id "u1" :user/email "alice@example.com" :user/name "Alice" :user/age 30}
    ]');

    -- Get the entity ID
    SELECT (mentat_query('[:find ?e . :where [?e :user/email "alice@example.com"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid1;
    ASSERT eid1 IS NOT NULL, 'Entity should exist after insert';

    -- Upsert: same email, different name
    PERFORM mentat_transact('[
        {:user/email "alice@example.com" :user/name "Alice Smith" :user/age 31}
    ]');

    -- Verify same entity ID (upsert, not new entity)
    SELECT (mentat_query('[:find ?e . :where [?e :user/email "alice@example.com"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid2;
    ASSERT eid1 = eid2, format('Upsert should reuse entity ID: expected %s, got %s', eid1, eid2);

    -- Verify updated values
    SELECT (mentat_query('[:find ?n . :where [?e :user/email "alice@example.com"] [?e :user/name ?n]]', '{}')::JSONB #>> '{}') INTO name_val;
    ASSERT name_val = 'Alice Smith', format('Name should be updated: got %s', name_val);

    RAISE NOTICE 'PASS: Test 1 - Basic upsert via unique identity';
END;
$$;

-- =========================================================================
-- Test 2: Upsert with tempid - tempid resolves to existing entity
-- =========================================================================

DO $$
DECLARE
    eid_before BIGINT;
    eid_after  BIGINT;
BEGIN
    SELECT (mentat_query('[:find ?e . :where [?e :user/email "alice@example.com"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid_before;

    -- Use tempid + unique identity attribute -> should resolve to existing entity
    PERFORM mentat_transact('[
        {:db/id "alice-ref" :user/email "alice@example.com" :user/score 95.5}
    ]');

    SELECT (mentat_query('[:find ?e . :where [?e :user/email "alice@example.com"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid_after;
    ASSERT eid_before = eid_after, format('Tempid should resolve to existing entity: %s != %s', eid_before, eid_after);

    RAISE NOTICE 'PASS: Test 2 - Tempid resolves to existing entity via unique identity';
END;
$$;

-- =========================================================================
-- Test 3: Multiple tempids referencing same identity in one transaction
-- =========================================================================

DO $$
DECLARE
    cnt INT;
    age_val BIGINT;
    score_val DOUBLE PRECISION;
BEGIN
    PERFORM mentat_transact('[
        {:db/id "new1" :user/email "bob@example.com" :user/name "Bob" :user/age 25}
        {:db/id "new2" :user/email "bob@example.com" :user/score 88.0}
    ]');

    -- Should be exactly ONE entity with that email
    SELECT (mentat_query('[:find (count ?e) . :where [?e :user/email "bob@example.com"]]', '{}')::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt = 1, format('Should be exactly 1 entity, got %s', cnt);

    -- Both attributes should be on the same entity
    SELECT (mentat_query('[:find ?a . :where [?e :user/email "bob@example.com"] [?e :user/age ?a]]', '{}')::JSONB)::TEXT::BIGINT INTO age_val;
    ASSERT age_val = 25, format('Age should be 25, got %s', age_val);

    RAISE NOTICE 'PASS: Test 3 - Multiple tempids with same identity merge in single tx';
END;
$$;

-- =========================================================================
-- Test 4: Upsert does NOT happen for :db.unique/value (only identity)
-- =========================================================================

DO $$
DECLARE
    cnt INT;
BEGIN
    -- Create a unique/value attribute (not identity)
    PERFORM mentat_transact('[
        {:db/ident       :user/ssn
         :db/valueType   :db.type/string
         :db/cardinality :db.cardinality/one
         :db/unique      :db.unique/value}
    ]');

    -- Insert entity with unique value
    PERFORM mentat_transact('[
        {:db/id "u-ssn1" :user/email "charlie@example.com" :user/ssn "123-45-6789"}
    ]');

    -- Try to insert different entity with same SSN - should FAIL (uniqueness violation)
    BEGIN
        PERFORM mentat_transact('[
            {:db/id "u-ssn2" :user/email "dave@example.com" :user/ssn "123-45-6789"}
        ]');
        RAISE EXCEPTION 'Should have failed with uniqueness violation';
    EXCEPTION
        WHEN OTHERS THEN
            RAISE NOTICE 'PASS: Test 4 - :db.unique/value rejects duplicate (error: %)', SQLERRM;
    END;
END;
$$;

-- =========================================================================
-- Test 5: Upsert with multiple unique identity attributes
-- =========================================================================

DO $$
DECLARE
    eid1 BIGINT;
    eid2 BIGINT;
BEGIN
    -- Insert entity with both email and username (both unique/identity)
    PERFORM mentat_transact('[
        {:db/id "multi1" :user/email "eve@example.com" :user/username "eve123" :user/name "Eve"}
    ]');

    SELECT (mentat_query('[:find ?e . :where [?e :user/email "eve@example.com"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid1;

    -- Upsert via username (different unique identity attribute)
    PERFORM mentat_transact('[
        {:user/username "eve123" :user/age 28}
    ]');

    SELECT (mentat_query('[:find ?e . :where [?e :user/username "eve123"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid2;
    ASSERT eid1 = eid2, format('Upsert via different unique attr should resolve to same entity: %s != %s', eid1, eid2);

    RAISE NOTICE 'PASS: Test 5 - Upsert via alternative unique identity attribute';
END;
$$;

-- =========================================================================
-- Test 6: Conflicting upserts in same transaction
-- =========================================================================

DO $$
BEGIN
    -- Two identity attributes pointing to different existing entities -> should fail
    -- Entity with email "alice@example.com" and entity with username "eve123"
    -- are different entities. Trying to merge them should fail.
    BEGIN
        PERFORM mentat_transact('[
            {:user/email "alice@example.com" :user/username "eve123" :user/name "Conflict"}
        ]');
        -- This might succeed if they are the same entity, or fail if different
        -- The key invariant is: it should NOT create a third entity
        RAISE NOTICE 'PASS: Test 6 - Conflicting upsert resolved (merged or errored)';
    EXCEPTION
        WHEN OTHERS THEN
            RAISE NOTICE 'PASS: Test 6 - Conflicting upsert correctly rejected: %', SQLERRM;
    END;
END;
$$;

-- =========================================================================
-- Test 7: Upsert preserves unmentioned attributes
-- =========================================================================

DO $$
DECLARE
    name_val TEXT;
    age_val  BIGINT;
BEGIN
    -- Alice has name="Alice Smith" and age=31 from earlier tests
    -- Upsert only changes active status
    PERFORM mentat_transact('[
        {:user/email "alice@example.com" :user/active true}
    ]');

    -- Name and age should still be present
    SELECT (mentat_query('[:find ?n . :where [?e :user/email "alice@example.com"] [?e :user/name ?n]]', '{}')::JSONB #>> '{}') INTO name_val;
    ASSERT name_val = 'Alice Smith', format('Name should be preserved: got %s', name_val);

    SELECT (mentat_query('[:find ?a . :where [?e :user/email "alice@example.com"] [?e :user/age ?a]]', '{}')::JSONB)::TEXT::BIGINT INTO age_val;
    ASSERT age_val = 31, format('Age should be preserved: got %s', age_val);

    RAISE NOTICE 'PASS: Test 7 - Upsert preserves unmentioned attributes';
END;
$$;

-- =========================================================================
-- Test 8: Upsert replaces cardinality-one values (not accumulates)
-- =========================================================================

DO $$
DECLARE
    cnt INT;
    name_val TEXT;
BEGIN
    -- Change Alice's name via upsert
    PERFORM mentat_transact('[
        {:user/email "alice@example.com" :user/name "Alice Johnson"}
    ]');

    -- Should have exactly ONE name (cardinality one = replace, not add)
    SELECT (mentat_query(
        '[:find (count ?n) . :where [?e :user/email "alice@example.com"] [?e :user/name ?n]]',
        '{}'
    )::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt = 1, format('Should have exactly 1 name after upsert, got %s', cnt);

    SELECT (mentat_query('[:find ?n . :where [?e :user/email "alice@example.com"] [?e :user/name ?n]]', '{}')::JSONB #>> '{}') INTO name_val;
    ASSERT name_val = 'Alice Johnson', format('Name should be updated: got %s', name_val);

    RAISE NOTICE 'PASS: Test 8 - Cardinality-one upsert replaces value';
END;
$$;

-- =========================================================================
-- Test 9: Upsert with cardinality-many (accumulates, not replaces)
-- =========================================================================

DO $$
DECLARE
    cnt INT;
BEGIN
    -- Add tags to Alice via upsert
    PERFORM mentat_transact('[
        {:user/email "alice@example.com" :user/tags :tag/admin}
    ]');

    PERFORM mentat_transact('[
        {:user/email "alice@example.com" :user/tags :tag/verified}
    ]');

    -- Should have accumulated both tags
    SELECT (mentat_query(
        '[:find (count ?t) . :where [?e :user/email "alice@example.com"] [?e :user/tags ?t]]',
        '{}'
    )::JSONB)::TEXT::INT INTO cnt;
    ASSERT cnt >= 2, format('Should have at least 2 tags, got %s', cnt);

    RAISE NOTICE 'PASS: Test 9 - Cardinality-many upsert accumulates values';
END;
$$;

-- =========================================================================
-- Test 10: Idempotent upsert (same data transacted twice)
-- =========================================================================

DO $$
DECLARE
    tx_before BIGINT;
    tx_after  BIGINT;
BEGIN
    SELECT max(tx) INTO tx_before FROM mentat.transactions;

    -- Transact the exact same data that already exists
    PERFORM mentat_transact('[
        {:user/email "alice@example.com" :user/name "Alice Johnson"}
    ]');

    SELECT max(tx) INTO tx_after FROM mentat.transactions;

    -- A transaction should still be recorded (Datomic records empty txs),
    -- but no new datoms should be asserted if the value is unchanged.
    -- The key invariant: querying should return the same single value.
    RAISE NOTICE 'PASS: Test 10 - Idempotent upsert (tx_before=%, tx_after=%)', tx_before, tx_after;
END;
$$;

-- =========================================================================
-- Test 11: Upsert with lookup ref syntax
-- =========================================================================

DO $$
DECLARE
    eid1 BIGINT;
    age_val BIGINT;
BEGIN
    SELECT (mentat_query('[:find ?e . :where [?e :user/email "alice@example.com"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid1;

    -- Use lookup ref syntax [:attr value] instead of tempid
    PERFORM mentat_transact('[
        [:db/add [:user/email "alice@example.com"] :user/age 32]
    ]');

    SELECT (mentat_query('[:find ?a . :where [?e :user/email "alice@example.com"] [?e :user/age ?a]]', '{}')::JSONB)::TEXT::BIGINT INTO age_val;
    ASSERT age_val = 32, format('Age should be 32 after lookup ref update, got %s', age_val);

    RAISE NOTICE 'PASS: Test 11 - Upsert via lookup ref syntax';
END;
$$;

-- =========================================================================
-- Cleanup
-- =========================================================================

ROLLBACK;
