-- ==========================================================================
-- Example 5: Full-Text Search (SQL/Datalog) + Graph Queries
-- ==========================================================================
--
-- PROBLEM:
--   Search employee bios for domain expertise (e.g., "database"), then
--   discover their team connections: which projects they belong to, who
--   they report to, and what skills overlap with the search term context.
--
-- WHY HYBRID?
--   Full-text search produces a flat ranked list of matches. But a hiring
--   manager or team planner needs to see each match in context: their
--   projects, their manager chain, their co-workers. Datalog's graph
--   traversal enriches FTS results with relationship data that would
--   require multiple JOINs against the EAV store in pure SQL.
--
-- Prerequisite: run 00_setup.sql first.
-- ==========================================================================

-- ---------------------------------------------------------------------------
-- Pattern A: Datalog fulltext + graph enrichment + SQL presentation
-- ---------------------------------------------------------------------------

WITH fts_matches AS (
  SELECT
    (elem -> 0)::bigint   AS eid,
    elem ->> 1             AS name,
    elem ->> 2             AS bio_text,
    (elem -> 3)::float     AS relevance_score
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
-- Enrich each match with project and manager info via Datalog
enriched AS (
  SELECT
    fm.name,
    fm.bio_text,
    fm.relevance_score,
    -- Projects this person belongs to
    (mentat_query(
      '[:find [?proj-name ...]
        :where
        [?proj :project/member ' || fm.eid || ']
        [?proj :project/name ?proj-name]]',
      '{}'::jsonb
    )::jsonb -> 'result') AS projects,
    -- Manager name
    (mentat_query(
      '[:find ?mgr-name .
        :where
        [' || fm.eid || ' :person/manager ?mgr]
        [?mgr :person/name ?mgr-name]]',
      '{}'::jsonb
    )::jsonb ->> 'result') AS manager_name,
    -- Skills
    (mentat_query(
      '[:find [?skill ...]
        :where
        [' || fm.eid || ' :person/skills ?skill]]',
      '{}'::jsonb
    )::jsonb -> 'result') AS skills
  FROM fts_matches fm
)
SELECT
  name,
  LEFT(bio_text, 65) || '...' AS bio_excerpt,
  ROUND(relevance_score::numeric, 4) AS score,
  COALESCE(manager_name, '(top-level)') AS reports_to,
  projects,
  skills
FROM enriched
ORDER BY relevance_score DESC;

-- Expected output (columns truncated for readability):
--     name     |                    bio_excerpt                     | score  | reports_to  |       projects        |          skills
-- -------------+----------------------------------------------------+--------+-------------+-----------------------+---------------------------
--  Alice Chen   | VP of Engineering with 20 years of database exp... | 0.xxxx | (top-level) | ["pg_mentat"]         | ["Rust","PostgreSQL","Datalog"]
--  Dave Kim     | Senior engineer working on distributed database... | 0.xxxx | Bob Park    | ["pg_mentat","Dash..] | ["Go","Kubernetes","PostgreSQL"]

-- ---------------------------------------------------------------------------
-- Pattern B: Search + skill overlap matrix
-- ---------------------------------------------------------------------------
-- Find people whose bios match a search term, then show which of their
-- skills appear most frequently across the entire company.

WITH fts_people AS (
  SELECT
    (elem -> 0)::bigint AS eid,
    elem ->> 1          AS name
  FROM (
    SELECT mentat_query(
      '[:find ?e ?name
        :where
        [(fulltext $ :person/bio "database") [[?e _ _ _]]]
        [?e :person/name ?name]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
),
-- Get all skills for matched people
matched_skills AS (
  SELECT
    fp.name AS person_name,
    elem ->> 0 AS skill
  FROM fts_people fp,
  LATERAL (
    SELECT mentat_query(
      '[:find [?skill ...]
        :where
        [' || fp.eid || ' :person/skills ?skill]]',
      '{}'::jsonb
    )::jsonb AS result
  ) sq,
  jsonb_array_elements_text(sq.result -> 'result') AS elem
),
-- Count how many people in the company share each skill
skill_popularity AS (
  SELECT
    elem ->> 0 AS skill,
    (elem -> 1)::int AS company_wide_count
  FROM (
    SELECT mentat_query(
      '[:find ?skill (count ?e)
        :where
        [?e :person/skills ?skill]]',
      '{}'::jsonb
    )::jsonb AS result
  ) q,
  jsonb_array_elements(q.result -> 'results') AS elem
)
SELECT
  ms.person_name,
  ms.skill,
  sp.company_wide_count,
  CASE
    WHEN sp.company_wide_count >= 3 THEN 'common'
    WHEN sp.company_wide_count = 2  THEN 'shared'
    ELSE 'unique'
  END AS rarity
FROM matched_skills ms
JOIN skill_popularity sp ON ms.skill = sp.skill
ORDER BY ms.person_name, sp.company_wide_count DESC;

-- Expected output:
--  person_name |    skill    | company_wide_count | rarity
-- -------------+-------------+--------------------+--------
--  Alice Chen  | PostgreSQL  |                  3 | common
--  Alice Chen  | Rust        |                  2 | shared
--  Alice Chen  | Datalog     |                  1 | unique
--  Dave Kim    | PostgreSQL  |                  3 | common
--  Dave Kim    | Kubernetes  |                  2 | shared
--  Dave Kim    | Go          |                  2 | shared
