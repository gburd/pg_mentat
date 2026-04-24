-- ==========================================================================
-- Example 2: SQL Bulk Filtering + Datalog Validation
-- ==========================================================================
--
-- PROBLEM:
--   Identify senior engineers (age >= 30, in an engineering sub-department)
--   and for each one determine: which projects they lead, whether they have
--   direct reports, and whether they have the "PostgreSQL" skill.
--
-- WHY HYBRID?
--   SQL excels at set-based filtering with WHERE clauses and IN lists.
--   Datalog excels at checking graph relationships: "does person X lead
--   any project?", "does person X have any reports?", "does person X
--   have skill Y in a cardinality-many attribute?". Each of these is a
--   simple Datalog pattern; in SQL they require correlated subqueries
--   against the EAV store.
--
-- Prerequisite: run 00_setup.sql first.
-- ==========================================================================

-- ---------------------------------------------------------------------------
-- Step 1: Datalog fetches people with department info
-- Step 2: SQL filters to senior engineers
-- Step 3: Datalog validates relationships per person
-- ---------------------------------------------------------------------------

WITH people AS (
  SELECT
    (elem -> 0)::bigint AS eid,
    elem ->> 1          AS name,
    (elem -> 2)::int    AS age,
    elem ->> 3          AS dept_name
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
-- SQL filter: senior engineers only
senior_engineers AS (
  SELECT * FROM people
  WHERE age >= 30
    AND dept_name IN ('Backend', 'Frontend', 'Infrastructure', 'Engineering')
),
-- Datalog validation: check project leadership for each person
validated AS (
  SELECT
    se.name,
    se.age,
    se.dept_name,
    -- Does this person lead a project?
    (mentat_query(
      '[:find ?proj-name .
        :where
        [?proj :project/lead ' || se.eid || ']
        [?proj :project/name ?proj-name]]',
      '{}'::jsonb
    )::jsonb ->> 'result') AS leads_project,
    -- Does this person have direct reports?
    (mentat_query(
      '[:find (count ?report) .
        :where
        [?report :person/manager ' || se.eid || ']]',
      '{}'::jsonb
    )::jsonb -> 'result')::int AS direct_report_count,
    -- Does this person have the PostgreSQL skill?
    (mentat_query(
      '[:find ?skill .
        :where
        [' || se.eid || ' :person/skills ?skill]
        [(= ?skill "PostgreSQL")]]',
      '{}'::jsonb
    )::jsonb ->> 'result') IS NOT NULL AS has_postgresql_skill
  FROM senior_engineers se
)
SELECT
  name,
  age,
  dept_name,
  COALESCE(leads_project, '(none)') AS leads_project,
  direct_report_count,
  has_postgresql_skill
FROM validated
ORDER BY age DESC;

-- Expected output:
--      name     | age | dept_name   | leads_project | direct_report_count | has_postgresql_skill
-- -------------+-----+-------------+---------------+---------------------+---------------------
--  Alice Chen   |  42 | Engineering | pg_mentat     |                   2 | t
--  Bob Park     |  35 | Backend     | Cloud Migr... |                   2 | t
--  Dave Kim     |  31 | Backend     | (none)        |                   0 | t
