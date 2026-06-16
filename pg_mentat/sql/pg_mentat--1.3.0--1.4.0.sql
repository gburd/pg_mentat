-- pg_mentat upgrade from 1.3.0 to 1.4.0
--
-- 1.4.0 is a performance + operations release driven by production
-- feedback. SQL-side changes shipped here:
--   * Retune autovacuum on all 9 narrow datom tables + transactions
--     to scale_factor=0 + fixed threshold (the previous 0.05 scale
--     factor stopped triggering on large tables -> unbounded bloat).
--   * New operational accessors: mentat.attr_id, mentat.current,
--     mentat.attribute_health (sql/23_operational_accessors.sql).
--
-- The cardinality-one transact fast path (single-table probe instead
-- of a 9-way UNION ALL) lives in the loadable library and needs no
-- SQL migration.

-- === autovacuum retune (mirrors sql/10_narrow_storage.sql) ===
DO $$
DECLARE t TEXT;
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'datoms_ref_new', 'datoms_boolean_new', 'datoms_long_new',
        'datoms_double_new', 'datoms_instant_new', 'datoms_text_new',
        'datoms_keyword_new', 'datoms_uuid_new', 'datoms_bytes_new'
    ] LOOP
        EXECUTE format(
            'ALTER TABLE mentat.%I SET ('
            || 'autovacuum_vacuum_scale_factor = 0, '
            || 'autovacuum_vacuum_threshold = 50000, '
            || 'autovacuum_analyze_scale_factor = 0, '
            || 'autovacuum_analyze_threshold = 50000)',
            t
        );
    END LOOP;
END $$;

ALTER TABLE mentat.transactions SET (
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 50000,
    autovacuum_analyze_scale_factor = 0,
    autovacuum_analyze_threshold = 50000
);

-- === operational accessors ===
-- pg_mentat operational + read-path accessors.
--
-- Added in response to production feedback (see CHANGELOG 1.4.0):
--   * mentat.attr_id(ident)        — resolve an attribute keyword to its
--                                     opaque entid for use in views/SQL.
--   * mentat.current(e, a)         — index-backed "current value of
--                                     attribute a for entity e" without a
--                                     per-query DISTINCT ON / LATERAL.
--   * mentat.current_text(e, a)    — typed convenience wrappers returning
--     mentat.current_long(e, a)      the value already cast, for the common
--     mentat.current_ref(e, a)       single-type attributes.
--     mentat.current_instant(e, a)
--   * mentat.attribute_health()    — per-attribute datom counts + dead-tuple
--                                     %, so operators can alert before bloat
--                                     bites.

-- ---------------------------------------------------------------------------
-- mentat.attr_id(ident) -> BIGINT
--
-- Resolve a ':namespace/name' keyword to its entid. Marked STABLE so the
-- planner can fold it once per query. Self-documenting alternative to the
-- opaque 'a = 1308861' literals that otherwise appear in generated viewdefs.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION mentat.attr_id(attr_ident TEXT)
RETURNS BIGINT
LANGUAGE sql STABLE
AS $$
    SELECT entid FROM mentat.schema WHERE ident = attr_ident;
$$;

-- ---------------------------------------------------------------------------
-- mentat.current(e, a) -> TEXT
--
-- Returns the current (latest, added=true) value of attribute `a` for
-- entity `e`, rendered as TEXT, by probing the narrow datom tables in
-- value-type order. Each probe is a single index lookup on the
-- (store_id, e, a, tx DESC) ... WHERE added covering index, so this is
-- the cheap "live projection" accessor the views can call per (e, a)
-- instead of a DISTINCT ON / LATERAL fan-out.
--
-- `a` is the attribute entid (use mentat.attr_id(':ns/name') to resolve).
-- `store` defaults to 0 (the default store).
--
-- Returns NULL if the entity has no current value for the attribute.
--
-- The function dispatches on the attribute's declared value_type so only
-- ONE narrow table is touched. For attributes whose type is unknown at
-- call time it falls back to probing all tables in tag order.
CREATE OR REPLACE FUNCTION mentat.current(p_e BIGINT, p_a BIGINT, p_store BIGINT DEFAULT 0)
RETURNS TEXT
LANGUAGE plpgsql STABLE
AS $$
DECLARE
    v_type TEXT;
    v_out  TEXT;
