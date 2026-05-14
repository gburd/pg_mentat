-- pg_mentat upgrade from 1.2.1 to 1.3.0
--
-- New helper SQL added in 1.3.0:
--   12_fuzzystrmatch_helpers, 13_pg_trgm_helpers, 14_rum_helpers,
--   15_pgvector_helpers, 16_pgque_helpers, 17_pg_infer_helpers,
--   18_postgis_helpers, 19_pg19_graph_helpers, 20_timescaledb_helpers,
--   21_pg_partman_helpers, 22_pg_cron_helpers.
--
-- Plus query.rs additions: 12 new where-fns (levenshtein, soundex,
-- metaphone, daitch-mokotoff, similar-to, rum-fulltext, vector-near,
-- infer-near, infer-similar, infer-implies, infer-walk, infer-describe,
-- infer-predict, geom-near, geom-within, geom-contains, geom-intersects).
-- These live in the .so library; this upgrade only ships the SQL
-- helper functions.

-- ================================================================
-- 12_fuzzystrmatch_helpers.sql
-- ================================================================
-- pg_mentat <-> fuzzystrmatch integration helper.
--
-- fuzzystrmatch is a built-in PostgreSQL contrib extension (PG13+) that
-- provides Levenshtein edit distance, Soundex, Metaphone, and
-- Daitch-Mokotoff phonetic matching as scalar SQL functions. pg_mentat
-- treats it as a SOFT dependency: nothing in pg_mentat requires it, and
-- the Datalog where-fns that compile to its functions only succeed when
-- the extension is installed in the current database.
--
-- Reference: https://www.postgresql.org/docs/current/fuzzystrmatch.html

-- mentat.has_fuzzystrmatch(): true if the contrib extension is installed
-- in this database. Use as a guard in application code or test setup.
CREATE OR REPLACE FUNCTION mentat.has_fuzzystrmatch()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (
        SELECT 1 FROM pg_extension WHERE extname = 'fuzzystrmatch'
    );
$$;

-- ================================================================
-- 13_pg_trgm_helpers.sql
-- ================================================================
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

-- ================================================================
-- 14_rum_helpers.sql
-- ================================================================
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
    v_idx_name := 'datoms_text_rum_' || v_entid || '_' || lang;

    v_sql := format(
        'CREATE INDEX IF NOT EXISTS %I ON mentat.datoms_text_new ' ||
        'USING rum (to_tsvector(%L, v) rum_tsvector_ops) ' ||
        'WHERE a = %s AND added = true',
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
    v_idx_name := 'datoms_text_rum_' || v_entid || '_' || lang;
    SELECT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'mentat' AND indexname = v_idx_name
    ) INTO v_existed;
    EXECUTE format('DROP INDEX IF EXISTS mentat.%I', v_idx_name);
    RETURN v_existed;
END;
$$;

-- ================================================================
-- 15_pgvector_helpers.sql
-- ================================================================
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

