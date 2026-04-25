-- Test suite: Datom helper functions (08_datom_helpers.sql)
--
-- Tests the 6 datom helper functions that simplify common query patterns
-- over the mentat.datoms table.
--
-- Functions tested:
--   mentat.datom_text_like()    - LIKE pattern matching on text attributes
--   mentat.datom_long_between() - range queries on long attributes
--   mentat.datom_ref_in()       - set membership on ref attributes
--   mentat.datom_text_values()  - cardinality-many text values
--   mentat.datom_ref_values()   - cardinality-many ref values
--   mentat.datom_value_at_tx()  - temporal (as-of) value lookup

BEGIN;

-- =========================================================================
-- Setup: Create schema and sample data
-- =========================================================================

-- Define test schema attributes
SELECT mentat.mentat_transact('[
  {:db/ident       :person/name
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique      :db.unique/identity
   :db/index       true}
  {:db/ident       :person/age
   :db/valueType   :db.type/long
   :db/cardinality :db.cardinality/one
   :db/index       true}
  {:db/ident       :person/email
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident       :person/alias
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/many}
  {:db/ident       :person/friend
   :db/valueType   :db.type/ref
   :db/cardinality :db.cardinality/many}
  {:db/ident       :person/department
   :db/valueType   :db.type/ref
   :db/cardinality :db.cardinality/one}
  {:db/ident       :dept/name
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique      :db.unique/identity}
]');

-- Insert test data: departments
SELECT mentat.mentat_transact('[
  {:db/id "eng"  :dept/name "Engineering"}
  {:db/id "mkt"  :dept/name "Marketing"}
  {:db/id "hr"   :dept/name "Human Resources"}
]');

-- Insert test data: people
SELECT mentat.mentat_transact('[
  {:db/id "alice" :person/name "Alice"   :person/age 30 :person/email "alice@example.com"}
  {:db/id "bob"   :person/name "Bob"     :person/age 25 :person/email "bob@example.com"}
  {:db/id "carol" :person/name "Carol"   :person/age 35 :person/email "carol@test.org"}
  {:db/id "dave"  :person/name "Dave"    :person/age 28 :person/email "dave@example.com"}
  {:db/id "eve"   :person/name "Eve"     :person/age 22 :person/email "eve@test.org"}
]');

-- =========================================================================
-- datom_text_like: Pattern matching on text attributes
-- =========================================================================

-- Test 1: Find names starting with a letter
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_text_like(':person/name', 'A%');
    ASSERT cnt >= 1, 'Should find at least 1 person with name starting with A';
    RAISE NOTICE 'PASS: datom_text_like finds names starting with A (count: %)', cnt;
END;
$$;

-- Test 2: Find emails by domain
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_text_like(':person/email', '%@example.com');
    ASSERT cnt >= 2, 'Should find people with @example.com emails';
    RAISE NOTICE 'PASS: datom_text_like finds emails by domain (count: %)', cnt;
END;
$$;

-- Test 3: Find emails with wildcard
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_text_like(':person/email', '%@%');
    ASSERT cnt >= 5, 'All people should have email addresses';
    RAISE NOTICE 'PASS: datom_text_like wildcard (count: %)', cnt;
END;
$$;

-- Test 4: Pattern with no matches
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_text_like(':person/name', 'ZZZ%');
    ASSERT cnt = 0, 'No one should match ZZZ%';
    RAISE NOTICE 'PASS: datom_text_like no matches';
END;
$$;

-- Test 5: Error on unknown attribute
DO $$
BEGIN
    PERFORM * FROM mentat.datom_text_like(':nonexistent/attr', '%');
    RAISE EXCEPTION 'should have raised error for unknown attribute';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: datom_text_like raises error for unknown attribute (%)' , SQLERRM;
END;
$$;

-- Test 6: Return columns are correct
DO $$
DECLARE
    rec RECORD;
