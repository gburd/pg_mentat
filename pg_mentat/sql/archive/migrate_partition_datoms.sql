-- Migration: Convert monolithic datoms table to LIST-partitioned table
--
-- Partitions by value_type_tag (9 type partitions + 1 default).
-- Benefits:
--   - Partition pruning: queries with value_type_tag filter skip irrelevant partitions
--   - Smaller per-partition indexes (each partition indexes only its type's rows)
--   - Independent VACUUM per partition (faster maintenance at scale)
--   - Better data locality (rows of same type are physically co-located)
--
-- IMPORTANT: This migration requires downtime. The datoms table is renamed,
-- data is migrated, and the new partitioned table replaces it atomically.
--
-- Prerequisites:
--   - Run migrate_reduce_indexes.sql first (fewer indexes = faster migration)
--   - Ensure no concurrent transactions are modifying datoms
--   - Back up the database before running this migration
--
-- Estimated time: ~1 minute per 1M rows (depends on disk I/O)

BEGIN;

-- ==========================================================================
-- Phase 1: Rename the old table
-- ==========================================================================
ALTER TABLE mentat.datoms RENAME TO datoms_old;

-- Drop all indexes on the old table (they reference the old table name)
-- This makes the data copy faster and we'll create new ones on the partitions.
DROP INDEX IF EXISTS mentat.idx_datoms_eavt;
DROP INDEX IF EXISTS mentat.idx_datoms_aevt;
DROP INDEX IF EXISTS mentat.idx_datoms_tx;
DROP INDEX IF EXISTS mentat.idx_datoms_vaet;
DROP INDEX IF EXISTS mentat.idx_datoms_avet_ref;
DROP INDEX IF EXISTS mentat.idx_datoms_avet_long;
DROP INDEX IF EXISTS mentat.idx_datoms_avet_text;
DROP INDEX IF EXISTS mentat.idx_datoms_avet_keyword;
DROP INDEX IF EXISTS mentat.idx_datoms_unique_ref;
DROP INDEX IF EXISTS mentat.idx_datoms_unique_long;
DROP INDEX IF EXISTS mentat.idx_datoms_unique_text;
DROP INDEX IF EXISTS mentat.idx_datoms_unique_keyword;

-- Drop triggers on old table (they'll be recreated on the new one)
DROP TRIGGER IF EXISTS validate_datom_value_type_trigger ON mentat.datoms_old;

-- ==========================================================================
-- Phase 2: Create the partitioned table
-- ==========================================================================
CREATE TABLE mentat.datoms (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    value_type_tag SMALLINT NOT NULL,
    v_ref BIGINT,
    v_bool BOOLEAN,
    v_long BIGINT,
    v_double DOUBLE PRECISION,
    v_text TEXT,
    v_keyword TEXT,
    v_instant TIMESTAMPTZ,
    v_uuid UUID,
    v_bytes BYTEA,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,

    CONSTRAINT chk_datom_value CHECK (
        (CASE WHEN v_ref IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_bool IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_long IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_double IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_text IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_keyword IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_instant IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_uuid IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_bytes IS NOT NULL THEN 1 ELSE 0 END) = 1
    )
) PARTITION BY LIST (value_type_tag);

-- Create partitions
CREATE TABLE mentat.datoms_ref PARTITION OF mentat.datoms FOR VALUES IN (0);
CREATE TABLE mentat.datoms_bool PARTITION OF mentat.datoms FOR VALUES IN (1);
CREATE TABLE mentat.datoms_long PARTITION OF mentat.datoms FOR VALUES IN (2);
CREATE TABLE mentat.datoms_double PARTITION OF mentat.datoms FOR VALUES IN (3);
CREATE TABLE mentat.datoms_instant PARTITION OF mentat.datoms FOR VALUES IN (4);
CREATE TABLE mentat.datoms_text PARTITION OF mentat.datoms FOR VALUES IN (7);
CREATE TABLE mentat.datoms_keyword PARTITION OF mentat.datoms FOR VALUES IN (8);
CREATE TABLE mentat.datoms_uuid PARTITION OF mentat.datoms FOR VALUES IN (10);
CREATE TABLE mentat.datoms_bytes PARTITION OF mentat.datoms FOR VALUES IN (11);
CREATE TABLE mentat.datoms_default PARTITION OF mentat.datoms DEFAULT;

-- ==========================================================================
-- Phase 3: Migrate data from old table to partitioned table
-- ==========================================================================
-- PostgreSQL automatically routes each row to the correct partition
-- based on value_type_tag.
INSERT INTO mentat.datoms
    SELECT * FROM mentat.datoms_old;

-- Report migration stats
DO $$
DECLARE
    old_count BIGINT;
    new_count BIGINT;
BEGIN
    SELECT COUNT(*) INTO old_count FROM mentat.datoms_old;
    SELECT COUNT(*) INTO new_count FROM mentat.datoms;

    IF old_count != new_count THEN
        RAISE EXCEPTION 'Data migration mismatch: old=%, new=%', old_count, new_count;
    END IF;

    RAISE NOTICE 'Migrated % datoms to partitioned table', new_count;

    -- Show partition distribution
    FOR old_count IN
        SELECT COUNT(*) FROM mentat.datoms_ref
    LOOP
        RAISE NOTICE '  datoms_ref: % rows', old_count;
    END LOOP;
END $$;

-- ==========================================================================
-- Phase 4: Create indexes on the partitioned table
-- (Indexes are automatically created on each partition)
-- ==========================================================================
CREATE INDEX idx_datoms_eavt ON mentat.datoms
    USING BTREE (e, a, value_type_tag, tx)
    WHERE added = TRUE;

CREATE INDEX idx_datoms_aevt ON mentat.datoms
    USING BTREE (a, e, value_type_tag, tx)
    WHERE added = TRUE;

CREATE INDEX idx_datoms_vaet ON mentat.datoms
    USING BTREE (v_ref, a, e, tx)
    WHERE added = TRUE AND value_type_tag = 0;

CREATE INDEX idx_datoms_tx ON mentat.datoms
    USING BTREE (tx DESC);

CREATE INDEX idx_datoms_avet_ref ON mentat.datoms
    USING BTREE (a, v_ref, e, tx)
    WHERE added = TRUE AND value_type_tag = 0;

CREATE INDEX idx_datoms_avet_long ON mentat.datoms
    USING BTREE (a, v_long, e, tx)
    WHERE added = TRUE AND value_type_tag = 2;

CREATE INDEX idx_datoms_avet_text ON mentat.datoms
    USING BTREE (a, v_text, e, tx)
    WHERE added = TRUE AND value_type_tag = 7;

CREATE INDEX idx_datoms_avet_keyword ON mentat.datoms
    USING BTREE (a, v_keyword, e, tx)
    WHERE added = TRUE AND value_type_tag = 8;

-- Recreate unique constraint indexes on specific partitions
CREATE UNIQUE INDEX idx_datoms_unique_ref ON mentat.datoms_ref (a, v_ref)
    WHERE added = TRUE
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

CREATE UNIQUE INDEX idx_datoms_unique_long ON mentat.datoms_long (a, v_long)
    WHERE added = TRUE
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

CREATE UNIQUE INDEX idx_datoms_unique_text ON mentat.datoms_text (a, v_text)
    WHERE added = TRUE
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

CREATE UNIQUE INDEX idx_datoms_unique_keyword ON mentat.datoms_keyword (a, v_keyword)
    WHERE added = TRUE
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

-- ==========================================================================
-- Phase 5: Recreate trigger on partitioned table
-- ==========================================================================
CREATE TRIGGER validate_datom_value_type_trigger
    BEFORE INSERT OR UPDATE ON mentat.datoms
    FOR EACH ROW
    EXECUTE FUNCTION mentat.validate_datom_value_type();

-- Configure autovacuum per partition for efficient maintenance
ALTER TABLE mentat.datoms_ref SET (autovacuum_vacuum_scale_factor = 0.05);
ALTER TABLE mentat.datoms_long SET (autovacuum_vacuum_scale_factor = 0.05);
ALTER TABLE mentat.datoms_text SET (autovacuum_vacuum_scale_factor = 0.05);
ALTER TABLE mentat.datoms_keyword SET (autovacuum_vacuum_scale_factor = 0.05);
ALTER TABLE mentat.datoms_instant SET (autovacuum_vacuum_scale_factor = 0.05);

-- ==========================================================================
-- Phase 6: Drop old table and analyze
-- ==========================================================================
DROP TABLE mentat.datoms_old;

-- Update planner statistics for all partitions
ANALYZE mentat.datoms;

-- ==========================================================================
-- Verify partition pruning works
-- ==========================================================================
DO $$
DECLARE
    plan_text TEXT;
BEGIN
    EXECUTE 'EXPLAIN (FORMAT TEXT) SELECT * FROM mentat.datoms WHERE value_type_tag = 2 AND a = 1 AND added = true'
    INTO plan_text;

    IF plan_text NOT LIKE '%datoms_long%' THEN
        RAISE WARNING 'Partition pruning may not be working. Check EXPLAIN output.';
    ELSE
        RAISE NOTICE 'Partition pruning verified: queries filter to correct partition';
    END IF;
END $$;

COMMIT;
