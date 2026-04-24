-- ==========================================================================
-- Example 3: PostgreSQL Window Functions over Datalog Results
-- ==========================================================================
--
-- PROBLEM:
--   Rank employees within their department by salary, compute each person's
--   salary as a percentage of their department total, and show the company-
--   wide percentile for each employee.
--
-- WHY HYBRID?
--   Datalog has no window functions. It cannot express RANK(), PERCENT_RANK(),
--   NTILE(), or any partitioned computation. By extracting entity data
--   through a single Datalog query and feeding it into SQL, you get the
--   full power of PostgreSQL's window function engine.
--
-- Prerequisite: run 00_setup.sql first.
-- ==========================================================================

-- ---------------------------------------------------------------------------
-- Single Datalog extraction, full SQL analytics
-- ---------------------------------------------------------------------------

WITH people AS (
  SELECT
    elem ->> 0          AS name,
    (elem -> 1)::int    AS age,
    (elem -> 2)::bigint AS salary,
    elem ->> 3          AS dept_name
  FROM (
    SELECT mentat_query(
      '[:find ?name ?age ?salary ?dept-name
        :where
        [?e :person/name ?name]
        [?e :person/age ?age]
        [?e :person/salary ?salary]
        [?e :person/department ?d]
        [?d :dept/name ?dept-name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
)
SELECT
  name,
  dept_name,
  salary,
  -- Rank within department by salary (highest = 1)
  RANK() OVER (
    PARTITION BY dept_name ORDER BY salary DESC
  ) AS dept_salary_rank,
  -- Salary as percentage of department total
  ROUND(
    100.0 * salary / SUM(salary) OVER (PARTITION BY dept_name),
    1
  ) AS pct_of_dept_payroll,
  -- Company-wide salary percentile
  ROUND(
    PERCENT_RANK() OVER (ORDER BY salary)::numeric,
    2
  ) AS company_percentile,
  -- Department headcount
  COUNT(*) OVER (PARTITION BY dept_name) AS dept_size,
  -- Salary quartile across entire company
  NTILE(4) OVER (ORDER BY salary) AS salary_quartile
FROM people
ORDER BY dept_name, salary DESC;

-- Expected output:
--      name      |   dept_name    | salary | dept_salary_rank | pct_of_dept_payroll | company_percentile | dept_size | salary_quartile
-- ---------------+----------------+--------+------------------+---------------------+--------------------+-----------+-----------------
--  Bob Park      | Backend        | 185000 |                1 |                52.1 |               0.83 |         2 |               4
--  Dave Kim      | Backend        | 170000 |                2 |                47.9 |               0.67 |         2 |               3
--  Alice Chen    | Engineering    | 220000 |                1 |               100.0 |               1.00 |         1 |               4
--  Carol Davis   | Frontend       | 160000 |                1 |               100.0 |               0.50 |         1 |               3
--  Eve Lopez     | Infrastructure | 145000 |                1 |               100.0 |               0.17 |         1 |               1
--  Grace Hopper  | Marketing      | 155000 |                1 |               100.0 |               0.33 |         1 |               2
--  Frank Wu      | Sales          | 175000 |                1 |               100.0 |               0.83 |         1 |               3

-- ---------------------------------------------------------------------------
-- Salary band analysis: bucket employees into bands and summarize
-- ---------------------------------------------------------------------------

WITH people AS (
  SELECT
    elem ->> 0          AS name,
    (elem -> 1)::bigint AS salary,
    elem ->> 2          AS dept_name
  FROM (
    SELECT mentat_query(
      '[:find ?name ?salary ?dept-name
        :where
        [?e :person/name ?name]
        [?e :person/salary ?salary]
        [?e :person/department ?d]
        [?d :dept/name ?dept-name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
banded AS (
  SELECT
    name,
    salary,
    dept_name,
    CASE
      WHEN salary >= 200000 THEN 'Senior Leadership'
      WHEN salary >= 170000 THEN 'Senior IC / Manager'
      WHEN salary >= 150000 THEN 'Mid-Level'
      ELSE 'Early Career'
    END AS salary_band
  FROM people
)
SELECT
  salary_band,
  COUNT(*)            AS headcount,
  ROUND(AVG(salary))  AS avg_salary,
  MIN(salary)         AS min_salary,
  MAX(salary)         AS max_salary
FROM banded
GROUP BY salary_band
ORDER BY avg_salary DESC;

-- Expected output:
--      salary_band       | headcount | avg_salary | min_salary | max_salary
-- -----------------------+-----------+------------+------------+------------
--  Senior Leadership     |         1 |     220000 |     220000 |     220000
--  Senior IC / Manager   |         3 |     176667 |     170000 |     185000
--  Mid-Level             |         2 |     157500 |     155000 |     160000
--  Early Career          |         1 |     145000 |     145000 |     145000