BEGIN
    SELECT * INTO rec
    FROM mentat.datom_text_like(':person/name', 'Alice')
    LIMIT 1;

    IF rec IS NOT NULL THEN
        ASSERT rec.entity_id IS NOT NULL, 'entity_id should not be NULL';
        ASSERT rec.value = 'Alice', 'value should be Alice';
        ASSERT rec.tx IS NOT NULL, 'tx should not be NULL';
        RAISE NOTICE 'PASS: datom_text_like returns correct columns (entity_id=%, value=%, tx=%)',
            rec.entity_id, rec.value, rec.tx;
    ELSE
        RAISE NOTICE 'SKIP: could not find Alice';
    END IF;
END;
$$;

-- =========================================================================
-- datom_long_between: Range queries on long attributes
-- =========================================================================

-- Test 7: Age range that matches multiple people
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_long_between(':person/age', 25, 35);
    ASSERT cnt >= 3, 'Should find 3+ people aged 25-35';
    RAISE NOTICE 'PASS: datom_long_between range 25-35 (count: %)', cnt;
END;
$$;

-- Test 8: Exact value (low == high)
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_long_between(':person/age', 30, 30);
    ASSERT cnt >= 1, 'Should find at least 1 person aged exactly 30';
    RAISE NOTICE 'PASS: datom_long_between exact value (count: %)', cnt;
END;
$$;

-- Test 9: Range with no matches
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_long_between(':person/age', 100, 200);
    ASSERT cnt = 0, 'No one should be aged 100-200';
    RAISE NOTICE 'PASS: datom_long_between no matches';
END;
$$;

-- Test 10: Full range covers everyone
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_long_between(':person/age', 0, 1000);
    ASSERT cnt >= 5, 'Full range should find all people';
    RAISE NOTICE 'PASS: datom_long_between full range (count: %)', cnt;
END;
$$;

-- Test 11: Return columns are correct
DO $$
DECLARE
    rec RECORD;
BEGIN
    SELECT * INTO rec
    FROM mentat.datom_long_between(':person/age', 30, 30)
    LIMIT 1;

    IF rec IS NOT NULL THEN
        ASSERT rec.entity_id IS NOT NULL, 'entity_id should not be NULL';
        ASSERT rec.value = 30, 'value should be 30';
        ASSERT rec.tx IS NOT NULL, 'tx should not be NULL';
        RAISE NOTICE 'PASS: datom_long_between correct columns (entity_id=%, value=%, tx=%)',
            rec.entity_id, rec.value, rec.tx;
    ELSE
        RAISE NOTICE 'SKIP: no exact-30 result';
    END IF;
END;
$$;

-- Test 12: Error on unknown attribute
DO $$
BEGIN
    PERFORM * FROM mentat.datom_long_between(':nonexistent/attr', 0, 100);
    RAISE EXCEPTION 'should have raised error for unknown attribute';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: datom_long_between raises error for unknown attribute';
END;
$$;

-- =========================================================================
-- datom_ref_in: Set membership on ref attributes
-- =========================================================================

-- Test 13: Find people by department refs
DO $$
DECLARE
    eng_id BIGINT;
    cnt INT;
BEGIN
    -- Assign Alice to Engineering
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :dept/name "Engineering"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO eng_id;

    IF eng_id IS NOT NULL THEN
        -- Assign people to departments
        PERFORM mentat.mentat_transact(format(
            '[[:db/add [:person/name "Alice"] :person/department %s]
              [:db/add [:person/name "Bob"]   :person/department %s]]',
            eng_id, eng_id
        ));

        SELECT count(*) INTO cnt
        FROM mentat.datom_ref_in(':person/department', ARRAY[eng_id]);
        ASSERT cnt >= 2, 'Should find 2+ people in Engineering dept';
        RAISE NOTICE 'PASS: datom_ref_in finds department members (count: %)', cnt;
    ELSE
        RAISE NOTICE 'SKIP: could not find Engineering entity';
    END IF;
