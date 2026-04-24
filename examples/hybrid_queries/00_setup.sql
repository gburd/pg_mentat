-- pg_mentat Hybrid Query Examples: Setup
-- Run this file first to create the schema and sample data used by all examples.
--
-- Usage:
--   psql -d your_database -f 00_setup.sql

-- Load the extension
CREATE EXTENSION IF NOT EXISTS pg_mentat;

-- ==========================================================================
-- Schema: Company directory with departments, people, projects, and skills
-- ==========================================================================

SELECT mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}

  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}

  {:db/ident :person/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}

  {:db/ident :person/department
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one}

  {:db/ident :person/manager
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one}

  {:db/ident :person/skills
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/many}

  {:db/ident :person/bio
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/fulltext true}

  {:db/ident :person/salary
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}

  {:db/ident :dept/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}

  {:db/ident :dept/parent
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one}

  {:db/ident :dept/budget
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}

  {:db/ident :project/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}

  {:db/ident :project/member
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many}

  {:db/ident :project/lead
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one}

  {:db/ident :project/status
   :db/valueType :db.type/keyword
   :db/cardinality :db.cardinality/one}
]');

-- ==========================================================================
-- Sample data
-- ==========================================================================

SELECT mentat_transact('[
  {:db/id "eng"    :dept/name "Engineering"     :dept/budget 5000000}
  {:db/id "fe"     :dept/name "Frontend"        :dept/budget 1500000 :dept/parent "eng"}
  {:db/id "be"     :dept/name "Backend"         :dept/budget 2000000 :dept/parent "eng"}
  {:db/id "infra"  :dept/name "Infrastructure"  :dept/budget 1500000 :dept/parent "eng"}
  {:db/id "sales"  :dept/name "Sales"           :dept/budget 3000000}
  {:db/id "mktg"   :dept/name "Marketing"       :dept/budget 2000000}

  {:db/id "alice"
   :person/name "Alice Chen"
   :person/email "alice@example.com"
   :person/age 42
   :person/salary 220000
   :person/department "eng"
   :person/skills ["Rust" "PostgreSQL" "Datalog"]
   :person/bio "VP of Engineering with 20 years of database experience"}

  {:db/id "bob"
   :person/name "Bob Park"
   :person/email "bob@example.com"
   :person/age 35
   :person/salary 185000
   :person/department "be"
   :person/manager "alice"
   :person/skills ["Rust" "Go" "PostgreSQL"]
   :person/bio "Backend lead specializing in high-performance systems"}

  {:db/id "carol"
   :person/name "Carol Davis"
   :person/email "carol@example.com"
   :person/age 29
   :person/salary 160000
   :person/department "fe"
   :person/manager "alice"
   :person/skills ["TypeScript" "React" "GraphQL"]
   :person/bio "Frontend architect passionate about developer experience"}

  {:db/id "dave"
   :person/name "Dave Kim"
   :person/email "dave@example.com"
   :person/age 31
   :person/salary 170000
   :person/department "be"
   :person/manager "bob"
   :person/skills ["Go" "Kubernetes" "PostgreSQL"]
   :person/bio "Senior engineer working on distributed database systems"}

  {:db/id "eve"
   :person/name "Eve Lopez"
   :person/email "eve@example.com"
   :person/age 27
   :person/salary 145000
   :person/department "infra"
   :person/manager "bob"
   :person/skills ["Kubernetes" "Terraform" "Linux"]
   :person/bio "Infrastructure engineer focused on cloud-native deployments"}

  {:db/id "frank"
   :person/name "Frank Wu"
   :person/email "frank@example.com"
   :person/age 38
   :person/salary 175000
   :person/department "sales"
   :person/skills ["CRM" "Analytics"]
   :person/bio "Sales director driving enterprise adoption"}

  {:db/id "grace"
   :person/name "Grace Hopper"
   :person/email "grace@example.com"
   :person/age 33
   :person/salary 155000
   :person/department "mktg"
   :person/skills ["SEO" "Analytics" "Content"]
   :person/bio "Marketing lead with a focus on developer relations and content strategy"}

  {:db/id "proj-mentat"
   :project/name "pg_mentat"
   :project/lead "alice"
   :project/status :status/active
   :project/member ["alice" "bob" "dave"]}

  {:db/id "proj-ui"
   :project/name "Dashboard UI"
   :project/lead "carol"
   :project/status :status/active
   :project/member ["carol" "dave"]}

  {:db/id "proj-infra"
   :project/name "Cloud Migration"
   :project/lead "bob"
   :project/status :status/planning
   :project/member ["bob" "eve"]}
]');

-- ==========================================================================
-- Relational table for time tracking (used in example 05)
-- ==========================================================================

CREATE TABLE IF NOT EXISTS time_entries (
  id SERIAL PRIMARY KEY,
  person_email TEXT NOT NULL,
  project_name TEXT NOT NULL,
  hours NUMERIC(5,2) NOT NULL,
  entry_date DATE NOT NULL DEFAULT CURRENT_DATE
);

INSERT INTO time_entries (person_email, project_name, hours, entry_date) VALUES
  ('alice@example.com', 'pg_mentat',     8.0, '2026-04-20'),
  ('bob@example.com',   'pg_mentat',     6.5, '2026-04-20'),
  ('dave@example.com',  'pg_mentat',     7.0, '2026-04-20'),
  ('carol@example.com', 'Dashboard UI',  8.0, '2026-04-20'),
  ('dave@example.com',  'Dashboard UI',  3.0, '2026-04-20'),
  ('eve@example.com',   'Cloud Migration', 6.0, '2026-04-20'),
  ('alice@example.com', 'pg_mentat',     7.5, '2026-04-21'),
  ('bob@example.com',   'pg_mentat',     8.0, '2026-04-21'),
  ('bob@example.com',   'Cloud Migration', 2.0, '2026-04-21'),
  ('eve@example.com',   'Cloud Migration', 8.0, '2026-04-21'),
  ('carol@example.com', 'Dashboard UI',  7.5, '2026-04-22'),
  ('dave@example.com',  'pg_mentat',     6.0, '2026-04-22')
ON CONFLICT DO NOTHING;
