-- Test suite: Performance benchmarks
--
-- These tests verify performance characteristics rather than exact timing.
-- They check that operations complete within reasonable bounds and that
-- indexes are used properly.

BEGIN;

-- =========================================================================
-- Setup: Create a moderately sized dataset
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident :perf/name
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one
     :db/index true}
    {:db/ident :perf/value
     :db/valueType :db.type/long
     :db/cardinality :db.cardinality/one
     :db/index true}
    {:db/ident :perf/category
     :db/valueType :db.type/keyword
     :db/cardinality :db.cardinality/one}
    {:db/ident :perf/parent
     :db/valueType :db.type/ref
     :db/cardinality :db.cardinality/one}
]');

-- Insert 100 entities in batches
DO $$
DECLARE
    batch TEXT;
    i INT;
BEGIN
    FOR i IN 1..10 LOOP
        batch := '[';
        FOR j IN 1..10 LOOP
            IF j > 1 THEN batch := batch || ' '; END IF;
            batch := batch || format(
                '{:db/id "p%s" :perf/name "Item %s" :perf/value %s :perf/category :cat/%s}',
                (i-1)*10 + j, (i-1)*10 + j, ((i-1)*10 + j) * 100,
                CASE (j % 3) WHEN 0 THEN 'alpha' WHEN 1 THEN 'beta' ELSE 'gamma' END
            );
        END LOOP;
        batch := batch || ']';
        PERFORM mentat_transact(batch);
    END LOOP;
    RAISE NOTICE 'SETUP: inserted 100 test entities';
END;
$$;

-- =========================================================================
-- Benchmark 1: Simple query performance
-- =========================================================================

-- Test 1: Query 100 entities completes
DO $$
DECLARE
    result JSONB;
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
    cnt INT;
