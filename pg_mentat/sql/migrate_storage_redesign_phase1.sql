-- Storage Redesign Phase 1: Type-Specific Tables with store_id
--
-- This migration creates new optimized tables alongside the existing datoms table
-- to eliminate the wide-row anti-pattern and add multi-store support via store_id.
--
-- Benefits:
-- - No NULL overhead (80 bytes saved per row)
-- - Smaller, more efficient indexes
-- - Better TOAST management
-- - HOT updates possible
-- - Better compression
-- - Partition pruning by store_id

-- First, add store_id to stores metadata table
ALTER TABLE mentat.stores ADD COLUMN IF NOT EXISTS store_id SERIAL;

-- Create unique constraint on store_id
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'stores_store_id_key'
    ) THEN
        ALTER TABLE mentat.stores ADD CONSTRAINT stores_store_id_key UNIQUE (store_id);
    END IF;
END $$;

-- Ensure default store has store_id = 0
UPDATE mentat.stores SET store_id = 0 WHERE store_name = 'default';

-- --------------------------------------------------------------------------
-- New type-specific tables (no wide row, no NULLs)
-- --------------------------------------------------------------------------

-- Reference values (entity references)
CREATE TABLE IF NOT EXISTS mentat.datoms_ref_new (
    store_id INT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BIGINT NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

COMMENT ON TABLE mentat.datoms_ref_new IS
  'Optimized storage for reference values - no NULL overhead, single value column';

-- Long/Integer values
CREATE TABLE IF NOT EXISTS mentat.datoms_long_new (
    store_id INT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BIGINT NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

COMMENT ON TABLE mentat.datoms_long_new IS
  'Optimized storage for long/integer values';

-- Text/String values
CREATE TABLE IF NOT EXISTS mentat.datoms_text_new (
    store_id INT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v TEXT NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (
    fillfactor = 85,           -- More room for TOAST pointers
    toast_tuple_target = 8192  -- Larger TOAST threshold
);

COMMENT ON TABLE mentat.datoms_text_new IS
  'Optimized storage for text values with enhanced TOAST settings';

-- Double/Float values
CREATE TABLE IF NOT EXISTS mentat.datoms_double_new (
    store_id INT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v DOUBLE PRECISION NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

COMMENT ON TABLE mentat.datoms_double_new IS
  'Optimized storage for double/float values';

-- Instant/Timestamp values
CREATE TABLE IF NOT EXISTS mentat.datoms_instant_new (
    store_id INT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v TIMESTAMPTZ NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

COMMENT ON TABLE mentat.datoms_instant_new IS
  'Optimized storage for timestamp values';

-- Keyword values
CREATE TABLE IF NOT EXISTS mentat.datoms_keyword_new (
    store_id INT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v TEXT NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

COMMENT ON TABLE mentat.datoms_keyword_new IS
  'Optimized storage for keyword values';

-- UUID values
CREATE TABLE IF NOT EXISTS mentat.datoms_uuid_new (
    store_id INT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v UUID NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

COMMENT ON TABLE mentat.datoms_uuid_new IS
  'Optimized storage for UUID values';

-- Bytes/Blob values
CREATE TABLE IF NOT EXISTS mentat.datoms_bytes_new (
    store_id INT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BYTEA NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (
    fillfactor = 85,
    toast_tuple_target = 8192
);

COMMENT ON TABLE mentat.datoms_bytes_new IS
  'Optimized storage for binary/blob values with enhanced TOAST settings';

-- Boolean values
CREATE TABLE IF NOT EXISTS mentat.datoms_boolean_new (
    store_id INT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BOOLEAN NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (store_id, e, a, tx)
) WITH (fillfactor = 90);

COMMENT ON TABLE mentat.datoms_boolean_new IS
  'Optimized storage for boolean values';

-- --------------------------------------------------------------------------
-- Indexes for new tables
-- --------------------------------------------------------------------------

-- Reference table indexes (all 4 access patterns needed for ref traversal)
CREATE INDEX IF NOT EXISTS idx_datoms_ref_new_eavt
    ON mentat.datoms_ref_new (store_id, e, a, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_ref_new_aevt
    ON mentat.datoms_ref_new (store_id, a, e, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_ref_new_vaet
    ON mentat.datoms_ref_new (store_id, v, a, e, tx)
    WHERE added;

CREATE INDEX IF NOT EXISTS idx_datoms_ref_new_tx
    ON mentat.datoms_ref_new (store_id, tx DESC)
    WHERE added INCLUDE (e, a, v);

-- Long table indexes (skip VAET - numeric values rarely queried by value)
CREATE INDEX IF NOT EXISTS idx_datoms_long_new_eavt
    ON mentat.datoms_long_new (store_id, e, a, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_long_new_aevt
    ON mentat.datoms_long_new (store_id, a, e, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_long_new_tx
    ON mentat.datoms_long_new (store_id, tx DESC)
    WHERE added INCLUDE (e, a, v);

-- Text table indexes (use GIN for text search instead of VAET)
CREATE INDEX IF NOT EXISTS idx_datoms_text_new_eavt
    ON mentat.datoms_text_new (store_id, e, a, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_text_new_aevt
    ON mentat.datoms_text_new (store_id, a, e, tx)
    WHERE added;

CREATE INDEX IF NOT EXISTS idx_datoms_text_new_tx
    ON mentat.datoms_text_new (store_id, tx DESC)
    WHERE added INCLUDE (e, a, v);

-- Full-text search index for text values
CREATE INDEX IF NOT EXISTS idx_datoms_text_new_fts
    ON mentat.datoms_text_new
    USING gin(to_tsvector('english', v))
    WHERE added;

-- Trigram index if pg_trgm is available
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_trgm') THEN
        EXECUTE 'CREATE INDEX IF NOT EXISTS idx_datoms_text_new_trgm
                 ON mentat.datoms_text_new
                 USING gin(v gin_trgm_ops)
                 WHERE added';
    END IF;
END $$;

-- Double table indexes
CREATE INDEX IF NOT EXISTS idx_datoms_double_new_eavt
    ON mentat.datoms_double_new (store_id, e, a, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_double_new_aevt
    ON mentat.datoms_double_new (store_id, a, e, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_double_new_tx
    ON mentat.datoms_double_new (store_id, tx DESC)
    WHERE added INCLUDE (e, a, v);

-- Instant table indexes
CREATE INDEX IF NOT EXISTS idx_datoms_instant_new_eavt
    ON mentat.datoms_instant_new (store_id, e, a, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_instant_new_aevt
    ON mentat.datoms_instant_new (store_id, a, e, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_instant_new_tx
    ON mentat.datoms_instant_new (store_id, tx DESC)
    WHERE added INCLUDE (e, a, v);

-- Keyword table indexes (include VAET - keywords often used as lookup keys)
CREATE INDEX IF NOT EXISTS idx_datoms_keyword_new_eavt
    ON mentat.datoms_keyword_new (store_id, e, a, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_keyword_new_aevt
    ON mentat.datoms_keyword_new (store_id, a, e, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_keyword_new_vaet
    ON mentat.datoms_keyword_new (store_id, v, a, e, tx)
    WHERE added;

CREATE INDEX IF NOT EXISTS idx_datoms_keyword_new_tx
    ON mentat.datoms_keyword_new (store_id, tx DESC)
    WHERE added INCLUDE (e, a, v);

-- UUID table indexes
CREATE INDEX IF NOT EXISTS idx_datoms_uuid_new_eavt
    ON mentat.datoms_uuid_new (store_id, e, a, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_uuid_new_aevt
    ON mentat.datoms_uuid_new (store_id, a, e, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_uuid_new_tx
    ON mentat.datoms_uuid_new (store_id, tx DESC)
    WHERE added INCLUDE (e, a, v);

-- Bytes table indexes
CREATE INDEX IF NOT EXISTS idx_datoms_bytes_new_eavt
    ON mentat.datoms_bytes_new (store_id, e, a, tx)
    WHERE added;

CREATE INDEX IF NOT EXISTS idx_datoms_bytes_new_aevt
    ON mentat.datoms_bytes_new (store_id, a, e, tx)
    WHERE added;

CREATE INDEX IF NOT EXISTS idx_datoms_bytes_new_tx
    ON mentat.datoms_bytes_new (store_id, tx DESC)
    WHERE added INCLUDE (e, a);

-- Boolean table indexes
CREATE INDEX IF NOT EXISTS idx_datoms_boolean_new_eavt
    ON mentat.datoms_boolean_new (store_id, e, a, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_boolean_new_aevt
    ON mentat.datoms_boolean_new (store_id, a, e, tx)
    WHERE added INCLUDE (v);

CREATE INDEX IF NOT EXISTS idx_datoms_boolean_new_tx
    ON mentat.datoms_boolean_new (store_id, tx DESC)
    WHERE added INCLUDE (e, a, v);

-- --------------------------------------------------------------------------
-- Autovacuum tuning for new tables
-- --------------------------------------------------------------------------

-- Tables with frequent updates: aggressive vacuum
ALTER TABLE mentat.datoms_ref_new SET (
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_analyze_scale_factor = 0.02
);

ALTER TABLE mentat.datoms_long_new SET (
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_analyze_scale_factor = 0.02
);

ALTER TABLE mentat.datoms_text_new SET (
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_analyze_scale_factor = 0.02
);

-- Transactions table: mostly append-only
ALTER TABLE mentat.transactions SET (
    fillfactor = 100,
    autovacuum_vacuum_scale_factor = 0.1
);

-- --------------------------------------------------------------------------
-- Dual-write trigger function
-- --------------------------------------------------------------------------

-- Function to synchronize writes to both old and new tables
CREATE OR REPLACE FUNCTION mentat.dual_write_datoms()
RETURNS TRIGGER AS $$
BEGIN
    -- Insert into new type-specific tables based on value_type_tag
    -- Default store_id = 0 for backwards compatibility

    IF TG_OP = 'INSERT' THEN
        CASE NEW.value_type_tag
            WHEN 0 THEN  -- Ref
                INSERT INTO mentat.datoms_ref_new (store_id, e, a, v, tx, added)
                VALUES (0, NEW.e, NEW.a, NEW.v_ref, NEW.tx, NEW.added);
            WHEN 1 THEN  -- Boolean
                INSERT INTO mentat.datoms_boolean_new (store_id, e, a, v, tx, added)
                VALUES (0, NEW.e, NEW.a, NEW.v_bool, NEW.tx, NEW.added);
            WHEN 2 THEN  -- Long
                INSERT INTO mentat.datoms_long_new (store_id, e, a, v, tx, added)
                VALUES (0, NEW.e, NEW.a, NEW.v_long, NEW.tx, NEW.added);
            WHEN 3 THEN  -- Double
                INSERT INTO mentat.datoms_double_new (store_id, e, a, v, tx, added)
                VALUES (0, NEW.e, NEW.a, NEW.v_double, NEW.tx, NEW.added);
            WHEN 4 THEN  -- Instant
                INSERT INTO mentat.datoms_instant_new (store_id, e, a, v, tx, added)
                VALUES (0, NEW.e, NEW.a, NEW.v_instant, NEW.tx, NEW.added);
            WHEN 7 THEN  -- Text
                INSERT INTO mentat.datoms_text_new (store_id, e, a, v, tx, added)
                VALUES (0, NEW.e, NEW.a, NEW.v_text, NEW.tx, NEW.added);
            WHEN 8 THEN  -- Keyword
                INSERT INTO mentat.datoms_keyword_new (store_id, e, a, v, tx, added)
                VALUES (0, NEW.e, NEW.a, NEW.v_keyword, NEW.tx, NEW.added);
            WHEN 10 THEN  -- UUID
                INSERT INTO mentat.datoms_uuid_new (store_id, e, a, v, tx, added)
                VALUES (0, NEW.e, NEW.a, NEW.v_uuid, NEW.tx, NEW.added);
            WHEN 11 THEN  -- Bytes
                INSERT INTO mentat.datoms_bytes_new (store_id, e, a, v, tx, added)
                VALUES (0, NEW.e, NEW.a, NEW.v_bytes, NEW.tx, NEW.added);
        END CASE;
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

COMMENT ON FUNCTION mentat.dual_write_datoms() IS
  'Trigger function to maintain dual-write compatibility during storage migration';

-- Create trigger (disabled by default - enable manually when ready to test)
-- To enable: ALTER TABLE mentat.datoms ENABLE TRIGGER dual_write_datoms_trigger;
CREATE TRIGGER dual_write_datoms_trigger
    AFTER INSERT ON mentat.datoms
    FOR EACH ROW
    EXECUTE FUNCTION mentat.dual_write_datoms();

-- Start with trigger disabled
ALTER TABLE mentat.datoms DISABLE TRIGGER dual_write_datoms_trigger;

-- --------------------------------------------------------------------------
-- Migration tracking
-- --------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS mentat.storage_migration_status (
    phase INT PRIMARY KEY,
    description TEXT NOT NULL,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    notes TEXT
);

INSERT INTO mentat.storage_migration_status (phase, description, notes)
VALUES
    (1, 'Create new type-specific tables', 'Phase 1: New schema created with dual-write capability'),
    (2, 'Backfill existing data', 'Phase 2: Migrate data from old to new tables'),
    (3, 'Update application code', 'Phase 3: Modify Rust code to use new tables'),
    (4, 'Cutover and cleanup', 'Phase 4: Drop old tables after verification')
ON CONFLICT (phase) DO NOTHING;

-- Mark Phase 1 as started
UPDATE mentat.storage_migration_status
SET started_at = NOW()
WHERE phase = 1 AND started_at IS NULL;

COMMENT ON TABLE mentat.storage_migration_status IS
  'Tracks progress of storage layer redesign migration';
