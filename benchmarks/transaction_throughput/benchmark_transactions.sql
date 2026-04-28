-- Transaction Throughput Benchmarks
-- Tests transaction performance to validate "600 TPS" claim

-- =============================================================================
-- Benchmark 1: Single Transaction Performance
-- =============================================================================

CREATE OR REPLACE FUNCTION benchmark_single_tx(tx_count int DEFAULT 1000)
RETURNS TABLE(
    transactions_completed int,
    duration_ms float,
    tps float,
    avg_tx_ms float
) AS $$
DECLARE
    start_time timestamp;
    end_time timestamp;
    i int;
BEGIN
    RAISE NOTICE 'Running % single transactions...', tx_count;
    start_time := clock_timestamp();

    FOR i IN 1..tx_count LOOP
        PERFORM mentat_transact(format('[
            {"db/id": "bench%s",
             "bench/name": "BenchUser%s",
             "bench/age": %s,
             "bench/email": "bench%s@test.com",
             "bench/dept": "Engineering",
             "bench/salary": %s.0,
             "bench/active": true}
        ]', i, i, 20 + (i % 50), i, 50000 + (i % 100000))::text);
    END LOOP;

    end_time := clock_timestamp();

    RETURN QUERY SELECT
        tx_count,
        EXTRACT(EPOCH FROM (end_time - start_time)) * 1000 AS duration_ms,
        tx_count / EXTRACT(EPOCH FROM (end_time - start_time)) AS tps,
        (EXTRACT(EPOCH FROM (end_time - start_time)) * 1000 / tx_count) AS avg_tx_ms;
END;
$$ LANGUAGE plpgsql;

-- =============================================================================
-- Benchmark 2: Batch Transaction Performance
-- =============================================================================

CREATE OR REPLACE FUNCTION benchmark_batch_tx(
    batch_count int DEFAULT 100,
    batch_size int DEFAULT 100
)
RETURNS TABLE(
    transactions_completed int,
    datoms_written int,
    duration_ms float,
    tps float,
    datoms_per_sec float
) AS $$
DECLARE
    start_time timestamp;
    end_time timestamp;
    i int;
    j int;
    tx_data text;
    entities_json text[];
BEGIN
    RAISE NOTICE 'Running % batches of % entities each...', batch_count, batch_size;
    start_time := clock_timestamp();

    FOR i IN 1..batch_count LOOP
        entities_json := ARRAY[]::text[];

        FOR j IN 1..batch_size LOOP
            entities_json := array_append(entities_json, format(
                '{"db/id": "batch%s_%s", "bench/name": "BatchUser%s_%s", "bench/age": %s, "bench/email": "batch%s_%s@test.com"}',
                i, j, i, j, 20 + ((i * batch_size + j) % 50), i, j
            ));
        END LOOP;

        tx_data := '[' || array_to_string(entities_json, ',') || ']';
        PERFORM mentat_transact(tx_data::text);
    END LOOP;

    end_time := clock_timestamp();

    RETURN QUERY SELECT
        batch_count,
        batch_count * batch_size * 4, -- 4 datoms per entity
        EXTRACT(EPOCH FROM (end_time - start_time)) * 1000 AS duration_ms,
        batch_count / EXTRACT(EPOCH FROM (end_time - start_time)) AS tps,
        (batch_count * batch_size * 4) / EXTRACT(EPOCH FROM (end_time - start_time)) AS datoms_per_sec;
END;
$$ LANGUAGE plpgsql;

-- =============================================================================
-- Benchmark 3: CAS (Compare-And-Swap) Performance
-- =============================================================================

CREATE OR REPLACE FUNCTION benchmark_cas_operations(op_count int DEFAULT 1000)
RETURNS TABLE(
    operations_completed int,
    duration_ms float,
    ops_per_sec float
) AS $$
DECLARE
    start_time timestamp;
    end_time timestamp;
    i int;
    test_entity_id int;
BEGIN
    -- Create a test entity
    PERFORM mentat_transact('[
        {"db/id": "cas-test", "bench/name": "CASTest", "bench/age": 30}
    ]'::text);

    -- Get the entity ID
    test_entity_id := (
        SELECT json_array_element(
            json_array_element(
                (mentat_query('[:find ?e :where [?e :bench/name "CASTest"]]'::text, '{}'::jsonb)::json)->'results',
                0
            ),
            0
        )::text::int
    );

    RAISE NOTICE 'Running % CAS operations on entity %...', op_count, test_entity_id;
    start_time := clock_timestamp();

    FOR i IN 1..op_count LOOP
        PERFORM mentat_transact(format('[
            [":db.fn/cas", %s, ":bench/age", %s, %s]
        ]', test_entity_id, 30 + i - 1, 30 + i)::text);
    END LOOP;

    end_time := clock_timestamp();

    RETURN QUERY SELECT
        op_count,
        EXTRACT(EPOCH FROM (end_time - start_time)) * 1000 AS duration_ms,
        op_count / EXTRACT(EPOCH FROM (end_time - start_time)) AS ops_per_sec;
END;
$$ LANGUAGE plpgsql;

-- =============================================================================
-- Benchmark 4: Upsert Performance (unique identity)
-- =============================================================================

CREATE OR REPLACE FUNCTION benchmark_upsert_operations(op_count int DEFAULT 1000)
RETURNS TABLE(
    operations_completed int,
    duration_ms float,
    ops_per_sec float
) AS $$
DECLARE
    start_time timestamp;
    end_time timestamp;
    i int;
BEGIN
    RAISE NOTICE 'Running % upsert operations...', op_count;
    start_time := clock_timestamp();

    FOR i IN 1..op_count LOOP
        -- Repeatedly upsert the same email (should find existing entity)
        PERFORM mentat_transact(format('[
            {"db/id": "upsert%s",
             "bench/email": "upsert@test.com",
             "bench/name": "UpsertUser%s",
             "bench/age": %s}
        ]', i, i, 20 + (i % 50))::text);
    END LOOP;

    end_time := clock_timestamp();

    RETURN QUERY SELECT
        op_count,
        EXTRACT(EPOCH FROM (end_time - start_time)) * 1000 AS duration_ms,
        op_count / EXTRACT(EPOCH FROM (end_time - start_time)) AS ops_per_sec;
END;
$$ LANGUAGE plpgsql;

-- =============================================================================
-- Benchmark 5: Retraction Performance
-- =============================================================================

CREATE OR REPLACE FUNCTION benchmark_retraction_operations(op_count int DEFAULT 1000)
RETURNS TABLE(
    operations_completed int,
    duration_ms float,
    ops_per_sec float
) AS $$
DECLARE
    start_time timestamp;
    end_time timestamp;
    i int;
BEGIN
    -- Create entities to retract
    PERFORM mentat_transact('[' || string_agg(format(
        '{"db/id": "retract%s", "bench/name": "RetractUser%s", "bench/age": %s}',
        gs, gs, 20 + (gs % 50)
    ), ',') || ']'::text)
    FROM generate_series(1, op_count) gs;

    RAISE NOTICE 'Running % retraction operations...', op_count;
    start_time := clock_timestamp();

    FOR i IN 1..op_count LOOP
        PERFORM mentat_transact(format('[
            [":db.fn/retractEntity", %s]
        ]', i)::text);
    END LOOP;

    end_time := clock_timestamp();

    RETURN QUERY SELECT
        op_count,
        EXTRACT(EPOCH FROM (end_time - start_time)) * 1000 AS duration_ms,
        op_count / EXTRACT(EPOCH FROM (end_time - start_time)) AS ops_per_sec;
END;
$$ LANGUAGE plpgsql;

-- =============================================================================
-- Run All Benchmarks
-- =============================================================================

\echo '\n=== Transaction Throughput Benchmarks ==='

\echo '\n--- Benchmark 1: Single Transaction Performance ---'
\echo 'Expected: >600 TPS'
SELECT * FROM benchmark_single_tx(1000);

\echo '\n--- Benchmark 2: Batch Transaction Performance ---'
\echo 'Expected: >5000 datoms/sec'
SELECT * FROM benchmark_batch_tx(100, 100);

\echo '\n--- Benchmark 3: CAS Operation Performance ---'
\echo 'Expected: >500 ops/sec'
SELECT * FROM benchmark_cas_operations(500);

\echo '\n--- Benchmark 4: Upsert Performance ---'
\echo 'Expected: >400 ops/sec (includes lookup)'
SELECT * FROM benchmark_upsert_operations(500);

\echo '\n--- Benchmark 5: Retraction Performance ---'
\echo 'Expected: >300 ops/sec'
SELECT * FROM benchmark_retraction_operations(500);

-- =============================================================================
-- Usage Instructions
-- =============================================================================

-- To run all transaction benchmarks:
-- \i benchmarks/transaction_throughput/benchmark_transactions.sql

-- To save results to file:
-- \o benchmarks/results/transaction_throughput.txt
-- \i benchmarks/transaction_throughput/benchmark_transactions.sql
-- \o

-- To run individual benchmarks:
-- SELECT * FROM benchmark_single_tx(1000);
-- SELECT * FROM benchmark_batch_tx(100, 100);
-- SELECT * FROM benchmark_cas_operations(500);
-- SELECT * FROM benchmark_upsert_operations(500);
-- SELECT * FROM benchmark_retraction_operations(500);