-- ================================================================
-- 16_pgque_helpers.sql
-- ================================================================
-- pg_mentat <-> PgQue integration helpers.
--
-- PgQue (https://github.com/NikolayS/PgQue, Apache 2.0) is a pure
-- PL/pgSQL revival of Skype's PgQ queue: snapshot-based batching,
-- TRUNCATE-based event-table rotation, zero-bloat under sustained
-- load, no C extension required, no external daemon (optional
-- pg_cron / pg_timetable for ticking). Works on any PG14+.
--
-- This integration is OPTIONAL. The helpers here detect PgQue at
-- runtime via mentat.has_pgque() and refuse to install triggers if
-- it isn't present. The wire shape is a JSON event per pg_mentat
-- transaction, emitted from a deferred constraint trigger that fires
-- at COMMIT time so the datoms for the tx are fully visible when
-- the payload is assembled.
--
-- Reference: https://github.com/NikolayS/PgQue

-- Detect whether PgQue is installed (it's a schema, not a PG
-- extension, so pg_extension lookup doesn't apply).
CREATE OR REPLACE FUNCTION mentat.has_pgque()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (
        SELECT 1 FROM pg_namespace WHERE nspname = 'pgque'
    ) AND EXISTS (
        SELECT 1 FROM pg_proc p
        JOIN pg_namespace n ON p.pronamespace = n.oid
        WHERE n.nspname = 'pgque' AND p.proname = 'send'
    );
$$;

-- Internal: build the per-tx JSON payload by aggregating datoms
-- across the 9 narrow typed tables. Excluded values for binary
-- (`v_bytes`) which are hex-encoded for transport safety.
CREATE OR REPLACE FUNCTION mentat._pgque_build_tx_payload(tx_id BIGINT)
RETURNS jsonb
LANGUAGE sql STABLE
AS $$
    WITH all_datoms AS (
        SELECT e, a, v::text AS v, 'string' AS vt, tx, added FROM mentat.datoms_text_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v, 'keyword', tx, added FROM mentat.datoms_keyword_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'long', tx, added FROM mentat.datoms_long_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'ref', tx, added FROM mentat.datoms_ref_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'double', tx, added FROM mentat.datoms_double_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'boolean', tx, added FROM mentat.datoms_boolean_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'instant', tx, added FROM mentat.datoms_instant_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'uuid', tx, added FROM mentat.datoms_uuid_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, encode(v, 'hex'), 'bytes', tx, added FROM mentat.datoms_bytes_new WHERE tx = tx_id
    )
    SELECT jsonb_build_object(
        'tx', tx_id,
        'tx_instant', (SELECT tx_instant FROM mentat.transactions WHERE tx = tx_id),
        'store_id', current_setting('mentat.current_store_id', true),
        'datom_count', (SELECT count(*) FROM all_datoms),
        'datoms', COALESCE(
            (SELECT jsonb_agg(
                jsonb_build_object(
                    'e', e, 'a', a, 'v', v, 'vt', vt, 'tx', tx, 'added', added
                ) ORDER BY e, a
            ) FROM all_datoms),
            '[]'::jsonb
        )
    );
$$;

-- Internal: deferred constraint trigger function. Fires at COMMIT
-- time per AFTER INSERT on mentat.transactions so the datom rows
-- for the tx are fully visible. The queue name is passed via TG_ARGV.
--
-- If PgQue isn't present (e.g. user dropped the schema after
-- installing the trigger), the trigger emits a NOTICE and returns
-- gracefully rather than failing the user's transaction.
CREATE OR REPLACE FUNCTION mentat._pgque_emit_tx_trigger()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    v_queue_name TEXT := TG_ARGV[0];
    v_payload jsonb;
BEGIN
    IF NOT mentat.has_pgque() THEN
        RAISE NOTICE 'mentat: PgQue is not installed; skipping emit for tx %', NEW.tx;
        RETURN NULL;
    END IF;

    v_payload := mentat._pgque_build_tx_payload(NEW.tx);

    -- pgque.insert_event takes ev_type + ev_data; keep ev_type = 'mentat.tx'
    -- so consumers can subscribe to a stable event type. The full payload
    -- goes in ev_data as the JSON text representation.
    PERFORM pgque.insert_event(v_queue_name, 'mentat.tx', v_payload::text);
    RETURN NULL;
EXCEPTION WHEN OTHERS THEN
    -- We deliberately swallow exceptions in the deferred trigger to avoid
    -- rolling back user data because of a queue-side problem. The error
    -- is surfaced as a NOTICE so it's still visible in logs.
    RAISE NOTICE 'mentat: pgque emit for queue % tx % failed: %',
        v_queue_name, NEW.tx, SQLERRM;
    RETURN NULL;
END;
$$;

-- Public: enable per-transaction emit to PgQue. Creates a queue if
-- it doesn't already exist, then attaches a deferred constraint
-- trigger to mentat.transactions that calls insert_event at commit
-- time.
--
-- Idempotent: re-running with the same queue name is a no-op.
-- Returns the queue name for chaining.
CREATE OR REPLACE FUNCTION mentat.pgque_emit_tx(queue_name TEXT)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_trig_name TEXT;
BEGIN
    IF NOT mentat.has_pgque() THEN
        RAISE EXCEPTION ':db.error/missing-extension PgQue is not installed in this database. Run \i sql/pgque.sql from the PgQue source tree first.';
    END IF;

    -- Trigger names must be valid identifiers; sanitize the queue name.
    v_trig_name := 'mentat_pgque_emit_' ||
        regexp_replace(queue_name, '[^a-zA-Z0-9_]', '_', 'g');

    -- Create the queue if missing (idempotent).
    PERFORM pgque.create_queue(queue_name);

    -- Attach the deferred constraint trigger if not already present.
    -- DEFERRABLE INITIALLY DEFERRED means it fires once per inserted
    -- row at COMMIT time, by which point the tx's datoms are visible.
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = v_trig_name
          AND tgrelid = 'mentat.transactions'::regclass
    ) THEN
        EXECUTE format(
            'CREATE CONSTRAINT TRIGGER %I AFTER INSERT ON mentat.transactions ' ||
            'DEFERRABLE INITIALLY DEFERRED FOR EACH ROW ' ||
            'EXECUTE FUNCTION mentat._pgque_emit_tx_trigger(%L)',
            v_trig_name, queue_name
        );
    END IF;

    RETURN queue_name;
END;
$$;

-- Public: disable per-transaction emit. Drops the trigger; the
-- queue itself is left intact (consumers may still want to drain it).
-- Returns true if a trigger existed and was dropped.
CREATE OR REPLACE FUNCTION mentat.pgque_disable_tx(queue_name TEXT)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_trig_name TEXT;
    v_existed boolean;
BEGIN
    v_trig_name := 'mentat_pgque_emit_' ||
        regexp_replace(queue_name, '[^a-zA-Z0-9_]', '_', 'g');

    SELECT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = v_trig_name
          AND tgrelid = 'mentat.transactions'::regclass
    ) INTO v_existed;

    IF v_existed THEN
        EXECUTE format(
            'DROP TRIGGER IF EXISTS %I ON mentat.transactions',
            v_trig_name
        );
    END IF;
    RETURN v_existed;
