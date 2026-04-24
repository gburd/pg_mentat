-- ==========================================================================
-- Example 1: Datalog for Graph Traversal, SQL for Aggregation
-- ==========================================================================
--
-- PROBLEM:
--   Find everyone who reports to Alice (directly or through the management
--   chain), then compute workforce statistics: headcount, average age,
--   salary distribution.
--
-- WHY HYBRID?
--   Pure SQL requires WITH RECURSIVE, which is verbose and hard to compose
--   for arbitrary-depth graph traversal. Datalog expresses recursive
--   relationships in two lines via rules. But Datalog has no window
--   functions, percentiles, or STDDEV -- SQL handles those naturally.
--
-- Prerequisite: run 00_setup.sql first.
-- ==========================================================================

-- ---------------------------------------------------------------------------
-- Approach A: Single Datalog query, SQL aggregation layer
-- ---------------------------------------------------------------------------

WITH datalog_result AS (
  SELECT mentat_query(
    '[:find ?name ?age ?salary
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
      [?r :person/age ?age]
      [?r :person/salary ?salary]]',
    '{}'::jsonb
  )::jsonb AS result
),
reports AS (
  SELECT
    elem ->> 0                AS name,
    (elem -> 1)::int          AS age,
    (elem -> 2)::bigint       AS salary
  FROM datalog_result,
       jsonb_array_elements(result -> 'results') AS elem
)
SELECT
  COUNT(*)                                 AS team_size,
  ROUND(AVG(age), 1)                      AS avg_age,
  MIN(age)                                 AS youngest,
  MAX(age)                                 AS oldest,
  SUM(salary)                              AS total_payroll,
  ROUND(AVG(salary))                       AS avg_salary,
  ROUND(STDDEV(salary))                    AS salary_stddev
FROM reports;

-- Expected output:
--  team_size | avg_age | youngest | oldest | total_payroll | avg_salary | salary_stddev
-- -----------+---------+----------+--------+---------------+------------+---------------
--          4 |    30.5 |       27 |     35 |        660000 |     165000 |         17078

-- ---------------------------------------------------------------------------
-- Approach B: Recursive Datalog + per-department breakdown
-- ---------------------------------------------------------------------------

WITH datalog_result AS (
  SELECT mentat_query(
    '[:find ?name ?age ?salary ?dept-name
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
      [?r :person/age ?age]
      [?r :person/salary ?salary]
      [?r :person/department ?d]
      [?d :dept/name ?dept-name]]',
    '{}'::jsonb
  )::jsonb AS result
),
reports AS (
  SELECT
    elem ->> 0          AS name,
    (elem -> 1)::int    AS age,
    (elem -> 2)::bigint AS salary,
    elem ->> 3          AS dept_name
  FROM datalog_result,
       jsonb_array_elements(result -> 'results') AS elem
)
SELECT
  dept_name,
  COUNT(*)               AS headcount,
  ROUND(AVG(salary))     AS avg_salary,
  SUM(salary)            AS dept_payroll
FROM reports
GROUP BY dept_name
ORDER BY dept_payroll DESC;

-- Expected output:
--   dept_name     | headcount | avg_salary | dept_payroll
-- ----------------+-----------+------------+--------------
--  Backend        |         2 |     177500 |       355000
--  Frontend       |         1 |     160000 |       160000
--  Infrastructure |         1 |     145000 |       145000
