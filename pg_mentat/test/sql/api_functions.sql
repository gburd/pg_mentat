-- Test suite for pg_mentat SQL API functions
-- Tests mentat_schema(), mentat_entity(), and mentat_query()

-- Setup: Create test schema and data
BEGIN;

-- Test 1: mentat_schema() - Returns complete schema as JSON
-- Expected: JSON object with all attributes and their properties
SELECT mentat.mentat_schema();

-- Verify schema structure has expected keys
SELECT jsonb_typeof(mentat.mentat_schema());
-- Should return 'object'

-- Test 2: mentat_entity() - Fetch entity data
-- First, create a test entity via transaction
SELECT mentat.mentat_transact('
[[:db/add "person1" :person/name "Alice"]
 [:db/add "person1" :person/age 30]]
');

-- Get the entity ID from the transaction result
-- (In real usage, parse the tempids map from the transaction result)
-- For testing, look up by attribute
SELECT e FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.ident = 'person:name'
AND d.added = true
LIMIT 1;

-- Test mentat_entity with a known entity ID
-- Note: Replace 123 with actual entity ID from above query
SELECT mentat.mentat_entity(100);

-- Verify entity structure
SELECT jsonb_typeof(mentat.mentat_entity(100));
-- Should return 'object'

-- Verify entity has :db/id key
SELECT mentat.mentat_entity(100) ? ':db/id';
-- Should return true

-- Test 3: mentat_query() - Execute datalog queries
-- Simple query to find all person names
SELECT mentat.mentat_query('
[:find ?name
 :where
 [?e :person/name ?name]]
', '{}'::jsonb);

-- Query with multiple variables
SELECT mentat.mentat_query('
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]]
', '{}'::jsonb);

-- Verify query result structure has columns and results
SELECT
    result->>'columns' as columns,
    jsonb_array_length(result->'results') as result_count
FROM (
    SELECT mentat.mentat_query('
        [:find ?name
         :where
         [?e :person/name ?name]]
    ', '{}'::jsonb) as result
) q;

-- Test 4: Empty result cases
-- Query for non-existent attribute
SELECT mentat.mentat_query('
[:find ?x
 :where
 [?e :nonexistent/attr ?x]]
', '{}'::jsonb);
-- Should return {"columns": ["?x"], "results": []}

-- Entity lookup for non-existent ID
SELECT mentat.mentat_entity(999999);
-- Should return {"db/id": 999999} with no other attributes

-- Test 5: Schema introspection
-- Check that core schema attributes exist
SELECT
    (mentat.mentat_schema()->':db/ident') IS NOT NULL as has_db_ident,
    (mentat.mentat_schema()->':db/valueType') IS NOT NULL as has_value_type,
    (mentat.mentat_schema()->':db/cardinality') IS NOT NULL as has_cardinality;

-- Verify an attribute has expected properties
SELECT
    attr_info->>'entid' as entid,
    attr_info->>'valueType' as value_type,
    attr_info->>'cardinality' as cardinality
FROM (
    SELECT mentat.mentat_schema()->':db/ident' as attr_info
) s;

ROLLBACK;