END;
$$;

-- Convenience wrapper: register a consumer on the queue. Pure
-- forwarding to pgque.register_consumer so users don't have to
-- juggle two namespaces in application code that already imports
-- mentat.*.
CREATE OR REPLACE FUNCTION mentat.pgque_register_consumer(
    queue_name TEXT,
    consumer_name TEXT
)
RETURNS integer
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT mentat.has_pgque() THEN
        RAISE EXCEPTION ':db.error/missing-extension PgQue is not installed.';
    END IF;
    RETURN pgque.register_consumer(queue_name, consumer_name);
END;
$$;

-- ================================================================
-- 17_pg_infer_helpers.sql
-- ================================================================
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
    v_idx_name := 'datoms_text_infer_' || v_entid || '_' ||
        regexp_replace(model_name, '[^a-zA-Z0-9_]', '_', 'g');

    v_sql := format(
        'CREATE INDEX IF NOT EXISTS %I ON mentat.datoms_text_new ' ||
        'USING infer (v) WITH (model = %L) ' ||
        'WHERE a = %s AND added = true',
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
    v_idx_name := 'datoms_text_infer_' || v_entid || '_' ||
        regexp_replace(model_name, '[^a-zA-Z0-9_]', '_', 'g');
    SELECT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE schemaname = 'mentat' AND indexname = v_idx_name
    ) INTO v_existed;
    EXECUTE format('DROP INDEX IF EXISTS mentat.%I', v_idx_name);
    RETURN v_existed;
END;
$$;

-- ================================================================
-- 18_postgis_helpers.sql
-- ================================================================
-- pg_mentat <-> PostGIS integration helpers.
--
-- PostGIS (https://postgis.net, GPL-2.0+) is the industry-standard
-- geospatial extension for PostgreSQL: WKT/WKB geometry types,
-- GEOS-backed spatial predicates, GiST indexing, and SRID-aware
-- coordinate transforms. pg_mentat treats it as a SOFT dependency
-- via a side-table aux pattern (mirroring pgvector): pg_mentat does
-- not yet add :db.type/geometry to the schema; geometry data lives
-- in per-attribute aux tables keyed by entid.
--
-- The (geom-near ...), (geom-within ...), (geom-contains ...),
-- (geom-intersects ...) Datalog where-fns produce SQL that calls
-- PostGIS's ST_* operators directly. Without PostGIS installed
-- queries compile cleanly and fail at execution with the standard
-- PG "type/operator does not exist" error.
--
-- Reference: https://postgis.net/docs/

CREATE OR REPLACE FUNCTION mentat.has_postgis()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'postgis');
$$;

-- Attach a per-attribute geometry aux table.
--
-- `srid` is the EPSG code (4326 for WGS84 lat/long, 3857 for
-- Web Mercator, 0 for unknown). `geom_type` is the PostGIS subtype:
-- 'POINT', 'POLYGON', 'LINESTRING', 'MULTIPOLYGON', 'GEOMETRY' (the
-- generic untyped variant), etc.
--
-- Idempotent. Returns the qualified table name.
CREATE OR REPLACE FUNCTION mentat.attach_geometry_attribute(
    attr_ident TEXT,
    srid INTEGER DEFAULT 4326,
    geom_type TEXT DEFAULT 'GEOMETRY'
)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
BEGIN
    IF NOT mentat.has_postgis() THEN
        RAISE EXCEPTION ':db.error/missing-extension PostGIS is not installed in this database. CREATE EXTENSION postgis;';
    END IF;

    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;

    -- PostGIS rejects unknown geom_type; whitelist common values for
    -- a clearer error than "geometry type X is unknown".
    IF upper(geom_type) NOT IN (
        'GEOMETRY', 'POINT', 'LINESTRING', 'POLYGON',
        'MULTIPOINT', 'MULTILINESTRING', 'MULTIPOLYGON',
        'GEOMETRYCOLLECTION', 'CIRCULARSTRING', 'COMPOUNDCURVE',
        'CURVEPOLYGON', 'MULTICURVE', 'MULTISURFACE'
    ) THEN
        RAISE EXCEPTION ':db.error/fn-arg geom_type % is not a recognized PostGIS geometry subtype.', geom_type;
    END IF;

    v_table := format('attr_%s_geom', v_entid);
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS mentat.%I (' ||
        '    e BIGINT PRIMARY KEY,' ||
        '    geom geometry(%s, %s) NOT NULL' ||
        ')',
        v_table, upper(geom_type), srid
    );
    RETURN 'mentat.' || v_table;
END;
$$;

