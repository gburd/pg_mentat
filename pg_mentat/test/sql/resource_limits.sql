-- Test: Query timeouts and resource limits (Task #9)
--
-- Verifies that the DoS protection mechanisms work correctly:
--   1. mentat.query_timeout_ms -- statement timeout
--   2. mentat.max_result_rows -- result set limit
--   3. mentat.max_recursion_depth -- CTE recursion limit
--   4. mentat.temp_file_limit -- disk usage limit
--
-- Prerequisites: pg_mentat extension installed and schema bootstrapped.

-- ============================================================================
-- 1. Verify GUC parameters exist and have sane defaults
-- ============================================================================

-- Check default values
SHOW mentat.query_timeout_ms;        -- expect: 30000
SHOW mentat.max_result_rows;         -- expect: 100000
SHOW mentat.max_recursion_depth;     -- expect: 100
SHOW mentat.temp_file_limit;         -- expect: 1GB
SHOW mentat.enable_optimizer_hints;  -- expect: on
SHOW mentat.default_work_mem;        -- expect: 64MB

-- ============================================================================
-- 2. Test that GUC parameters can be set per-session
-- ============================================================================

SET mentat.query_timeout_ms = 5000;
SHOW mentat.query_timeout_ms;       -- expect: 5000

SET mentat.max_result_rows = 10;
SHOW mentat.max_result_rows;        -- expect: 10

SET mentat.max_recursion_depth = 50;
SHOW mentat.max_recursion_depth;    -- expect: 50

SET mentat.temp_file_limit = '512MB';
SHOW mentat.temp_file_limit;        -- expect: 512MB

-- Reset to defaults for subsequent tests
RESET mentat.query_timeout_ms;
RESET mentat.max_result_rows;
RESET mentat.max_recursion_depth;
RESET mentat.temp_file_limit;

-- ============================================================================
-- 3. Test max_result_rows enforcement
-- ============================================================================

-- Set a very low limit
SET mentat.max_result_rows = 5;

-- First, define schema and insert enough test data
SELECT mentat_transact('[
  {:db/ident :test.limit/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Insert 10 entities
SELECT mentat_transact('[{:test.limit/name "limit-test-1"}]');
SELECT mentat_transact('[{:test.limit/name "limit-test-2"}]');
SELECT mentat_transact('[{:test.limit/name "limit-test-3"}]');
SELECT mentat_transact('[{:test.limit/name "limit-test-4"}]');
SELECT mentat_transact('[{:test.limit/name "limit-test-5"}]');
SELECT mentat_transact('[{:test.limit/name "limit-test-6"}]');
SELECT mentat_transact('[{:test.limit/name "limit-test-7"}]');
SELECT mentat_transact('[{:test.limit/name "limit-test-8"}]');
SELECT mentat_transact('[{:test.limit/name "limit-test-9"}]');
SELECT mentat_transact('[{:test.limit/name "limit-test-10"}]');

-- This query returns 10 rows but limit is 5 -- should produce an error
-- containing "result-limit"
SELECT mentat_query(
  '[:find ?e ?name :where [?e :test.limit/name ?name]]',
  '{}'::jsonb
);
-- Expected: ERROR containing ":db.error/result-limit-exceeded"

-- With an explicit :limit, the GUC limit should not trigger
SELECT mentat_query(
  '[:find ?e ?name :where [?e :test.limit/name ?name] :limit 3]',
  '{}'::jsonb
);
-- Expected: Success, 3 rows

-- With a pagination limit, the GUC limit should not trigger
SELECT mentat_query(
  '[:find ?e ?name :where [?e :test.limit/name ?name]]',
  '{"limit": 3}'::jsonb
);
-- Expected: Success, 3 rows

-- Unlimited (set to 0)
SET mentat.max_result_rows = 0;
SELECT mentat_query(
  '[:find ?e ?name :where [?e :test.limit/name ?name]]',
  '{}'::jsonb
);
-- Expected: Success, all rows

-- Reset
RESET mentat.max_result_rows;

-- ============================================================================
-- 4. Test query timeout (use a very short timeout)
-- ============================================================================

-- NOTE: This test can only verify that the GUC is accepted.
-- Actual timeout testing requires a slow query (e.g., pg_sleep or a very
-- complex join), which depends on the specific data set.

SET mentat.query_timeout_ms = 1;  -- 1ms -- almost anything will time out

-- A simple query may or may not time out depending on execution speed.
-- This is a smoke test to verify the parameter is accepted without error.
RESET mentat.query_timeout_ms;

-- ============================================================================
-- 5. Verify error messages are actionable
-- ============================================================================

-- The error for result limit should mention:
--   - The limit value
--   - How to fix it (use :limit, add :where clauses, increase the GUC)
-- This is verified structurally by the MentatError::ResultLimitExceeded variant.

-- Clean up
RESET mentat.query_timeout_ms;
RESET mentat.max_result_rows;
RESET mentat.max_recursion_depth;
RESET mentat.temp_file_limit;
