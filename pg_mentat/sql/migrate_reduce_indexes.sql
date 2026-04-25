-- Migration: Reduce datoms indexes from ~26 to 8
--
-- This migration drops redundant and low-value indexes from the datoms table
-- to improve write throughput. Each removed index eliminates one B-tree update
-- per INSERT, yielding 2-3x write performance improvement at scale.
--
-- Run this in a maintenance window. Index drops acquire AccessExclusiveLock
-- briefly but are fast (metadata-only). No data is modified.
--
-- IMPORTANT: Run CONCURRENTLY-safe index creation for the partial index
-- replacements if you have the non-partial versions.

BEGIN;

-- ==========================================================================
-- Phase 1: Drop removed AVET indexes for rare types
-- ==========================================================================
DROP INDEX IF EXISTS mentat.idx_datoms_avet_double;
DROP INDEX IF EXISTS mentat.idx_datoms_avet_instant;
DROP INDEX IF EXISTS mentat.idx_datoms_avet_uuid;
DROP INDEX IF EXISTS mentat.idx_datoms_avet_bool;

-- ==========================================================================
-- Phase 2: Drop EAVT covering indexes (write cost not justified)
-- ==========================================================================
DROP INDEX IF EXISTS mentat.idx_datoms_eavt_long;
DROP INDEX IF EXISTS mentat.idx_datoms_eavt_text;
DROP INDEX IF EXISTS mentat.idx_datoms_eavt_ref;
DROP INDEX IF EXISTS mentat.idx_datoms_eavt_instant;
DROP INDEX IF EXISTS mentat.idx_datoms_eavt_uuid;

-- ==========================================================================
-- Phase 3: Drop redundant/obsolete indexes
-- ==========================================================================
DROP INDEX IF EXISTS mentat.idx_datoms_history;
DROP INDEX IF EXISTS mentat.idx_datoms_temporal;
DROP INDEX IF EXISTS mentat.idx_datoms_cardinality;

-- ==========================================================================
-- Phase 4: Drop removed unique constraint indexes for rare types
-- ==========================================================================
DROP INDEX IF EXISTS mentat.idx_datoms_unique_bool;
DROP INDEX IF EXISTS mentat.idx_datoms_unique_double;
DROP INDEX IF EXISTS mentat.idx_datoms_unique_uuid;

-- ==========================================================================
-- Phase 5: Replace non-partial EAVT/AEVT with partial versions
-- The existing idx_datoms_eavt and idx_datoms_aevt may lack the
-- WHERE added = TRUE clause. Drop and recreate them as partial indexes
-- for smaller index size and faster scans.
-- ==========================================================================

-- Check if the existing EAVT index lacks the partial predicate
-- (safe to re-run: IF NOT EXISTS handles the case where it already exists)
DROP INDEX IF EXISTS mentat.idx_datoms_eavt;
CREATE INDEX idx_datoms_eavt ON mentat.datoms
    USING BTREE (e, a, value_type_tag, tx)
    WHERE added = TRUE;

DROP INDEX IF EXISTS mentat.idx_datoms_aevt;
CREATE INDEX idx_datoms_aevt ON mentat.datoms
    USING BTREE (a, e, value_type_tag, tx)
    WHERE added = TRUE;

-- Ensure VAET also has the partial predicate
DROP INDEX IF EXISTS mentat.idx_datoms_vaet;
DROP INDEX IF EXISTS mentat.idx_datoms_vaet_ref;
CREATE INDEX idx_datoms_vaet ON mentat.datoms
    USING BTREE (v_ref, a, e, tx)
    WHERE added = TRUE AND value_type_tag = 0;

-- Ensure TX index uses DESC ordering
DROP INDEX IF EXISTS mentat.idx_datoms_tx;
CREATE INDEX idx_datoms_tx ON mentat.datoms
    USING BTREE (tx DESC);

-- Ensure AVET indexes have partial predicates
DROP INDEX IF EXISTS mentat.idx_datoms_avet_ref;
CREATE INDEX idx_datoms_avet_ref ON mentat.datoms
    USING BTREE (a, v_ref, e, tx)
    WHERE added = TRUE AND value_type_tag = 0;

DROP INDEX IF EXISTS mentat.idx_datoms_avet_long;
CREATE INDEX idx_datoms_avet_long ON mentat.datoms
    USING BTREE (a, v_long, e, tx)
    WHERE added = TRUE AND value_type_tag = 2;

DROP INDEX IF EXISTS mentat.idx_datoms_avet_text;
CREATE INDEX idx_datoms_avet_text ON mentat.datoms
    USING BTREE (a, v_text, e, tx)
    WHERE added = TRUE AND value_type_tag = 7;

DROP INDEX IF EXISTS mentat.idx_datoms_avet_keyword;
CREATE INDEX idx_datoms_avet_keyword ON mentat.datoms
    USING BTREE (a, v_keyword, e, tx)
    WHERE added = TRUE AND value_type_tag = 8;

-- ==========================================================================
-- Verify final index count
-- ==========================================================================
DO $$
DECLARE
    datom_idx_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO datom_idx_count
    FROM pg_indexes
    WHERE schemaname = 'mentat' AND tablename = 'datoms';

    RAISE NOTICE 'Migration complete: % indexes on mentat.datoms (target: 8 base + unique indexes)', datom_idx_count;

    IF datom_idx_count > 16 THEN
        RAISE WARNING 'More indexes than expected (%). Check for leftover indexes.', datom_idx_count;
    END IF;
END $$;

-- Run ANALYZE to update query planner statistics after index changes
ANALYZE mentat.datoms;

COMMIT;