-- Set or replace the geometry for an entity.
CREATE OR REPLACE FUNCTION mentat.set_geometry(
    e BIGINT,
    attr_ident TEXT,
    wkt_text TEXT,
    srid INTEGER DEFAULT 4326
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
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    v_table := format('attr_%s_geom', v_entid);
    EXECUTE format(
        'INSERT INTO mentat.%I (e, geom) VALUES ($1, ST_GeomFromText($2, $3)) ' ||
        'ON CONFLICT (e) DO UPDATE SET geom = EXCLUDED.geom',
        v_table
    ) USING e, wkt_text, srid;
END;
$$;

-- Remove the geometry for an entity. Returns true if a row was deleted.
CREATE OR REPLACE FUNCTION mentat.del_geometry(
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
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    v_table := format('attr_%s_geom', v_entid);
    EXECUTE format('DELETE FROM mentat.%I WHERE e = $1 RETURNING true', v_table)
        INTO v_existed USING e;
    RETURN COALESCE(v_existed, false);
END;
$$;

-- Create a GiST spatial index on the aux table. PostGIS's GIST index
-- on geometry columns is what backs every ST_DWithin / ST_Intersects /
-- ST_Contains / ST_Distance KNN query plan. Idempotent.
CREATE OR REPLACE FUNCTION mentat.create_gist_geometry_index(attr_ident TEXT)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
    v_idx TEXT;
BEGIN
    IF NOT mentat.has_postgis() THEN
        RAISE EXCEPTION ':db.error/missing-extension PostGIS is not installed.';
    END IF;
    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    v_table := format('attr_%s_geom', v_entid);
    v_idx := format('attr_%s_geom_gist', v_entid);
    EXECUTE format(
        'CREATE INDEX IF NOT EXISTS %I ON mentat.%I USING GIST (geom)',
        v_idx, v_table
    );
    RETURN v_idx;
END;
$$;

-- Detach (drop) a geometry attribute's aux table and any indexes on
-- it. Returns true if the table existed.
CREATE OR REPLACE FUNCTION mentat.detach_geometry_attribute(attr_ident TEXT)
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
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    v_table := format('attr_%s_geom', v_entid);
    SELECT EXISTS (
        SELECT 1 FROM pg_class c
        JOIN pg_namespace n ON c.relnamespace = n.oid
        WHERE n.nspname = 'mentat' AND c.relname = v_table
    ) INTO v_existed;
    EXECUTE format('DROP TABLE IF EXISTS mentat.%I', v_table);
    RETURN v_existed;
END;
$$;

-- ================================================================
-- 19_pg19_graph_helpers.sql
-- ================================================================
-- pg_mentat <-> PostgreSQL 19 SQL/PGQ (Property Graph Queries) helpers.
--
-- PG19 introduces ISO SQL/PGQ: the GRAPH_TABLE clause and CREATE
-- PROPERTY GRAPH DDL for graph pattern matching over relational
-- data. pg_mentat treats it as a SOFT dependency (PG19+ only).
--
-- The data model fit is partial: pg_mentat's narrow datom tables are
-- EAV-shaped, while SQL/PGQ expects entity-typed vertex tables and
-- typed edge tables. The helper here lets users declare which
-- attributes are vertex labels and which are edge labels; pg_mentat
-- generates a CREATE PROPERTY GRAPH that maps those onto views over
-- the narrow datom storage.
--
-- This is alpha-level: the API and generated DDL may change as PG19
-- ships and as we learn what graph queries users actually run.
--
-- Reference:
--   https://www.postgresql.org/docs/devel/sql-createpropertygraph.html
--   https://www.postgresql.org/docs/devel/queries-graph.html

CREATE OR REPLACE FUNCTION mentat.has_pg19_graph()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT current_setting('server_version_num')::int >= 190000
       AND EXISTS (SELECT 1 FROM pg_proc WHERE proname = 'pg_get_propertygraphdef');
$$;

-- Build the per-attribute vertex view name. Deterministic so users
-- can refer to it from raw SQL queries that mix Datalog + GRAPH_TABLE.
CREATE OR REPLACE FUNCTION mentat._pg19_vertex_view_name(attr_ident TEXT)
RETURNS TEXT
LANGUAGE sql STABLE
AS $$
    SELECT 'v_' || regexp_replace(attr_ident, '[^a-zA-Z0-9_]', '_', 'g');
$$;

-- Build the per-attribute edge view name.
CREATE OR REPLACE FUNCTION mentat._pg19_edge_view_name(attr_ident TEXT)
RETURNS TEXT
LANGUAGE sql STABLE
AS $$
    SELECT 'e_' || regexp_replace(attr_ident, '[^a-zA-Z0-9_]', '_', 'g');
$$;

-- Materialize a vertex view for a string/long/ref attribute.
-- Vertices = entities that have a value for this attribute. The
-- view exposes (e BIGINT, label TEXT) so SQL/PGQ can use the entid
-- as the vertex key and the value as a vertex property.
--
-- Example: mentat.create_vertex_view(':person/name')
-- creates view mentat.v_person_name AS
--   SELECT e, v AS label FROM mentat.datoms_text_new
--   WHERE a = <entid for :person/name> AND added.
CREATE OR REPLACE FUNCTION mentat.create_vertex_view(attr_ident TEXT)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_view TEXT;
    v_type TEXT;
    v_table TEXT;
BEGIN
    SELECT entid, value_type::text INTO v_entid, v_type
        FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    v_view := mentat._pg19_vertex_view_name(attr_ident);

    -- Map value_type to the narrow storage table.
    v_table := CASE v_type
        WHEN 'string'  THEN 'datoms_text_new'
        WHEN 'keyword' THEN 'datoms_keyword_new'
        WHEN 'long'    THEN 'datoms_long_new'
        WHEN 'ref'     THEN 'datoms_ref_new'
        WHEN 'double'  THEN 'datoms_double_new'
        WHEN 'boolean' THEN 'datoms_boolean_new'
        WHEN 'instant' THEN 'datoms_instant_new'
        WHEN 'uuid'    THEN 'datoms_uuid_new'
        WHEN 'bytes'   THEN 'datoms_bytes_new'
        ELSE NULL
    END;
    IF v_table IS NULL THEN
        RAISE EXCEPTION ':db.error/fn-arg attribute % has unsupported value_type %.',
            attr_ident, v_type;
    END IF;

    EXECUTE format(
        'CREATE OR REPLACE VIEW mentat.%I AS ' ||
        'SELECT e, v::TEXT AS label FROM mentat.%I WHERE a = %s AND added',
        v_view, v_table, v_entid
    );
    RETURN 'mentat.' || v_view;
END;
$$;

-- Materialize an edge view for a ref-type attribute. Edges connect
-- entity (e) to entity (v_ref) via the named attribute. The view
-- exposes (id BIGSERIAL, src BIGINT, dst BIGINT, label TEXT) so
-- SQL/PGQ can use it as an edge table.
CREATE OR REPLACE FUNCTION mentat.create_edge_view(attr_ident TEXT)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_view TEXT;
    v_type TEXT;
BEGIN
    SELECT entid, value_type::text INTO v_entid, v_type
        FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    IF v_type <> 'ref' THEN
        RAISE EXCEPTION ':db.error/fn-arg attribute % must be :db.type/ref to be used as an edge (got type %).',
            attr_ident, v_type;
    END IF;
    v_view := mentat._pg19_edge_view_name(attr_ident);

    EXECUTE format(
        'CREATE OR REPLACE VIEW mentat.%I AS ' ||
        'SELECT (e * 100000 + v) AS id, e AS src, v AS dst, %L AS label ' ||
        'FROM mentat.datoms_ref_new WHERE a = %s AND added',
        v_view, attr_ident, v_entid
    );
    RETURN 'mentat.' || v_view;
END;
$$;

-- Drop the vertex/edge view for an attribute. Returns true if dropped.
CREATE OR REPLACE FUNCTION mentat.drop_vertex_view(attr_ident TEXT)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_view TEXT;
    v_existed boolean;
BEGIN
    v_view := mentat._pg19_vertex_view_name(attr_ident);
    SELECT EXISTS (SELECT 1 FROM pg_views WHERE schemaname='mentat' AND viewname=v_view)
        INTO v_existed;
    EXECUTE format('DROP VIEW IF EXISTS mentat.%I', v_view);
    RETURN v_existed;
END;
$$;

CREATE OR REPLACE FUNCTION mentat.drop_edge_view(attr_ident TEXT)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_view TEXT;
    v_existed boolean;
BEGIN
    v_view := mentat._pg19_edge_view_name(attr_ident);
    SELECT EXISTS (SELECT 1 FROM pg_views WHERE schemaname='mentat' AND viewname=v_view)
        INTO v_existed;
    EXECUTE format('DROP VIEW IF EXISTS mentat.%I', v_view);
    RETURN v_existed;
END;
$$;

-- Generate (but do NOT execute) the CREATE PROPERTY GRAPH DDL for a
-- given graph name, vertex-attribute list, and edge-attribute list.
-- Returns the DDL as text so users can review and execute it
-- themselves. The vertex and edge views must already exist (call
-- create_vertex_view / create_edge_view first).
--
-- Example:
--   SELECT mentat.create_property_graph_ddl(
--       'social',
--       ARRAY[':person/name', ':company/name'],
--       ARRAY[':person/employer', ':person/friend']
--   );
CREATE OR REPLACE FUNCTION mentat.create_property_graph_ddl(
    graph_name TEXT,
    vertex_attrs TEXT[],
    edge_attrs TEXT[]
)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    -- Per-attribute clauses
    v_attr TEXT;
    v_vertex_clauses TEXT[] := ARRAY[]::TEXT[];
    v_edge_clauses TEXT[] := ARRAY[]::TEXT[];
    v_src_attr TEXT;
    v_dst_attr TEXT;
BEGIN
    -- DDL generation does not require SQL/PGQ to be available; the
    -- caller decides whether to EXECUTE the returned text on a
    -- PG19+ cluster. The detection helper is exposed separately as
    -- mentat.has_pg19_graph().

    FOREACH v_attr IN ARRAY vertex_attrs LOOP
        v_vertex_clauses := array_append(
            v_vertex_clauses,
            format('mentat.%I LABEL %I', mentat._pg19_vertex_view_name(v_attr),
                   regexp_replace(v_attr, '^:', ''))
        );
    END LOOP;

    FOREACH v_attr IN ARRAY edge_attrs LOOP
        -- For edges we use the first vertex_attr as src/dst label by
        -- default; users can override by hand-editing the DDL.
        v_src_attr := vertex_attrs[1];
        v_dst_attr := vertex_attrs[1];
        v_edge_clauses := array_append(
            v_edge_clauses,
            format('mentat.%I SOURCE mentat.%I DESTINATION mentat.%I LABEL %I',
                mentat._pg19_edge_view_name(v_attr),
                mentat._pg19_vertex_view_name(v_src_attr),
                mentat._pg19_vertex_view_name(v_dst_attr),
                regexp_replace(v_attr, '^:', ''))
        );
    END LOOP;

    RETURN format(
        E'CREATE PROPERTY GRAPH %I\n    VERTEX TABLES (\n        %s\n    )\n    EDGE TABLES (\n        %s\n    );',
        graph_name,
        array_to_string(v_vertex_clauses, E',\n        '),
        array_to_string(v_edge_clauses, E',\n        ')
    );
END;
$$;

-- ================================================================
-- 20_timescaledb_helpers.sql
-- ================================================================
-- pg_mentat <-> TimescaleDB integration helpers.
--
-- TimescaleDB (https://github.com/timescale/timescaledb, Apache 2.0
-- for the OSS edition) is a time-series extension that converts
-- regular tables into "hypertables" — automatically partitioned by
-- a time column. pg_mentat's transaction log
-- (mentat.transactions) and instant-typed datoms
-- (mentat.datoms_instant_new) are natural fits.
--
-- Treat as a SOFT dependency.
--
-- Reference: https://docs.timescale.com/

CREATE OR REPLACE FUNCTION mentat.has_timescaledb()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'timescaledb');
$$;

-- Convert mentat.transactions into a hypertable partitioned by
-- tx_instant. The chunk_time_interval defaults to 1 month — adjust
-- via the argument for high-volume stores. Idempotent: if the
-- table is already a hypertable, this is a no-op.
--
-- Returns the hypertable id from create_hypertable's output.
CREATE OR REPLACE FUNCTION mentat.timescale_attach_transactions(
    chunk_time_interval INTERVAL DEFAULT INTERVAL '1 month'
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
DECLARE
    v_hyper_id bigint;
    v_already boolean;
BEGIN
    IF NOT mentat.has_timescaledb() THEN
        RAISE EXCEPTION ':db.error/missing-extension TimescaleDB is not installed in this database. CREATE EXTENSION timescaledb;';
    END IF;

    -- Skip if already a hypertable.
    SELECT EXISTS (
        SELECT 1 FROM timescaledb_information.hypertables
        WHERE hypertable_schema = 'mentat' AND hypertable_name = 'transactions'
    ) INTO v_already;
    IF v_already THEN
        RAISE NOTICE 'mentat.transactions is already a hypertable; skipping.';
        SELECT id INTO v_hyper_id
            FROM _timescaledb_catalog.hypertable
            WHERE schema_name = 'mentat' AND table_name = 'transactions';
        RETURN v_hyper_id;
    END IF;

    -- create_hypertable returns a SETOF (hypertable_id, schema_name,
    -- table_name, created); call as `(create_hypertable(...)).hypertable_id`.
    EXECUTE format(
        'SELECT (create_hypertable(%L, by_range(%L, %L::INTERVAL), if_not_exists => true)).hypertable_id',
        'mentat.transactions', 'tx_instant', chunk_time_interval
    ) INTO v_hyper_id;

    RETURN v_hyper_id;
END;
$$;

-- Convert mentat.datoms_instant_new into a hypertable partitioned by
-- the v (instant) column. Useful when datom-stream queries are
-- time-window-heavy. Optional; most workloads do fine with the
-- transactions hypertable alone.
CREATE OR REPLACE FUNCTION mentat.timescale_attach_instant_datoms(
    chunk_time_interval INTERVAL DEFAULT INTERVAL '1 month'
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
DECLARE
    v_hyper_id bigint;
    v_already boolean;
BEGIN
    IF NOT mentat.has_timescaledb() THEN
        RAISE EXCEPTION ':db.error/missing-extension TimescaleDB is not installed.';
    END IF;
    SELECT EXISTS (
        SELECT 1 FROM timescaledb_information.hypertables
        WHERE hypertable_schema = 'mentat' AND hypertable_name = 'datoms_instant_new'
    ) INTO v_already;
    IF v_already THEN
        SELECT id INTO v_hyper_id
            FROM _timescaledb_catalog.hypertable
            WHERE schema_name = 'mentat' AND table_name = 'datoms_instant_new';
        RETURN v_hyper_id;
    END IF;
    EXECUTE format(
        'SELECT (create_hypertable(%L, by_range(%L, %L::INTERVAL), if_not_exists => true)).hypertable_id',
        'mentat.datoms_instant_new', 'v', chunk_time_interval
    ) INTO v_hyper_id;
    RETURN v_hyper_id;
END;
$$;

-- Add a retention policy: drop transaction history older than the
-- given interval. WARNING: this drops underlying datoms transactionally.
-- Datalog queries with `:as-of` will fail for txs older than this.
CREATE OR REPLACE FUNCTION mentat.timescale_set_transaction_retention(
    keep_for INTERVAL
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
DECLARE
    v_job_id bigint;
BEGIN
    IF NOT mentat.has_timescaledb() THEN
        RAISE EXCEPTION ':db.error/missing-extension TimescaleDB is not installed.';
    END IF;
    EXECUTE format(
        'SELECT add_retention_policy(%L, INTERVAL %L)',
        'mentat.transactions', keep_for
    ) INTO v_job_id;
    RETURN v_job_id;
END;
$$;

-- ================================================================
-- 21_pg_partman_helpers.sql
-- ================================================================
-- pg_mentat <-> pg_partman integration helpers.
--
-- pg_partman (https://github.com/pgpartman/pg_partman, PostgreSQL
-- license) is the canonical declarative partition-management
-- extension. It builds on PostgreSQL's native partitioning to
-- automate creation, retention, and rotation of time-/range-based
-- partitions.
--
-- pg_mentat treats it as a SOFT dependency.
--
-- The natural fit is partitioning mentat.transactions on tx_instant
-- — pg_partman creates and rotates monthly/weekly partitions
-- automatically, so old tx history can be dropped or archived
-- without manual maintenance.
--
-- Reference: https://github.com/pgpartman/pg_partman/blob/master/doc/pg_partman.md

CREATE OR REPLACE FUNCTION mentat.has_pg_partman()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_partman');
$$;

-- Bootstrap declarative partitioning on mentat.transactions.
--
-- This is destructive: mentat.transactions is renamed to
-- mentat.transactions_template, and a new partitioned mentat.transactions
-- is created with the same shape, partitioned by tx_instant on the
-- given interval. Existing rows are migrated by pg_partman during
-- create_parent.
--
-- Returns the partman config row's parent_table.
--
-- WARNING: this rewrites the transactions table layout. Run during
-- a maintenance window. The Datalog query path is unaffected
-- because mentat.transactions is queried by tx (the PK), not
-- scanned in bulk.
CREATE OR REPLACE FUNCTION mentat.partman_attach_transactions(
    interval_str TEXT DEFAULT '1 month',
    premake INT DEFAULT 4
)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_count BIGINT;
    v_existing TEXT;
BEGIN
    IF NOT mentat.has_pg_partman() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_partman is not installed in this database. CREATE EXTENSION pg_partman SCHEMA partman;';
    END IF;

    -- Detect whether already partman-managed.
    SELECT parent_table INTO v_existing FROM partman.part_config
        WHERE parent_table = 'mentat.transactions';
    IF v_existing IS NOT NULL THEN
        RAISE NOTICE 'mentat.transactions is already managed by pg_partman; skipping.';
        RETURN v_existing;
    END IF;

    -- Detect whether mentat.transactions is already a partitioned
    -- table (i.e. someone ran this before but registration was
    -- removed). If not, this transformation must convert plain
    -- table to partitioned root.
    -- pg_partman 5.x can manage existing native PARTITION BY
    -- tables; if mentat.transactions is plain, we need a manual
    -- conversion which is more invasive than this helper provides.
    -- Refuse with a clear message in that case.
    IF NOT EXISTS (
        SELECT 1 FROM pg_partitioned_table pt
        JOIN pg_class c ON pt.partrelid = c.oid
        JOIN pg_namespace n ON c.relnamespace = n.oid
        WHERE n.nspname = 'mentat' AND c.relname = 'transactions'
    ) THEN
        RAISE EXCEPTION ':db.error/manual-step mentat.transactions is not a partitioned table. Convert it manually first by recreating it as PARTITION BY RANGE (tx_instant) and migrating rows; then re-run partman_attach_transactions. See docs/src/pg_partman.md.';
    END IF;

    -- Register with pg_partman as a native-partitioned parent.
    PERFORM partman.create_parent(
        p_parent_table := 'mentat.transactions',
        p_control      := 'tx_instant',
        p_type         := 'native',
        p_interval     := interval_str,
        p_premake      := premake
    );
    RETURN 'mentat.transactions';
END;
$$;

-- Set retention on the partman-managed transactions parent.
-- `keep_for` is a string like '90 days' or '6 months'.
CREATE OR REPLACE FUNCTION mentat.partman_set_transaction_retention(keep_for TEXT)
RETURNS void
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT mentat.has_pg_partman() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_partman is not installed.';
    END IF;
    UPDATE partman.part_config
       SET retention = keep_for, retention_keep_table = false
     WHERE parent_table = 'mentat.transactions';
    IF NOT FOUND THEN
        RAISE EXCEPTION ':db.error/missing-config mentat.transactions is not registered with pg_partman. Run mentat.partman_attach_transactions first.';
    END IF;
END;
$$;

-- Run partman maintenance on mentat-managed parents. Typically
-- scheduled via pg_cron; can also be invoked manually.
CREATE OR REPLACE FUNCTION mentat.partman_run_maintenance()
RETURNS void
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT mentat.has_pg_partman() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_partman is not installed.';
    END IF;
    -- Limit to mentat-managed parents.
    PERFORM partman.run_maintenance(
        p_parent_table := pc.parent_table,
        p_analyze      := true
    )
    FROM partman.part_config pc
    WHERE pc.parent_table LIKE 'mentat.%';
END;
$$;

-- ================================================================
-- 22_pg_cron_helpers.sql
-- ================================================================
-- pg_mentat <-> pg_cron integration helpers.
--
-- pg_cron (https://github.com/citusdata/pg_cron, PostgreSQL license)
-- is the standard scheduler extension: cron-style schedules stored
-- in cron.job, executed by a background worker. Treat as a SOFT
-- dependency.
--
-- pg_cron requires shared_preload_libraries = 'pg_cron' (or
-- 'pg_cron,...') and a cluster restart, plus
-- cron.database_name = '<your-db>' to point the scheduler at the
-- right database. Without those, CREATE EXTENSION pg_cron fails;
-- the helpers here detect via has_pg_cron() and refuse to install
-- jobs if pg_cron is missing.
--
-- Common pg_mentat use cases:
--   * Periodic mentat.partman_run_maintenance() — partition
--     creation/retention.
--   * Periodic CLUSTER / VACUUM ANALYZE on the narrow datom tables.
--   * Periodic refresh of materialized views (FDW caches, vertex
--     views, etc).
--
-- Reference: https://github.com/citusdata/pg_cron

CREATE OR REPLACE FUNCTION mentat.has_pg_cron()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_cron');
$$;

-- Schedule a one-line SQL command on a cron schedule. Returns the
-- pg_cron job id. The job_name is required (pg_cron uses it for
-- updates / deletes).
--
-- Example:
--   SELECT mentat.cron_schedule(
--       'mentat-partman-maint',
--       '0 3 * * *',  -- every day at 03:00 UTC
--       'SELECT mentat.partman_run_maintenance();'
--   );
CREATE OR REPLACE FUNCTION mentat.cron_schedule(
    job_name TEXT,
    schedule TEXT,
    command TEXT
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
DECLARE
    v_id bigint;
BEGIN
    IF NOT mentat.has_pg_cron() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_cron is not installed (or not in shared_preload_libraries). See docs/src/pg_cron.md.';
    END IF;
    -- cron.schedule(name, schedule, command) returns the job id.
    EXECUTE format('SELECT cron.schedule(%L, %L, %L)', job_name, schedule, command)
        INTO v_id;
    RETURN v_id;
END;
$$;

-- Cancel a scheduled job by name.
CREATE OR REPLACE FUNCTION mentat.cron_unschedule(job_name TEXT)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_existed boolean;
BEGIN
    IF NOT mentat.has_pg_cron() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_cron is not installed.';
    END IF;
    EXECUTE format('SELECT cron.unschedule(%L)', job_name) INTO v_existed;
    RETURN v_existed;
END;
$$;

-- Convenience: schedule a daily 03:00 UTC partman maintenance job.
-- Idempotent: removes any existing job with the same name first.
CREATE OR REPLACE FUNCTION mentat.cron_schedule_partman_maintenance(
    schedule TEXT DEFAULT '0 3 * * *'
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT mentat.has_pg_cron() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_cron is not installed.';
    END IF;
    -- Best-effort unschedule; silently ignore if not present.
    BEGIN
        PERFORM mentat.cron_unschedule('mentat-partman-maintenance');
    EXCEPTION WHEN OTHERS THEN NULL;
    END;
    RETURN mentat.cron_schedule(
        'mentat-partman-maintenance',
        schedule,
        'SELECT mentat.partman_run_maintenance();'
    );
END;
$$;

-- Convenience: schedule periodic VACUUM ANALYZE on the narrow
-- datom tables. Useful in append-mostly workloads where autovacuum
-- doesn't keep up with planner statistics.
CREATE OR REPLACE FUNCTION mentat.cron_schedule_vacuum_datoms(
    schedule TEXT DEFAULT '0 4 * * *'
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT mentat.has_pg_cron() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_cron is not installed.';
    END IF;
    BEGIN
        PERFORM mentat.cron_unschedule('mentat-vacuum-datoms');
    EXCEPTION WHEN OTHERS THEN NULL;
    END;
    RETURN mentat.cron_schedule(
        'mentat-vacuum-datoms',
        schedule,
        $vac$
        VACUUM (ANALYZE) mentat.datoms_text_new;
        VACUUM (ANALYZE) mentat.datoms_keyword_new;
        VACUUM (ANALYZE) mentat.datoms_long_new;
        VACUUM (ANALYZE) mentat.datoms_ref_new;
        VACUUM (ANALYZE) mentat.datoms_double_new;
        VACUUM (ANALYZE) mentat.datoms_boolean_new;
        VACUUM (ANALYZE) mentat.datoms_instant_new;
        VACUUM (ANALYZE) mentat.datoms_uuid_new;
        VACUUM (ANALYZE) mentat.datoms_bytes_new;
        $vac$
    );
END;
$$;

