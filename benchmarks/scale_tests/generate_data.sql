-- =============================================================================
-- Data Generation for pg_mentat Scale Tests
-- =============================================================================
--
-- This script populates a pg_mentat store with test data at configurable scale.
-- It creates a realistic schema with multiple value types and populates entities
-- in batches using mentat_transact().
--
-- Usage:
--   psql -v scale=1000 -f generate_data.sql
--
-- Scale parameter controls the number of "person" entities created.
-- Each person has ~5 attributes, so total datoms = scale * ~7 (including refs).
--
-- Recommended scale values:
--   1000    -> ~7K datoms    (quick smoke test)
--   10000   -> ~70K datoms   (development testing)
--   100000  -> ~700K datoms  (integration testing)
--   1000000 -> ~7M datoms    (performance baseline)
--   10000000-> ~70M datoms   (stress testing)
--
-- Prerequisites:
--   CREATE EXTENSION pg_mentat;
--   SELECT mentat_create_store('default', 'Default store');
-- =============================================================================

\set ON_ERROR_STOP on
\timing on

-- Default scale if not provided via -v
SELECT coalesce(:'scale', '1000') AS effective_scale \gset

DO $$
BEGIN
    RAISE NOTICE '=== pg_mentat Scale Test Data Generator ===';
    RAISE NOTICE 'Target entity count: %', :'effective_scale';
END;
$$;

-- =============================================================================
-- Step 1: Install schema attributes
-- =============================================================================

SELECT mentat_transact('[
    ;; Person attributes
    {:db/ident       :person/name
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/index       true
     :db/doc         "A person full name"}

    {:db/ident       :person/email
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique      :db.unique/identity
     :db/doc         "Unique email address"}

    {:db/ident       :person/age
     :db/valueType   :db.type/long
     :db/cardinality :db.cardinality/one
     :db/index       true}

    {:db/ident       :person/active
     :db/valueType   :db.type/boolean
     :db/cardinality :db.cardinality/one}

    {:db/ident       :person/score
     :db/valueType   :db.type/double
     :db/cardinality :db.cardinality/one}

    {:db/ident       :person/joined
     :db/valueType   :db.type/instant
     :db/cardinality :db.cardinality/one}

    {:db/ident       :person/manager
     :db/valueType   :db.type/ref
     :db/cardinality :db.cardinality/one
     :db/doc         "Reference to manager entity"}

    {:db/ident       :person/tags
     :db/valueType   :db.type/keyword
     :db/cardinality :db.cardinality/many
     :db/doc         "Tags (cardinality many)"}

    {:db/ident       :person/bio
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/fulltext    true
     :db/doc         "Full-text searchable biography"}

    ;; Department attributes
    {:db/ident       :dept/name
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/unique      :db.unique/identity}

    {:db/ident       :dept/budget
     :db/valueType   :db.type/double
     :db/cardinality :db.cardinality/one}

    {:db/ident       :person/department
     :db/valueType   :db.type/ref
     :db/cardinality :db.cardinality/one}
]');

-- =============================================================================
-- Step 2: Create departments (fixed, small set)
-- =============================================================================

SELECT mentat_transact('[
    {:db/id "eng"  :dept/name "Engineering"  :dept/budget 5000000.0}
    {:db/id "mkt"  :dept/name "Marketing"    :dept/budget 2000000.0}
    {:db/id "sale" :dept/name "Sales"        :dept/budget 3000000.0}
    {:db/id "hr"   :dept/name "HR"           :dept/budget 1000000.0}
    {:db/id "fin"  :dept/name "Finance"      :dept/budget 1500000.0}
    {:db/id "ops"  :dept/name "Operations"   :dept/budget 2500000.0}
    {:db/id "rd"   :dept/name "R&D"          :dept/budget 4000000.0}
    {:db/id "sup"  :dept/name "Support"      :dept/budget 1200000.0}
]');

-- =============================================================================
-- Step 3: Batch insert person entities
-- =============================================================================

-- We insert in batches of 100 to balance transaction overhead vs memory usage.
-- Each person gets: name, email, age, active, score, joined, bio, 1-3 tags.
-- Managers and departments are assigned from already-created entities.

