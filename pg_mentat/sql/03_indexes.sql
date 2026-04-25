-- Index definitions for pg_mentat
-- Implements EAVT, AEVT, AVET, VAET index pattern from Datomic/Mentat
--
-- DESIGN: Minimal index set for maximum write throughput.
-- Previous versions had 26+ indexes on the datoms table, causing each INSERT
-- to update all indexes. This reduced set keeps only the indexes proven
-- necessary by the actual query patterns in the codebase.
--
-- Values are stored in type-specific columns (v_ref, v_long, v_text, etc.)
-- so AVET indexes are type-specific partial indexes for correct range queries.

-- ==========================================================================
-- Core Datom Indexes (6 base indexes)
-- ==========================================================================

-- EAVT: Primary index for entity-centric lookups
-- "What are all the facts about entity E?"
-- Used by: pull.rs, entity.rs, transact.rs (retract, CAS), helpers.rs
-- Partial index on added=TRUE covers the dominant query pattern.
CREATE INDEX idx_datoms_eavt ON mentat.datoms
    USING BTREE (e, a, value_type_tag, tx)
    WHERE added = TRUE;

-- AEVT: Index for attribute-centric scans
-- "What are all the facts with attribute A?"
-- Used by: query.rs (attribute pattern scans), helpers.rs
CREATE INDEX idx_datoms_aevt ON mentat.datoms
    USING BTREE (a, e, value_type_tag, tx)
    WHERE added = TRUE;

-- VAET: Reverse reference index for ref types only
-- "What entities reference entity E via attribute A?"
-- Used by: pull.rs (reverse ref lookups, _<attr> patterns)
-- Partial index: only ref types (value_type_tag = 0) need reverse lookups.
CREATE INDEX idx_datoms_vaet ON mentat.datoms
    USING BTREE (v_ref, a, e, tx)
    WHERE added = TRUE AND value_type_tag = 0;

-- TX: Transaction lookup index
-- Used by: stats.rs (transaction datom counts), time-travel queries
CREATE INDEX idx_datoms_tx ON mentat.datoms
    USING BTREE (tx DESC);

-- ==========================================================================
-- Type-specific AVET indexes for value lookups and range queries
-- These are essential for:
--   - Unique constraint checking (transact.rs check_unique_typed_value)
--   - Value-based entity lookups (query.rs, helpers.rs)
--   - Correct native type comparison operators (fixes BYTEA ordering bug)
--
-- Only the most commonly queried types get dedicated AVET indexes.
-- Rare types (bool, bytes) rely on the AEVT index + filter.
-- ==========================================================================

-- AVET for ref values (entity ID lookups, foreign key traversals)
CREATE INDEX idx_datoms_avet_ref ON mentat.datoms
    USING BTREE (a, v_ref, e, tx)
    WHERE added = TRUE AND value_type_tag = 0;

-- AVET for long/integer values (numeric range queries, most common value type)
CREATE INDEX idx_datoms_avet_long ON mentat.datoms
    USING BTREE (a, v_long, e, tx)
    WHERE added = TRUE AND value_type_tag = 2;

-- AVET for text/string values (equality and prefix queries)
CREATE INDEX idx_datoms_avet_text ON mentat.datoms
    USING BTREE (a, v_text, e, tx)
    WHERE added = TRUE AND value_type_tag = 7;

-- AVET for keyword values (ident resolution, enum-like values)
CREATE INDEX idx_datoms_avet_keyword ON mentat.datoms
    USING BTREE (a, v_keyword, e, tx)
    WHERE added = TRUE AND value_type_tag = 8;

-- ==========================================================================
-- REMOVED INDEXES (previously in 03_indexes.sql + lib.rs):
--
-- idx_datoms_avet_double   -- Rare type; use AEVT + filter when needed
-- idx_datoms_avet_instant  -- Rare type; use AEVT + filter when needed
-- idx_datoms_avet_uuid     -- Rare type; use AEVT + filter when needed
-- idx_datoms_avet_bool     -- Rare type; use AEVT + filter when needed
-- idx_datoms_history        -- Retracted datoms (added=FALSE) rarely queried
-- idx_datoms_temporal       -- Redundant with EAVT (same columns, different order)
-- idx_datoms_cardinality    -- Redundant with EAVT
-- idx_datoms_eavt_long      -- Covering indexes add write cost without proven benefit
-- idx_datoms_eavt_text      -- (PostgreSQL heap fetch is fast enough)
-- idx_datoms_eavt_ref       --
-- idx_datoms_eavt_instant   --
-- idx_datoms_eavt_uuid      --
--
-- If slow queries appear, add indexes back based on pg_stat_user_indexes
-- and slow query log analysis.
-- ==========================================================================

-- ==========================================================================
-- Support Table Indexes
-- ==========================================================================

-- Fulltext search index (GIN for fast tsvector matching)
CREATE INDEX idx_fulltext_search ON mentat.fulltext
    USING GIN (search_vector);

-- Fulltext rowid lookups
CREATE INDEX idx_fulltext_rowid ON mentat.fulltext
    USING BTREE (rowid);

-- Schema lookups by ident
CREATE INDEX idx_schema_ident ON mentat.schema
    USING BTREE (ident);

-- Partition lookups
CREATE INDEX idx_partitions_name ON mentat.partitions
    USING BTREE (name);

-- Transaction timestamp lookups
CREATE INDEX idx_transactions_instant ON mentat.transactions
    USING BTREE (tx_instant DESC);
