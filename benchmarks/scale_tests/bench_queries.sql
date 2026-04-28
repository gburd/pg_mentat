-- =============================================================================
-- Query Performance Benchmarks for pg_mentat
-- =============================================================================
--
-- Compares three query access patterns:
--   1. Schema-aware Datalog (single type-specific table via mentat_query)
--   2. UNION ALL SQL views (the mentat.facts virtual table)
--   3. Direct type-specific table queries (baseline)
--
-- Run AFTER generate_data.sql has populated the store.
--
-- Usage:
--   psql -f bench_queries.sql 2>&1 | tee results/query_benchmark.txt
--
-- =============================================================================

\set ON_ERROR_STOP on
\timing on

DO $$
BEGIN
    RAISE NOTICE '=============================================================';
    RAISE NOTICE '  pg_mentat Query Performance Benchmarks';
    RAISE NOTICE '=============================================================';
    RAISE NOTICE '';
END;
$$;

-- =============================================================================
-- Warmup: Run each query type once to prime caches and prepared statements
-- =============================================================================

DO $$
DECLARE
    r JSONB;
    cnt INT;
BEGIN
    -- Warm Datalog path
    SELECT mentat_query('[:find (count ?e) :where [?e :person/name _]]', '{}')::JSONB INTO r;
    -- Warm virtual table path
    SELECT COUNT(*) INTO cnt FROM mentat.facts WHERE attribute = ':person/name';
    -- Warm direct table path
    SELECT COUNT(*) INTO cnt FROM mentat.datoms_text_new WHERE added = true;
    RAISE NOTICE 'Warmup complete';
END;
$$;

-- =============================================================================
-- Benchmark 1: Count entities by attribute (string)
-- =============================================================================

DO $$
DECLARE
    iter       INT := 10;
    i          INT;
    r          JSONB;
    cnt        INT;
    start_ts   TIMESTAMPTZ;
    total_ms   DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 1: Count entities with :person/name ---';

    -- 1a: Schema-aware Datalog (hits datoms_text_new directly)
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT mentat_query(
            '[:find (count ?e) :where [?e :person/name _]]', '{}'
        )::JSONB INTO r;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Datalog (schema-aware):  %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 1b: UNION ALL virtual table (mentat.facts)
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt FROM mentat.facts WHERE attribute = ':person/name';
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  SQL view (UNION ALL):    %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 1c: Direct type-specific table
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt
        FROM mentat.datoms_text_new dt
        JOIN mentat.schema s ON s.entid = dt.a AND s.ident = ':person/name'
        WHERE dt.store_id = (SELECT store_id FROM mentat.stores WHERE store_name = 'default')
          AND dt.added = true;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Direct table scan:       %.2f ms avg (%s iterations)', total_ms / iter, iter;
END;
$$;

-- =============================================================================
-- Benchmark 2: Point lookup by entity value (unique identity)
-- =============================================================================

DO $$
DECLARE
    iter       INT := 50;
    i          INT;
    r          JSONB;
    cnt        INT;
    start_ts   TIMESTAMPTZ;
    total_ms   DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 2: Lookup entity by :person/email (unique identity) ---';

    -- 2a: Schema-aware Datalog
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT mentat_query(
            '[:find ?e . :where [?e :person/email "user42@example.com"]]', '{}'
        )::JSONB INTO r;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Datalog (schema-aware):  %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 2b: UNION ALL virtual table
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt
        FROM mentat.facts
        WHERE attribute = ':person/email' AND value = 'user42@example.com';
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  SQL view (UNION ALL):    %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 2c: Direct type-specific table
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt
        FROM mentat.datoms_text_new dt
        JOIN mentat.schema s ON s.entid = dt.a AND s.ident = ':person/email'
        WHERE dt.store_id = (SELECT store_id FROM mentat.stores WHERE store_name = 'default')
          AND dt.v = 'user42@example.com'
          AND dt.added = true;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Direct table scan:       %.2f ms avg (%s iterations)', total_ms / iter, iter;
END;
$$;

-- =============================================================================
-- Benchmark 3: Range scan on numeric attribute
-- =============================================================================