DO $$
DECLARE
    target_count INT := :'effective_scale'::INT;
    batch_size   INT := 100;
    num_batches  INT;
    batch        TEXT;
    i            INT;
    j            INT;
    entity_num   INT;
    dept_names   TEXT[] := ARRAY['Engineering','Marketing','Sales','HR','Finance','Operations','R&D','Support'];
    tag_pool     TEXT[] := ARRAY[':tag/senior',':tag/junior',':tag/lead',':tag/remote',':tag/onsite',':tag/fulltime',':tag/contract',':tag/mentor'];
    first_names  TEXT[] := ARRAY['Alice','Bob','Carol','Dave','Eve','Frank','Grace','Hank','Iris','Jack',
                                  'Kate','Leo','Mia','Nate','Olga','Pete','Quinn','Ruth','Sam','Tina',
                                  'Uma','Vic','Wren','Xena','Yuri','Zara'];
    last_names   TEXT[] := ARRAY['Smith','Jones','Brown','Wilson','Taylor','Clark','Lewis','Walker','Hall','King',
                                  'Adams','Baker','Chen','Davis','Evans','Ford','Garcia','Hill','Ito','Kim'];
    bios         TEXT[] := ARRAY[
        'Experienced engineer with background in distributed systems and database internals.',
        'Product manager focused on developer tools and API design.',
        'Full-stack developer passionate about performance optimization.',
        'Data scientist specializing in natural language processing and ML pipelines.',
        'DevOps engineer with expertise in Kubernetes and cloud infrastructure.',
        'Security researcher focused on application security and threat modeling.',
        'Technical writer creating documentation for complex systems.',
        'UX designer bridging the gap between engineering and user experience.'
    ];
    start_ts     TIMESTAMPTZ;
    batch_start  TIMESTAMPTZ;
    elapsed_ms   DOUBLE PRECISION;