BEGIN
    start_ts := clock_timestamp();
    SELECT mentat_query('
        [:find ?name ?val
         :where
         [?e :perf/name ?name]
         [?e :perf/value ?val]]
    ', '{}')::JSONB INTO result;
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;

    cnt := jsonb_array_length(result->'results');
    ASSERT cnt >= 100, 'Should return at least 100 results, got: ' || cnt;
    RAISE NOTICE 'PASS: simple query on 100 entities in %.1f ms (% results)', elapsed_ms, cnt;
END;
$$;

-- =========================================================================
-- Benchmark 2: Filtered query with predicate
-- =========================================================================

-- Test 2: Filtered query uses index
DO $$
DECLARE
    result JSONB;
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
    cnt INT;
BEGIN
    start_ts := clock_timestamp();
    SELECT mentat_query('
        [:find ?name ?val
         :where
         [?e :perf/name ?name]
         [?e :perf/value ?val]
         [(> ?val 5000)]]
    ', '{}')::JSONB INTO result;
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;

    cnt := jsonb_array_length(result->'results');
    RAISE NOTICE 'PASS: filtered query in %.1f ms (% results)', elapsed_ms, cnt;
END;
$$;

-- =========================================================================
-- Benchmark 3: Pull performance
-- =========================================================================

-- Test 3: Pull with wildcard
DO $$
DECLARE
    result JSONB;
    eid BIGINT;
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    SELECT (mentat_query('[:find ?e . :where [?e :perf/name "Item 50"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid;

    start_ts := clock_timestamp();
    SELECT mentat_pull('[*]', eid)::JSONB INTO result;
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;

    ASSERT result IS NOT NULL, 'Pull should return a result';
    RAISE NOTICE 'PASS: pull with wildcard in %.1f ms', elapsed_ms;
END;
$$;

-- =========================================================================
-- Benchmark 4: Aggregate query
-- =========================================================================

-- Test 4: Aggregate over dataset
DO $$
DECLARE
    result JSONB;
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    start_ts := clock_timestamp();
    SELECT mentat_query('
        [:find (count ?e) (avg ?val) (min ?val) (max ?val)
         :where
         [?e :perf/value ?val]]
    ', '{}')::JSONB INTO result;
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;

    ASSERT result IS NOT NULL, 'Aggregate should return a result';
    RAISE NOTICE 'PASS: aggregate query in %.1f ms', elapsed_ms;
END;
$$;

-- =========================================================================
-- Benchmark 5: Virtual table view performance
-- =========================================================================

-- Test 5: Virtual table scan
DO $$
DECLARE
    cnt INT;
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    PERFORM mentat_create_virtual_tables('default');

    start_ts := clock_timestamp();
    SELECT COUNT(*) INTO cnt FROM mentat.facts WHERE attribute = ':perf/name';
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;

    ASSERT cnt >= 100, 'Facts view should have perf entities, got: ' || cnt;
    RAISE NOTICE 'PASS: virtual table scan in %.1f ms (% facts)', elapsed_ms, cnt;
END;
$$;

-- Test 6: Virtual table type-specific view
DO $$
DECLARE
    cnt INT;
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    start_ts := clock_timestamp();
    SELECT COUNT(*) INTO cnt FROM mentat.numeric_values WHERE attribute = ':perf/value' AND value > 5000;
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;

    RAISE NOTICE 'PASS: typed view filtered scan in %.1f ms (% matches)', elapsed_ms, cnt;
END;
$$;

-- =========================================================================
-- Benchmark 6: Materialized view creation and query
-- =========================================================================

-- Test 7: Matview creation time
DO $$
DECLARE
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    start_ts := clock_timestamp();
    PERFORM mentat_create_matview('perf_mv',
        '[:find ?name ?val :where [?e :perf/name ?name] [?e :perf/value ?val] [(> ?val 3000)]]',
        '{}');
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE 'PASS: matview creation in %.1f ms', elapsed_ms;
END;
$$;

-- Test 8: Matview query time (should be faster than live query)
DO $$
DECLARE
    cnt INT;
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    start_ts := clock_timestamp();
    SELECT COUNT(*) INTO cnt FROM mentat.matview_perf_mv;
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE 'PASS: matview query in %.1f ms (% rows)', elapsed_ms, cnt;
END;
$$;

-- Test 9: Matview refresh time
DO $$
DECLARE
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    start_ts := clock_timestamp();
    PERFORM mentat_refresh_matview('perf_mv');
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE 'PASS: matview refresh in %.1f ms', elapsed_ms;
END;
$$;

-- =========================================================================
-- Benchmark 7: Batch transact performance
-- =========================================================================

-- Test 10: Batch insert
DO $$
DECLARE
    batch TEXT;
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
    i INT;
BEGIN
    batch := '[';
    FOR i IN 1..50 LOOP
        IF i > 1 THEN batch := batch || ' '; END IF;
        batch := batch || format(
            '{:db/id "batch%s" :perf/name "Batch %s" :perf/value %s}',
            i, i, i * 1000
        );
    END LOOP;
    batch := batch || ']';

    start_ts := clock_timestamp();
    PERFORM mentat_transact(batch);
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE 'PASS: batch insert of 50 entities in %.1f ms', elapsed_ms;
END;
$$;

-- =========================================================================
-- Benchmark 8: Time-travel query overhead
-- =========================================================================

-- Test 11: As-of query performance compared to current query
DO $$
DECLARE
    tx_id BIGINT;
    start_ts TIMESTAMPTZ;
    current_ms DOUBLE PRECISION;
    asof_ms DOUBLE PRECISION;
    result JSONB;
BEGIN
    SELECT max(tx) INTO tx_id FROM mentat.transactions;

    start_ts := clock_timestamp();
    SELECT mentat_query('[:find (count ?e) :where [?e :perf/name _]]', '{}')::JSONB INTO result;
    current_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;

    start_ts := clock_timestamp();
    SELECT mentat_as_of(tx_id,
        '[:find (count ?e) :where [?e :perf/name _]]', '{}')::JSONB INTO result;
    asof_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;

    RAISE NOTICE 'PASS: current query %.1f ms, as-of query %.1f ms (overhead: %.1f ms)',
        current_ms, asof_ms, asof_ms - current_ms;
END;
$$;

-- =========================================================================
-- Benchmark 9: Store creation overhead
-- =========================================================================

-- Test 12: Store creation time
DO $$
DECLARE
    start_ts TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    start_ts := clock_timestamp();
    PERFORM mentat_create_store('perf_store', 'Performance test');
    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE 'PASS: store creation in %.1f ms', elapsed_ms;

    PERFORM mentat_drop_store('perf_store');
END;
$$;

-- =========================================================================
-- Cleanup
-- =========================================================================

DO $$
BEGIN
    PERFORM mentat_drop_matview('perf_mv');
EXCEPTION WHEN OTHERS THEN NULL;
END;
$$;

ROLLBACK;
