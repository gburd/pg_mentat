-- pg_mentat <-> pg_tre integration helpers.
--
-- pg_tre is a PostgreSQL 18+ extension that adds a native index access
-- method for approximate-regex matching with edit-distance semantics.
-- pg_mentat treats it as a SOFT dependency: nothing in this file forces
-- pg_tre to be installed. The helpers below check at call time and emit
-- a clear error message pointing at the install instructions if it isn't.
--
-- Reference: https://codeberg.org/gregburd/pg_tre

-- mentat.has_pg_tre(): true if the pg_tre extension is installed in this
-- database AND its tre_pattern type is creatable. Used as the gate by the
-- create_tre_index() helper below; clients can call it directly to decide
-- whether to use (fuzzy-match ...) where-fns in their Datalog queries.
CREATE OR REPLACE FUNCTION mentat.has_pg_tre()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (
        SELECT 1 FROM pg_extension WHERE extname = 'pg_tre'
    );
$$;

-- mentat.create_tre_index(attr_ident text)
--
-- Creates a partial pg_tre index on mentat.datoms_text_new for the given
-- :db.type/string attribute, scoped to the default store (store_id = 0)
-- and live datoms (added = TRUE). Once the index exists, queries of the
-- form
--
--     [(fuzzy-match $ :issue/title "databse" 1) [[?e ?val]]]
--
-- can use it to find values matching the regex with up to k edits per
-- sub-expression in sub-millisecond time.
--
-- Idempotent: re-running with the same attr_ident is a no-op (the
-- underlying CREATE INDEX uses IF NOT EXISTS).
--
-- Errors with a specific :db.error/* code if:
--   - pg_tre is not installed (suggests CREATE EXTENSION)
--   - the attribute is not :db.type/string (TRE only indexes text)
--   - the attribute does not exist
CREATE OR REPLACE FUNCTION mentat.create_tre_index(attr_ident text)
RETURNS text
LANGUAGE plpgsql VOLATILE
AS $$
DECLARE
    attr_entid bigint;
    attr_type  text;
    idx_name   text;
BEGIN
    IF NOT mentat.has_pg_tre() THEN
        RAISE EXCEPTION USING
            ERRCODE = 'feature_not_supported',
            MESSAGE = ':db.error/missing-extension pg_tre is not installed in this database. ' ||
                      'pg_tre is an OPTIONAL approximate-regex index for pg_mentat. To enable: ' ||
                      'install pg_tre (PostgreSQL 18+) per https://codeberg.org/gregburd/pg_tre, ' ||
                      'add ''pg_tre'' to shared_preload_libraries in postgresql.conf, restart, ' ||
                      'then run CREATE EXTENSION pg_tre; before retrying.';
    END IF;

    SELECT s.entid, s.value_type::text
      INTO attr_entid, attr_type
      FROM mentat.schema s
     WHERE s.ident = attr_ident;

    IF attr_entid IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'undefined_object',
            MESSAGE = ':db.error/attribute-not-found Attribute ' || quote_literal(attr_ident) ||
                      ' is not defined in mentat.schema. Define it first via mentat_transact ' ||
                      'with :db/valueType :db.type/string and :db/cardinality :db.cardinality/one.';
    END IF;

    IF attr_type <> 'string' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'invalid_parameter_value',
            MESSAGE = ':db.error/wrong-type-for-tre-index Attribute ' || quote_literal(attr_ident) ||
                      ' has value type ''' || attr_type || ''' but pg_tre indexes only :db.type/string. ' ||
                      'For other types use the existing AVET / AEVT covering indexes ' ||
                      '(automatically maintained on every datoms_<type>_new table).';
    END IF;

    -- One TRE index per attribute, partial on (store_id=0, a=attr_entid, added=TRUE).
    -- Index name is derived from the attribute's entid so it is stable across
    -- ident renames and survives a DROP+CREATE INDEX cycle.
    idx_name := format('idx_datoms_text_new_tre_a%s', attr_entid);

    EXECUTE format(
        'CREATE INDEX IF NOT EXISTS %I ON mentat.datoms_text_new USING tre (v) ' ||
        'WHERE store_id = 0 AND a = %s AND added = TRUE',
        idx_name, attr_entid
    );

    RETURN format(
        'TRE index %s created on mentat.datoms_text_new for attribute %s (entid %s). ' ||
        'Use (fuzzy-match $ %s "pattern" k) to query.',
        idx_name, attr_ident, attr_entid, attr_ident
    );
END;
$$;

-- mentat.drop_tre_index(attr_ident text)
--
-- Drops the TRE index for the given attribute. Idempotent.
CREATE OR REPLACE FUNCTION mentat.drop_tre_index(attr_ident text)
RETURNS text
LANGUAGE plpgsql VOLATILE
AS $$
DECLARE
    attr_entid bigint;
    idx_name   text;
BEGIN
    SELECT entid INTO attr_entid FROM mentat.schema WHERE ident = attr_ident;
    IF attr_entid IS NULL THEN
        RETURN format('Attribute %s not found; nothing to drop.', attr_ident);
    END IF;
    idx_name := format('idx_datoms_text_new_tre_a%s', attr_entid);
    EXECUTE format('DROP INDEX IF EXISTS mentat.%I', idx_name);
    RETURN format('TRE index %s dropped (was for attribute %s).', idx_name, attr_ident);
END;
$$;
