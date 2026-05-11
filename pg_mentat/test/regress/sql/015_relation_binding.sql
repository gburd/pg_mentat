-- Test relation :in bindings [[?x ?y]]
-- Phase D: Relation bindings generate VALUES joins

-- Prerequisite: schema and data from previous tests

-- Test: relation binding with multiple rows
SELECT mentat_query(
  '[:find ?name :in [[?age ?name_prefix]] :where [?e :person/age ?age] [?e :person/name ?name]]',
  '{"inputs": [[[25, "Alice"], [30, "Bob"]]]}'
);
