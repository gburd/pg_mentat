-- Test Suite: Per-store Row-Level Security (multi-tenancy)
--
-- Verifies that:
--   1) Without RLS armed, all sessions see datoms from every store.
--   2) After mentat.enable_multi_tenant_rls(true), a session only sees
--      datoms whose store_id matches mentat.current_store_id.
--   3) Cross-tenant writes are rejected by the WITH CHECK predicate.
--   4) mentat.enable_multi_tenant_rls(false) returns the system to the
--      single-store, no-overhead default.
--
-- IMPORTANT: PostgreSQL bypasses RLS for both the table owner and any
-- BYPASSRLS / superuser role. The pgrx-managed test cluster connects as
-- a superuser, so the test creates a dedicated NOSUPERUSER role and
-- exercises every RLS-sensitive step under SET ROLE -- otherwise the
-- policies would silently no-op.
--
-- Style: matches the existing pg_mentat/sql/tests/test_*.sql files.
-- Aborts on any RAISE EXCEPTION (\set ON_ERROR_STOP on).

\set ON_ERROR_STOP on
\pset pager off

\echo '=== Testing Multi-Tenant RLS ==='
\echo ''

-- Pre-flight: clear any leftover state from prior runs of this file.
SELECT mentat.enable_multi_tenant_rls(false);
DELETE FROM mentat.datoms_long_new WHERE e IN (8000001, 8000002, 8000099);
DELETE FROM mentat.stores WHERE store_name IN ('alice', 'bob');

-- Step 0: create / refresh the non-superuser test role ---------------------
\echo 'Step 0: create non-superuser test role mentat_rls_test'
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mentat_rls_test') THEN
        CREATE ROLE mentat_rls_test NOSUPERUSER NOBYPASSRLS NOINHERIT;
    END IF;
END $$;

GRANT USAGE ON SCHEMA mentat TO mentat_rls_test;
GRANT SELECT, INSERT, DELETE ON mentat.datoms_long_new TO mentat_rls_test;
GRANT SELECT ON mentat.stores TO mentat_rls_test;
GRANT USAGE ON SEQUENCE mentat.partition_tx_seq TO mentat_rls_test;
GRANT EXECUTE ON FUNCTION mentat.current_store_id() TO mentat_rls_test;

-- Step 1: register two stores ----------------------------------------------
-- We bypass mentat_create_store() on purpose: that helper creates a whole
-- per-store schema, which is the legacy single-store-per-schema path. The
-- narrow datom tables in the `mentat` schema are shared across all stores
-- and discriminated by store_id, which is what RLS protects.
\echo 'Step 1: register two stores (alice, bob)'
INSERT INTO mentat.stores (store_name, schema_name, description)
VALUES
    ('alice', 'mentat_rls_test_alice', 'RLS-isolation test tenant alice'),
    ('bob',   'mentat_rls_test_bob',   'RLS-isolation test tenant bob');

DO $$
DECLARE
    alice_id BIGINT;
    bob_id   BIGINT;
BEGIN
    SELECT store_id INTO alice_id FROM mentat.stores WHERE store_name = 'alice';
    SELECT store_id INTO bob_id   FROM mentat.stores WHERE store_name = 'bob';
    IF alice_id IS NULL OR bob_id IS NULL OR alice_id = bob_id THEN
        RAISE EXCEPTION 'expected distinct store_ids for alice/bob, got % and %',
            alice_id, bob_id;
    END IF;
    -- Stash for later steps via session GUC (placeholders are fine).
    PERFORM set_config('mentat.test_alice_id', alice_id::text, false);
    PERFORM set_config('mentat.test_bob_id',   bob_id::text,   false);
    RAISE NOTICE 'alice store_id=%, bob store_id=%', alice_id, bob_id;
END $$;

-- Step 2: insert one datom per store as the (superuser) owner --------------
-- This step deliberately runs as the table owner so that the WITH CHECK
-- predicate is bypassed during fixture setup.
\echo 'Step 2: insert one datom per store into mentat.datoms_long_new'
DO $$
DECLARE
    alice_id BIGINT := current_setting('mentat.test_alice_id')::bigint;
    bob_id   BIGINT := current_setting('mentat.test_bob_id')::bigint;
    tx_a     BIGINT := nextval('mentat.partition_tx_seq');
    tx_b     BIGINT := nextval('mentat.partition_tx_seq');
BEGIN
    INSERT INTO mentat.datoms_long_new (store_id, e, a, v, tx, added)
    VALUES (alice_id, 8000001, 0, 100, tx_a, true);
    INSERT INTO mentat.datoms_long_new (store_id, e, a, v, tx, added)
    VALUES (bob_id,   8000002, 0, 200, tx_b, true);
END $$;

-- Step 3: without RLS, both rows are visible -------------------------------
\echo 'Step 3: without RLS, the test role sees both datoms'
SET ROLE mentat_rls_test;
DO $$
DECLARE
    visible BIGINT;
BEGIN
    SELECT count(*) INTO visible
      FROM mentat.datoms_long_new
     WHERE e IN (8000001, 8000002);
    IF visible <> 2 THEN
        RAISE EXCEPTION
            'pre-RLS: expected 2 visible datoms, got %', visible;
    END IF;
    RAISE NOTICE 'Step 3 OK: pre-RLS sees both datoms (count=%)', visible;
END $$;
RESET ROLE;

-- Step 4: enable RLS, set alice -> only alice's datom is visible -----------
\echo 'Step 4: enable RLS, set mentat.current_store_id to alice'
SELECT mentat.enable_multi_tenant_rls(true);

