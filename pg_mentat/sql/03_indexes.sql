-- Index definitions for pg_mentat
-- Implements EAVT, AEVT, AVET, VAET index pattern from Datomic/Mentat
--
-- Values are stored in type-specific columns (v_ref, v_long, v_text, etc.)
-- so AVET indexes are type-specific partial indexes for correct range queries.

-- EAVT: Primary index for entity-centric lookups
-- "What are all the facts about entity E?"
CREATE INDEX idx_datoms_eavt ON mentat.datoms
    USING BTREE (e, a, value_type_tag, tx)
    WHERE added = TRUE;

-- AEVT: Index for attribute-centric scans
-- "What are all the facts with attribute A?"
CREATE INDEX idx_datoms_aevt ON mentat.datoms
    USING BTREE (a, e, value_type_tag, tx)
    WHERE added = TRUE;

-- VAET: Reverse reference index for ref types
-- "What entities reference entity E?"
-- Partial index: only for ref types (value_type_tag = 0)
CREATE INDEX idx_datoms_vaet ON mentat.datoms
    USING BTREE (v_ref, a, e, tx)
    WHERE added = TRUE AND value_type_tag = 0;

-- Type-specific AVET indexes for correct range queries
-- Each index covers one type so PostgreSQL uses native type comparison operators.
-- This fixes the core bug: BYTEA binary comparison produced wrong ordering
-- (e.g., "2" > "10" in binary because 0x32 > 0x31).

-- AVET for ref values (entity ID lookups)
CREATE INDEX idx_datoms_avet_ref ON mentat.datoms
    USING BTREE (a, v_ref, e, tx)
    WHERE added = TRUE AND value_type_tag = 0;

-- AVET for long/integer values (numeric range queries)
CREATE INDEX idx_datoms_avet_long ON mentat.datoms
    USING BTREE (a, v_long, e, tx)
    WHERE added = TRUE AND value_type_tag = 2;

-- AVET for double/float values
CREATE INDEX idx_datoms_avet_double ON mentat.datoms
    USING BTREE (a, v_double, e, tx)
    WHERE added = TRUE AND value_type_tag = 3;

-- AVET for instant/timestamp values (temporal range queries)
CREATE INDEX idx_datoms_avet_instant ON mentat.datoms
    USING BTREE (a, v_instant, e, tx)
    WHERE added = TRUE AND value_type_tag = 4;

-- AVET for text/string values (lexicographic ordering)
CREATE INDEX idx_datoms_avet_text ON mentat.datoms
    USING BTREE (a, v_text, e, tx)
    WHERE added = TRUE AND value_type_tag = 7;

-- AVET for keyword values
CREATE INDEX idx_datoms_avet_keyword ON mentat.datoms
    USING BTREE (a, v_keyword, e, tx)
    WHERE added = TRUE AND value_type_tag = 8;

-- AVET for UUID values
CREATE INDEX idx_datoms_avet_uuid ON mentat.datoms
    USING BTREE (a, v_uuid, e, tx)
    WHERE added = TRUE AND value_type_tag = 10;

-- Transaction temporal index
CREATE INDEX idx_datoms_tx ON mentat.datoms
    USING BTREE (tx DESC);

-- Index for retracted datoms (added = FALSE)
-- Used for history queries
CREATE INDEX idx_datoms_history ON mentat.datoms
    USING BTREE (e, a, tx DESC)
    WHERE added = FALSE;

-- Fulltext search index
-- GIN index on tsvector for fast fulltext queries
CREATE INDEX idx_fulltext_search ON mentat.fulltext
    USING GIN (search_vector);

-- Index for fulltext rowid lookups
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
    USING BTREE (instant DESC);

-- Additional temporal index for as-of/since queries
-- Optimizes time-travel queries that filter by transaction range
CREATE INDEX idx_datoms_temporal ON mentat.datoms
    USING BTREE (e, a, tx DESC)
    WHERE added = TRUE;

-- Covering index for cardinality checks during validation
-- Avoids table lookups when checking for existing values
CREATE INDEX idx_datoms_cardinality ON mentat.datoms
    USING BTREE (e, a, added)
    INCLUDE (value_type_tag, tx);

-- Fulltext entity/attribute reference index
-- Speeds up joins between fulltext and datoms tables
CREATE INDEX idx_fulltext_entity_attr ON mentat.fulltext
    USING BTREE (entity, attribute);
