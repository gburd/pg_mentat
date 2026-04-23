-- Cardinality-many attribute tests
-- Tests the implementation of :db.cardinality/many attributes
-- which allow multiple values for the same (entity, attribute) pair.

\echo 'Testing cardinality-many attributes'

-- Clean up any existing test data
DELETE FROM mentat.datoms WHERE e > 10;
DELETE FROM mentat.transactions WHERE tx > 10;
DELETE FROM mentat.schema WHERE entid > 10;
DELETE FROM mentat.idents WHERE entid > 10;

-- Test 1: Define a cardinality-many attribute
\echo 'Test 1: Define :person/hobby as cardinality-many'
SELECT mentat_transact('[
  {:db/ident :person/hobby
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/many}
]');

-- Verify schema was created correctly
SELECT ident, value_type::TEXT, cardinality::TEXT
FROM mentat.schema
WHERE ident = ':person/hobby';

-- Test 2: Assert multiple hobbies in a single transaction using map notation
\echo 'Test 2: Assert multiple hobbies for Alice in one transaction'
SELECT mentat_transact('[
  {:db/id "alice"
   :person/name "Alice"
   :person/hobby "chess"
   :person/hobby "reading"
   :person/hobby "hiking"}
]');

-- Query to verify all three hobbies were stored
\echo 'Verify all three hobbies are stored:'
SELECT mentat_query('[:find ?hobby :where [?e :person/name "Alice"] [?e :person/hobby ?hobby]]', '{}');

-- Count the hobbies
SELECT COUNT(*) as hobby_count
FROM mentat.datoms d1
JOIN mentat.idents i ON d1.a = i.entid
WHERE i.ident = ':person/hobby'
  AND d1.e IN (SELECT e FROM mentat.datoms d2
               JOIN mentat.idents i2 ON d2.a = i2.entid
               WHERE i2.ident = ':person/name'
               AND decode(d2.v, 'escape')::text = 'Alice')
  AND d1.added = true;

-- Test 3: Add more hobbies in a subsequent transaction
\echo 'Test 3: Add more hobbies to Alice'
SELECT mentat_transact('[
  [:db/add [:person/name "Alice"] :person/hobby "painting"]
  [:db/add [:person/name "Alice"] :person/hobby "gardening"]
]');

-- Query all hobbies (should now be 5)
\echo 'Verify all five hobbies are stored:'
SELECT mentat_query('[:find ?hobby :where [?e :person/name "Alice"] [?e :person/hobby ?hobby]]', '{}');

-- Count again
SELECT COUNT(*) as hobby_count
FROM mentat.datoms d1
JOIN mentat.idents i ON d1.a = i.entid
WHERE i.ident = ':person/hobby'
  AND d1.e IN (SELECT e FROM mentat.datoms d2
               JOIN mentat.idents i2 ON d2.a = i2.entid
               WHERE i2.ident = ':person/name'
               AND decode(d2.v, 'escape')::text = 'Alice')
  AND d1.added = true;

-- Test 4: Retract one specific hobby (should keep the others)
\echo 'Test 4: Retract "chess" hobby'
SELECT mentat_transact('[
  [:db/retract [:person/name "Alice"] :person/hobby "chess"]
]');

-- Query remaining hobbies (should be 4, without chess)
\echo 'Verify chess was removed but others remain:'
SELECT mentat_query('[:find ?hobby :where [?e :person/name "Alice"] [?e :person/hobby ?hobby]]', '{}');

-- Count again (should be 4)
SELECT COUNT(*) as hobby_count
FROM mentat.datoms d1
JOIN mentat.idents i ON d1.a = i.entid
WHERE i.ident = ':person/hobby'
  AND d1.e IN (SELECT e FROM mentat.datoms d2
               JOIN mentat.idents i2 ON d2.a = i2.entid
               WHERE i2.ident = ':person/name'
               AND decode(d2.v, 'escape')::text = 'Alice')
  AND d1.added = true;

-- Test 5: Test duplicate values (should be idempotent)
\echo 'Test 5: Re-assert an existing hobby (should be idempotent)'
SELECT mentat_transact('[
  [:db/add [:person/name "Alice"] :person/hobby "reading"]
]');

-- Count should still be 4 (no duplicate)
SELECT COUNT(*) as hobby_count
FROM mentat.datoms d1
JOIN mentat.idents i ON d1.a = i.entid
WHERE i.ident = ':person/hobby'
  AND d1.e IN (SELECT e FROM mentat.datoms d2
               JOIN mentat.idents i2 ON d2.a = i2.entid
               WHERE i2.ident = ':person/name'
               AND decode(d2.v, 'escape')::text = 'Alice')
  AND d1.added = true;

-- Test 6: Define cardinality-many ref attribute
\echo 'Test 6: Define :person/friend as cardinality-many ref'
SELECT mentat_transact('[
  {:db/ident :person/friend
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many}
]');

-- Create multiple people and establish friendships
SELECT mentat_transact('[
  {:db/id "bob" :person/name "Bob"}
  {:db/id "carol" :person/name "Carol"}
  {:db/id "dave" :person/name "Dave"}
]');

-- Alice befriends multiple people
SELECT mentat_transact('[
  [:db/add [:person/name "Alice"] :person/friend [:person/name "Bob"]]
  [:db/add [:person/name "Alice"] :person/friend [:person/name "Carol"]]
  [:db/add [:person/name "Alice"] :person/friend [:person/name "Dave"]]
]');

-- Query Alice's friends
\echo 'Verify Alice has three friends:'
SELECT mentat_query('[:find ?fname :where
  [?alice :person/name "Alice"]
  [?alice :person/friend ?friend]
  [?friend :person/name ?fname]]', '{}');

-- Test 7: Cardinality-one should still work (for comparison)
\echo 'Test 7: Verify cardinality-one still auto-retracts old values'
-- :person/name should be cardinality-one (defined in bootstrap)
SELECT mentat_transact('[
  [:db/add [:person/name "Alice"] :person/name "Alice Smith"]
]');

-- Should only have one name (the new one)
SELECT COUNT(*) as name_count
FROM mentat.datoms d1
JOIN mentat.idents i ON d1.a = i.entid
WHERE i.ident = ':person/name'
  AND d1.e IN (SELECT e FROM mentat.datoms d2
               JOIN mentat.idents i2 ON d2.a = i2.entid
               WHERE i2.ident = ':person/name'
               AND decode(d2.v, 'escape')::text LIKE 'Alice%')
  AND d1.added = true;

\echo 'Verify the name was updated (not accumulated):'
SELECT mentat_query('[:find ?name :where [?e :person/name ?name] [?e :person/hobby "reading"]]', '{}');

-- Test 8: Test invalid cardinality-one with multiple values in same transaction
\echo 'Test 8: Attempt to assert multiple values for cardinality-one attribute (should fail)'
DO $$
BEGIN
  PERFORM mentat_transact('[
    {:db/id "test-person"
     :person/name "John"
     :person/name "Johnny"}
  ]');
  RAISE EXCEPTION 'Expected cardinality violation but transaction succeeded';
EXCEPTION
  WHEN OTHERS THEN
    IF SQLERRM LIKE '%Cardinality violation%' THEN
      RAISE NOTICE 'Correctly rejected multiple cardinality-one values: %', SQLERRM;
    ELSE
      RAISE;
    END IF;
END $$;

\echo ''
\echo 'All cardinality-many tests completed successfully!'