DO $$
DECLARE
    alice_id BIGINT := current_setting('mentat.test_alice_id')::bigint;
BEGIN
    PERFORM set_config('mentat.current_store_id', alice_id::text, false);
END $$;

SET ROLE mentat_rls_test;
DO $$
DECLARE
    visible  BIGINT;
    e_seen   BIGINT;
BEGIN
    SELECT count(*) INTO visible
      FROM mentat.datoms_long_new
     WHERE e IN (8000001, 8000002);
    IF visible <> 1 THEN
        RAISE EXCEPTION
            'RLS+alice: expected 1 visible datom, got %', visible;
    END IF;
    SELECT e INTO e_seen
      FROM mentat.datoms_long_new
     WHERE e IN (8000001, 8000002);
    IF e_seen <> 8000001 THEN
        RAISE EXCEPTION
            'RLS+alice: expected to see e=8000001, got e=%', e_seen;
    END IF;
    RAISE NOTICE 'Step 4 OK: alice sees only e=8000001';
END $$;
RESET ROLE;

-- Step 5: switch to bob -> only bob's datom is visible ---------------------
\echo 'Step 5: switch session to bob, verify isolation'
DO $$
DECLARE
    bob_id  BIGINT := current_setting('mentat.test_bob_id')::bigint;
BEGIN
    PERFORM set_config('mentat.current_store_id', bob_id::text, false);
END $$;

SET ROLE mentat_rls_test;
DO $$
DECLARE
    visible BIGINT;
    e_seen  BIGINT;
BEGIN
    SELECT count(*) INTO visible
      FROM mentat.datoms_long_new
     WHERE e IN (8000001, 8000002);
    IF visible <> 1 THEN
        RAISE EXCEPTION
            'RLS+bob: expected 1 visible datom, got %', visible;
    END IF;
    SELECT e INTO e_seen
      FROM mentat.datoms_long_new
     WHERE e IN (8000001, 8000002);
    IF e_seen <> 8000002 THEN
        RAISE EXCEPTION
            'RLS+bob: expected to see e=8000002, got e=%', e_seen;
    END IF;
    RAISE NOTICE 'Step 5 OK: bob sees only e=8000002';
END $$;
RESET ROLE;

-- Step 6: WITH CHECK rejects cross-tenant writes ---------------------------
-- Bob's session must not be able to insert a row tagged with alice's
-- store_id. The policy's WITH CHECK clause rejects it. Both
-- check_violation (the CREATE POLICY ... WITH CHECK path) and
-- insufficient_privilege (the older RLS denial code) are accepted as a
-- pass.
\echo 'Step 6: WITH CHECK rejects cross-tenant writes'
SET ROLE mentat_rls_test;
DO $$
DECLARE
    alice_id BIGINT := current_setting('mentat.test_alice_id')::bigint;
    tx_x     BIGINT := nextval('mentat.partition_tx_seq');
    rejected BOOLEAN := false;
BEGIN
    BEGIN
        INSERT INTO mentat.datoms_long_new (store_id, e, a, v, tx, added)
        VALUES (alice_id, 8000099, 0, 999, tx_x, true);
    EXCEPTION
        WHEN insufficient_privilege THEN
            rejected := true;
        WHEN check_violation THEN
            rejected := true;
    END;
    IF NOT rejected THEN
        RAISE EXCEPTION
            'expected RLS WITH CHECK to reject cross-tenant write (bob -> alice)';
    END IF;
    RAISE NOTICE 'Step 6 OK: cross-tenant write rejected';
END $$;
RESET ROLE;

-- Step 7: disable RLS and clean up -----------------------------------------
\echo 'Step 7: disable RLS and clean up test fixtures'
RESET mentat.current_store_id;

DO $$
DECLARE
    alice_id BIGINT := current_setting('mentat.test_alice_id')::bigint;
    bob_id   BIGINT := current_setting('mentat.test_bob_id')::bigint;
    n_off    INT;
    visible  BIGINT;
BEGIN
    n_off := mentat.enable_multi_tenant_rls(false);
    IF n_off <> 9 THEN
        RAISE EXCEPTION 'enable_multi_tenant_rls(false) reported %, expected 9', n_off;
    END IF;

    -- Sanity: with RLS off, both rows are visible again from any role.
    SELECT count(*) INTO visible
      FROM mentat.datoms_long_new
     WHERE e IN (8000001, 8000002);
    IF visible <> 2 THEN
        RAISE EXCEPTION
            'post-disable: expected 2 visible datoms, got %', visible;
    END IF;

    DELETE FROM mentat.datoms_long_new
     WHERE store_id IN (alice_id, bob_id) OR e IN (8000099);
    DELETE FROM mentat.stores WHERE store_name IN ('alice', 'bob');
    RAISE NOTICE 'Step 7 OK: RLS disabled, fixtures removed';
END $$;

-- Drop privileges and the test role so subsequent runs do not warn about
-- pre-existing grants. We do this last so any error above leaves an audit
-- trail in pg_roles for triage.
REVOKE ALL ON mentat.datoms_long_new      FROM mentat_rls_test;
REVOKE ALL ON mentat.stores               FROM mentat_rls_test;
REVOKE ALL ON SEQUENCE mentat.partition_tx_seq FROM mentat_rls_test;
REVOKE ALL ON FUNCTION mentat.current_store_id() FROM mentat_rls_test;
REVOKE USAGE ON SCHEMA mentat             FROM mentat_rls_test;
DROP ROLE IF EXISTS mentat_rls_test;

\echo ''
\echo '=== Multi-Tenant RLS Tests Complete ==='
