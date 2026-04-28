-- =============================================================================
-- Transaction Throughput Benchmarks for pg_mentat
-- =============================================================================
--
-- Measures transaction performance at various batch sizes and patterns:
--   1. Single-entity transactions (worst case overhead)
--   2. Batch transactions (10, 50, 100, 500 entities per tx)
--   3. Mixed read-write workloads
--   4. Update-heavy workloads (upsert via unique identity)
--   5. Retraction performance
--
-- Run AFTER generate_data.sql has populated the store.
--
-- Usage:
--   psql -f bench_transactions.sql 2>&1 | tee results/tx_benchmark.txt
--
-- =============================================================================

\set ON_ERROR_STOP on
\timing on

DO $$
BEGIN
    RAISE NOTICE '=============================================================';
    RAISE NOTICE '  pg_mentat Transaction Throughput Benchmarks';
    RAISE NOTICE '=============================================================';
    RAISE NOTICE '';
END;
$$;

-- =============================================================================
-- Benchmark 1: Single-entity transactions
-- =============================================================================

DO $$
DECLARE
    iter       INT := 100;
    i          INT;
    start_ts   TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '--- Benchmark 1: Single-entity transactions (%s iterations) ---', iter;
    start_ts := clock_timestamp();

    FOR i IN 1..iter LOOP
        PERFORM mentat_transact(format(
            '[{:db/id "bench1_%s" :person/name "Bench Single %s" :person/email "bench1_%s@test.com" :person/age %s}]',
            i, i, i, 20 + (i % 40)
        ));
    END LOOP;

    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Total: %.0f ms | Avg: %.2f ms/tx | Throughput: %.0f tx/sec',
        elapsed_ms, elapsed_ms / iter, iter / (elapsed_ms / 1000.0);
    RAISE NOTICE '  Datoms/sec: %.0f (4 datoms/entity)', iter * 4 / (elapsed_ms / 1000.0);
END;
$$;

-- =============================================================================
-- Benchmark 2: Batch transactions at various sizes
-- =============================================================================

DO $$
DECLARE
    batch_sizes INT[] := ARRAY[10, 50, 100, 500];
    batch_size  INT;
    iter        INT := 10;
    i           INT;
    j           INT;
    batch       TEXT;
    start_ts    TIMESTAMPTZ;
    elapsed_ms  DOUBLE PRECISION;
    seq         INT := 0;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 2: Batch transactions (varying batch sizes) ---';

    FOREACH batch_size IN ARRAY batch_sizes LOOP
        start_ts := clock_timestamp();

        FOR i IN 1..iter LOOP
            batch := '[';
            FOR j IN 1..batch_size LOOP
                seq := seq + 1;
                IF j > 1 THEN batch := batch || ' '; END IF;
                batch := batch || format(
                    '{:db/id "b2_%s" :person/name "Batch%s Item%s" :person/email "b2_%s@test.com" :person/age %s :person/active true}',
                    seq, batch_size, seq, seq, 18 + (seq % 48)
                );
            END LOOP;
            batch := batch || ']';
            PERFORM mentat_transact(batch);
        END LOOP;

        elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
        RAISE NOTICE '  Batch size %: %.0f ms total | %.2f ms/tx | %.0f datoms/sec',
            lpad(batch_size::TEXT, 3),
            elapsed_ms,
            elapsed_ms / iter,
            (iter * batch_size * 5) / (elapsed_ms / 1000.0);
    END LOOP;
END;
$$;

-- =============================================================================
-- Benchmark 3: Mixed read-write workload
-- =============================================================================

DO $$
DECLARE
    total_ops  INT := 100;
    reads      INT := 0;
    writes     INT := 0;
    i          INT;
    r          JSONB;
    start_ts   TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 3: Mixed workload (70%% read / 30%% write, %s ops) ---', total_ops;
    start_ts := clock_timestamp();

    FOR i IN 1..total_ops LOOP
        IF random() < 0.7 THEN
            -- Read operation: query by attribute
            SELECT mentat_query(
                '[:find ?name :where [?e :person/name ?name] [?e :person/age ?a] [(> ?a 30)]]',
                format('{"limit": 10}')
            )::JSONB INTO r;
            reads := reads + 1;
        ELSE
            -- Write operation: insert a new entity
            PERFORM mentat_transact(format(
                '[{:db/id "mixed_%s" :person/name "Mixed %s" :person/email "mixed_%s@test.com" :person/age %s}]',
                i, i, i, 20 + (i % 40)
            ));
            writes := writes + 1;
        END IF;
    END LOOP;

    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Total: %.0f ms | %s reads + %s writes | Avg: %.2f ms/op',
        elapsed_ms, reads, writes, elapsed_ms / total_ops;
    RAISE NOTICE '  Throughput: %.0f ops/sec', total_ops / (elapsed_ms / 1000.0);
