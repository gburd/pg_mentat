-- Narrow per-type storage tables.
--
-- After Phase 1 these are the ONLY place actual datom data lives. The
-- old wide-row `mentat.datoms` table is now a VIEW over these tables
-- (defined at the bottom of this file) with INSTEAD OF INSERT/DELETE
-- triggers for compatibility with callers that still write to it.
--
-- Each table has one non-NULL value column (`v`), matching the value's
-- native PG type. Covering indexes with INCLUDE clauses enable
-- index-only scans for EAVT / AEVT access patterns.

-- ---------------------------------------------------------------------------
-- Nine narrow per-type tables
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS mentat.datoms_ref_new (
    store_id BIGINT  NOT NULL DEFAULT 0,
    e        BIGINT  NOT NULL,
    a        BIGINT  NOT NULL,
    v        BIGINT  NOT NULL,
    tx       BIGINT  NOT NULL,
    added    BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, v, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_long_new (
    store_id BIGINT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v BIGINT NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, v, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_text_new (
    store_id BIGINT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v TEXT NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, v, tx)
) WITH (fillfactor = 85, toast_tuple_target = 8160);

CREATE TABLE IF NOT EXISTS mentat.datoms_double_new (
    store_id BIGINT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v DOUBLE PRECISION NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, v, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_instant_new (
    store_id BIGINT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v TIMESTAMPTZ NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, v, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_keyword_new (
    store_id BIGINT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v TEXT NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, v, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_uuid_new (
    store_id BIGINT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v UUID NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, v, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_bytes_new (
    store_id BIGINT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v BYTEA NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, v, tx)
) WITH (fillfactor = 85, toast_tuple_target = 8160);

CREATE TABLE IF NOT EXISTS mentat.datoms_boolean_new (
    store_id BIGINT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v BOOLEAN NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, v, tx)
) WITH (fillfactor = 90);

-- ---------------------------------------------------------------------------
-- Covering indexes. Each table gets EAVT, AEVT, TX; VAET only where reverse
-- lookups make sense (ref + keyword). Partial on `added` to keep live-query
-- indexes small; retractions still sit in the heap for history queries.
-- ---------------------------------------------------------------------------

-- ref: all four access patterns (refs are the backbone of graph traversal)
CREATE INDEX IF NOT EXISTS idx_datoms_ref_new_eavt
    ON mentat.datoms_ref_new (store_id, e, a, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_ref_new_aevt
    ON mentat.datoms_ref_new (store_id, a, e, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_ref_new_vaet
    ON mentat.datoms_ref_new (store_id, v, a, e, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_ref_new_tx
    ON mentat.datoms_ref_new (store_id, tx DESC) INCLUDE (e, a, v) WHERE added;

-- long: no VAET (range queries by value are uncommon; AVET covers the rest)
CREATE INDEX IF NOT EXISTS idx_datoms_long_new_eavt
    ON mentat.datoms_long_new (store_id, e, a, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_long_new_aevt
    ON mentat.datoms_long_new (store_id, a, e, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_long_new_tx
    ON mentat.datoms_long_new (store_id, tx DESC) INCLUDE (e, a, v) WHERE added;

-- text: no VAET (too wide); GIN fulltext index instead
CREATE INDEX IF NOT EXISTS idx_datoms_text_new_eavt
    ON mentat.datoms_text_new (store_id, e, a, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_text_new_aevt
    ON mentat.datoms_text_new (store_id, a, e, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_text_new_tx
    ON mentat.datoms_text_new (store_id, tx DESC) INCLUDE (e, a, v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_text_new_fts
    ON mentat.datoms_text_new USING gin(to_tsvector('english', v)) WHERE added;

-- double, instant: standard three-way coverage
CREATE INDEX IF NOT EXISTS idx_datoms_double_new_eavt
    ON mentat.datoms_double_new (store_id, e, a, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_double_new_aevt
    ON mentat.datoms_double_new (store_id, a, e, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_double_new_tx
    ON mentat.datoms_double_new (store_id, tx DESC) INCLUDE (e, a, v) WHERE added;

CREATE INDEX IF NOT EXISTS idx_datoms_instant_new_eavt
    ON mentat.datoms_instant_new (store_id, e, a, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_instant_new_aevt
    ON mentat.datoms_instant_new (store_id, a, e, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_instant_new_tx
    ON mentat.datoms_instant_new (store_id, tx DESC) INCLUDE (e, a, v) WHERE added;

-- keyword: VAET matters (idents resolve keywords <-> entity-ids)
CREATE INDEX IF NOT EXISTS idx_datoms_keyword_new_eavt
    ON mentat.datoms_keyword_new (store_id, e, a, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_keyword_new_aevt
    ON mentat.datoms_keyword_new (store_id, a, e, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_keyword_new_vaet
    ON mentat.datoms_keyword_new (store_id, v, a, e, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_keyword_new_tx
    ON mentat.datoms_keyword_new (store_id, tx DESC) INCLUDE (e, a, v) WHERE added;

-- uuid, bytes, boolean
CREATE INDEX IF NOT EXISTS idx_datoms_uuid_new_eavt
    ON mentat.datoms_uuid_new (store_id, e, a, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_uuid_new_aevt
    ON mentat.datoms_uuid_new (store_id, a, e, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_uuid_new_tx
    ON mentat.datoms_uuid_new (store_id, tx DESC) INCLUDE (e, a, v) WHERE added;

CREATE INDEX IF NOT EXISTS idx_datoms_bytes_new_eavt
    ON mentat.datoms_bytes_new (store_id, e, a, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_bytes_new_aevt
    ON mentat.datoms_bytes_new (store_id, a, e, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_bytes_new_tx
    ON mentat.datoms_bytes_new (store_id, tx DESC) INCLUDE (e, a) WHERE added;

CREATE INDEX IF NOT EXISTS idx_datoms_boolean_new_eavt
    ON mentat.datoms_boolean_new (store_id, e, a, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_boolean_new_aevt
    ON mentat.datoms_boolean_new (store_id, a, e, tx) INCLUDE (v) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_boolean_new_tx
    ON mentat.datoms_boolean_new (store_id, tx DESC) INCLUDE (e, a, v) WHERE added;

-- Aggressive autovacuum on high-churn narrow tables (retraction-heavy)
ALTER TABLE mentat.datoms_ref_new     SET (autovacuum_vacuum_scale_factor = 0.05, autovacuum_analyze_scale_factor = 0.02);
ALTER TABLE mentat.datoms_long_new    SET (autovacuum_vacuum_scale_factor = 0.05, autovacuum_analyze_scale_factor = 0.02);
ALTER TABLE mentat.datoms_text_new    SET (autovacuum_vacuum_scale_factor = 0.05, autovacuum_analyze_scale_factor = 0.02);
ALTER TABLE mentat.datoms_keyword_new SET (autovacuum_vacuum_scale_factor = 0.05, autovacuum_analyze_scale_factor = 0.02);

-- ---------------------------------------------------------------------------
-- mentat.datoms VIEW + INSTEAD OF triggers
--
-- Phase 1 result: the wide-row mentat.datoms TABLE is gone. In its place
-- is a VIEW over the nine narrow tables that reproduces the old column
-- shape (e, a, value_type_tag, v_ref, v_bool, ..., tx, added) so that
-- existing readers keep working. Two INSTEAD OF triggers route INSERT
-- and DELETE to the appropriate narrow table, so existing writers keep
-- working too. The view is a backwards-compatibility shim, not a
-- long-term API: new code should read and write the narrow tables
-- directly, via the query engine in functions/query.rs or the transact
-- pipeline in functions/transact.rs.
--
-- Value type tags (must match functions/query.rs::type_tag and
-- functions/transact.rs):
--    0 = ref       1 = boolean   2 = long      3 = double
--    4 = instant   7 = string    8 = keyword   10 = uuid
--   11 = bytes
-- ---------------------------------------------------------------------------

CREATE OR REPLACE VIEW mentat.datoms AS
    SELECT e, a,  0::SMALLINT AS value_type_tag,
           v AS v_ref, NULL::BOOLEAN AS v_bool, NULL::BIGINT AS v_long,
           NULL::DOUBLE PRECISION AS v_double, NULL::TEXT AS v_text,
           NULL::TEXT AS v_keyword, NULL::TIMESTAMPTZ AS v_instant,
           NULL::UUID AS v_uuid, NULL::BYTEA AS v_bytes, tx, added
    FROM mentat.datoms_ref_new WHERE store_id = 0
    UNION ALL
    SELECT e, a, 1::SMALLINT, NULL, v, NULL, NULL, NULL, NULL, NULL, NULL, NULL, tx, added
    FROM mentat.datoms_boolean_new WHERE store_id = 0
    UNION ALL
    SELECT e, a, 2::SMALLINT, NULL, NULL, v, NULL, NULL, NULL, NULL, NULL, NULL, tx, added
    FROM mentat.datoms_long_new WHERE store_id = 0
    UNION ALL
    SELECT e, a, 3::SMALLINT, NULL, NULL, NULL, v, NULL, NULL, NULL, NULL, NULL, tx, added
    FROM mentat.datoms_double_new WHERE store_id = 0
    UNION ALL
    SELECT e, a, 4::SMALLINT, NULL, NULL, NULL, NULL, NULL, NULL, v, NULL, NULL, tx, added
    FROM mentat.datoms_instant_new WHERE store_id = 0
    UNION ALL
    SELECT e, a, 7::SMALLINT, NULL, NULL, NULL, NULL, v, NULL, NULL, NULL, NULL, tx, added
    FROM mentat.datoms_text_new WHERE store_id = 0
    UNION ALL
    SELECT e, a, 8::SMALLINT, NULL, NULL, NULL, NULL, NULL, v, NULL, NULL, NULL, tx, added
    FROM mentat.datoms_keyword_new WHERE store_id = 0
    UNION ALL
    SELECT e, a, 10::SMALLINT, NULL, NULL, NULL, NULL, NULL, NULL, NULL, v, NULL, tx, added
    FROM mentat.datoms_uuid_new WHERE store_id = 0
    UNION ALL
    SELECT e, a, 11::SMALLINT, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, v, tx, added
    FROM mentat.datoms_bytes_new WHERE store_id = 0;

-- INSTEAD OF INSERT: route to the correct narrow table by value_type_tag.
-- Default store_id to 0 (the wide-row shim has no notion of multi-store;
-- the narrow-table writers in transact.rs handle stores directly).
CREATE OR REPLACE FUNCTION mentat.datoms_view_insert()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.tx IS NULL OR NEW.added IS NULL THEN
        RAISE EXCEPTION 'mentat.datoms INSERT requires tx and added to be non-NULL';
    END IF;
    CASE NEW.value_type_tag
        WHEN 0  THEN INSERT INTO mentat.datoms_ref_new     (store_id, e, a, v, tx, added) VALUES (0, NEW.e, NEW.a, NEW.v_ref,     NEW.tx, NEW.added) ON CONFLICT DO NOTHING;
        WHEN 1  THEN INSERT INTO mentat.datoms_boolean_new (store_id, e, a, v, tx, added) VALUES (0, NEW.e, NEW.a, NEW.v_bool,    NEW.tx, NEW.added) ON CONFLICT DO NOTHING;
        WHEN 2  THEN INSERT INTO mentat.datoms_long_new    (store_id, e, a, v, tx, added) VALUES (0, NEW.e, NEW.a, NEW.v_long,    NEW.tx, NEW.added) ON CONFLICT DO NOTHING;
        WHEN 3  THEN INSERT INTO mentat.datoms_double_new  (store_id, e, a, v, tx, added) VALUES (0, NEW.e, NEW.a, NEW.v_double,  NEW.tx, NEW.added) ON CONFLICT DO NOTHING;
        WHEN 4  THEN INSERT INTO mentat.datoms_instant_new (store_id, e, a, v, tx, added) VALUES (0, NEW.e, NEW.a, NEW.v_instant, NEW.tx, NEW.added) ON CONFLICT DO NOTHING;
        WHEN 7  THEN INSERT INTO mentat.datoms_text_new    (store_id, e, a, v, tx, added) VALUES (0, NEW.e, NEW.a, NEW.v_text,    NEW.tx, NEW.added) ON CONFLICT DO NOTHING;
        WHEN 8  THEN INSERT INTO mentat.datoms_keyword_new (store_id, e, a, v, tx, added) VALUES (0, NEW.e, NEW.a, NEW.v_keyword, NEW.tx, NEW.added) ON CONFLICT DO NOTHING;
        WHEN 10 THEN INSERT INTO mentat.datoms_uuid_new    (store_id, e, a, v, tx, added) VALUES (0, NEW.e, NEW.a, NEW.v_uuid,    NEW.tx, NEW.added) ON CONFLICT DO NOTHING;
        WHEN 11 THEN INSERT INTO mentat.datoms_bytes_new   (store_id, e, a, v, tx, added) VALUES (0, NEW.e, NEW.a, NEW.v_bytes,   NEW.tx, NEW.added) ON CONFLICT DO NOTHING;
        ELSE RAISE EXCEPTION 'mentat.datoms INSERT: unknown value_type_tag %', NEW.value_type_tag;
    END CASE;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS datoms_view_insert ON mentat.datoms;
CREATE TRIGGER datoms_view_insert
    INSTEAD OF INSERT ON mentat.datoms
    FOR EACH ROW EXECUTE FUNCTION mentat.datoms_view_insert();

-- INSTEAD OF DELETE: fan out the DELETE across all narrow tables.
-- The test code uses `DELETE FROM mentat.datoms WHERE e IN (...)` to
-- clear fixtures; most uses are e-based and the narrow tables all have
-- `e` as the second PK column, so the DELETE is cheap on each table.
CREATE OR REPLACE FUNCTION mentat.datoms_view_delete()
RETURNS TRIGGER AS $$
BEGIN
    CASE OLD.value_type_tag
        WHEN 0  THEN DELETE FROM mentat.datoms_ref_new     WHERE store_id = 0 AND e = OLD.e AND a = OLD.a AND tx = OLD.tx;
        WHEN 1  THEN DELETE FROM mentat.datoms_boolean_new WHERE store_id = 0 AND e = OLD.e AND a = OLD.a AND tx = OLD.tx;
        WHEN 2  THEN DELETE FROM mentat.datoms_long_new    WHERE store_id = 0 AND e = OLD.e AND a = OLD.a AND tx = OLD.tx;
        WHEN 3  THEN DELETE FROM mentat.datoms_double_new  WHERE store_id = 0 AND e = OLD.e AND a = OLD.a AND tx = OLD.tx;
        WHEN 4  THEN DELETE FROM mentat.datoms_instant_new WHERE store_id = 0 AND e = OLD.e AND a = OLD.a AND tx = OLD.tx;
        WHEN 7  THEN DELETE FROM mentat.datoms_text_new    WHERE store_id = 0 AND e = OLD.e AND a = OLD.a AND tx = OLD.tx;
        WHEN 8  THEN DELETE FROM mentat.datoms_keyword_new WHERE store_id = 0 AND e = OLD.e AND a = OLD.a AND tx = OLD.tx;
        WHEN 10 THEN DELETE FROM mentat.datoms_uuid_new    WHERE store_id = 0 AND e = OLD.e AND a = OLD.a AND tx = OLD.tx;
        WHEN 11 THEN DELETE FROM mentat.datoms_bytes_new   WHERE store_id = 0 AND e = OLD.e AND a = OLD.a AND tx = OLD.tx;
        ELSE NULL;
    END CASE;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS datoms_view_delete ON mentat.datoms;
CREATE TRIGGER datoms_view_delete
    INSTEAD OF DELETE ON mentat.datoms
    FOR EACH ROW EXECUTE FUNCTION mentat.datoms_view_delete();

-- ---------------------------------------------------------------------------
-- Extended statistics for the planner.
--
-- For Datalog workloads the planner's default assumption (columns are
-- independent) is wrong: `a` (attribute) and `e` (entity) are correlated
-- via cardinality, and user attributes exhibit strong ndistinct skew.
-- These statistics teach the planner about those correlations so it picks
-- the right index without having to force `enable_seqscan = off`.
-- ---------------------------------------------------------------------------

CREATE STATISTICS IF NOT EXISTS mentat.stats_datoms_ref_new_ae
    (ndistinct, dependencies, mcv)  ON a, e FROM mentat.datoms_ref_new;
CREATE STATISTICS IF NOT EXISTS mentat.stats_datoms_long_new_ae
    (ndistinct, dependencies, mcv)  ON a, e FROM mentat.datoms_long_new;
CREATE STATISTICS IF NOT EXISTS mentat.stats_datoms_text_new_ae
    (ndistinct, dependencies, mcv)  ON a, e FROM mentat.datoms_text_new;
CREATE STATISTICS IF NOT EXISTS mentat.stats_datoms_keyword_new_ae
    (ndistinct, dependencies, mcv)  ON a, e FROM mentat.datoms_keyword_new;
CREATE STATISTICS IF NOT EXISTS mentat.stats_datoms_instant_new_ae
    (ndistinct, dependencies, mcv)  ON a, e FROM mentat.datoms_instant_new;

-- ---------------------------------------------------------------------------
-- Multi-tenant Row-Level Security (RLS) for the nine narrow datom tables.
--
-- Design choice (the simpler of the two options described in the task):
--   * The policies are ALWAYS DEFINED on every narrow table so that
--     CREATE EXTENSION leaves a consistent, ready-to-arm configuration.
--   * Row-level security is DISABLED on every table by default. With RLS
--     disabled the policies are inert and queries see zero overhead, so
--     single-store users (the overwhelmingly common case) pay nothing.
--   * To opt in, an operator calls `mentat.enable_multi_tenant_rls(true)`
--     ONCE per database. That single call ENABLEs ROW LEVEL SECURITY on
--     all nine tables and from that point forward every session that
--     wants to read or write datoms must `SET mentat.current_store_id =
--     '<store-id>'` first; otherwise `mentat.current_store_id()` falls
--     back to 0 (the default store) and only datoms in store 0 are
--     visible.
--   * A matching session GUC `mentat.enable_multi_tenant_rls` (registered
--     in src/planner/hooks.rs, default OFF) signals operator intent and
--     is documented as the contract: "set the GUC AND call the toggle
--     function". The GUC itself does not gate enforcement -- enforcement
--     is purely a property of the table-level RLS state -- but the GUC
--     is the canonical user-visible knob and is checked by tooling and
--     audit views.
--
-- Threat model (also documented in docs/src/multi-tenancy.md):
--   * Protects: cross-tenant reads/writes from regular roles that forget
--     a `WHERE store_id = ...` clause. The database silently filters.
--   * Does NOT protect against: superusers (BYPASSRLS), the table owner
--     (BYPASSRLS by default), `SECURITY DEFINER` functions owned by a
--     superuser, direct access to the underlying file system, or a
--     malicious tenant who can SET mentat.current_store_id to another
--     tenant's id. Tenant-id assignment must be done by a trusted
--     middleware/role -- typically by SET ROLE-ing into a per-tenant
--     non-superuser role and using `ALTER ROLE ... SET
--     mentat.current_store_id = '<id>'`.
-- ---------------------------------------------------------------------------

-- Helper: read mentat.current_store_id from the session, defaulting to 0.
-- STABLE so the planner can fold it into index scans.
CREATE OR REPLACE FUNCTION mentat.current_store_id()
RETURNS BIGINT AS $$
BEGIN
    RETURN current_setting('mentat.current_store_id', true)::BIGINT;
EXCEPTION
    WHEN OTHERS THEN
        -- Setting unset, empty, or non-integer -> default store.
        RETURN 0;
END;
$$ LANGUAGE plpgsql STABLE;

COMMENT ON FUNCTION mentat.current_store_id() IS
  'Returns the current session''s mentat.current_store_id GUC as BIGINT, '
  'or 0 (the default store) if unset or unparseable. Used by the per-store '
  'RLS policies on the nine narrow datom tables.';

-- Define one policy per narrow table. Idempotent via DROP POLICY IF EXISTS
-- so a re-run of CREATE EXTENSION (or a DROP/CREATE cycle) succeeds.
DO $rls$
DECLARE
    t TEXT;
    p TEXT;
    tables_policies TEXT[][] := ARRAY[
        ARRAY['datoms_ref_new',     'store_isolation_ref'],
        ARRAY['datoms_long_new',    'store_isolation_long'],
        ARRAY['datoms_text_new',    'store_isolation_text'],
        ARRAY['datoms_double_new',  'store_isolation_double'],
        ARRAY['datoms_instant_new', 'store_isolation_instant'],
        ARRAY['datoms_keyword_new', 'store_isolation_keyword'],
        ARRAY['datoms_uuid_new',    'store_isolation_uuid'],
        ARRAY['datoms_bytes_new',   'store_isolation_bytes'],
        ARRAY['datoms_boolean_new', 'store_isolation_boolean']
    ];
BEGIN
    FOR i IN 1 .. array_length(tables_policies, 1) LOOP
        t := tables_policies[i][1];
        p := tables_policies[i][2];
        EXECUTE format('DROP POLICY IF EXISTS %I ON mentat.%I', p, t);
        EXECUTE format(
            'CREATE POLICY %I ON mentat.%I '
            'USING (store_id = mentat.current_store_id()) '
            'WITH CHECK (store_id = mentat.current_store_id())',
            p, t
        );
        -- Belt-and-braces: leave RLS DISABLED at install time. The
        -- mentat.enable_multi_tenant_rls(boolean) wrapper below is the
        -- only supported way to flip it on.
        EXECUTE format('ALTER TABLE mentat.%I DISABLE ROW LEVEL SECURITY', t);
    END LOOP;
END;
$rls$ LANGUAGE plpgsql;

-- Wrapper: toggle RLS enforcement on/off across all nine narrow tables in
-- a single call. Returns the count of tables affected.
--
-- This is intentionally an explicit one-shot operator action rather than
-- something keyed off a GUC: enabling RLS on a table requires AccessExclusive
-- briefly and we do NOT want it happening implicitly per-session.
CREATE OR REPLACE FUNCTION mentat.enable_multi_tenant_rls(enable BOOLEAN)
RETURNS INT AS $$
DECLARE
    t TEXT;
    n INT := 0;
    narrow_tables TEXT[] := ARRAY[
        'datoms_ref_new', 'datoms_long_new', 'datoms_text_new',
        'datoms_double_new', 'datoms_instant_new', 'datoms_keyword_new',
        'datoms_uuid_new', 'datoms_bytes_new', 'datoms_boolean_new'
    ];
BEGIN
    FOREACH t IN ARRAY narrow_tables LOOP
        IF enable THEN
            EXECUTE format('ALTER TABLE mentat.%I ENABLE ROW LEVEL SECURITY', t);
        ELSE
            EXECUTE format('ALTER TABLE mentat.%I DISABLE ROW LEVEL SECURITY', t);
        END IF;
        n := n + 1;
    END LOOP;
    RETURN n;
END;
$$ LANGUAGE plpgsql;

COMMENT ON FUNCTION mentat.enable_multi_tenant_rls(boolean) IS
  'Toggle Row-Level Security on the nine narrow datom tables. '
  'Pass true to enforce per-store isolation (queries are filtered by '
  'store_id = mentat.current_store_id()); pass false to disable enforcement '
  'and revert to single-store behaviour. Returns the number of tables '
  'affected (always 9 on success). Pair with the mentat.enable_multi_tenant_rls '
  'GUC, which signals operator intent at the session/role level.';
