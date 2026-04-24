-- ==========================================================================
-- Example 7: Joining Mentat Entities with Relational Tables
-- ==========================================================================
--
-- PROBLEM:
--   Your application stores flexible, schema-evolving entity data in Mentat
--   (people, projects, org structure) and high-volume, fixed-schema
--   operational data in relational tables (time entries, access logs,
--   metrics). You need to combine both for reporting.
--
-- WHY HYBRID?
--   This is the core value proposition of pg_mentat: Mentat and relational
--   tables live in the same PostgreSQL database. You can JOIN across both
--   without ETL, message queues, or external connectors. Mentat handles
--   the parts of your data model that evolve frequently; relational tables
--   handle the parts that are high-volume and fixed.
--
-- Prerequisite: run 00_setup.sql first (creates time_entries table).
-- ==========================================================================

-- ---------------------------------------------------------------------------
-- Pattern A: Time tracking report enriched with entity data
-- ---------------------------------------------------------------------------

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
  SUM(te.hours)                           AS total_hours,
  COUNT(DISTINCT te.entry_date)           AS days_worked,
  ROUND(SUM(te.hours) / COUNT(DISTINCT te.entry_date), 1) AS avg_hours_per_day
FROM mentat_people mp
JOIN time_entries te ON mp.email = te.person_email
GROUP BY mp.name, mp.dept_name, te.project_name
ORDER BY mp.dept_name, mp.name, total_hours DESC;

-- Expected output:
--     name     | dept_name   | project_name    | total_hours | days_worked | avg_hours_per_day
-- -------------+-------------+-----------------+-------------+-------------+-------------------
--  Bob Park    | Backend     | pg_mentat       |       14.50 |           2 |               7.3
--  Bob Park    | Backend     | Cloud Migration |        2.00 |           1 |               2.0
--  Dave Kim    | Backend     | pg_mentat       |       13.00 |           2 |               6.5
--  Dave Kim    | Backend     | Dashboard UI    |        3.00 |           1 |               3.0
--  Alice Chen  | Engineering | pg_mentat       |       15.50 |           2 |               7.8
--  Carol Davis | Frontend    | Dashboard UI    |       15.50 |           2 |               7.8
--  Eve Lopez   | Infra...    | Cloud Migration |       14.00 |           2 |               7.0

-- ---------------------------------------------------------------------------
-- Pattern B: Cross-project utilization with Mentat role context
-- ---------------------------------------------------------------------------
-- For each person, show their Mentat-stored role (project lead vs member)
-- alongside their relational time tracking data.

WITH project_roles AS (
  -- Leaders
  SELECT
    (elem -> 0)::bigint AS person_eid,
    elem ->> 1          AS person_name,
    elem ->> 2          AS project_name,
    'Lead' AS role
  FROM (
    SELECT mentat_query(
      '[:find ?person ?pname ?projname
        :where
        [?proj :project/lead ?person]
        [?person :person/name ?pname]
        [?proj :project/name ?projname]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem

  UNION ALL

  -- Members (who are not also the lead)
  SELECT
    (elem -> 0)::bigint AS person_eid,
    elem ->> 1          AS person_name,
    elem ->> 2          AS project_name,
    'Member' AS role
  FROM (
    SELECT mentat_query(
      '[:find ?person ?pname ?projname
        :where
        [?proj :project/member ?person]
        [?person :person/name ?pname]
        [?proj :project/name ?projname]
        (not [?proj :project/lead ?person])]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
-- Get email mapping for join
email_map AS (
  SELECT
    elem ->> 0 AS name,
    elem ->> 1 AS email
  FROM (
    SELECT mentat_query(
      '[:find ?name ?email
        :where
        [?e :person/name ?name]
        [?e :person/email ?email]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
)
SELECT
  pr.person_name,
  pr.project_name,
  pr.role,
  COALESCE(SUM(te.hours), 0) AS hours_logged,
  COUNT(DISTINCT te.entry_date) AS days_active,
  CASE
    WHEN pr.role = 'Lead' AND COALESCE(SUM(te.hours), 0) < 10
    THEN 'WARNING: Lead with low hours'
    ELSE 'OK'
  END AS flag
FROM project_roles pr
JOIN email_map em ON pr.person_name = em.name
LEFT JOIN time_entries te
  ON em.email = te.person_email
  AND te.project_name = pr.project_name
GROUP BY pr.person_name, pr.project_name, pr.role
ORDER BY pr.project_name, pr.role, hours_logged DESC;

-- ---------------------------------------------------------------------------
-- Pattern C: Department cost allocation using both data sources
-- ---------------------------------------------------------------------------
-- Compute cost per project per department using Mentat salary data
-- and relational time tracking data.

WITH people_with_salary AS (
  SELECT
    elem ->> 0          AS name,
    elem ->> 1          AS email,
    (elem -> 2)::bigint AS annual_salary,
    elem ->> 3          AS dept_name
  FROM (
    SELECT mentat_query(
      '[:find ?name ?email ?salary ?dept-name
        :where
        [?e :person/name ?name]
        [?e :person/email ?email]
        [?e :person/salary ?salary]
        [?e :person/department ?d]
        [?d :dept/name ?dept-name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
-- Assume 2080 working hours per year (40 hrs * 52 weeks)
hourly_costs AS (
  SELECT
    ps.name,
    ps.email,
    ps.dept_name,
    ps.annual_salary,
    ROUND(ps.annual_salary / 2080.0, 2) AS hourly_rate,
    te.project_name,
    SUM(te.hours) AS hours_on_project
  FROM people_with_salary ps
  JOIN time_entries te ON ps.email = te.person_email
  GROUP BY ps.name, ps.email, ps.dept_name, ps.annual_salary, te.project_name
)
SELECT
  project_name,
  dept_name,
  COUNT(DISTINCT name)                       AS contributors,
  SUM(hours_on_project)                      AS total_hours,
  ROUND(SUM(hours_on_project * hourly_rate)) AS allocated_cost
FROM hourly_costs
GROUP BY project_name, dept_name
ORDER BY project_name, allocated_cost DESC;

-- Expected output:
--  project_name    |   dept_name    | contributors | total_hours | allocated_cost
-- ----------------+----------------+--------------+-------------+----------------
--  Cloud Migration | Infrastructure |            1 |       14.00 |          977
--  Cloud Migration | Backend        |            1 |        2.00 |          178
--  Dashboard UI    | Frontend       |            1 |       15.50 |        1192
--  Dashboard UI    | Backend        |            1 |        3.00 |          245
--  pg_mentat       | Engineering    |            1 |       15.50 |        1639
--  pg_mentat       | Backend        |            2 |       27.50 |        2452
