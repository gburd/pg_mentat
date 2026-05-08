-- Narrow per-type storage tables.
--
-- The production storage model is one table per value type, each with a
-- single non-NULL value column. This is what the query engine in
-- functions/query.rs, functions/transact.rs, and parts of functions/pull.rs
-- actually reads and writes. The wide-row `mentat.datoms` table (defined in
-- the inline bootstrap in lib.rs) is still populated by bootstrap.rs,
-- storage.rs, and helpers.rs; the `dual_write_datoms` trigger mirrors each
-- insert into the correct narrow table so both paths see the same data.
--
-- The long-term plan (see docs/ROADMAP.md, "Storage unification") is to move
-- all legacy write sites onto the narrow tables directly and drop the
-- wide-row table. Until then, the trigger is the load-bearing piece that
-- makes CREATE EXTENSION produce a consistent, immediately-usable database.

-- ---------------------------------------------------------------------------
-- Nine narrow per-type tables
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS mentat.datoms_ref_new (
    store_id INT     NOT NULL DEFAULT 0,
    e        BIGINT  NOT NULL,
    a        BIGINT  NOT NULL,
    v        BIGINT  NOT NULL,
    tx       BIGINT  NOT NULL,
    added    BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_long_new (
    store_id INT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v BIGINT NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_text_new (
    store_id INT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v TEXT NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 85, toast_tuple_target = 8160);

CREATE TABLE IF NOT EXISTS mentat.datoms_double_new (
    store_id INT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v DOUBLE PRECISION NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_instant_new (
    store_id INT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v TIMESTAMPTZ NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_keyword_new (
    store_id INT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v TEXT NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_uuid_new (
    store_id INT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v UUID NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS mentat.datoms_bytes_new (
    store_id INT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v BYTEA NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 85, toast_tuple_target = 8160);

CREATE TABLE IF NOT EXISTS mentat.datoms_boolean_new (
    store_id INT NOT NULL DEFAULT 0,
    e  BIGINT NOT NULL, a BIGINT NOT NULL, v BOOLEAN NOT NULL,
    tx BIGINT NOT NULL, added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
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

-- keyword: VAET matters (idents resolve keywords \u2194 entity-ids)
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
-- Dual-write bridge.
--
-- Legacy writers (bootstrap.rs, storage.rs, pull.rs test helpers, etc.)
-- still INSERT into the wide-row mentat.datoms table. This trigger copies
-- each insert into the matching narrow table so queries see it.
--
-- When the remaining legacy writers are ported to write the narrow tables
-- directly (tracked in docs/ROADMAP.md), this trigger and the wide-row
-- mentat.datoms table can be dropped.
-- ---------------------------------------------------------------------------

CREATE OR REPLACE FUNCTION mentat.dual_write_datoms()
RETURNS TRIGGER AS $$
BEGIN
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
    END CASE;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS dual_write_datoms_trigger ON mentat.datoms;
CREATE TRIGGER dual_write_datoms_trigger
    AFTER INSERT ON mentat.datoms
    FOR EACH ROW EXECUTE FUNCTION mentat.dual_write_datoms();
