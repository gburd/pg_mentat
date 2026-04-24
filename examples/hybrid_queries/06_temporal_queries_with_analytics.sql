-- ==========================================================================
-- Example 6: Temporal Queries (Datalog) + SQL Analytics
-- ==========================================================================
--
-- PROBLEM:
--   Track how entities change over time: build an audit log, compare
--   snapshots, and generate change-frequency reports.
--
-- WHY HYBRID?
--   Datalog's temporal features (asOf, since, history) provide time-travel
--   semantics that SQL cannot replicate without manual versioning tables.
--   But analyzing the change data -- computing rates, grouping by time
--   windows, building trend lines -- is SQL's strength.
--
-- Prerequisite: run 00_setup.sql first.
-- ==========================================================================

-- ---------------------------------------------------------------------------
-- First, create some history by making changes
-- ---------------------------------------------------------------------------

-- Promote Dave (salary increase, new skills)
SELECT mentat_transact('[
  [:db/add [:person/email "dave@example.com"] :person/salary 185000]
  [:db/add [:person/email "dave@example.com"] :person/skills "Rust"]
]');

-- Carol gets a raise
SELECT mentat_transact('[
  [:db/add [:person/email "carol@example.com"] :person/salary 175000]
]');

-- Eve transitions to Backend
SELECT mentat_transact('[
  [:db/add [:person/email "eve@example.com"] :person/salary 155000]
]');

-- ---------------------------------------------------------------------------
-- Audit trail: full change history for an entity
-- ---------------------------------------------------------------------------

WITH history AS (
  SELECT
    (elem -> 0)::bigint   AS attr_id,
    elem ->> 1             AS value,
    (elem -> 2)::bigint   AS tx_id,
    (elem -> 3)::boolean  AS was_added
  FROM (
    SELECT mentat_query(
      '[:find ?a ?v ?tx ?added
        :where
        [?e :person/email "dave@example.com"]
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
  CASE WHEN h.was_added THEN 'ASSERT' ELSE 'RETRACT' END AS operation,
  -- Compute time since previous change to this attribute
  t.tx_instant - LAG(t.tx_instant) OVER (
    PARTITION BY h.attr_id ORDER BY h.tx_id
  ) AS time_since_last_change
FROM history h
JOIN mentat.transactions t ON h.tx_id = t.tx
ORDER BY t.tx_instant DESC, h.attr_id;

-- ---------------------------------------------------------------------------
-- Point-in-time comparison: team size and payroll at different snapshots
-- ---------------------------------------------------------------------------

WITH
  -- Current state
  current_state AS (
    SELECT
      elem ->> 0          AS dept_name,
      (elem -> 1)::int    AS headcount,
      (elem -> 2)::bigint AS total_salary
    FROM (
      SELECT mentat_query(
        '[:find ?dept-name (count ?e) (sum ?salary)
          :where
          [?e :person/salary ?salary]
          [?e :person/department ?d]
          [?d :dept/name ?dept-name]]',
        '{}'::jsonb
      )::jsonb AS result
    ) q,
    jsonb_array_elements(q.result -> 'results') AS elem
  ),
  -- State at the first transaction (initial load)
  initial_state AS (
    SELECT
      elem ->> 0          AS dept_name,
      (elem -> 1)::int    AS headcount,
      (elem -> 2)::bigint AS total_salary
    FROM (
      SELECT mentat_query(
        '[:find ?dept-name (count ?e) (sum ?salary)
          :where
          [?e :person/salary ?salary]
          [?e :person/department ?d]
          [?d :dept/name ?dept-name]]',
        '{"asOf": 1000002}'::jsonb
      )::jsonb AS result
    ) q,
    jsonb_array_elements(q.result -> 'results') AS elem
  )
SELECT
  COALESCE(c.dept_name, i.dept_name) AS dept_name,
  COALESCE(i.headcount, 0)           AS initial_headcount,
  COALESCE(c.headcount, 0)           AS current_headcount,
  COALESCE(c.headcount, 0) - COALESCE(i.headcount, 0) AS headcount_change,
  COALESCE(i.total_salary, 0)        AS initial_payroll,
  COALESCE(c.total_salary, 0)        AS current_payroll,
  COALESCE(c.total_salary, 0) - COALESCE(i.total_salary, 0) AS payroll_change
FROM current_state c
FULL OUTER JOIN initial_state i ON c.dept_name = i.dept_name
ORDER BY payroll_change DESC;

-- ---------------------------------------------------------------------------
-- Recent changes feed with SQL grouping
-- ---------------------------------------------------------------------------
-- Get all changes since a known transaction and summarize by entity

WITH recent_changes AS (
  SELECT
    (elem -> 0)::bigint AS entity_id,
    (elem -> 1)::bigint AS attr_id,
    elem ->> 2          AS value,
    (elem -> 3)::bigint AS tx_id
  FROM (
    SELECT mentat_query(
      '[:find ?e ?a ?v ?tx
        :where
        [?e ?a ?v ?tx]]',
      '{"since": 1000002}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
change_summary AS (
  SELECT
    rc.entity_id,
    COUNT(*)                                AS total_changes,
    COUNT(DISTINCT rc.attr_id)              AS attributes_changed,
    MIN(t.tx_instant)                       AS first_change,
    MAX(t.tx_instant)                       AS last_change,
    COUNT(DISTINCT rc.tx_id)                AS transactions_involved
  FROM recent_changes rc
  JOIN mentat.transactions t ON rc.tx_id = t.tx
  GROUP BY rc.entity_id
)
SELECT
  cs.entity_id,
  -- Try to resolve entity name via Datalog
  (mentat_query(
    '[:find ?name .
      :where
      [' || cs.entity_id || ' :person/name ?name]]',
    '{}'::jsonb
  )::jsonb ->> 'result') AS entity_name,
  cs.total_changes,
  cs.attributes_changed,
  cs.transactions_involved,
  cs.first_change,
  cs.last_change
FROM change_summary cs
ORDER BY cs.total_changes DESC
LIMIT 20;
