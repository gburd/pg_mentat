-- pg_mentat regression: schema definition and data insertion
-- Define attributes and transact test data

-- Define schema attributes
SELECT mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :person/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
]');

-- Insert test entities
SELECT mentat_transact('[
  {:person/name "Alice" :person/age 30 :person/email "alice@example.com"}
  {:person/name "Bob" :person/age 25 :person/email "bob@example.com"}
  {:person/name "Carol" :person/age 35 :person/email "carol@example.com"}
  {:person/name "Dave" :person/age 28 :person/email "dave@example.com"}
]');

SELECT 'transact_done' AS status;