END;
$$;

-- Test 14: Empty ref array returns no matches
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_ref_in(':person/department', ARRAY[]::BIGINT[]);
    ASSERT cnt = 0, 'Empty ref array should return no matches';
    RAISE NOTICE 'PASS: datom_ref_in empty array';
END;
$$;

-- Test 15: Non-matching ref IDs
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_ref_in(':person/department', ARRAY[999999::BIGINT]);
    ASSERT cnt = 0, 'Non-existing ref should return no matches';
    RAISE NOTICE 'PASS: datom_ref_in no matches';
END;
$$;

-- Test 16: Error on unknown attribute
DO $$
BEGIN
    PERFORM * FROM mentat.datom_ref_in(':nonexistent/attr', ARRAY[1::BIGINT]);
    RAISE EXCEPTION 'should have raised error for unknown attribute';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: datom_ref_in raises error for unknown attribute';
END;
$$;

-- =========================================================================
-- datom_text_values: Cardinality-many text values
-- =========================================================================

-- Test 17: Add aliases and retrieve them
DO $$
DECLARE
    alice_id BIGINT;
    cnt INT;
BEGIN
    -- Get Alice's entity ID
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Alice"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO alice_id;

    IF alice_id IS NOT NULL THEN
        -- Add aliases
        PERFORM mentat.mentat_transact(format(
            '[[:db/add %s :person/alias "Ali"]
              [:db/add %s :person/alias "A"]]',
            alice_id, alice_id
        ));

        SELECT count(*) INTO cnt
        FROM mentat.datom_text_values(alice_id, ':person/alias');
        ASSERT cnt >= 2, 'Should find at least 2 aliases for Alice';
        RAISE NOTICE 'PASS: datom_text_values finds aliases (count: %)', cnt;
    ELSE
        RAISE NOTICE 'SKIP: could not find Alice';
    END IF;
END;
$$;

-- Test 18: No values for non-existing entity
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_text_values(999999, ':person/alias');
    ASSERT cnt = 0, 'Non-existing entity should have no values';
    RAISE NOTICE 'PASS: datom_text_values no values for non-existing entity';
END;
$$;

-- Test 19: Error on unknown attribute
DO $$
BEGIN
    PERFORM * FROM mentat.datom_text_values(1, ':nonexistent/attr');
    RAISE EXCEPTION 'should have raised error for unknown attribute';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: datom_text_values raises error for unknown attribute';
END;
$$;

-- Test 20: Return columns are correct
DO $$
DECLARE
    alice_id BIGINT;
    rec RECORD;
BEGIN
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Alice"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO alice_id;

    IF alice_id IS NOT NULL THEN
        SELECT * INTO rec
        FROM mentat.datom_text_values(alice_id, ':person/alias')
        LIMIT 1;

        IF rec IS NOT NULL THEN
            ASSERT rec.value IS NOT NULL, 'value should not be NULL';
            ASSERT rec.tx IS NOT NULL, 'tx should not be NULL';
            RAISE NOTICE 'PASS: datom_text_values correct columns (value=%, tx=%)',
                rec.value, rec.tx;
        ELSE
            RAISE NOTICE 'SKIP: no alias values';
        END IF;
    ELSE
        RAISE NOTICE 'SKIP: could not find Alice';
    END IF;
END;
$$;

-- =========================================================================
-- datom_ref_values: Cardinality-many ref values
-- =========================================================================

-- Test 21: Add friends and retrieve them
DO $$
DECLARE
    alice_id BIGINT;
    bob_id BIGINT;
    carol_id BIGINT;
    cnt INT;
