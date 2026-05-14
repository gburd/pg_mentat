-- pg_mentat <-> pgvector integration helpers.
--
-- pgvector (https://github.com/pgvector/pgvector, PostgreSQL license) is
-- the standard PostgreSQL vector-similarity extension. This integration
-- is a SOFT dependency.
--
-- Design: pg_mentat does NOT add :db.type/vector to the schema (that's a
-- bigger transact/storage change tracked in docs/INTEGRATIONS.md and
-- docs/src/pgvector.md). Instead, vectors live in per-attribute auxiliary
-- tables keyed by entid:
--
--   mentat.attr_<entid>_vector(e BIGINT PRIMARY KEY, v vector(N))
--
-- Created by mentat.attach_vector_attribute('<:attr/ident>', dim).
-- Populated and queried with the helper functions defined below.
-- KNN queries use the (vector-near $ :attr "[...]" k) Datalog where-fn,
-- which JOINs the rest of the where-clause graph back through the
-- entid column.
--
-- Reference: https://github.com/pgvector/pgvector

CREATE OR REPLACE FUNCTION mentat.has_pgvector()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'vector');
$$;

-- Create the per-attribute aux table for storing vectors. `dim` must be
-- the fixed dimensionality of the embedding (e.g. 384, 768, 1536). The
-- function does NOT add :db.type/vector to the schema; the attribute
-- must already be registered as :db.type/string (the textual
-- representation of the vector) or `:db.type/long` (a placeholder
-- entity-link). The aux table is keyed by entid only.
--
-- Idempotent. Returns the table name.
CREATE OR REPLACE FUNCTION mentat.attach_vector_attribute(
    attr_ident TEXT,
    dim INT
)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
BEGIN
    IF NOT mentat.has_pgvector() THEN
        RAISE EXCEPTION ':db.error/missing-extension pgvector is not installed in this database. Build pgvector and run CREATE EXTENSION vector;';
    END IF;

    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered in the schema.', attr_ident;
    END IF;

    IF dim <= 0 OR dim > 16000 THEN
        RAISE EXCEPTION ':db.error/fn-arg vector dimensionality must be in (0, 16000], got %.', dim;
    END IF;

    v_table := format('attr_%s_vector', v_entid);
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS mentat.%I (' ||
        '    e BIGINT PRIMARY KEY,' ||
        '    v vector(%s) NOT NULL' ||
        ')',
        v_table, dim
    );
    RETURN 'mentat.' || v_table;
END;
$$;

-- Set or replace the vector for an entity on a vector-attached attribute.
-- ON CONFLICT updates the existing row.
CREATE OR REPLACE FUNCTION mentat.set_vector(
    e BIGINT,
    attr_ident TEXT,
    v_text TEXT
)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
BEGIN
    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered in the schema.', attr_ident;
    END IF;
    v_table := format('attr_%s_vector', v_entid);
    -- The aux table must already exist; users must call attach first.
    EXECUTE format(
        'INSERT INTO mentat.%I (e, v) VALUES ($1, $2::vector) ' ||
        'ON CONFLICT (e) DO UPDATE SET v = EXCLUDED.v',
        v_table
    ) USING e, v_text;
END;
$$;

-- Remove the vector for one entity on a vector-attached attribute.
-- Returns true if a row was deleted, false otherwise.
CREATE OR REPLACE FUNCTION mentat.del_vector(
    e BIGINT,
    attr_ident TEXT
)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
    v_existed boolean;
BEGIN
    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered in the schema.', attr_ident;
    END IF;
    v_table := format('attr_%s_vector', v_entid);
    EXECUTE format(
        'DELETE FROM mentat.%I WHERE e = $1 RETURNING true',
        v_table
    ) INTO v_existed USING e;
    RETURN COALESCE(v_existed, false);
END;
$$;

-- Create an HNSW index on a vector-attached attribute. `dist_op` must be
-- one of 'cosine', 'l2', 'inner' (the integration's keyword form maps
-- to these strings). Idempotent.
CREATE OR REPLACE FUNCTION mentat.create_hnsw_vector_index(
    attr_ident TEXT,
    dist_op TEXT DEFAULT 'cosine'
)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
    v_idx TEXT;
    v_opclass TEXT;
BEGIN
    IF NOT mentat.has_pgvector() THEN
        RAISE EXCEPTION ':db.error/missing-extension pgvector is not installed.';
    END IF;
    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;

    v_opclass := CASE dist_op
        WHEN 'cosine' THEN 'vector_cosine_ops'
        WHEN 'l2'     THEN 'vector_l2_ops'
        WHEN 'inner'  THEN 'vector_ip_ops'
        ELSE NULL
    END;
    IF v_opclass IS NULL THEN
        RAISE EXCEPTION ':db.error/fn-arg dist_op must be one of cosine, l2, inner; got %.', dist_op;
    END IF;

    v_table := format('attr_%s_vector', v_entid);
    v_idx := format('attr_%s_vector_hnsw_%s', v_entid, dist_op);
    EXECUTE format(
        'CREATE INDEX IF NOT EXISTS %I ON mentat.%I USING hnsw (v %s)',
        v_idx, v_table, v_opclass
    );
    RETURN v_idx;
END;
$$;