END;
$$;

-- =============================================================================
-- Benchmark 4: Upsert (update via unique identity)
-- =============================================================================

DO $$
DECLARE
    iter       INT := 100;
    i          INT;
    start_ts   TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 4: Upsert via unique identity (%s iterations) ---', iter;
    start_ts := clock_timestamp();

    -- Update existing entities by email (unique identity triggers upsert)
    FOR i IN 1..iter LOOP
        PERFORM mentat_transact(format(
            '[{:person/email "user%s@example.com" :person/score %s}]',
            i, round((random() * 100)::NUMERIC, 2)
        ));
    END LOOP;

    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Total: %.0f ms | Avg: %.2f ms/upsert | Throughput: %.0f upserts/sec',
        elapsed_ms, elapsed_ms / iter, iter / (elapsed_ms / 1000.0);
END;
$$;

-- =============================================================================
-- Benchmark 5: Retraction performance
-- =============================================================================

DO $$
DECLARE
    iter       INT := 50;
    i          INT;
    eid        BIGINT;
    start_ts   TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 5: Retraction performance (%s iterations) ---', iter;

    -- First, create entities to retract
    FOR i IN 1..iter LOOP
        PERFORM mentat_transact(format(
            '[{:db/id "retract_%s" :person/name "ToRetract %s" :person/email "retract_%s@test.com" :person/age %s}]',
            i, i, i, 25
        ));
    END LOOP;

    start_ts := clock_timestamp();

    -- Retract a specific attribute from each entity
    FOR i IN 1..iter LOOP
        -- Look up entity by email then retract its age
        SELECT (mentat_query(
            format('[:find ?e . :where [?e :person/email "retract_%s@test.com"]]', i),
            '{}'
        )::JSONB)::TEXT::BIGINT INTO eid;

        IF eid IS NOT NULL THEN
            PERFORM mentat_transact(format(
                '[[:db/retract %s :person/age 25]]', eid
            ));
        END IF;
    END LOOP;

    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Total: %.0f ms | Avg: %.2f ms/retract (lookup+retract)',
        elapsed_ms, elapsed_ms / iter;
END;
$$;

-- =============================================================================
-- Benchmark 6: Transaction with many attributes per entity
-- =============================================================================

DO $$
DECLARE
    iter       INT := 20;
    i          INT;
    start_ts   TIMESTAMPTZ;
    elapsed_ms DOUBLE PRECISION;
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '--- Benchmark 6: Wide entities (8+ attributes per entity, %s iterations) ---', iter;
    start_ts := clock_timestamp();

    FOR i IN 1..iter LOOP
        PERFORM mentat_transact(format(
            '[{:db/id "wide_%s" :person/name "Wide Entity %s" :person/email "wide_%s@test.com" :person/age %s :person/active true :person/score %s :person/joined #inst "2024-06-15T00:00:00Z" :person/bio "A detailed biography for benchmarking wide entity transact performance." :person/tags [:tag/senior :tag/lead :tag/remote]}]',
            i, i, i, 30 + (i % 20), round((random() * 100)::NUMERIC, 2)
        ));
    END LOOP;

    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE '  Total: %.0f ms | Avg: %.2f ms/entity | ~%s datoms/entity',
        elapsed_ms, elapsed_ms / iter, 10;
    RAISE NOTICE '  Datoms/sec: %.0f', (iter * 10) / (elapsed_ms / 1000.0);
END;
$$;

-- =============================================================================
-- Summary
-- =============================================================================

DO $$
BEGIN
    RAISE NOTICE '';
    RAISE NOTICE '=============================================================';
    RAISE NOTICE '  Transaction benchmark complete.';
    RAISE NOTICE '  Review NOTICE output for throughput measurements.';
    RAISE NOTICE '=============================================================';
END;
$$;