BEGIN
    SELECT value_type::text INTO v_type FROM mentat.schema WHERE entid = p_a;

    -- Dispatch on the declared type: a single indexed lookup.
    IF v_type = 'string' THEN
        SELECT v INTO v_out FROM mentat.datoms_text_new
            WHERE store_id = p_store AND e = p_e AND a = p_a AND added
            ORDER BY tx DESC LIMIT 1;
    ELSIF v_type = 'keyword' THEN
        SELECT v INTO v_out FROM mentat.datoms_keyword_new
            WHERE store_id = p_store AND e = p_e AND a = p_a AND added
            ORDER BY tx DESC LIMIT 1;
    ELSIF v_type = 'long' THEN
        SELECT v::text INTO v_out FROM mentat.datoms_long_new
            WHERE store_id = p_store AND e = p_e AND a = p_a AND added
            ORDER BY tx DESC LIMIT 1;
    ELSIF v_type = 'ref' THEN
        SELECT v::text INTO v_out FROM mentat.datoms_ref_new
            WHERE store_id = p_store AND e = p_e AND a = p_a AND added
            ORDER BY tx DESC LIMIT 1;
    ELSIF v_type = 'double' THEN
        SELECT v::text INTO v_out FROM mentat.datoms_double_new
            WHERE store_id = p_store AND e = p_e AND a = p_a AND added
            ORDER BY tx DESC LIMIT 1;
    ELSIF v_type = 'boolean' THEN
        SELECT v::text INTO v_out FROM mentat.datoms_boolean_new
            WHERE store_id = p_store AND e = p_e AND a = p_a AND added
            ORDER BY tx DESC LIMIT 1;
    ELSIF v_type = 'instant' THEN
        SELECT to_char(v, 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') INTO v_out
            FROM mentat.datoms_instant_new
            WHERE store_id = p_store AND e = p_e AND a = p_a AND added
            ORDER BY tx DESC LIMIT 1;
    ELSIF v_type = 'uuid' THEN
        SELECT v::text INTO v_out FROM mentat.datoms_uuid_new
            WHERE store_id = p_store AND e = p_e AND a = p_a AND added
            ORDER BY tx DESC LIMIT 1;
    ELSIF v_type = 'bytes' THEN
        SELECT encode(v, 'hex') INTO v_out FROM mentat.datoms_bytes_new
            WHERE store_id = p_store AND e = p_e AND a = p_a AND added
            ORDER BY tx DESC LIMIT 1;
    ELSE
        -- Type unknown: probe all tables, newest tx wins.
        SELECT value INTO v_out FROM (
            SELECT v AS value, tx FROM mentat.datoms_text_new    WHERE store_id=p_store AND e=p_e AND a=p_a AND added
            UNION ALL SELECT v, tx FROM mentat.datoms_keyword_new WHERE store_id=p_store AND e=p_e AND a=p_a AND added
            UNION ALL SELECT v::text, tx FROM mentat.datoms_long_new    WHERE store_id=p_store AND e=p_e AND a=p_a AND added
            UNION ALL SELECT v::text, tx FROM mentat.datoms_ref_new     WHERE store_id=p_store AND e=p_e AND a=p_a AND added
            UNION ALL SELECT v::text, tx FROM mentat.datoms_double_new  WHERE store_id=p_store AND e=p_e AND a=p_a AND added
            UNION ALL SELECT v::text, tx FROM mentat.datoms_boolean_new WHERE store_id=p_store AND e=p_e AND a=p_a AND added
            UNION ALL SELECT to_char(v,'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'), tx FROM mentat.datoms_instant_new WHERE store_id=p_store AND e=p_e AND a=p_a AND added
            UNION ALL SELECT v::text, tx FROM mentat.datoms_uuid_new    WHERE store_id=p_store AND e=p_e AND a=p_a AND added
            UNION ALL SELECT encode(v,'hex'), tx FROM mentat.datoms_bytes_new WHERE store_id=p_store AND e=p_e AND a=p_a AND added
            ORDER BY tx DESC LIMIT 1
        ) probe;
    END IF;

    RETURN v_out;
END;
$$;

-- Keyword-arg convenience overload: mentat.current(e, ':ns/name').
CREATE OR REPLACE FUNCTION mentat.current(p_e BIGINT, attr_ident TEXT, p_store BIGINT DEFAULT 0)
RETURNS TEXT
LANGUAGE sql STABLE
AS $$
    SELECT mentat.current(p_e, mentat.attr_id(attr_ident), p_store);
$$;

-- ---------------------------------------------------------------------------
-- mentat.attribute_health() -> TABLE
--
-- Per-attribute operational snapshot: live datom count, total (incl.
-- retracted) datom count, and the dead-tuple % of the backing narrow
-- table. Lets operators alert before instant-datom bloat bites (user
-- feedback §4).
--
-- dead_pct is table-level (the narrow tables are shared across
-- attributes of the same type), so it is repeated for every attribute
-- of that type; it is still the right signal for "which table needs a
-- vacuum".
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION mentat.attribute_health()
RETURNS TABLE (
    attr_ident   TEXT,
    value_type   TEXT,
    backing_table TEXT,
    live_datoms  BIGINT,
    dead_pct     NUMERIC
)
LANGUAGE sql STABLE
AS $$
    WITH tbl AS (
        SELECT
            s.ident                                   AS attr_ident,
            s.value_type::text                        AS value_type,
            'mentat.datoms_' || CASE s.value_type::text
                WHEN 'string'  THEN 'text'
                WHEN 'boolean' THEN 'boolean'
                ELSE s.value_type::text
            END || '_new'                             AS backing_table,
            s.entid                                   AS a
        FROM mentat.schema s
    ),
    deadstats AS (
        SELECT
            ('mentat.' || c.relname)                  AS backing_table,
            n_live_tup, n_dead_tup,
            CASE WHEN (n_live_tup + n_dead_tup) > 0
                 THEN round(100.0 * n_dead_tup / (n_live_tup + n_dead_tup), 1)
                 ELSE 0 END                           AS dead_pct
        FROM pg_stat_user_tables c
        WHERE c.schemaname = 'mentat' AND c.relname LIKE 'datoms_%_new'
    )
    SELECT
        t.attr_ident,
        t.value_type,
        t.backing_table,
        (SELECT count(*) FROM mentat.datoms WHERE a = t.a AND added)::BIGINT AS live_datoms,
        COALESCE(d.dead_pct, 0) AS dead_pct
    FROM tbl t
    LEFT JOIN deadstats d ON d.backing_table = t.backing_table
    ORDER BY live_datoms DESC;
$$;
