-- ==========================================================================
-- Example 8: Pull API Results in SQL Pipelines
-- ==========================================================================
--
-- PROBLEM:
--   Use mentat_pull() to retrieve rich, nested entity data (following refs,
--   reverse lookups), then process the JSONB output with PostgreSQL's JSON
--   operators to build flat reports, nested API responses, or feed
--   downstream systems.
--
-- WHY HYBRID?
--   The Pull API returns hierarchical data in a single call -- no N+1
--   queries. SQL's JSONB operators then reshape that data for any
--   consumer: flat CSV exports, JSON API responses, or further joins.
--   This is more efficient than running multiple Datalog queries and
--   more flexible than Datalog's flat result tuples.
--
-- Prerequisite: run 00_setup.sql first.
-- ==========================================================================

-- ---------------------------------------------------------------------------
-- Pattern A: Pull project members and build a flat report
-- ---------------------------------------------------------------------------

WITH project_members AS (
  SELECT (elem -> 0)::bigint AS person_eid
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
      '[:person/name :person/email :person/age :person/salary
        {:person/department [:dept/name :dept/budget]}
        :person/skills]',
      pm.person_eid
    )::jsonb AS profile
  FROM project_members pm
)
-- Flatten JSONB into columnar report
SELECT
  profile ->> ':person/name'                              AS name,
  profile ->> ':person/email'                             AS email,
  (profile -> ':person/age')::int                         AS age,
  (profile -> ':person/salary')::bigint                   AS salary,
  profile -> ':person/department' ->> ':dept/name'        AS department,
  (profile -> ':person/department' -> ':dept/budget')::bigint AS dept_budget,
  jsonb_array_length(
    COALESCE(profile -> ':person/skills', '[]'::jsonb)
  )                                                       AS skill_count,
  profile -> ':person/skills'                             AS skills
FROM member_profiles
ORDER BY (profile -> ':person/salary')::bigint DESC;

-- ---------------------------------------------------------------------------
-- Pattern B: Pull with reverse lookups -- find who reports to each manager
-- ---------------------------------------------------------------------------

WITH managers AS (
  SELECT DISTINCT (elem -> 0)::bigint AS eid
  FROM (
    SELECT mentat_query(
      '[:find ?mgr
        :where
        [_ :person/manager ?mgr]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
manager_data AS (
  SELECT
    m.eid,
    mentat_pull(
      '[:person/name
        :person/email
        {:person/_manager [:person/name :person/email :person/age]}]',
      m.eid
    )::jsonb AS data
  FROM managers m
)
SELECT
  data ->> ':person/name'   AS manager_name,
  data ->> ':person/email'  AS manager_email,
  jsonb_array_length(
    COALESCE(data -> ':person/_manager', '[]'::jsonb)
  )                          AS direct_report_count,
  data -> ':person/_manager' AS direct_reports
FROM manager_data
ORDER BY direct_report_count DESC;

-- ---------------------------------------------------------------------------
-- Pattern C: Build a JSON API response using Pull + SQL
-- ---------------------------------------------------------------------------
-- Construct a complete project detail payload suitable for an API endpoint.

WITH projects AS (
  SELECT
    (elem -> 0)::bigint AS proj_eid,
    elem ->> 1          AS proj_name
  FROM (
    SELECT mentat_query(
      '[:find ?proj ?name
        :where
        [?proj :project/name ?name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
project_details AS (
  SELECT
    p.proj_name,
    mentat_pull(
      '[:project/name
        :project/status
        {:project/lead [:person/name :person/email]}
        {:project/member [:person/name :person/email :person/age
                          {:person/department [:dept/name]}]}]',
      p.proj_eid
    )::jsonb AS detail
  FROM projects p
)
SELECT jsonb_build_object(
  'project', pd.detail ->> ':project/name',
  'status',  pd.detail ->> ':project/status',
  'lead',    pd.detail -> ':project/lead' ->> ':person/name',
  'team_size', jsonb_array_length(
    COALESCE(pd.detail -> ':project/member', '[]'::jsonb)
  ),
  'members', (
    SELECT jsonb_agg(jsonb_build_object(
      'name',       m ->> ':person/name',
      'email',      m ->> ':person/email',
      'age',        (m -> ':person/age')::int,
      'department', m -> ':person/department' ->> ':dept/name'
    ) ORDER BY m ->> ':person/name')
    FROM jsonb_array_elements(
      COALESCE(pd.detail -> ':project/member', '[]'::jsonb)
    ) AS m
  )
) AS api_response
FROM project_details pd
ORDER BY pd.proj_name;

-- This produces JSON payloads like:
-- {
--   "project": "pg_mentat",
--   "status": ":status/active",
--   "lead": "Alice Chen",
--   "team_size": 3,
--   "members": [
--     {"name": "Alice Chen", "email": "alice@example.com", "age": 42, "department": "Engineering"},
--     {"name": "Bob Park",   "email": "bob@example.com",   "age": 35, "department": "Backend"},
--     {"name": "Dave Kim",   "email": "dave@example.com",  "age": 31, "department": "Backend"}
--   ]
-- }
