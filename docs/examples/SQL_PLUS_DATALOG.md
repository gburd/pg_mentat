# SQL + Datalog Integration Examples

pg_mentat brings Datalog query semantics into PostgreSQL, but the real power
comes from combining both query paradigms. Datalog excels at graph traversal,
recursive queries, and pattern matching over entity-attribute-value data. SQL
excels at aggregation, analytics, window functions, and joining with relational
tables. Together they cover use cases that neither can handle well alone.

This guide shows practical patterns for mixing `mentat_query()`, `mentat_pull()`,
and `mentat_transact()` with standard SQL.

---

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Pattern 1: Datalog for Graph Traversal, SQL for Aggregation](#pattern-1-datalog-for-graph-traversal-sql-for-aggregation)
3. [Pattern 2: SQL for Bulk Filtering, Datalog for Validation](#pattern-2-sql-for-bulk-filtering-datalog-for-validation)
4. [Pattern 3: Window Functions over Datalog Results](#pattern-3-window-functions-over-datalog-results)
5. [Pattern 4: Full-Text Search (SQL) + Graph Queries (Datalog)](#pattern-4-full-text-search-sql--graph-queries-datalog)
6. [Pattern 5: Hybrid CTEs Combining SQL and Datalog](#pattern-5-hybrid-ctes-combining-sql-and-datalog)
7. [Pattern 6: Pull API Results in SQL Pipelines](#pattern-6-pull-api-results-in-sql-pipelines)
8. [Pattern 7: Temporal Queries with SQL Analytics](#pattern-7-temporal-queries-with-sql-analytics)
9. [Pattern 8: Joining Mentat Entities with Relational Tables](#pattern-8-joining-mentat-entities-with-relational-tables)
10. [Performance Considerations](#performance-considerations)
11. [Best Practices](#best-practices)

---

## Prerequisites

These examples assume you have pg_mentat installed and the following schema
defined:

```sql
CREATE EXTENSION pg_mentat;

-- Define schema for a company directory
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
]');

-- Insert sample data
SELECT mentat_transact('[
  {:db/id "eng"    :dept/name "Engineering" :dept/budget 5000000}
  {:db/id "fe"     :dept/name "Frontend"    :dept/budget 1500000 :dept/parent "eng"}
  {:db/id "be"     :dept/name "Backend"     :dept/budget 2000000 :dept/parent "eng"}
  {:db/id "infra"  :dept/name "Infrastructure" :dept/budget 1500000 :dept/parent "eng"}
  {:db/id "sales"  :dept/name "Sales"       :dept/budget 3000000}

  {:db/id "alice"
   :person/name "Alice Chen"
   :person/email "alice@example.com"
   :person/age 42
   :person/department "eng"
   :person/skills ["Rust" "PostgreSQL" "Datalog"]
   :person/bio "VP of Engineering with 20 years of database experience"}

  {:db/id "bob"
   :person/name "Bob Park"
   :person/email "bob@example.com"
   :person/age 35
   :person/department "be"
   :person/manager "alice"
   :person/skills ["Rust" "Go" "PostgreSQL"]
   :person/bio "Backend lead specializing in high-performance systems"}

  {:db/id "carol"
   :person/name "Carol Davis"
   :person/email "carol@example.com"
   :person/age 29
   :person/department "fe"
   :person/manager "alice"
   :person/skills ["TypeScript" "React" "GraphQL"]
   :person/bio "Frontend architect passionate about developer experience"}

  {:db/id "dave"
   :person/name "Dave Kim"
   :person/email "dave@example.com"
   :person/age 31
   :person/department "be"
   :person/manager "bob"
   :person/skills ["Go" "Kubernetes" "PostgreSQL"]
   :person/bio "Senior engineer working on distributed database systems"}

  {:db/id "eve"
   :person/name "Eve Lopez"
   :person/email "eve@example.com"
   :person/age 27
   :person/department "infra"
   :person/manager "bob"
   :person/skills ["Kubernetes" "Terraform" "Linux"]
   :person/bio "Infrastructure engineer focused on cloud-native deployments"}

  {:db/id "frank"
   :person/name "Frank Wu"
   :person/email "frank@example.com"
   :person/age 38
   :person/department "sales"
   :person/skills ["CRM" "Analytics"]
   :person/bio "Sales director driving enterprise adoption"}

  {:db/id "proj-mentat"
   :project/name "pg_mentat"
   :project/lead "alice"
   :project/member ["alice" "bob" "dave"]}

  {:db/id "proj-ui"
   :project/name "Dashboard UI"
   :project/lead "carol"
   :project/member ["carol" "dave"]}
]');
```

---

## Pattern 1: Datalog for Graph Traversal, SQL for Aggregation

**Use case:** Find all people who report (directly or indirectly) to a manager,
then compute workforce statistics.

Datalog's recursive rules navigate the org tree. SQL's aggregate functions
summarize the results.

```sql
-- Step 1: Use Datalog to find all reports (recursive)
WITH all_reports AS (
  SELECT
    (jsonb_array_elements(
      mentat_query(
        '[:find ?report-name ?report-age
          :with
          [[(reports-to ?e ?mgr)
            [?e :person/manager ?mgr]]
           [(reports-to ?e ?mgr)
            [?e :person/manager ?mid]
            (reports-to ?mid ?mgr)]]
          :where
          [?alice :person/name "Alice Chen"]
          (reports-to ?report ?alice)
          [?report :person/name ?report-name]
          [?report :person/age ?report-age]]',
        '{}'::jsonb
      )::jsonb -> 'results'
    ) ->> 0) AS name,
    (jsonb_array_elements(
      mentat_query(
        '[:find ?report-name ?report-age
          :with
          [[(reports-to ?e ?mgr)
            [?e :person/manager ?mgr]]
           [(reports-to ?e ?mgr)
            [?e :person/manager ?mid]
            (reports-to ?mid ?mgr)]]
          :where
          [?alice :person/name "Alice Chen"]
          (reports-to ?report ?alice)
          [?report :person/name ?report-name]
          [?report :person/age ?report-age]]',
        '{}'::jsonb
      )::jsonb -> 'results'
    ) -> 1)::int AS age
)
-- Step 2: Use SQL for aggregation
SELECT
  COUNT(*)          AS total_reports,
  AVG(age)::numeric(5,1) AS avg_age,
  MIN(age)          AS youngest,
  MAX(age)          AS oldest
FROM all_reports;
```

**A cleaner approach** uses a helper function to avoid repeating the query:

```sql
-- Wrap the Datalog result in a CTE, then aggregate
WITH datalog_result AS (
  SELECT mentat_query(
    '[:find ?name ?age
      :with
      [[(reports-to ?e ?mgr)
        [?e :person/manager ?mgr]]
       [(reports-to ?e ?mgr)
        [?e :person/manager ?mid]
        (reports-to ?mid ?mgr)]]
      :where
      [?alice :person/name "Alice Chen"]
      (reports-to ?r ?alice)
      [?r :person/name ?name]
      [?r :person/age ?age]]',
    '{}'::jsonb
  )::jsonb AS result
),
reports AS (
  SELECT
    elem ->> 0 AS name,
    (elem -> 1)::int AS age
  FROM datalog_result,
       jsonb_array_elements(result -> 'results') AS elem
)
SELECT
  COUNT(*)          AS team_size,
  ROUND(AVG(age), 1)  AS avg_age,
  MIN(age)          AS youngest,
  MAX(age)          AS oldest
FROM reports;
```

---

## Pattern 2: SQL for Bulk Filtering, Datalog for Validation

**Use case:** Find all people in the database, use SQL to filter the result set,
then use Datalog to validate relationships on the filtered set.

```sql
-- Step 1: Use Datalog to get all people with their departments
WITH people AS (
  SELECT
    (elem -> 0)::bigint AS eid,
    elem ->> 1 AS name,
    (elem -> 2)::int AS age,
    elem ->> 3 AS dept_name
  FROM (
    SELECT mentat_query(
      '[:find ?e ?name ?age ?dept-name
        :where
        [?e :person/name ?name]
        [?e :person/age ?age]
        [?e :person/department ?d]
        [?d :dept/name ?dept-name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
-- Step 2: SQL filter -- people aged 30+ in engineering sub-departments
senior_engineers AS (
  SELECT * FROM people
  WHERE age >= 30
    AND dept_name IN ('Backend', 'Frontend', 'Infrastructure')
)
-- Step 3: For each senior engineer, check if they lead a project (Datalog)
SELECT
  se.name,
  se.age,
  se.dept_name,
  (mentat_query(
    '[:find ?proj-name .
      :where
      [?proj :project/lead ' || se.eid || ']
      [?proj :project/name ?proj-name]]',
    '{}'::jsonb
  )::jsonb ->> 'result') AS leads_project
FROM senior_engineers se
ORDER BY se.age DESC;
```

---

## Pattern 3: Window Functions over Datalog Results

**Use case:** Rank employees within their department by age, and compute
running totals -- operations that Datalog cannot express but SQL handles
naturally.

```sql
WITH people AS (
  SELECT
    elem ->> 0 AS name,
    (elem -> 1)::int AS age,
    elem ->> 2 AS dept_name
  FROM (
    SELECT mentat_query(
      '[:find ?name ?age ?dept-name
        :where
        [?e :person/name ?name]
        [?e :person/age ?age]
        [?e :person/department ?d]
        [?d :dept/name ?dept-name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
)
SELECT
  name,
  age,
  dept_name,
  -- Rank within department by age (oldest = 1)
  RANK() OVER (PARTITION BY dept_name ORDER BY age DESC) AS dept_seniority_rank,
  -- Percentile within entire company
  PERCENT_RANK() OVER (ORDER BY age) AS company_age_percentile,
  -- Running count per department
  COUNT(*) OVER (PARTITION BY dept_name) AS dept_size
FROM people
ORDER BY dept_name, age DESC;
```

**Expected output structure:**

```
     name      | age | dept_name      | dept_seniority_rank | company_age_percentile | dept_size
---------------+-----+----------------+---------------------+------------------------+-----------
 Bob Park      |  35 | Backend        |                   1 |                   0.60 |         2
 Dave Kim      |  31 | Backend        |                   2 |                   0.40 |         2
 Carol Davis   |  29 | Frontend       |                   1 |                   0.20 |         1
 Eve Lopez     |  27 | Infrastructure |                   1 |                   0.00 |         1
```

---

## Pattern 4: Full-Text Search (SQL) + Graph Queries (Datalog)

**Use case:** Search employee bios for relevant terms, then use Datalog to
find their team connections and project participation.

pg_mentat supports full-text search natively in Datalog via the `fulltext`
where-function. This pattern shows how to combine FTS results with additional
Datalog graph traversal, then use SQL for presentation.

```sql
-- Find people whose bios mention "database", then get their projects
WITH fts_matches AS (
  SELECT
    (elem -> 0)::bigint AS eid,
    elem ->> 1 AS name,
    elem ->> 2 AS bio,
    (elem -> 3)::float AS relevance_score
  FROM (
    SELECT mentat_query(
      '[:find ?e ?name ?text ?score
        :where
        [(fulltext $ :person/bio "database") [[?e ?text _ ?score]]]
        [?e :person/name ?name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
-- For each match, find their projects via Datalog
enriched AS (
  SELECT
    fm.name,
    fm.bio,
    fm.relevance_score,
    (mentat_query(
      '[:find [?proj-name ...]
        :where
        [?proj :project/member ' || fm.eid || ']
        [?proj :project/name ?proj-name]]',
      '{}'::jsonb
    )::jsonb -> 'result') AS projects
  FROM fts_matches fm
)
SELECT
  name,
  LEFT(bio, 60) || '...' AS bio_excerpt,
  ROUND(relevance_score::numeric, 4) AS score,
  projects
FROM enriched
ORDER BY relevance_score DESC;
```

**Phrase search variant** -- find people whose bios mention "distributed database"
as a phrase:

```sql
SELECT mentat_query(
  '[:find ?name ?text ?score
    :where
    [(fulltext $ :person/bio "\"distributed database\"") [[?e ?text _ ?score]]]
    [?e :person/name ?name]
    :order (desc ?score)]',
  '{}'::jsonb
);
```

---

## Pattern 5: Hybrid CTEs Combining SQL and Datalog

**Use case:** Build a department budget report that traverses the department
hierarchy (Datalog), aggregates headcount per department (Datalog + SQL), and
computes budget-per-head ratios (SQL).

```sql
-- CTE 1: Get all sub-departments of Engineering via recursive Datalog rule
WITH eng_depts AS (
  SELECT
    (elem -> 0)::bigint AS dept_eid,
    elem ->> 1 AS dept_name,
    (elem -> 2)::bigint AS budget
  FROM (
    SELECT mentat_query(
      '[:find ?d ?name ?budget
        :with
        [[(sub-dept ?child ?parent)
          [?child :dept/parent ?parent]]
         [(sub-dept ?child ?ancestor)
          [?child :dept/parent ?mid]
          (sub-dept ?mid ?ancestor)]]
        :where
        [?eng :dept/name "Engineering"]
        (sub-dept ?d ?eng)
        [?d :dept/name ?name]
        [?d :dept/budget ?budget]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
-- CTE 2: Count people per department
dept_headcount AS (
  SELECT
    elem ->> 0 AS dept_name,
    (elem -> 1)::int AS headcount
  FROM (
    SELECT mentat_query(
      '[:find ?dept-name (count ?person)
        :where
        [?person :person/department ?d]
        [?d :dept/name ?dept-name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
)
-- Final: Join and compute per-head budget
SELECT
  ed.dept_name,
  ed.budget,
  COALESCE(dh.headcount, 0) AS headcount,
  CASE
    WHEN COALESCE(dh.headcount, 0) > 0
    THEN (ed.budget / dh.headcount)
    ELSE NULL
  END AS budget_per_head
FROM eng_depts ed
LEFT JOIN dept_headcount dh ON ed.dept_name = dh.dept_name
ORDER BY ed.budget DESC;
```

---

## Pattern 6: Pull API Results in SQL Pipelines

**Use case:** Use `mentat_pull()` to get rich nested entity data, then process
the JSONB output with PostgreSQL's JSON operators for reporting.

```sql
-- Get entity IDs from a Datalog query
WITH project_members AS (
  SELECT
    (elem -> 0)::bigint AS person_eid
  FROM (
    SELECT mentat_query(
      '[:find ?person
        :where
        [?proj :project/name "pg_mentat"]
        [?proj :project/member ?person]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
-- Pull rich entity data for each member
member_profiles AS (
  SELECT
    pm.person_eid,
    mentat_pull(
      '[:person/name :person/email :person/age
        {:person/department [:dept/name :dept/budget]}
        (:person/skills :limit 5)]',
      pm.person_eid
    )::jsonb AS profile
  FROM project_members pm
)
-- Use SQL JSON operators to build a report
SELECT
  profile ->> ':person/name' AS name,
  profile ->> ':person/email' AS email,
  (profile -> ':person/age')::int AS age,
  profile -> ':person/department' ->> ':dept/name' AS department,
  jsonb_array_length(COALESCE(profile -> ':person/skills', '[]'::jsonb)) AS skill_count,
  profile -> ':person/skills' AS skills
FROM member_profiles
ORDER BY (profile -> ':person/age')::int DESC;
```

**Pull with reverse lookups** -- find who reports to each person:

```sql
WITH managers AS (
  SELECT (elem -> 0)::bigint AS eid
  FROM (
    SELECT mentat_query(
      '[:find ?mgr
        :where
        [_ :person/manager ?mgr]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
)
SELECT
  mentat_pull(
    '[:person/name {:person/_manager [:person/name :person/email]}]',
    m.eid
  )::jsonb AS manager_with_reports
FROM managers m;
```

---

## Pattern 7: Temporal Queries with SQL Analytics

**Use case:** Track how an entity changes over time using Mentat's temporal
query features (`asOf`, `since`, `history`), then analyze the change patterns
with SQL.

### Audit trail: full change history for an entity

```sql
-- Get the complete history of Alice's attributes
WITH history AS (
  SELECT
    (elem -> 0)::bigint AS attr_id,
    elem ->> 1 AS value,
    (elem -> 2)::bigint AS tx_id,
    (elem -> 3)::boolean AS was_added
  FROM (
    SELECT mentat_query(
      '[:find ?a ?v ?tx ?added
        :where
        [?e :person/name "Alice Chen"]
        [?e ?a ?v ?tx ?added]]',
      '{"history": true}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
)
SELECT
  h.tx_id,
  t.tx_instant,
  h.attr_id,
  h.value,
  CASE WHEN h.was_added THEN 'ASSERT' ELSE 'RETRACT' END AS operation
FROM history h
JOIN mentat.transactions t ON h.tx_id = t.tx
ORDER BY t.tx_instant, h.attr_id;
```

### Point-in-time comparison

```sql
-- Compare team size at two different points in time
WITH
  -- Team size at transaction 1000005
  team_then AS (
    SELECT mentat_query(
      '[:find (count ?e)
        :where [?e :person/name _]]',
      '{"asOf": 1000005}'::jsonb
    )::jsonb -> 'result' AS count
  ),
  -- Team size now
  team_now AS (
    SELECT mentat_query(
      '[:find (count ?e)
        :where [?e :person/name _]]',
      '{}'::jsonb
    )::jsonb -> 'result' AS count
  )
SELECT
  team_then.count::int AS team_size_then,
  team_now.count::int AS team_size_now,
  (team_now.count::int - team_then.count::int) AS growth
FROM team_then, team_now;
```

### Recent changes feed

```sql
-- Get all changes since a known transaction
WITH changes AS (
  SELECT
    (elem -> 0)::bigint AS entity_id,
    (elem -> 1)::bigint AS attr_id,
    elem ->> 2 AS value,
    (elem -> 3)::bigint AS tx_id
  FROM (
    SELECT mentat_query(
      '[:find ?e ?a ?v ?tx
        :where
        [?e ?a ?v ?tx]]',
      '{"since": 1000003}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
)
SELECT
  c.entity_id,
  c.attr_id,
  c.value,
  c.tx_id,
  t.tx_instant AS changed_at
FROM changes c
JOIN mentat.transactions t ON c.tx_id = t.tx
ORDER BY t.tx_instant DESC
LIMIT 50;
```

---

## Pattern 8: Joining Mentat Entities with Relational Tables

**Use case:** Your application stores some data in Mentat (flexible, schema-
evolving entities) and some in traditional relational tables (high-volume,
fixed-schema data). This pattern joins across both.

```sql
-- Suppose you have a relational table for time tracking
CREATE TABLE IF NOT EXISTS time_entries (
  id SERIAL PRIMARY KEY,
  person_email TEXT NOT NULL,
  project_name TEXT NOT NULL,
  hours NUMERIC(5,2) NOT NULL,
  entry_date DATE NOT NULL DEFAULT CURRENT_DATE
);

-- Sample data
INSERT INTO time_entries (person_email, project_name, hours, entry_date) VALUES
  ('alice@example.com', 'pg_mentat', 8.0, '2026-04-20'),
  ('bob@example.com',   'pg_mentat', 6.5, '2026-04-20'),
  ('dave@example.com',  'pg_mentat', 7.0, '2026-04-20'),
  ('carol@example.com', 'Dashboard UI', 8.0, '2026-04-20'),
  ('dave@example.com',  'Dashboard UI', 3.0, '2026-04-20'),
  ('alice@example.com', 'pg_mentat', 7.5, '2026-04-21'),
  ('bob@example.com',   'pg_mentat', 8.0, '2026-04-21');

-- Join Mentat entity data with relational time entries
WITH mentat_people AS (
  SELECT
    elem ->> 0 AS name,
    elem ->> 1 AS email,
    elem ->> 2 AS dept_name
  FROM (
    SELECT mentat_query(
      '[:find ?name ?email ?dept-name
        :where
        [?e :person/name ?name]
        [?e :person/email ?email]
        [?e :person/department ?d]
        [?d :dept/name ?dept-name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
)
SELECT
  mp.name,
  mp.dept_name,
  te.project_name,
  SUM(te.hours) AS total_hours,
  COUNT(DISTINCT te.entry_date) AS days_worked
FROM mentat_people mp
JOIN time_entries te ON mp.email = te.person_email
GROUP BY mp.name, mp.dept_name, te.project_name
ORDER BY mp.name, total_hours DESC;
```

---

## Performance Considerations

### 1. Minimize Datalog calls in loops

Each `mentat_query()` call parses EDN, compiles Datalog to SQL, and executes
it via SPI. When you need to process many entities, prefer a single Datalog
query that returns all data over calling `mentat_query()` per entity in a loop.

```sql
-- SLOW: N+1 query pattern
SELECT mentat_pull('[*]', eid)
FROM generate_series(10000, 10100) AS eid;

-- FAST: Single query returning all data
SELECT mentat_query(
  '[:find ?e ?name ?age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]]',
  '{}'::jsonb
);
```

### 2. Use Datalog for what it does best

Let Datalog handle:
- Recursive graph traversal (`:with` rules)
- Pattern matching across entity-attribute-value triples
- Temporal queries (`asOf`, `since`, `history`)
- Full-text search (`fulltext`)

Let SQL handle:
- Aggregations with `GROUP BY` and window functions
- Sorting and pagination of large result sets
- Joining with relational tables
- Complex arithmetic and string manipulation

### 3. Materialize intermediate results

When a Datalog query result is used multiple times in SQL, wrap it in a CTE
rather than calling `mentat_query()` repeatedly. PostgreSQL evaluates CTEs
once and reuses the result.

```sql
-- The CTE is evaluated once, then reused
WITH team AS (
  SELECT mentat_query(
    '[:find ?e ?name ?age :where ...]',
    '{}'::jsonb
  )::jsonb AS result
)
SELECT ... FROM team ...
UNION ALL
SELECT ... FROM team ...;
```

### 4. Use `:limit` in Datalog when possible

If you only need the top N results, add `:limit` to the Datalog query rather
than returning all results and filtering in SQL:

```sql
-- Let Datalog limit early
SELECT mentat_query(
  '[:find ?name ?age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    :order (desc ?age)
    :limit 10]',
  '{}'::jsonb
);
```

### 5. Index attributes used in joins

When joining Mentat entity data with relational tables, the join key attribute
(e.g., `:person/email`) should have `:db/unique :db.unique/identity` or at
least `:db/index true` for efficient lookup.

---

## Best Practices

### When to use Datalog vs SQL

| Task | Recommendation |
|------|---------------|
| Find all ancestors of entity X | Datalog (recursive rules) |
| Count entities grouped by attribute | Datalog `(count ?e)` or SQL `COUNT(*)` |
| Rank entities within groups | SQL window functions |
| Full-text search | Datalog `fulltext` |
| Join with external tables | SQL `JOIN` |
| Point-in-time queries | Datalog temporal options |
| Complex aggregation (median, percentiles) | SQL |
| Existence checks (entity has/lacks attribute) | Datalog `not` clauses |
| Pagination | Datalog `:limit` + `:order` |
| Change tracking / audit log | Datalog `history` mode |

### Unwrapping Datalog JSON results

`mentat_query()` returns JSONB. The result structure depends on the `:find`
spec:

```sql
-- :find ?a ?b (relation) -> {"columns": [...], "results": [[...], ...]}
(result -> 'results')           -- array of row arrays

-- :find ?a . (scalar)    -> {"result": value}
(result -> 'result')            -- single value

-- :find [?a ...] (coll)  -> {"result": [value, ...]}
(result -> 'result')            -- array of scalars

-- :find [?a ?b] (tuple)  -> {"result": [value1, value2]}
(result -> 'result')            -- single array
```

Use `jsonb_array_elements()` to unnest relation results into rows:

```sql
SELECT elem ->> 0 AS col1, (elem -> 1)::int AS col2
FROM jsonb_array_elements(
  (mentat_query('...', '{}'::jsonb)::jsonb) -> 'results'
) AS elem;
```

### Transaction boundaries

When mixing `mentat_transact()` with SQL reads, all operations within a single
PostgreSQL transaction see a consistent snapshot. The Datalog query engine
translates to SQL that runs against `mentat.datoms`, so standard PostgreSQL
MVCC isolation applies.

```sql
BEGIN;
  -- Transaction 1: add data
  SELECT mentat_transact('[
    {:db/id "new-person" :person/name "Grace" :person/age 33}
  ]');

  -- This query will see Grace (same transaction)
  SELECT mentat_query(
    '[:find ?name :where [?e :person/name ?name]]',
    '{}'::jsonb
  );
COMMIT;
```

### Error handling in mixed queries

Wrap `mentat_query()` and `mentat_transact()` calls in exception handlers when
building production SQL pipelines:

```sql
DO $$
DECLARE
  result jsonb;
BEGIN
  result := mentat_query(
    '[:find ?name :where [?e :person/name ?name]]',
    '{}'::jsonb
  )::jsonb;

  -- Process result...
  RAISE NOTICE 'Found % people', jsonb_array_length(result -> 'results');
EXCEPTION
  WHEN OTHERS THEN
    RAISE WARNING 'Mentat query failed: %', SQLERRM;
END $$;
```
