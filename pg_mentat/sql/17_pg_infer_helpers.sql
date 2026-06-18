-- pg_mentat <-> pg_infer integration helpers.
--
-- pg_infer (https://codeberg.org/gregburd/pg_infer, Apache 2.0) is an
-- experimental PostgreSQL extension that exposes transformer model
-- knowledge as SQL relations. It provides:
--
--   * The <~> distance operator (text <~> text -> float8) backed by
--     a custom index AM ('infer'), enabling index-driven top-K ranked
--     retrieval ordered by model-knowledge similarity.
--   * The <~ similarity operator (text <~ text -> float8).
--   * The @> implication operator (text @> text -> bool).
--   * Scalar functions: infer_distance, infer_similarity, implies,
--     similar_to, walk, describe, infer.
--   * Index access method: USING infer (column) WITH (model = 'name').
--
-- pg_mentat treats it as a SOFT dependency. The (infer-near ...),
-- (infer-similar ...), and (infer-implies ...) Datalog where-fns
-- compile to SQL that calls pg_infer's operators and functions
-- directly; without pg_infer installed, queries fail at execution
-- with the standard PG "function/operator does not exist" error.
--
-- pg_infer requires PostgreSQL 18+ and a registered model
-- (`SELECT infer_create_model('name', '/path/to/model.vindex');`)
-- before any query function will return useful results.
--
-- Reference: https://codeberg.org/gregburd/pg_infer

CREATE OR REPLACE FUNCTION mentat.has_pg_infer()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_infer');
$$;

-- Create a partial pg_infer index on mentat.datoms_text_new(v) keyed
-- by the entid of the given attribute. Once present, `(infer-near ...)`
-- queries against this attribute become index-driven top-K rather
-- than sequential scans.
--
-- The index uses the default `infer_text_ops` opclass and is named
-- deterministically so re-runs are idempotent.
--
-- `model_name` is the registered pg_infer model (created via
-- `infer_create_model('name', '/path/to/model.vindex')` first).
CREATE OR REPLACE FUNCTION mentat.create_infer_index(
    attr_ident TEXT,
    model_name TEXT
)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_idx_name TEXT;
    v_sql TEXT;
BEGIN
    IF NOT mentat.has_pg_infer() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_infer is not installed in this database. Build pg_infer (PG18+) and run CREATE EXTENSION pg_infer;';
    END IF;

    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered in the schema.', attr_ident;
    END IF;

    -- Sanitize model_name for inclusion in the index identifier.
    v_idx_name := 'current_text_infer_' || v_entid || '_' ||
        regexp_replace(model_name, '[^a-zA-Z0-9_]', '_', 'g');

    -- Built on the current-state projection so (infer-near ...) ranks only
    -- LIVE values. The projection has no `added` column.
    v_sql := format(
        'CREATE INDEX IF NOT EXISTS %I ON mentat.current_text ' ||
        'USING infer (v) WITH (model = %L) ' ||
        'WHERE a = %s',
        v_idx_name, model_name, v_entid
    );
    EXECUTE v_sql;
    RETURN v_idx_name;
END;
$$;

-- Drop the partial pg_infer index for an attribute + model. Returns
-- true if the index existed and was dropped, false otherwise.
CREATE OR REPLACE FUNCTION mentat.drop_infer_index(
    attr_ident TEXT,
    model_name TEXT
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
    v_idx_name := 'current_text_infer_' || v_entid || '_' ||
        regexp_replace(model_name, '[^a-zA-Z0-9_]', '_', 'g');
    SELECT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'mentat' AND indexname = v_idx_name
    ) INTO v_existed;
    EXECUTE format('DROP INDEX IF EXISTS mentat.%I', v_idx_name);
    RETURN v_existed;
END;
$$;
