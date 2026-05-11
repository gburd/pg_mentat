-- Test tuple :in bindings [?x ?y]
-- Phase D: Tuple bindings bind multiple variables simultaneously

-- Prerequisite: schema and data from 013_collection_binding.sql
-- (assumes same session or the attributes already exist)

-- Setup additional attributes
SELECT mentat_transact('[
  [:db/add "attr3" :db/ident :person/first]
  [:db/add "attr3" :db/valueType :db.type/string]
  [:db/add "attr3" :db/cardinality :db.cardinality/one]
  [:db/add "attr4" :db/ident :person/last]
  [:db/add "attr4" :db/valueType :db.type/string]
  [:db/add "attr4" :db/cardinality :db.cardinality/one]
]');

SELECT mentat_transact('[
  [:db/add "p5" :person/first "Alice"]
  [:db/add "p5" :person/last "Smith"]
  [:db/add "p5" :person/name "Alice Smith"]
  [:db/add "p6" :person/first "Bob"]
  [:db/add "p6" :person/last "Jones"]
  [:db/add "p6" :person/name "Bob Jones"]
]');

-- Test: tuple binding with two string variables
SELECT mentat_query(
  '[:find ?name :in [?first ?last] :where [?e :person/first ?first] [?e :person/last ?last] [?e :person/name ?name]]',
  '{"inputs": [["Alice", "Smith"]]}'
);
