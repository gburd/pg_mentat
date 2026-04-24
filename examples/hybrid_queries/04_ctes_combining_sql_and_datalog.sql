-- ==========================================================================
-- Example 4: Hybrid CTEs Combining SQL and Datalog
-- ==========================================================================
--
-- PROBLEM:
--   Build a department budget report that:
--   1. Traverses the department hierarchy to find all sub-departments of
--      Engineering (graph problem -> Datalog)
--   2. Counts headcount and computes average salary per department
--      (aggregation -> either Datalog or SQL)
--   3. Computes budget utilization ratios and flags departments that are
--      over or under budget based on headcount (arithmetic -> SQL)
--
-- WHY HYBRID?
--   CTEs are the natural bridge between Datalog and SQL. Each CTE can call
--   mentat_query() once, extract its results, and pass them downstream.
--   This avoids N+1 patterns and keeps the query plan readable.
--   Datalog handles the recursive hierarchy; SQL handles the math.
--
-- Prerequisite: run 00_setup.sql first.
-- ==========================================================================

-- ---------------------------------------------------------------------------
-- Department budget utilization report
-- ---------------------------------------------------------------------------

-- CTE 1: Traverse department tree via Datalog
WITH eng_tree AS (
  SELECT
    (elem -> 0)::bigint AS dept_eid,
    elem ->> 1          AS dept_name,
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

  UNION ALL

  -- Include the Engineering department itself
  SELECT
    (elem -> 0)::bigint AS dept_eid,
    elem ->> 1          AS dept_name,
    (elem -> 2)::bigint AS budget
  FROM (
    SELECT mentat_query(
      '[:find ?d ?name ?budget
        :where
        [?d :dept/name "Engineering"]
        [?d :dept/name ?name]
        [?d :dept/budget ?budget]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),

-- CTE 2: Get headcount and salary data per department from Datalog
dept_people AS (
  SELECT
    elem ->> 0          AS dept_name,
    (elem -> 1)::int    AS headcount,
    (elem -> 2)::bigint AS total_salary
  FROM (
    SELECT mentat_query(
      '[:find ?dept-name (count ?person) (sum ?salary)
        :where
        [?person :person/department ?d]
        [?person :person/salary ?salary]
        [?d :dept/name ?dept-name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),

-- CTE 3: Join and compute budget metrics (pure SQL)
budget_report AS (
  SELECT
    et.dept_name,
    et.budget,
    COALESCE(dp.headcount, 0)    AS headcount,
    COALESCE(dp.total_salary, 0) AS total_salary,
    CASE
      WHEN COALESCE(dp.headcount, 0) > 0
      THEN et.budget / dp.headcount
      ELSE NULL
    END AS budget_per_head,
    CASE
      WHEN COALESCE(dp.total_salary, 0) > 0
      THEN ROUND(100.0 * dp.total_salary / et.budget, 1)
      ELSE 0
    END AS salary_budget_pct
  FROM eng_tree et
  LEFT JOIN dept_people dp ON et.dept_name = dp.dept_name
)

-- Final output with status flags
SELECT
  dept_name,
  budget,
  headcount,
  total_salary,
  budget_per_head,
  salary_budget_pct || '%' AS payroll_utilization,
  CASE
    WHEN salary_budget_pct > 80 THEN 'OVER-UTILIZED'
    WHEN salary_budget_pct > 50 THEN 'HEALTHY'
    WHEN headcount = 0          THEN 'UNSTAFFED'
    ELSE 'UNDER-UTILIZED'
  END AS status
FROM budget_report
ORDER BY budget DESC;

-- Expected output:
--    dept_name     | budget  | headcount | total_salary | budget_per_head | payroll_utilization | status
-- ----------------+---------+-----------+--------------+-----------------+---------------------+----------------
--  Engineering     | 5000000 |         1 |       220000 |         5000000 | 4.4%                | UNDER-UTILIZED
--  Backend         | 2000000 |         2 |       355000 |         1000000 | 17.8%               | UNDER-UTILIZED
--  Frontend        | 1500000 |         1 |       160000 |         1500000 | 10.7%               | UNDER-UTILIZED
--  Infrastructure  | 1500000 |         1 |       145000 |         1500000 | 9.7%                | UNDER-UTILIZED

-- ---------------------------------------------------------------------------
-- Cross-department project staffing analysis
-- ---------------------------------------------------------------------------
-- Shows which projects draw from which departments

WITH project_members AS (
  SELECT
    elem ->> 0          AS project_name,
    elem ->> 1          AS person_name,
    elem ->> 2          AS dept_name
  FROM (
    SELECT mentat_query(
      '[:find ?proj-name ?person-name ?dept-name
        :where
        [?proj :project/name ?proj-name]
        [?proj :project/member ?person]
        [?person :person/name ?person-name]
        [?person :person/department ?d]
        [?d :dept/name ?dept-name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
)
SELECT
  project_name,
  COUNT(DISTINCT dept_name)               AS departments_involved,
  COUNT(*)                                AS team_size,
  STRING_AGG(DISTINCT dept_name, ', '
    ORDER BY dept_name)                   AS departments,
  STRING_AGG(person_name, ', '
    ORDER BY person_name)                 AS members
FROM project_members
GROUP BY project_name
ORDER BY team_size DESC;

-- Expected output:
--   project_name   | departments_involved | team_size | departments                     | members
-- ----------------+---------------------+-----------+---------------------------------+-----------------------------
--  pg_mentat       |                   2 |         3 | Backend, Engineering            | Alice Chen, Bob Park, Dave Kim
--  Cloud Migration |                   2 |         2 | Backend, Infrastructure         | Bob Park, Eve Lopez
--  Dashboard UI    |                   2 |         2 | Backend, Frontend               | Carol Davis, Dave Kim