DO $$
DECLARE
    iter       INT := 10;
    i          INT;
    r          JSONB;
    cnt        INT;
    start_ts   TIMESTAMPTZ;
    total_ms   DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 3: Range scan :person/age > 40 ---';

    -- 3a: Schema-aware Datalog (should hit datoms_long_new)
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT mentat_query(
            '[:find (count ?e) :where [?e :person/age ?a] [(> ?a 40)]]', '{}'
        )::JSONB INTO r;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Datalog (schema-aware):  %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 3b: UNION ALL virtual table (numeric_values view)
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt
        FROM mentat.numeric_values
        WHERE attribute = ':person/age' AND value > 40;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  SQL typed view:          %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 3c: Direct type-specific table
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt
        FROM mentat.datoms_long_new dt
        JOIN mentat.schema s ON s.entid = dt.a AND s.ident = ':person/age'
        WHERE dt.store_id = (SELECT store_id FROM mentat.stores WHERE store_name = 'default')
          AND dt.v > 40
          AND dt.added = true;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Direct table scan:       %.2f ms avg (%s iterations)', total_ms / iter, iter;
END;
$$;

-- =============================================================================
-- Benchmark 4: Multi-attribute join (name + age + active)
-- =============================================================================

DO $$
DECLARE
    iter       INT := 10;
    i          INT;
    r          JSONB;
    cnt        INT;
    start_ts   TIMESTAMPTZ;
    total_ms   DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 4: Multi-attribute join (name + age > 30 + active) ---';

    -- 4a: Schema-aware Datalog (should use 3 typed tables)
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT mentat_query('
            [:find ?name ?age
             :where
             [?e :person/name ?name]
             [?e :person/age ?age]
             [(> ?age 30)]
             [?e :person/active true]]
        ', '{}')::JSONB INTO r;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Datalog (schema-aware):  %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 4b: SQL join across UNION ALL views
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt
        FROM mentat.facts f1
        JOIN mentat.facts f2 ON f1.entity_id = f2.entity_id AND f2.attribute = ':person/age'
        JOIN mentat.facts f3 ON f1.entity_id = f3.entity_id AND f3.attribute = ':person/active'
        WHERE f1.attribute = ':person/name'
          AND f2.value::INT > 30
          AND f3.value = 'true';
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  SQL view (UNION ALL):    %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 4c: Direct typed table joins
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        WITH sid AS (SELECT store_id FROM mentat.stores WHERE store_name = 'default'),
             a_name AS (SELECT entid FROM mentat.schema WHERE ident = ':person/name'),
             a_age AS (SELECT entid FROM mentat.schema WHERE ident = ':person/age'),
             a_active AS (SELECT entid FROM mentat.schema WHERE ident = ':person/active')
        SELECT COUNT(*) INTO cnt
        FROM mentat.datoms_text_new dt
        JOIN mentat.datoms_long_new dl ON dt.e = dl.e AND dl.store_id = (SELECT store_id FROM sid)
                                          AND dl.a = (SELECT entid FROM a_age) AND dl.v > 30 AND dl.added = true
        JOIN mentat.datoms_boolean_new db ON dt.e = db.e AND db.store_id = (SELECT store_id FROM sid)
                                             AND db.a = (SELECT entid FROM a_active) AND db.v = true AND db.added = true
        WHERE dt.store_id = (SELECT store_id FROM sid)
          AND dt.a = (SELECT entid FROM a_name)
          AND dt.added = true;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Direct typed joins:      %.2f ms avg (%s iterations)', total_ms / iter, iter;
END;
$$;

-- =============================================================================
-- Benchmark 5: Reference traversal (entity graph)
-- =============================================================================

DO $$
DECLARE
    iter       INT := 10;
    i          INT;
    r          JSONB;
    cnt        INT;
    start_ts   TIMESTAMPTZ;
    total_ms   DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 5: Reference traversal (:person/manager chain) ---';

    -- 5a: Datalog - find all people who have a manager
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT mentat_query('
            [:find (count ?e)
             :where
             [?e :person/manager ?m]
             [?m :person/name _]]
        ', '{}')::JSONB INTO r;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Datalog (ref join):      %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 5b: SQL entity_references view
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt FROM mentat.entity_references WHERE attribute = ':person/manager';
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  SQL entity_references:   %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 5c: Direct ref table
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt
        FROM mentat.datoms_ref_new dr
        JOIN mentat.schema s ON s.entid = dr.a AND s.ident = ':person/manager'
        WHERE dr.store_id = (SELECT store_id FROM mentat.stores WHERE store_name = 'default')
          AND dr.added = true;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Direct ref table:        %.2f ms avg (%s iterations)', total_ms / iter, iter;
END;
$$;

-- =============================================================================
-- Benchmark 6: Full-text search
-- =============================================================================

DO $$
DECLARE
    iter       INT := 10;
    i          INT;
    r          JSONB;
    cnt        INT;
    start_ts   TIMESTAMPTZ;
    total_ms   DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 6: Full-text search on :person/bio ---';

    -- 6a: Datalog fulltext
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT mentat_query('
            [:find (count ?e)
             :where
             [(fulltext $ :person/bio "distributed systems") [[?e]]]]
        ', '{}')::JSONB INTO r;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Datalog fulltext:        %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 6b: SQL find_text function
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt
        FROM mentat.find_text(':person/bio', 'distributed systems');
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  SQL find_text():         %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 6c: Direct tsvector query
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT COUNT(*) INTO cnt
        FROM mentat.datoms_text_new dt
        JOIN mentat.schema s ON s.entid = dt.a AND s.ident = ':person/bio'
        WHERE dt.store_id = (SELECT store_id FROM mentat.stores WHERE store_name = 'default')
          AND dt.added = true
          AND to_tsvector('english', dt.v) @@ plainto_tsquery('english', 'distributed systems');
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Direct tsvector query:   %.2f ms avg (%s iterations)', total_ms / iter, iter;
END;
$$;

-- =============================================================================
-- Benchmark 7: Aggregate queries
-- =============================================================================

DO $$
DECLARE
    iter       INT := 10;
    i          INT;
    r          JSONB;
    start_ts   TIMESTAMPTZ;
    total_ms   DOUBLE PRECISION;
    avg_val    DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 7: Aggregate (avg, min, max on :person/score) ---';

    -- 7a: Datalog aggregate
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT mentat_query('
            [:find (avg ?s) (min ?s) (max ?s) (count ?e)
             :where [?e :person/score ?s]]
        ', '{}')::JSONB INTO r;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Datalog aggregate:       %.2f ms avg (%s iterations)', total_ms / iter, iter;

    -- 7b: SQL on typed view
    start_ts := clock_timestamp();
    FOR i IN 1..iter LOOP
        SELECT AVG(dt.v) INTO avg_val
        FROM mentat.datoms_double_new dt
        JOIN mentat.schema s ON s.entid = dt.a AND s.ident = ':person/score'
        WHERE dt.store_id = (SELECT store_id FROM mentat.stores WHERE store_name = 'default')
          AND dt.added = true;
    END LOOP;
    total_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Direct typed aggregate:  %.2f ms avg (%s iterations)', total_ms / iter, iter;
END;
$$;

-- =============================================================================
-- Benchmark 8: EXPLAIN ANALYZE comparison (single run)
-- =============================================================================

DO $$
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 8: EXPLAIN ANALYZE comparison ---';
    RAISE NOTICE 'See the EXPLAIN output below for plan differences.';
END;
$$;

-- 8a: UNION ALL view plan
EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT)
SELECT COUNT(*) FROM mentat.facts WHERE attribute = ':person/name';

-- 8b: Direct typed table plan
EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT)
SELECT COUNT(*)
FROM mentat.datoms_text_new dt
JOIN mentat.schema s ON s.entid = dt.a AND s.ident = ':person/name'
WHERE dt.store_id = (SELECT store_id FROM mentat.stores WHERE store_name = 'default')
  AND dt.added = true;

-- =============================================================================
-- Summary
-- =============================================================================

DO $$
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '=============================================================';
    RAISE NOTICE '  Benchmark complete. Review NOTICE output for timings.';
    RAISE NOTICE '  EXPLAIN ANALYZE plans shown above for plan comparison.';
    RAISE NOTICE '=============================================================';
END;
$$;
