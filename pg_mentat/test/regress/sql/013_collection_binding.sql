-- Test collection :in bindings [?x ...]
-- Phase D: Collection bindings generate IN clauses

-- Setup: create test attributes and data
SELECT mentat_transact('[
  [:db/add "attr1" :db/ident :person/age]
  [:db/add "attr1" :db/valueType :db.type/long]
  [:db/add "attr1" :db/cardinality :db.cardinality/one]
  [:db/add "attr2" :db/ident :person/name]
  [:db/add "attr2" :db/valueType :db.type/string]
  [:db/add "attr2" :db/cardinality :db.cardinality/one]
]');

-- Insert test people
SELECT mentat_transact('[
  [:db/add "p1" :person/name "Alice"]
  [:db/add "p1" :person/age 25]
  [:db/add "p2" :person/name "Bob"]
  [:db/add "p2" :person/age 30]
  [:db/add "p3" :person/name "Charlie"]
  [:db/add "p3" :person/age 35]
  [:db/add "p4" :person/name "Diana"]
  [:db/add "p4" :person/age 40]
]');

-- Test: collection binding with multiple ages
-- Expected: returns names for ages 25, 30, 35
SELECT mentat_query(
  '[:find ?name :in [?age ...] :where [?e :person/age ?age] [?e :person/name ?name]]',
  '{"inputs": [[25, 30, 35]]}'
);

-- Test: collection binding with single value (degenerates to equality)
SELECT mentat_query(
  '[:find ?name :in [?age ...] :where [?e :person/age ?age] [?e :person/name ?name]]',
  '{"inputs": [[30]]}'
);

-- Test: collection binding with string values
SELECT mentat_query(
  '[:find ?age :in [?name ...] :where [?e :person/name ?name] [?e :person/age ?age]]',
  '{"inputs": [["Alice", "Charlie"]]}'
);

-- Test: empty collection returns no results
SELECT mentat_query(
  '[:find ?name :in [?age ...] :where [?e :person/age ?age] [?e :person/name ?name]]',
  '{"inputs": [[]]}'
);
