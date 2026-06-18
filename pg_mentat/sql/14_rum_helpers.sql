-- pg_mentat <-> rum integration helpers.
--
-- rum (https://github.com/postgrespro/rum, PostgreSQL license) is a
-- GIN-derived index access method. Unlike GIN, rum stores positional
-- information alongside lexemes, enabling top-K ranked retrieval directly
-- from the index (via the `<=>` distance operator) without a post-fetch
-- sort. It is the closest permissive alternative to BM25 indexing in
-- PostgreSQL — pg_mentat's choice over the AGPL-licensed pg_search.
--
-- pg_mentat treats it as a SOFT dependency. The (rum-fulltext ...)
-- where-fn produces SQL that uses standard `@@` for filtering and
-- `rum_ts_score(...)` for ranking; both work without rum installed
-- (against a sequential scan) but are dramatically faster with rum's
-- index in place.
--
-- Reference: https://github.com/postgrespro/rum

CREATE OR REPLACE FUNCTION mentat.has_rum()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'rum');
$$;

-- Create a partial RUM index on mentat.datoms_text_new(to_tsvector(lang, v))
-- keyed by the entid of the given attribute. The partial-WHERE keeps
-- the index small even in workspaces with many string attributes.
--
-- Returns the index name. Idempotent: re-running with the same attribute
-- and language is a no-op.
CREATE OR REPLACE FUNCTION mentat.create_rum_fulltext_index(
    attr_ident TEXT,
    lang TEXT DEFAULT 'english'
)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_idx_name TEXT;
    v_sql TEXT;
BEGIN
    IF NOT mentat.has_rum() THEN
        RAISE EXCEPTION ':db.error/missing-extension rum is not installed in this database. Build from https://github.com/postgrespro/rum and run CREATE EXTENSION rum;';
    END IF;

    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered in the schema.', attr_ident;
    END IF;

    -- Deterministic name so re-runs are idempotent. Includes language
    -- because the same attribute could plausibly want indexes for
    -- multiple stemming configurations.
    v_idx_name := 'current_text_rum_' || v_entid || '_' || lang;

    -- Built on the current-state projection so (rum-fulltext ...) ranks only
    -- LIVE values; a replaced string is not in current_text. No `added`
    -- column on the projection.
    v_sql := format(
        'CREATE INDEX IF NOT EXISTS %I ON mentat.current_text ' ||
        'USING rum (to_tsvector(%L, v) rum_tsvector_ops) ' ||
        'WHERE a = %s',
        v_idx_name, lang, v_entid
    );
    EXECUTE v_sql;

    RETURN v_idx_name;
END;
$$;

-- Drop the partial RUM index for an attribute + language. Returns true
-- if the index existed and was dropped, false if it was absent.
CREATE OR REPLACE FUNCTION mentat.drop_rum_fulltext_index(
    attr_ident TEXT,
    lang TEXT DEFAULT 'english'
)
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
    v_idx_name := 'current_text_rum_' || v_entid || '_' || lang;
    SELECT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'mentat' AND indexname = v_idx_name
    ) INTO v_existed;
    EXECUTE format('DROP INDEX IF EXISTS mentat.%I', v_idx_name);
    RETURN v_existed;
END;
$$;
