-- pg_mentat smoke test
--
-- Guards against the three bug classes that the Phase-0 review surfaced:
--   1. CREATE EXTENSION failures (schema/object conflicts, bootstrap errors).
--   2. Install-time SQL errors such as ROUND(float, int) or i16/i32 mismatches
--      between Rust pg_extern signatures and the generated SQL wrapper.
--   3. Silent drift between the Rust bootstrap entids and the SQL bootstrap
--      rows, which breaks every later mentat_transact call.
--
-- Every step must succeed or the script aborts with a non-zero status.
-- RAISE EXCEPTION is used for assertions so the error appears in CI logs.

\set ON_ERROR_STOP on
\set QUIET on
\pset pager off

SET client_min_messages = NOTICE;

\echo '=== pg_mentat smoke test: start ==='

-- Step 1: CREATE EXTENSION ---------------------------------------------------
DROP EXTENSION IF EXISTS pg_mentat CASCADE;
CREATE EXTENSION pg_mentat;
\echo 'Step 1 OK: CREATE EXTENSION pg_mentat'

SET search_path = mentat, public;

-- Step 2: at least 19 base tables in the mentat schema ----------------------
DO $$
DECLARE n int;
BEGIN
    SELECT count(*) INTO n
      FROM information_schema.tables
     WHERE table_schema = 'mentat' AND table_type = 'BASE TABLE';
    IF n < 19 THEN
        RAISE EXCEPTION 'expected >= 19 tables in mentat schema, got %', n;
    END IF;
    RAISE NOTICE 'Step 2 OK: % tables in mentat schema', n;
END $$;

-- Step 3: >= 24 bootstrap rows in mentat.schema (the :db/* + :db.type/* set) -
DO $$
DECLARE n int;
BEGIN
    SELECT count(*) INTO n FROM mentat.schema;
    IF n < 24 THEN
        RAISE EXCEPTION 'expected >= 24 bootstrap rows in mentat.schema, got %', n;
    END IF;
    RAISE NOTICE 'Step 3 OK: % bootstrap rows in mentat.schema', n;
END $$;

-- Step 4: mentat.stores has store_name='default' with store_id=0 ------------
DO $$
DECLARE sid int;
BEGIN
    SELECT store_id INTO sid FROM mentat.stores WHERE store_name = 'default';
    IF sid IS NULL THEN
        RAISE EXCEPTION 'no row in mentat.stores for store_name=default';
    END IF;
    IF sid <> 0 THEN
        RAISE EXCEPTION 'default store_id is %, expected 0', sid;
    END IF;
    RAISE NOTICE 'Step 4 OK: default store has store_id=0';
END $$;

-- Step 5: all nine datoms_*_new narrow tables exist -------------------------
DO $$
DECLARE
    expected text[] := ARRAY[
        'datoms_ref_new', 'datoms_long_new', 'datoms_text_new',
        'datoms_double_new', 'datoms_instant_new', 'datoms_keyword_new',
        'datoms_uuid_new', 'datoms_bytes_new', 'datoms_boolean_new'
    ];
    t text;
BEGIN
    FOREACH t IN ARRAY expected LOOP
        IF NOT EXISTS (
            SELECT 1 FROM information_schema.tables
             WHERE table_schema = 'mentat' AND table_name = t
        ) THEN
            RAISE EXCEPTION 'narrow table mentat.% missing', t;
        END IF;
    END LOOP;
    RAISE NOTICE 'Step 5 OK: all 9 datoms_*_new narrow tables present';
END $$;

-- Step 6: dual_write_datoms trigger is enabled on mentat.datoms -------------
DO $$
DECLARE state char;
BEGIN
    SELECT t.tgenabled INTO state
      FROM pg_trigger t
      JOIN pg_class c     ON c.oid = t.tgrelid
      JOIN pg_namespace n ON n.oid = c.relnamespace
     WHERE n.nspname = 'mentat'
       AND c.relname = 'datoms'
       AND t.tgname  = 'dual_write_datoms_trigger'
       AND NOT t.tgisinternal;
    IF state IS NULL THEN
        RAISE EXCEPTION 'dual_write_datoms_trigger not found on mentat.datoms';
    END IF;
    IF state <> 'O' THEN
        RAISE EXCEPTION 'dual_write_datoms_trigger disabled (tgenabled=%)', state;
    END IF;
    RAISE NOTICE 'Step 6 OK: dual_write_datoms_trigger enabled';
END $$;

-- Step 7: define :person/name and :person/age via mentat_transact -----------
SELECT mentat_transact('[
  {:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
  {:db/ident :person/age  :db/valueType :db.type/long   :db/cardinality :db.cardinality/one}
]') \gset tx1_
\echo 'Step 7 OK: schema transaction succeeded'

-- Step 8: schema now has :person/name and :person/age -----------------------
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM mentat.schema WHERE ident = ':person/name') THEN
        RAISE EXCEPTION ':person/name missing from mentat.schema after transact';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM mentat.schema WHERE ident = ':person/age') THEN
        RAISE EXCEPTION ':person/age missing from mentat.schema after transact';
    END IF;
    RAISE NOTICE 'Step 8 OK: :person/name and :person/age registered';
END $$;

-- Step 9: assert a fact for alice -------------------------------------------
SELECT mentat_transact('[{:db/id "alice" :person/name "Alice" :person/age 30}]') \gset tx2_
\echo 'Step 9 OK: alice asserted'

-- Step 10: query returns exactly one row [["Alice", 30]] --------------------
DO $$
DECLARE
    result  jsonb;
    rows    jsonb;
    first   jsonb;
BEGIN
    SELECT mentat_query(
        '[:find ?n ?a :where [?e :person/name ?n] [?e :person/age ?a]]',
        '{}'::jsonb
    )::jsonb INTO result;
    rows := result->'results';
    IF jsonb_typeof(rows) <> 'array' THEN
        RAISE EXCEPTION 'mentat_query returned no results array: %', result;
    END IF;
    IF jsonb_array_length(rows) <> 1 THEN
        RAISE EXCEPTION 'expected exactly 1 result row, got %: %',
            jsonb_array_length(rows), result;
    END IF;
    first := rows->0;
    IF first->>0 <> 'Alice' OR first->>1 <> '30' THEN
        RAISE EXCEPTION 'expected [["Alice", 30]], got %', rows;
    END IF;
    RAISE NOTICE 'Step 10 OK: query returned [["Alice", 30]]';
END $$;

-- Step 11: DROP EXTENSION leaves no mentat.* objects behind -----------------
RESET search_path;
DROP EXTENSION pg_mentat CASCADE;
DO $$
DECLARE
    leftover_tables int;
    leftover_schemas int;
BEGIN
    SELECT count(*) INTO leftover_tables
      FROM information_schema.tables WHERE table_schema = 'mentat';
    SELECT count(*) INTO leftover_schemas
      FROM information_schema.schemata WHERE schema_name = 'mentat';
    IF leftover_tables <> 0 OR leftover_schemas <> 0 THEN
        RAISE EXCEPTION
            'DROP EXTENSION left % tables / % schemas behind',
            leftover_tables, leftover_schemas;
    END IF;
    RAISE NOTICE 'Step 11 OK: DROP EXTENSION cleaned up mentat schema';
END $$;

\echo '=== pg_mentat smoke test: PASS ==='
