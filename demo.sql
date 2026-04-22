-- pg_mentat demo: Mentat Datalog database for PostgreSQL
-- This script runs automatically when the Docker container starts for the first time.

-- ============================================================================
-- 1. Create the extension
-- ============================================================================
CREATE EXTENSION pg_mentat;

-- ============================================================================
-- 2. Define schema attributes
-- ============================================================================

-- Define person attributes (name, age, email)
SELECT mentat_transact('[
  [:db/add "name-attr" :db/ident :person/name]
  [:db/add "name-attr" :db/valueType :db.type/string]
  [:db/add "name-attr" :db/cardinality :db.cardinality/one]
  [:db/add "age-attr" :db/ident :person/age]
  [:db/add "age-attr" :db/valueType :db.type/long]
  [:db/add "age-attr" :db/cardinality :db.cardinality/one]
  [:db/add "email-attr" :db/ident :person/email]
  [:db/add "email-attr" :db/valueType :db.type/string]
  [:db/add "email-attr" :db/cardinality :db.cardinality/one]
]'::TEXT);

-- ============================================================================
-- 3. Add sample data
-- ============================================================================

-- Add some people
SELECT mentat_transact('[
  [:db/add "alice" :person/name "Alice"]
  [:db/add "alice" :person/age 30]
  [:db/add "alice" :person/email "alice@example.com"]
]'::TEXT);

SELECT mentat_transact('[
  [:db/add "bob" :person/name "Bob"]
  [:db/add "bob" :person/age 25]
  [:db/add "bob" :person/email "bob@example.com"]
]'::TEXT);

SELECT mentat_transact('[
  [:db/add "carol" :person/name "Carol"]
  [:db/add "carol" :person/age 35]
  [:db/add "carol" :person/email "carol@example.com"]
]'::TEXT);

-- ============================================================================
-- 4. Query examples
-- ============================================================================

-- Find all people (entity ID and name)
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]'::TEXT,
  '{}'::jsonb
);

-- Find a person by age using :in parameter binding
SELECT mentat_query(
  '[:find ?name :in ?age :where [?e :person/age ?age] [?e :person/name ?name]]'::TEXT,
  '{"inputs": [30]}'::jsonb
);

-- Find people older than 28
SELECT mentat_query(
  '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] [(> ?age 28)]]'::TEXT,
  '{}'::jsonb
);

-- View the full schema
SELECT mentat_schema();
