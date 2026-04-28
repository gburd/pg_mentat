-- Benchmark Dataset Creation Scripts
-- Creates realistic test datasets at 1M, 10M, and 100M datom scales

-- =============================================================================
-- Helper function: Generate random employee data
-- =============================================================================

CREATE OR REPLACE FUNCTION benchmark_random_string(length int) RETURNS text AS $$
DECLARE
    chars text := 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';
    result text := '';
    i int;
BEGIN
    FOR i IN 1..length LOOP
        result := result || substr(chars, floor(random() * length(chars) + 1)::int, 1);
    END LOOP;
    RETURN result;
END;
$$ LANGUAGE plpgsql;

-- =============================================================================
-- Dataset 1: 1M datoms (~100k entities with ~10 attributes each)
-- =============================================================================

CREATE OR REPLACE FUNCTION create_benchmark_data_1m() RETURNS TABLE(
    entities_created int,
    datoms_created int,
    duration_seconds float
) AS $$
DECLARE
    start_time timestamp;
    end_time timestamp;
    entity_count int := 100000;
    batch_size int := 1000;
    i int;
    tx_result text;
BEGIN
    RAISE NOTICE 'Creating 1M datom benchmark dataset...';
    start_time := clock_timestamp();

    -- Create schema attributes if they don't exist
    PERFORM mentat_transact('[
        {:db/id "name" :db/ident :bench/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
        {:db/id "age" :db/ident :bench/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
        {:db/id "email" :db/ident :bench/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
        {:db/id "dept" :db/ident :bench/dept :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
        {:db/id "salary" :db/ident :bench/salary :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
        {:db/id "active" :db/ident :bench/active :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
        {:db/id "score" :db/ident :bench/score :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
        {:db/id "joined" :db/ident :bench/joined :db/valueType :db.type/instant :db/cardinality :db.cardinality/one}
        {:db/id "bio" :db/ident :bench/bio :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/fulltext true}
        {:db/id "tags" :db/ident :bench/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
    ]'::text);

    RAISE NOTICE 'Schema created. Starting entity generation...';

    -- Generate entities in batches
    FOR i IN 1..entity_count BY batch_size LOOP
        DECLARE
            tx_data text;
            entities_json text[];
            j int;
            dept_choice text;
        BEGIN
            entities_json := ARRAY[]::text[];

            FOR j IN i..(i + batch_size - 1) LOOP
                dept_choice := CASE (j % 5)
                    WHEN 0 THEN 'Engineering'
                    WHEN 1 THEN 'Sales'
                    WHEN 2 THEN 'Marketing'
                    WHEN 3 THEN 'Support'
                    ELSE 'Product'
                END;

                entities_json := array_append(entities_json, format(
                    '{"db/id": "e%s", "bench/name": "Person%s", "bench/age": %s, "bench/email": "person%s@test.com", "bench/dept": "%s", "bench/salary": %s.0, "bench/active": %s, "bench/score": %s.5, "bench/joined": "%s", "bench/bio": "Bio for person %s"}',
                    j, j,
                    20 + (j % 50),
                    j,
                    dept_choice,
                    50000 + (j % 100000),
                    (j % 2 = 0)::text,
                    75.0 + (j % 25),
                    '2020-01-01'::timestamp + (j % 1500 || ' days')::interval,
                    j
                ));
            END LOOP;

            tx_data := '[' || array_to_string(entities_json, ',') || ']';
            PERFORM mentat_transact(tx_data::text);

            IF i % 10000 = 1 THEN
                RAISE NOTICE 'Created % entities...', i + batch_size - 1;
            END IF;
        END;
    END LOOP;

    -- Add cardinality-many tags (2 tags per entity = 200k additional datoms)
    RAISE NOTICE 'Adding tags (cardinality-many attributes)...';
    FOR i IN 1..entity_count BY batch_size LOOP
        DECLARE
            tx_data text;
            tag_assertions text[];
            j int;
        BEGIN
            tag_assertions := ARRAY[]::text[];

            FOR j IN i..(i + batch_size - 1) LOOP
                tag_assertions := array_append(tag_assertions, format(
                    '["db/add", %s, "bench/tags", "tag%s"]',
                    j, (j % 100)
                ));
                tag_assertions := array_append(tag_assertions, format(
                    '["db/add", %s, "bench/tags", "tag%s"]',
                    j, ((j + 1) % 100)
                ));
            END LOOP;

            tx_data := '[' || array_to_string(tag_assertions, ',') || ']';
            PERFORM mentat_transact(tx_data::text);
        END;
    END LOOP;

    end_time := clock_timestamp();

    RETURN QUERY SELECT
        entity_count,
        entity_count * 10 + entity_count * 2, -- 10 attributes + 2 tags per entity
        EXTRACT(EPOCH FROM (end_time - start_time))::float;
END;
$$ LANGUAGE plpgsql;

-- =============================================================================
-- Dataset 2: 10M datoms (~1M entities with ~10 attributes each)
-- =============================================================================

CREATE OR REPLACE FUNCTION create_benchmark_data_10m() RETURNS TABLE(
    entities_created int,
    datoms_created int,
    duration_seconds float
) AS $$
DECLARE
    start_time timestamp;
    end_time timestamp;
    entity_count int := 1000000;
    batch_size int := 1000;
    i int;
BEGIN
    RAISE NOTICE 'Creating 10M datom benchmark dataset...';
    RAISE NOTICE 'This will take approximately 10-20 minutes...';
    start_time := clock_timestamp();

    -- Create schema if needed
    PERFORM mentat_transact('[
        {:db/id "name" :db/ident :bench/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
        {:db/id "age" :db/ident :bench/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
        {:db/id "email" :db/ident :bench/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
        {:db/id "dept" :db/ident :bench/dept :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
        {:db/id "salary" :db/ident :bench/salary :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
        {:db/id "active" :db/ident :bench/active :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
        {:db/id "score" :db/ident :bench/score :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
        {:db/id "joined" :db/ident :bench/joined :db/valueType :db.type/instant :db/cardinality :db.cardinality/one}
        {:db/id "bio" :db/ident :bench/bio :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/fulltext true}
        {:db/id "tags" :db/ident :bench/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
    ]'::text);

    RAISE NOTICE 'Schema created. Starting entity generation...';

    -- Generate entities in batches
    FOR i IN 1..entity_count BY batch_size LOOP
        DECLARE
            tx_data text;
            entities_json text[];
            j int;
            dept_choice text;
        BEGIN
            entities_json := ARRAY[]::text[];

            FOR j IN i..(i + batch_size - 1) LOOP
                dept_choice := CASE (j % 5)
                    WHEN 0 THEN 'Engineering'
                    WHEN 1 THEN 'Sales'
                    WHEN 2 THEN 'Marketing'
                    WHEN 3 THEN 'Support'
                    ELSE 'Product'
                END;

                entities_json := array_append(entities_json, format(
                    '{"db/id": "e%s", "bench/name": "Person%s", "bench/age": %s, "bench/email": "person%s@test.com", "bench/dept": "%s", "bench/salary": %s.0, "bench/active": %s, "bench/score": %s.5, "bench/joined": "%s", "bench/bio": "Bio for person %s"}',
                    j, j,
                    20 + (j % 50),
                    j,
                    dept_choice,
                    50000 + (j % 100000),
                    (j % 2 = 0)::text,
                    75.0 + (j % 25),
                    '2020-01-01'::timestamp + (j % 1500 || ' days')::interval,
                    j
                ));
            END LOOP;

            tx_data := '[' || array_to_string(entities_json, ',') || ']';
            PERFORM mentat_transact(tx_data::text);

            IF i % 50000 = 1 THEN
                RAISE NOTICE 'Created % entities (%.1f%%)...', i + batch_size - 1, ((i::float / entity_count) * 100);
            END IF;
        END;
    END LOOP;

    RAISE NOTICE 'Adding tags...';
    FOR i IN 1..entity_count BY batch_size LOOP
        DECLARE
            tx_data text;
            tag_assertions text[];
            j int;
        BEGIN
            tag_assertions := ARRAY[]::text[];

            FOR j IN i..(i + batch_size - 1) LOOP
                tag_assertions := array_append(tag_assertions, format(
                    '["db/add", %s, "bench/tags", "tag%s"]',
                    j, (j % 100)
                ));
                tag_assertions := array_append(tag_assertions, format(
                    '["db/add", %s, "bench/tags", "tag%s"]',
                    j, ((j + 1) % 100)
                ));
            END LOOP;

            tx_data := '[' || array_to_string(tag_assertions, ',') || ']';
            PERFORM mentat_transact(tx_data::text);
        END;
    END LOOP;

    end_time := clock_timestamp();

    RETURN QUERY SELECT
        entity_count,
        entity_count * 10 + entity_count * 2,
        EXTRACT(EPOCH FROM (end_time - start_time))::float;
END;
$$ LANGUAGE plpgsql;

-- =============================================================================
-- Usage Instructions
-- =============================================================================

-- To create 1M datom dataset (~2-5 minutes):
-- SELECT * FROM create_benchmark_data_1m();

-- To create 10M datom dataset (~20-40 minutes):
-- SELECT * FROM create_benchmark_data_10m();

-- To check dataset size:
-- SELECT
--     schemaname,
--     tablename,
--     pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) AS size
-- FROM pg_tables
-- WHERE schemaname = 'mentat'
-- ORDER BY pg_total_relation_size(schemaname||'.'||tablename) DESC;

-- To count datoms:
-- SELECT COUNT(*) FROM (
--     SELECT * FROM mentat.datoms_ref_new WHERE added = true
--     UNION ALL
--     SELECT * FROM mentat.datoms_long_new WHERE added = true
--     UNION ALL
--     SELECT * FROM mentat.datoms_text_new WHERE added = true
--     UNION ALL
--     SELECT * FROM mentat.datoms_double_new WHERE added = true
--     UNION ALL
--     SELECT * FROM mentat.datoms_boolean_new WHERE added = true
--     UNION ALL
--     SELECT * FROM mentat.datoms_instant_new WHERE added = true
--     UNION ALL
--     SELECT * FROM mentat.datoms_keyword_new WHERE added = true
--     UNION ALL
--     SELECT * FROM mentat.datoms_uuid_new WHERE added = true
--     UNION ALL
--     SELECT * FROM mentat.datoms_bytes_new WHERE added = true
-- ) all_datoms;
