-- Test suite for :db/retract operation
-- Verifies that specific attribute-value pairs can be retracted

BEGIN;

-- Test 1: Setup - Add a person with multiple attributes
SELECT mentat.mentat_transact('
[{:db/id "alice"
  :person/name "Alice Smith"
  :person/age 30
  :person/email "alice@example.com"}]
');

-- Verify all attributes were added
SELECT mentat.mentat_query('
[:find ?attr ?value
 :where
 ["alice" ?attr ?value]]
', '{}'::jsonb);

-- Test 2: Retract a single attribute value
SELECT mentat.mentat_transact('
[[:db/retract "alice" :person/age 30]]
');

-- Verify age is gone but other attributes remain
SELECT mentat.mentat_query('
[:find ?attr ?value
 :where
 ["alice" ?attr ?value]]
', '{}'::jsonb);

-- Verify age query returns empty result
SELECT mentat.mentat_query('
[:find ?age
 :where
 ["alice" :person/age ?age]]
', '{}'::jsonb);
-- Expected: {"columns": ["?age"], "results": []}

-- Test 3: Verify history shows both add and retract
SELECT mentat.mentat_query('
[:find ?age ?tx ?added
 :where
 ["alice" :person/age ?age ?tx ?added]]
', '{"history": true}'::jsonb);
-- Expected: Two tuples - one with added=true, one with added=false

-- Test 4: Retract a wrong value should have no effect
-- First re-add the age
SELECT mentat.mentat_transact('
[[:db/add "alice" :person/age 31]]
');

-- Try to retract with wrong value
SELECT mentat.mentat_transact('
[[:db/retract "alice" :person/age 30]]
');

-- Verify age 31 is still present (wrong value was not retracted)
SELECT mentat.mentat_query('
[:find ?age
 :where
 ["alice" :person/age ?age]]
', '{}'::jsonb);
-- Expected: {"columns": ["?age"], "results": [[31]]}

-- Test 5: Retract the correct value
SELECT mentat.mentat_transact('
[[:db/retract "alice" :person/age 31]]
');

-- Verify age is now gone
SELECT mentat.mentat_query('
[:find ?age
 :where
 ["alice" :person/age ?age]]
', '{}'::jsonb);
-- Expected: {"columns": ["?age"], "results": []}

-- Test 6: Retract and re-add in same transaction
SELECT mentat.mentat_transact('
[[:db/retract "alice" :person/email "alice@example.com"]
 [:db/add "alice" :person/email "alice.smith@example.com"]]
');

-- Verify new email is present
SELECT mentat.mentat_query('
[:find ?email
 :where
 ["alice" :person/email ?email]]
', '{}'::jsonb);
-- Expected: {"columns": ["?email"], "results": [["alice.smith@example.com"]]}

-- Test 7: Test with numeric entity ID
-- First get alice's actual entity ID
SELECT e FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.ident = 'person:name'
AND mentat.decode_value(d.v, d.value_type_tag) = '"Alice Smith"'
AND d.added = true
LIMIT 1 \gset alice_

-- Retract using numeric entity ID
SELECT mentat.mentat_transact(format('
[[:db/retract %s :person/name "Alice Smith"]]
', :'alice_e'));

-- Verify name is gone
SELECT mentat.mentat_query(format('
[:find ?name
 :where
 [%s :person/name ?name]]
', :'alice_e'), '{}'::jsonb);
-- Expected: {"columns": ["?name"], "results": []}

-- Test 8: Verify :db/retractEntity still works alongside :db/retract
-- Add new entity with multiple attributes
SELECT mentat.mentat_transact('
[{:db/id "bob"
  :person/name "Bob Jones"
  :person/age 25}]
');

-- Retract entire entity
SELECT mentat.mentat_transact('
[[:db/retractEntity "bob"]]
');

-- Verify all attributes are gone
SELECT mentat.mentat_query('
[:find ?attr ?value
 :where
 ["bob" ?attr ?value]]
', '{}'::jsonb);
-- Expected: {"columns": ["?attr", "?value"], "results": []}

-- Test 9: Test retract with different value types
-- Add entity with various value types
SELECT mentat.mentat_transact('
[{:db/id "test"
  :db/ident :test/entity
  :db/doc "Test entity"
  :db/cardinality :db.cardinality/one}]
');

-- Retract the string value
SELECT mentat.mentat_transact('
[[:db/retract "test" :db/doc "Test entity"]]
');

-- Verify doc is gone
SELECT mentat.mentat_query('
[:find ?doc
 :where
 ["test" :db/doc ?doc]]
', '{}'::jsonb);
-- Expected: {"columns": ["?doc"], "results": []}

-- Retract the keyword reference value
SELECT mentat.mentat_transact('
[[:db/retract "test" :db/cardinality :db.cardinality/one]]
');

-- Verify cardinality is gone
SELECT mentat.mentat_query('
[:find ?card
 :where
 ["test" :db/cardinality ?card]]
', '{}'::jsonb);
-- Expected: {"columns": ["?card"], "results": []}

ROLLBACK;
