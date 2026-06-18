-- Demo initialization for the pg_mentat container image.
-- Runs once on first container start (via /docker-entrypoint-initdb.d).

CREATE EXTENSION IF NOT EXISTS pg_mentat;

-- A tiny schema + a couple of entities so `mentat.q(...)` returns something
-- immediately after the container comes up.
SELECT mentat.t($$[
  {:db/ident :person/name  :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :person/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
  {:db/ident :person/age   :db/valueType :db.type/long   :db/cardinality :db.cardinality/one}
]$$);

SELECT mentat.t($$[
  {:db/id "alice" :person/name "Alice" :person/email "alice@example.com" :person/age 30}
  {:db/id "bob"   :person/name "Bob"   :person/email "bob@example.com"   :person/age 25}
]$$);

-- Try it:  SELECT mentat.q('[:find ?n ?age :where [?e :person/name ?n][?e :person/age ?age]]');
