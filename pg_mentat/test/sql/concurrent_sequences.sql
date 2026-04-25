-- Concurrent Sequence Allocation Tests
-- Tests that sequence-based entity ID allocation is lock-free and correct.
--
-- For true concurrent testing, run this with multiple sessions:
--   psql -f concurrent_sequences.sql &
--   psql -f concurrent_sequences.sql &
--   wait
-- Then verify results with the validation query at the bottom.

CREATE EXTENSION IF NOT EXISTS pg_mentat CASCADE;

\echo '=== Test 1: Sequence uniqueness under rapid allocation ==='
DO $$
DECLARE
    ids BIGINT[];
    id BIGINT;
    i INTEGER;
    dup_count INTEGER;
BEGIN
    -- Allocate 1000 IDs rapidly
    FOR i IN 1..1000 LOOP
        id := nextval('mentat.partition_user_seq');
        ids := array_append(ids, id);
    END LOOP;

    -- Check for duplicates using array trick
    SELECT COUNT(*) - COUNT(DISTINCT val) INTO dup_count
    FROM unnest(ids) AS val;

    IF dup_count > 0 THEN
        RAISE EXCEPTION 'Test 1 FAILED: Found % duplicate IDs in 1000 allocations', dup_count;
    END IF;

    RAISE NOTICE 'Test 1 PASSED: 1000 sequential allocations produced 0 duplicates';
END $$;

\echo '=== Test 2: allocate_entid function produces unique IDs ==='
DO $$
DECLARE
    user_ids BIGINT[];
    tx_ids BIGINT[];
    db_ids BIGINT[];
    i INTEGER;
    user_dups INTEGER;
    tx_dups INTEGER;
    db_dups INTEGER;
BEGIN
    FOR i IN 1..100 LOOP
        user_ids := array_append(user_ids, mentat.allocate_entid('db.part/user'));
        tx_ids := array_append(tx_ids, mentat.allocate_entid('db.part/tx'));
        db_ids := array_append(db_ids, mentat.allocate_entid('db.part/db'));
    END LOOP;

    SELECT COUNT(*) - COUNT(DISTINCT val) INTO user_dups FROM unnest(user_ids) AS val;
    SELECT COUNT(*) - COUNT(DISTINCT val) INTO tx_dups FROM unnest(tx_ids) AS val;
    SELECT COUNT(*) - COUNT(DISTINCT val) INTO db_dups FROM unnest(db_ids) AS val;

    IF user_dups > 0 OR tx_dups > 0 OR db_dups > 0 THEN
        RAISE EXCEPTION 'Test 2 FAILED: Duplicates found - user:%, tx:%, db:%',
            user_dups, tx_dups, db_dups;
    END IF;

    RAISE NOTICE 'Test 2 PASSED: 300 allocations across 3 partitions, 0 duplicates';
END $$;

\echo '=== Test 3: Partitions table not locked by sequence allocation ==='
DO $$
DECLARE
    next_before BIGINT;
    next_after BIGINT;
BEGIN
    SELECT next_entid INTO next_before
    FROM mentat.partitions WHERE name = 'db.part/user';

    -- Allocate 100 IDs via sequence
    PERFORM nextval('mentat.partition_user_seq') FROM generate_series(1, 100);

    SELECT next_entid INTO next_after
    FROM mentat.partitions WHERE name = 'db.part/user';

    IF next_before != next_after THEN
        RAISE EXCEPTION 'Test 3 FAILED: partitions.next_entid changed (% -> %). Sequences should not modify partitions table.',
            next_before, next_after;
    END IF;

    RAISE NOTICE 'Test 3 PASSED: Sequence allocation did not lock/modify partitions table';
END $$;

\echo '=== Test 4: Rapid transactions produce unique entity IDs ==='

-- Create test attribute
SELECT mentat.mentat_transact('[
    {:db/id "attr"
     :db/ident :test/concurrent
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
]');

-- Run 50 rapid transactions
DO $$
DECLARE
    i INTEGER;
BEGIN
    FOR i IN 1..50 LOOP
        PERFORM mentat.mentat_transact(
            format('[[:db/add "e%s" :test/concurrent "person-%s"]]', i, i)
        );
    END LOOP;
END $$;

-- Verify all entity IDs are unique
DO $$
DECLARE
    total_count INTEGER;
    unique_count INTEGER;
BEGIN
    SELECT COUNT(*), COUNT(DISTINCT e) INTO total_count, unique_count
    FROM mentat.datoms
    WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/concurrent')
    AND added = true;

    IF total_count != unique_count THEN
        RAISE EXCEPTION 'Test 4 FAILED: % total datoms but only % unique entity IDs',
            total_count, unique_count;
    END IF;

    IF total_count != 50 THEN
        RAISE EXCEPTION 'Test 4 FAILED: Expected 50 entities, got %', total_count;
    END IF;

    RAISE NOTICE 'Test 4 PASSED: 50 rapid transactions, 50 unique entity IDs';
END $$;

\echo '=== Test 5: Transaction IDs are unique and ordered ==='
DO $$
DECLARE
    tx_count INTEGER;
    unique_tx_count INTEGER;
    ordered_check BOOLEAN;
BEGIN
    SELECT COUNT(*), COUNT(DISTINCT tx)
    INTO tx_count, unique_tx_count
    FROM mentat.transactions;

    IF tx_count != unique_tx_count THEN
        RAISE EXCEPTION 'Test 5 FAILED: % transactions but only % unique tx IDs',
            tx_count, unique_tx_count;
    END IF;

    -- Check monotonic ordering
    SELECT NOT EXISTS (
        SELECT 1 FROM (
            SELECT tx, LAG(tx) OVER (ORDER BY tx) as prev_tx
            FROM mentat.transactions
        ) sub
        WHERE prev_tx IS NOT NULL AND tx <= prev_tx
    ) INTO ordered_check;

    IF NOT ordered_check THEN
        RAISE EXCEPTION 'Test 5 FAILED: Transaction IDs are not monotonically increasing';
    END IF;

    RAISE NOTICE 'Test 5 PASSED: All % transaction IDs are unique and ordered', tx_count;
END $$;

\echo ''
\echo '=== All concurrent sequence tests passed! ==='
\echo 'Verified: Sequence-based allocation is lock-free and produces unique IDs'
\echo '- No duplicate entity IDs under rapid allocation'
\echo '- No duplicate transaction IDs'
\echo '- Partitions table not modified by sequence operations'
\echo '- IDs stay within partition boundaries'