BEGIN
    num_batches := ceil(target_count::FLOAT / batch_size)::INT;
    start_ts := clock_timestamp();

    RAISE NOTICE 'Generating % entities in % batches of %...', target_count, num_batches, batch_size;

    FOR i IN 1..num_batches LOOP
        batch_start := clock_timestamp();
        batch := '[';

        FOR j IN 1..batch_size LOOP
            entity_num := (i - 1) * batch_size + j;
            EXIT WHEN entity_num > target_count;

            IF j > 1 THEN batch := batch || E'\n'; END IF;

            batch := batch || format(
                '{:db/id "p%s" :person/name "%s %s" :person/email "user%s@example.com" :person/age %s :person/active %s :person/score %s :person/joined #inst "%s" :person/bio "%s" :person/tags %s}',
                entity_num,
                -- name
                first_names[1 + (entity_num % array_length(first_names, 1))],
                last_names[1 + (entity_num % array_length(last_names, 1))],
                -- email (unique)
                entity_num,
                -- age (18-65)
                18 + (entity_num % 48),
                -- active (80% true)
                CASE WHEN entity_num % 5 = 0 THEN 'false' ELSE 'true' END,
                -- score (0.0-100.0)
                round((random() * 100)::NUMERIC, 2),
                -- joined date (spread over 5 years)
                to_char(
                    '2020-01-01'::DATE + (entity_num % 1825) * INTERVAL '1 day',
                    'YYYY-MM-DD"T"HH24:MI:SS"Z"'
                ),
                -- bio
                replace(bios[1 + (entity_num % array_length(bios, 1))], '"', ''''),
                -- tags (1-3 random tags)
                CASE (entity_num % 3)
                    WHEN 0 THEN tag_pool[1 + (entity_num % 8)]
                    WHEN 1 THEN format('[%s %s]',
                        tag_pool[1 + (entity_num % 8)],
                        tag_pool[1 + ((entity_num + 3) % 8)])
                    ELSE format('[%s %s %s]',
                        tag_pool[1 + (entity_num % 8)],
                        tag_pool[1 + ((entity_num + 3) % 8)],
                        tag_pool[1 + ((entity_num + 5) % 8)])
                END
            );
        END LOOP;

        batch := batch || ']';
        PERFORM mentat_transact(batch);

        -- Progress reporting every 10 batches
        IF i % 10 = 0 THEN
            elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
            RAISE NOTICE '  Batch %/% complete (% entities, %.0f ms elapsed)',
                i, num_batches, least(i * batch_size, target_count), elapsed_ms;
        END IF;
    END LOOP;

    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE 'Data generation complete: % entities in %.0f ms (%.0f entities/sec)',
        target_count, elapsed_ms, target_count / (elapsed_ms / 1000.0);
END;
$$;

-- =============================================================================
-- Step 4: Add manager references (self-referential, 50% have managers)
-- =============================================================================

-- Assign managers: each person with even entity_num gets a manager with half
-- their entity_num. This creates a tree structure for graph traversal tests.

DO $$
DECLARE
    target_count INT := :'effective_scale'::INT;
    batch_size   INT := 200;
    num_batches  INT;
    batch        TEXT;
    i            INT;
    j            INT;
    entity_num   INT;
    start_ts     TIMESTAMPTZ;
    elapsed_ms   DOUBLE PRECISION;
BEGIN
    -- Only do this for scales >= 10 (need entities to reference)
    IF target_count < 10 THEN
        RAISE NOTICE 'Skipping manager assignment for small datasets (< 10 entities)';
        RETURN;
    END IF;

    num_batches := ceil((target_count / 2)::FLOAT / batch_size)::INT;
    start_ts := clock_timestamp();

    RAISE NOTICE 'Assigning managers for ~% entities...', target_count / 2;

    FOR i IN 1..num_batches LOOP
        batch := '[';
        FOR j IN 1..batch_size LOOP
            entity_num := ((i - 1) * batch_size + j) * 2;
            EXIT WHEN entity_num > target_count;

            IF j > 1 THEN batch := batch || E'\n'; END IF;

            -- Lookup the entity by email (upsert via unique identity)
            batch := batch || format(
                '{:person/email "user%s@example.com" :person/manager [:person/email "user%s@example.com"]}',
                entity_num,
                greatest(1, entity_num / 2)
            );
        END LOOP;
        batch := batch || ']';
        PERFORM mentat_transact(batch);
    END LOOP;

    elapsed_ms := EXTRACT(EPOCH FROM (clock_timestamp() - start_ts)) * 1000;
    RAISE NOTICE 'Manager assignment complete in %.0f ms', elapsed_ms;
END;
$$;

-- =============================================================================
-- Step 5: Create virtual tables for SQL benchmarks
-- =============================================================================

SELECT mentat_create_virtual_tables('default');

-- =============================================================================
-- Step 6: Analyze tables for accurate query plans
-- =============================================================================

ANALYZE mentat.datoms_ref_new;
ANALYZE mentat.datoms_boolean_new;
ANALYZE mentat.datoms_long_new;
ANALYZE mentat.datoms_double_new;
ANALYZE mentat.datoms_instant_new;
ANALYZE mentat.datoms_text_new;
ANALYZE mentat.datoms_keyword_new;
ANALYZE mentat.datoms_uuid_new;
ANALYZE mentat.datoms_bytes_new;

-- =============================================================================
-- Step 7: Report dataset statistics
-- =============================================================================

DO $$
DECLARE
    total_datoms BIGINT := 0;
    tbl_count    BIGINT;
    tables       TEXT[] := ARRAY[
        'mentat.datoms_ref_new', 'mentat.datoms_boolean_new',
        'mentat.datoms_long_new', 'mentat.datoms_double_new',
        'mentat.datoms_instant_new', 'mentat.datoms_text_new',
        'mentat.datoms_keyword_new', 'mentat.datoms_uuid_new',
        'mentat.datoms_bytes_new'
    ];
    tbl TEXT;
BEGIN
    RAISE NOTICE '=== Dataset Statistics ===';
    FOREACH tbl IN ARRAY tables LOOP
        EXECUTE format('SELECT COUNT(*) FROM %s WHERE added = true', tbl) INTO tbl_count;
        total_datoms := total_datoms + tbl_count;
        RAISE NOTICE '  %-35s %s rows', tbl, tbl_count;
    END LOOP;
    RAISE NOTICE '  %-35s %s total datoms', 'TOTAL', total_datoms;
END;
$$;