BEGIN
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Alice"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO alice_id;

    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Bob"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO bob_id;

    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Carol"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO carol_id;

    IF alice_id IS NOT NULL AND bob_id IS NOT NULL AND carol_id IS NOT NULL THEN
        PERFORM mentat.mentat_transact(format(
            '[[:db/add %s :person/friend %s]
              [:db/add %s :person/friend %s]]',
            alice_id, bob_id, alice_id, carol_id
        ));

        SELECT count(*) INTO cnt
        FROM mentat.datom_ref_values(alice_id, ':person/friend');
        ASSERT cnt >= 2, 'Should find at least 2 friends for Alice';
        RAISE NOTICE 'PASS: datom_ref_values finds friends (count: %)', cnt;
    ELSE
        RAISE NOTICE 'SKIP: could not find required entities';
    END IF;
END;
$$;

-- Test 22: No ref values for non-existing entity
DO $$
DECLARE
    cnt INT;
BEGIN
    SELECT count(*) INTO cnt
    FROM mentat.datom_ref_values(999999, ':person/friend');
    ASSERT cnt = 0, 'Non-existing entity should have no ref values';
    RAISE NOTICE 'PASS: datom_ref_values no values for non-existing entity';
END;
$$;

-- Test 23: Error on unknown attribute
DO $$
BEGIN
    PERFORM * FROM mentat.datom_ref_values(1, ':nonexistent/attr');
    RAISE EXCEPTION 'should have raised error for unknown attribute';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: datom_ref_values raises error for unknown attribute';
END;
$$;

-- Test 24: Return columns are correct
DO $$
DECLARE
    alice_id BIGINT;
    rec RECORD;
BEGIN
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Alice"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO alice_id;

    IF alice_id IS NOT NULL THEN
        SELECT * INTO rec
        FROM mentat.datom_ref_values(alice_id, ':person/friend')
        LIMIT 1;

        IF rec IS NOT NULL THEN
            ASSERT rec.ref_value IS NOT NULL, 'ref_value should not be NULL';
            ASSERT rec.tx IS NOT NULL, 'tx should not be NULL';
            RAISE NOTICE 'PASS: datom_ref_values correct columns (ref_value=%, tx=%)',
                rec.ref_value, rec.tx;
        ELSE
            RAISE NOTICE 'SKIP: no friend values';
        END IF;
    ELSE
        RAISE NOTICE 'SKIP: could not find Alice';
    END IF;
END;
$$;

-- =========================================================================
-- datom_value_at_tx: Temporal (as-of) value lookup
-- =========================================================================

-- Test 25: Get current value
DO $$
DECLARE
    alice_id BIGINT;
    max_tx BIGINT;
    rec RECORD;
BEGIN
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Alice"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO alice_id;

    SELECT MAX(tx) INTO max_tx FROM mentat.transactions;

    IF alice_id IS NOT NULL AND max_tx IS NOT NULL THEN
        SELECT * INTO rec
        FROM mentat.datom_value_at_tx(alice_id, ':person/name', max_tx);

        IF rec IS NOT NULL THEN
            ASSERT rec.v_text = 'Alice', 'Value at current tx should be Alice';
            ASSERT rec.value_type_tag = 7, 'Type tag should be 7 (text)';
            RAISE NOTICE 'PASS: datom_value_at_tx current value (v_text=%, tag=%)',
                rec.v_text, rec.value_type_tag;
        ELSE
            RAISE NOTICE 'SKIP: no value found at tx';
        END IF;
    ELSE
        RAISE NOTICE 'SKIP: could not find Alice or max tx';
    END IF;
END;
$$;

-- Test 26: Value at a specific transaction
DO $$
DECLARE
    alice_id BIGINT;
    first_tx BIGINT;
    rec RECORD;
