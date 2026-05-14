-- pg_mentat <-> pg_trgm (trigram) integration helpers.
--
-- pg_trgm is a built-in PostgreSQL contrib extension (PG13+). It provides
-- the `similarity(text, text)` function (returning real in [0.0, 1.0]),
-- the `%` similar-above-threshold operator, and the `gin_trgm_ops` /
-- `gist_trgm_ops` index access methods.
--
-- pg_mentat treats it as a SOFT dependency: nothing in pg_mentat requires
-- it. The (similar-to ...) Datalog where-fn produces SQL that calls
-- pg_trgm's similarity(), so queries succeed only when the extension is
-- installed.
--
-- Reference: https://www.postgresql.org/docs/current/pgtrgm.html

CREATE OR REPLACE FUNCTION mentat.has_pg_trgm()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (
        SELECT 1 FROM pg_extension WHERE extname = 'pg_trgm'
    );
$$;

-- Create a partial GIN trigram index on mentat.datoms_text_new(v) keyed by
-- the entid of the given attribute keyword. The partial-WHERE reduces the
-- index to only the rows for one attribute, so a workspace with thousands
-- of string attributes does not pay the cost of trigram-indexing every one.
--
-- After creating the index, queries of the form
--   (similar-to $ :attr "needle" threshold)
-- can use the GIN index for the `v % needle` filter (with a recheck on
-- similarity() >= threshold). The compiled SQL pg_mentat emits uses
-- similarity() >= threshold rather than %; the planner uses the index for
-- the equivalent `%` filter when statistics show a selective enough match.
--
-- Idempotent: re-running with the same attribute is a no-op.
CREATE OR REPLACE FUNCTION mentat.create_trgm_index(attr_ident TEXT)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_idx_name TEXT;
    v_sql TEXT;
BEGIN
    IF NOT mentat.has_pg_trgm() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_trgm is not installed in this database. Run CREATE EXTENSION pg_trgm;';
    END IF;

    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered in the schema.', attr_ident;
    END IF;

    -- Index name is deterministic so re-runs are idempotent.
    v_idx_name := 'datoms_text_trgm_' || v_entid;

    v_sql := format(
        'CREATE INDEX IF NOT EXISTS %I ON mentat.datoms_text_new ' ||
        'USING GIN (v gin_trgm_ops) WHERE a = %s AND added = true',
        v_idx_name, v_entid
    );
    EXECUTE v_sql;

    RETURN v_idx_name;
END;
$$;

-- Drop the partial trigram index for an attribute. Returns true if the
-- index existed and was dropped, false if it was absent.
CREATE OR REPLACE FUNCTION mentat.drop_trgm_index(attr_ident TEXT)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_idx_name TEXT;
    v_existed boolean;
BEGIN
    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered in the schema.', attr_ident;
    END IF;
    v_idx_name := 'datoms_text_trgm_' || v_entid;

    SELECT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'mentat' AND indexname = v_idx_name
    ) INTO v_existed;

    EXECUTE format('DROP INDEX IF EXISTS mentat.%I', v_idx_name);
    RETURN v_existed;
END;
$$;
