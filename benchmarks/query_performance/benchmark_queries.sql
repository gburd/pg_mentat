-- Query Performance Benchmarks
-- Tests various query patterns at scale to validate performance claims

-- =============================================================================
-- Setup: Enable timing and query analysis
-- =============================================================================

\timing on
\pset pager off

-- =============================================================================
-- Benchmark 1: Simple Pattern Query (baseline)
-- =============================================================================

\echo '\n=== Benchmark 1: Simple Pattern Query ==='
\echo 'Query: Find all names'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('[:find ?name :where [?e :bench/name ?name]]'::text, '{}'::jsonb);

-- Expected: <50ms for 1M datoms, <200ms for 10M datoms

-- =============================================================================
-- Benchmark 2: Join Query (2 patterns)
-- =============================================================================

\echo '\n=== Benchmark 2: Join Query (2 patterns) ==='
\echo 'Query: Find names and departments'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('
    [:find ?name ?dept
     :where [?e :bench/name ?name]
            [?e :bench/dept ?dept]]'::text, '{}'::jsonb);

-- Expected: <100ms for 1M datoms, <500ms for 10M datoms

-- =============================================================================
-- Benchmark 3: Join with Predicate
-- =============================================================================

\echo '\n=== Benchmark 3: Join with Predicate ==='
\echo 'Query: Find names and ages where age > 40'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('
    [:find ?name ?age
     :where [?e :bench/name ?name]
            [?e :bench/age ?age]
            [(> ?age 40)]]'::text, '{}'::jsonb);

-- Expected: <150ms for 1M datoms, <600ms for 10M datoms

-- =============================================================================
-- Benchmark 4: Complex Join (3+ patterns)
-- =============================================================================

\echo '\n=== Benchmark 4: Complex Join (3+ patterns) ==='
\echo 'Query: Find names, ages, departments, and salaries'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('
    [:find ?name ?age ?dept ?salary
     :where [?e :bench/name ?name]
            [?e :bench/age ?age]
            [?e :bench/dept ?dept]
            [?e :bench/salary ?salary]]'::text, '{}'::jsonb);

-- Expected: <200ms for 1M datoms, <800ms for 10M datoms

-- =============================================================================
-- Benchmark 5: OR-join Query
-- =============================================================================

\echo '\n=== Benchmark 5: OR-join Query ==='
\echo 'Query: Find entities in Engineering OR Sales departments'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('
    [:find ?name
     :where [?e :bench/name ?name]
            (or [?e :bench/dept "Engineering"]
                [?e :bench/dept "Sales"])]'::text, '{}'::jsonb);

-- Expected: <250ms for 1M datoms, <1000ms for 10M datoms

-- =============================================================================
-- Benchmark 6: OR-join with Predicates (newly implemented feature)
-- =============================================================================

\echo '\n=== Benchmark 6: OR-join with Predicates ==='
\echo 'Query: Find high-salary Engineering or high-score Sales employees'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('
    [:find ?name
     :where [?e :bench/name ?name]
            (or (and [?e :bench/dept "Engineering"]
                     [?e :bench/salary ?sal]
                     [(> ?sal 80000)])
                (and [?e :bench/dept "Sales"]
                     [?e :bench/score ?score]
                     [(> ?score 85)]))]'::text, '{}'::jsonb);

-- Expected: <300ms for 1M datoms, <1200ms for 10M datoms

-- =============================================================================
-- Benchmark 7: Aggregate Query
-- =============================================================================

\echo '\n=== Benchmark 7: Aggregate Query ==='
\echo 'Query: Count employees and average salary by department'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('
    [:find ?dept (count ?e) (avg ?salary)
     :where [?e :bench/dept ?dept]
            [?e :bench/salary ?salary]]'::text, '{}'::jsonb);

-- Expected: <500ms for 1M datoms, <2000ms for 10M datoms

-- =============================================================================
-- Benchmark 8: NOT clause
-- =============================================================================

\echo '\n=== Benchmark 8: NOT clause ==='
\echo 'Query: Find entities without bio attribute'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('
    [:find ?name
     :where [?e :bench/name ?name]
            (not [?e :bench/bio])]'::text, '{}'::jsonb);

-- Expected: <400ms for 1M datoms, <1600ms for 10M datoms

-- =============================================================================
-- Benchmark 9: Full-text Search
-- =============================================================================

\echo '\n=== Benchmark 9: Full-text Search ==='
\echo 'Query: Full-text search in bio field'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('
    [:find ?e ?name ?bio
     :where [?e :bench/name ?name]
            [?e :bench/bio ?bio]
            [(fulltext $ :bench/bio "person") [[?e ?bio]]]]'::text, '{}'::jsonb);

-- Expected: <600ms for 1M datoms, <2500ms for 10M datoms

-- =============================================================================
-- Benchmark 10: Cardinality-many Query
-- =============================================================================

\echo '\n=== Benchmark 10: Cardinality-many Query ==='
\echo 'Query: Find entities with specific tag'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('
    [:find ?name
     :where [?e :bench/name ?name]
            [?e :bench/tags "tag42"]]'::text, '{}'::jsonb);

-- Expected: <200ms for 1M datoms, <800ms for 10M datoms

-- =============================================================================
-- Benchmark 11: Rule Query (tests predicates in rule bodies)
-- =============================================================================

\echo '\n=== Benchmark 11: Rule Query with Predicate ==='
\echo 'Query: Find senior employees (age >= 40) using rule'

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
SELECT mentat_query('
    [:find ?name
     :in $ %
     :where (senior ?e)
            [?e :bench/name ?name]]'::text,
    '{"rules": "[(senior ?person) [?person :bench/age ?age] [(>= ?age 40)]]"}'::jsonb);

-- Expected: <300ms for 1M datoms, <1200ms for 10M datoms

-- =============================================================================
-- Benchmark 12: Recursive Rule Query
-- =============================================================================

\echo '\n=== Benchmark 12: Recursive Rule Query ==='
\echo 'Query: Test recursive rule performance'

-- Note: This requires manager relationships to be added to the dataset
-- Skipping for now, would need manager attribute in schema

-- =============================================================================
-- Summary Statistics
-- =============================================================================

\echo '\n=== Summary: Database Statistics ==='

-- Count entities
SELECT COUNT(DISTINCT e) AS entity_count
FROM (
    SELECT e FROM mentat.datoms_ref_new WHERE added = true
    UNION ALL
    SELECT e FROM mentat.datoms_long_new WHERE added = true
    UNION ALL
    SELECT e FROM mentat.datoms_text_new WHERE added = true
    UNION ALL
    SELECT e FROM mentat.datoms_double_new WHERE added = true
    UNION ALL
    SELECT e FROM mentat.datoms_boolean_new WHERE added = true
    UNION ALL
    SELECT e FROM mentat.datoms_instant_new WHERE added = true
) all_entities;

-- Count datoms
SELECT COUNT(*) AS datom_count
FROM (
    SELECT * FROM mentat.datoms_ref_new WHERE added = true
    UNION ALL
    SELECT * FROM mentat.datoms_long_new WHERE added = true
    UNION ALL
    SELECT * FROM mentat.datoms_text_new WHERE added = true
    UNION ALL
    SELECT * FROM mentat.datoms_double_new WHERE added = true
    UNION ALL
    SELECT * FROM mentat.datoms_boolean_new WHERE added = true
    UNION ALL
    SELECT * FROM mentat.datoms_instant_new WHERE added = true
    UNION ALL
    SELECT * FROM mentat.datoms_keyword_new WHERE added = true
    UNION ALL
    SELECT * FROM mentat.datoms_uuid_new WHERE added = true
    UNION ALL
    SELECT * FROM mentat.datoms_bytes_new WHERE added = true
) all_datoms;

-- Table sizes
SELECT
    tablename,
    pg_size_pretty(pg_total_relation_size('mentat.' || tablename)) AS total_size,
    pg_size_pretty(pg_relation_size('mentat.' || tablename)) AS table_size,
    pg_size_pretty(pg_total_relation_size('mentat.' || tablename) - pg_relation_size('mentat.' || tablename)) AS index_size
FROM pg_tables
WHERE schemaname = 'mentat'
  AND tablename LIKE 'datoms_%_new'
ORDER BY pg_total_relation_size('mentat.' || tablename) DESC;

-- =============================================================================
-- Usage Instructions
-- =============================================================================

-- To run all benchmarks:
-- \i benchmarks/query_performance/benchmark_queries.sql

-- To run with output to file:
-- \o benchmarks/results/query_performance_1m.txt
-- \i benchmarks/query_performance/benchmark_queries.sql
-- \o

-- To compare performance across dataset sizes:
-- 1. Run with 1M dataset: \o benchmarks/results/query_perf_1m.txt
-- 2. Run with 10M dataset: \o benchmarks/results/query_perf_10m.txt
-- 3. Compare results side-by-side