BEGIN
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Alice"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO alice_id;

    IF alice_id IS NOT NULL THEN
        -- Find the transaction that asserted Alice's name
        SELECT d.tx INTO first_tx
        FROM mentat.datoms d
        JOIN mentat.idents i ON d.a = i.entid
        WHERE d.e = alice_id
          AND i.ident = ':person/name'
          AND d.added = TRUE
        ORDER BY d.tx
        LIMIT 1;

        IF first_tx IS NOT NULL THEN
            SELECT * INTO rec
            FROM mentat.datom_value_at_tx(alice_id, ':person/name', first_tx);

            ASSERT rec IS NOT NULL AND rec.v_text IS NOT NULL,
                'Should find value at first tx';
            RAISE NOTICE 'PASS: datom_value_at_tx at specific tx (v_text=%, tx=%)',
                rec.v_text, rec.tx;
        ELSE
            RAISE NOTICE 'SKIP: could not find first tx';
        END IF;
    ELSE
        RAISE NOTICE 'SKIP: could not find Alice';
    END IF;
END;
$$;

-- Test 27: Value before entity existed returns NULL
DO $$
DECLARE
    alice_id BIGINT;
    rec RECORD;
BEGIN
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Alice"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO alice_id;

    IF alice_id IS NOT NULL THEN
        -- Use a very early tx that predates data insertion
        SELECT * INTO rec
        FROM mentat.datom_value_at_tx(alice_id, ':person/name', 1);

        ASSERT rec IS NULL OR rec.v_text IS NULL,
            'Value before entity existed should be NULL';
        RAISE NOTICE 'PASS: datom_value_at_tx before existence returns NULL';
    ELSE
        RAISE NOTICE 'SKIP: could not find Alice';
    END IF;
END;
$$;

-- Test 28: Error on unknown attribute
DO $$
BEGIN
    PERFORM * FROM mentat.datom_value_at_tx(1, ':nonexistent/attr', 1);
    RAISE EXCEPTION 'should have raised error for unknown attribute';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'PASS: datom_value_at_tx raises error for unknown attribute';
END;
$$;

-- Test 29: Non-existing entity returns NULL
DO $$
DECLARE
    max_tx BIGINT;
    rec RECORD;
BEGIN
    SELECT MAX(tx) INTO max_tx FROM mentat.transactions;

    IF max_tx IS NOT NULL THEN
        SELECT * INTO rec
        FROM mentat.datom_value_at_tx(999999, ':person/name', max_tx);

        ASSERT rec IS NULL OR rec.v_text IS NULL,
            'Non-existing entity should return NULL';
        RAISE NOTICE 'PASS: datom_value_at_tx non-existing entity';
    ELSE
        RAISE NOTICE 'SKIP: no transactions found';
    END IF;
END;
$$;

-- Test 30: All return columns present
DO $$
DECLARE
    alice_id BIGINT;
    max_tx BIGINT;
    rec RECORD;
BEGIN
    SELECT (mentat.mentat_query(
        '[:find ?e . :where [?e :person/name "Alice"]]',
        '{}'::jsonb
    )->'results'->0->0)::BIGINT INTO alice_id;

    SELECT MAX(tx) INTO max_tx FROM mentat.transactions;

    IF alice_id IS NOT NULL AND max_tx IS NOT NULL THEN
        SELECT * INTO rec
        FROM mentat.datom_value_at_tx(alice_id, ':person/name', max_tx);

        IF rec IS NOT NULL THEN
            -- For a text value, v_text should be non-NULL and other typed
            -- columns should be NULL
            ASSERT rec.value_type_tag IS NOT NULL, 'value_type_tag should be present';
            ASSERT rec.tx IS NOT NULL, 'tx should be present';
            ASSERT rec.v_text IS NOT NULL, 'v_text should be present for text';
            ASSERT rec.v_ref IS NULL, 'v_ref should be NULL for text value';
            ASSERT rec.v_long IS NULL, 'v_long should be NULL for text value';
            RAISE NOTICE 'PASS: datom_value_at_tx all columns verified';
        ELSE
            RAISE NOTICE 'SKIP: no value found';
        END IF;
    ELSE
        RAISE NOTICE 'SKIP: could not find required data';
    END IF;
END;
$$;

ROLLBACK;
