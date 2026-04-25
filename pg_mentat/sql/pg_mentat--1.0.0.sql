-- Type definitions for pg_mentat extension
-- Defines PostgreSQL types and enums used across the schema

-- Value type enumeration matching mentat's ValueType
CREATE TYPE mentat.value_type AS ENUM (
    'ref',
    'boolean',
    'instant',
    'long',
    'double',
    'string',
    'keyword',
    'uuid',
    'bytes'
);

-- Unique constraint types for attributes
CREATE TYPE mentat.unique_type AS ENUM (
    'value',    -- :db.unique/value - unique but not an identity
    'identity'  -- :db.unique/identity - unique and can be used for upsert
);

-- Cardinality types for attributes
CREATE TYPE mentat.cardinality_type AS ENUM (
    'one',  -- :db.cardinality/one - single value
    'many'  -- :db.cardinality/many - multiple values
);
-- Core tables for pg_mentat storage schema
-- Based on mentat's datom model and SQLite schema

-- Partitions: Entity ID allocation and partition management
-- Corresponds to mentat's PartitionMap
CREATE TABLE mentat.partitions (
    name TEXT PRIMARY KEY,
    start_entid BIGINT NOT NULL,
    end_entid BIGINT NOT NULL,
    next_entid BIGINT NOT NULL,
    allow_excision BOOLEAN NOT NULL DEFAULT FALSE,

    CONSTRAINT partition_bounds CHECK (
        start_entid <= next_entid AND
        next_entid <= end_entid
    )
);

-- Schema: Attribute definitions
-- Corresponds to mentat's Attribute struct and Schema
CREATE TABLE mentat.schema (
    entid BIGINT PRIMARY KEY,
    ident TEXT UNIQUE NOT NULL,
    value_type mentat.value_type NOT NULL,
    cardinality mentat.cardinality_type NOT NULL DEFAULT 'one',
    unique_constraint mentat.unique_type,
    indexed BOOLEAN NOT NULL DEFAULT FALSE,
    fulltext BOOLEAN NOT NULL DEFAULT FALSE,
    component BOOLEAN NOT NULL DEFAULT FALSE,
    no_history BOOLEAN NOT NULL DEFAULT FALSE
);

-- Transaction metadata
-- Stores transaction attributes (timestamp, user info, etc.)
CREATE TABLE mentat.transactions (
    tx BIGINT PRIMARY KEY,
    tx_instant TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Datoms: The core fact table (partitioned by value type)
-- Stores all assertions and retractions
-- Structure: [entity, attribute, value, transaction, added]
-- Values are stored in type-specific columns for correct native comparisons.
-- Exactly one v_* column must be NOT NULL per row (enforced by CHECK constraint).
--
-- PARTITIONING: LIST partition by value_type_tag enables:
--   - Partition pruning: queries filtering on type skip irrelevant partitions
--   - Smaller indexes per partition (each only indexes its type's rows)
--   - Better compression (NULL columns are consistent within partitions)
--   - Independent VACUUM per partition (faster maintenance)
--
-- NOTE: Foreign keys (tx -> transactions, a -> schema) are enforced at the
-- application level (transact.rs) because PostgreSQL partitioned tables
-- cannot reference non-partitioned tables via FK constraints.
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

    -- Ensure exactly one value column is populated
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

-- Partition for ref values (value_type_tag = 0)
-- Entity references, foreign key traversals
CREATE TABLE mentat.datoms_ref PARTITION OF mentat.datoms
    FOR VALUES IN (0);

-- Partition for boolean values (value_type_tag = 1)
CREATE TABLE mentat.datoms_bool PARTITION OF mentat.datoms
    FOR VALUES IN (1);

-- Partition for long/integer values (value_type_tag = 2)
-- Most common numeric type
CREATE TABLE mentat.datoms_long PARTITION OF mentat.datoms
    FOR VALUES IN (2);

-- Partition for double/float values (value_type_tag = 3)
CREATE TABLE mentat.datoms_double PARTITION OF mentat.datoms
    FOR VALUES IN (3);

-- Partition for instant/timestamp values (value_type_tag = 4)
CREATE TABLE mentat.datoms_instant PARTITION OF mentat.datoms
    FOR VALUES IN (4);

-- Partition for text/string values (value_type_tag = 7)
CREATE TABLE mentat.datoms_text PARTITION OF mentat.datoms
    FOR VALUES IN (7);

-- Partition for keyword values (value_type_tag = 8)
CREATE TABLE mentat.datoms_keyword PARTITION OF mentat.datoms
    FOR VALUES IN (8);

-- Partition for UUID values (value_type_tag = 10)
CREATE TABLE mentat.datoms_uuid PARTITION OF mentat.datoms
    FOR VALUES IN (10);

-- Partition for bytes/blob values (value_type_tag = 11)
CREATE TABLE mentat.datoms_bytes PARTITION OF mentat.datoms
    FOR VALUES IN (11);

-- Default partition for any future or unexpected type tags
-- Prevents INSERT failures if a new type tag is added before partitions
CREATE TABLE mentat.datoms_default PARTITION OF mentat.datoms
    DEFAULT;

-- Fulltext search support
-- Stores text values and their search vectors for FTS
CREATE TABLE mentat.fulltext (
    rowid BIGSERIAL PRIMARY KEY,
    text_value TEXT NOT NULL,
    search_vector TSVECTOR
);

-- Idents: Cached keyword->entid mappings
-- Used for fast keyword resolution
CREATE TABLE mentat.idents (
    ident TEXT PRIMARY KEY,
    entid BIGINT NOT NULL UNIQUE
);

-- Transaction attributes
-- Links transactions to their metadata attributes
CREATE TABLE mentat.transaction_attrs (
    tx_id BIGINT NOT NULL,
    attr_entid BIGINT NOT NULL,
    value BYTEA NOT NULL,
    value_type_tag SMALLINT NOT NULL,

    PRIMARY KEY (tx_id, attr_entid),

    CONSTRAINT fk_tx_attrs_tx FOREIGN KEY (tx_id)
        REFERENCES mentat.transactions(tx),
    CONSTRAINT fk_tx_attrs_attr FOREIGN KEY (attr_entid)
        REFERENCES mentat.schema(entid)
);

-- Sequences for lock-free entity ID allocation (replaces UPDATE-based locking)
-- CACHE pre-allocates IDs per backend connection for high concurrency without row locks.
CREATE SEQUENCE mentat.partition_db_seq START WITH 100 CACHE 10;
CREATE SEQUENCE mentat.partition_user_seq START WITH 10000 CACHE 100;
CREATE SEQUENCE mentat.partition_tx_seq START WITH 1000001 CACHE 100;
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
-- Constraints and triggers for pg_mentat
-- Enforces uniqueness constraints and maintains referential integrity

-- ==========================================================================
-- Unique value constraint enforcement
-- ==========================================================================
-- For attributes with :db/unique :db.unique/value or :db.unique/identity
-- Uniqueness is enforced in Rust code (transact.rs lines 1121-1164) using:
--   1. In-transaction duplicate checking
--   2. Advisory locks (pg_advisory_xact_lock) to prevent race conditions
--   3. Database lookups for existing values
--
-- Database-level unique indexes cannot be used here because PostgreSQL does not
-- support subqueries in index predicates (we would need to filter by attributes
-- marked with :db/unique, which requires a subquery against mentat.schema).
--
-- The Rust implementation provides complete enforcement, so no database-level
-- indexes are needed.

-- ==========================================================================
-- Validation Triggers
-- ==========================================================================

-- Function to validate value types match schema
CREATE OR REPLACE FUNCTION mentat.validate_datom_value_type()
RETURNS TRIGGER AS $$
DECLARE
    expected_type mentat.value_type;
    expected_tag SMALLINT;
BEGIN
    -- Get expected value type from schema
    SELECT value_type INTO expected_type
    FROM mentat.schema
    WHERE entid = NEW.a;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attribute % not found in schema', NEW.a;
    END IF;

    -- Map value_type enum to the SMALLINT type tag used in datoms.value_type_tag.
    -- These tags match the encoding in transact.rs encode_value() and the
    -- decoding in query.rs build_value_decode_expr() / pull.rs decode_typed_value().
    expected_tag := CASE expected_type
        WHEN 'ref'::mentat.value_type     THEN 0
        WHEN 'boolean'::mentat.value_type  THEN 1
        WHEN 'long'::mentat.value_type     THEN 2
        WHEN 'double'::mentat.value_type   THEN 3
        WHEN 'instant'::mentat.value_type  THEN 4
        WHEN 'string'::mentat.value_type   THEN 7
        WHEN 'keyword'::mentat.value_type  THEN 8
        WHEN 'uuid'::mentat.value_type     THEN 10
        WHEN 'bytes'::mentat.value_type    THEN 11
    END;

    IF NEW.value_type_tag != expected_tag THEN
        RAISE EXCEPTION 'Value type mismatch for attribute %: expected % (tag %), got tag %',
            NEW.a, expected_type, expected_tag, NEW.value_type_tag;
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger on the partitioned parent table (inherited by all partitions)
CREATE TRIGGER validate_datom_value_type_trigger
    BEFORE INSERT OR UPDATE ON mentat.datoms
    FOR EACH ROW
    EXECUTE FUNCTION mentat.validate_datom_value_type();

-- Function to update fulltext search vector
CREATE OR REPLACE FUNCTION mentat.update_fulltext_vector()
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := to_tsvector('english', NEW.text_value);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to automatically update tsvector on insert/update
CREATE TRIGGER update_fulltext_vector_trigger
    BEFORE INSERT OR UPDATE OF text_value ON mentat.fulltext
    FOR EACH ROW
    EXECUTE FUNCTION mentat.update_fulltext_vector();

-- Function to prevent invalid partition modifications
CREATE OR REPLACE FUNCTION mentat.validate_partition()
RETURNS TRIGGER AS $$
BEGIN
    -- Ensure next_entid is within partition bounds
    IF NEW.next_entid < NEW.start_entid OR NEW.next_entid > NEW.end_entid THEN
        RAISE EXCEPTION 'Partition % next_entid (%) must be between start (%) and end (%)',
            NEW.name, NEW.next_entid, NEW.start_entid, NEW.end_entid;
    END IF;

    -- Prevent modifying start/end on existing partitions
    IF TG_OP = 'UPDATE' THEN
        IF OLD.start_entid != NEW.start_entid OR OLD.end_entid != NEW.end_entid THEN
            RAISE EXCEPTION 'Cannot modify partition boundaries for existing partition %', NEW.name;
        END IF;
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to validate partition constraints
CREATE TRIGGER validate_partition_trigger
    BEFORE INSERT OR UPDATE ON mentat.partitions
    FOR EACH ROW
    EXECUTE FUNCTION mentat.validate_partition();
-- Helper functions for pg_mentat
-- Entity ID allocation, value encoding/decoding, and utility functions

-- Allocate a new entity ID from a partition using lock-free sequences.
-- Returns the next available entid without acquiring row-level locks.
CREATE OR REPLACE FUNCTION mentat.allocate_entid(partition_name TEXT)
RETURNS BIGINT AS $$
BEGIN
    CASE partition_name
        WHEN 'db.part/db' THEN RETURN nextval('mentat.partition_db_seq');
        WHEN 'db.part/user' THEN RETURN nextval('mentat.partition_user_seq');
        WHEN 'db.part/tx' THEN RETURN nextval('mentat.partition_tx_seq');
        ELSE RAISE EXCEPTION 'Partition % does not exist', partition_name;
    END CASE;
END;
$$ LANGUAGE plpgsql;

-- Allocate multiple entity IDs from a partition using lock-free sequences.
-- Returns an array of new entids without acquiring row-level locks.
CREATE OR REPLACE FUNCTION mentat.allocate_entids(partition_name TEXT, count INTEGER)
RETURNS BIGINT[] AS $$
DECLARE
    entids BIGINT[];
    i INTEGER;
BEGIN
    IF count <= 0 THEN
        RAISE EXCEPTION 'Count must be positive';
    END IF;

    entids := ARRAY[]::BIGINT[];
    FOR i IN 1..count LOOP
        entids := array_append(entids, mentat.allocate_entid(partition_name));
    END LOOP;

    RETURN entids;
END;
$$ LANGUAGE plpgsql;

-- Get the current transaction ID
-- Creates a new transaction record if needed
CREATE OR REPLACE FUNCTION mentat.current_tx()
RETURNS BIGINT AS $$
DECLARE
    tx_id BIGINT;
BEGIN
    -- Allocate from :db.part/tx partition using lock-free sequence
    tx_id := nextval('mentat.partition_tx_seq');

    -- Create transaction record
    INSERT INTO mentat.transactions (tx, tx_instant)
    VALUES (tx_id, CURRENT_TIMESTAMP);

    RETURN tx_id;
END;
$$ LANGUAGE plpgsql;

-- Resolve a keyword ident to its entid
CREATE OR REPLACE FUNCTION mentat.resolve_ident(keyword TEXT)
RETURNS BIGINT AS $$
DECLARE
    result BIGINT;
BEGIN
    SELECT entid INTO result
    FROM mentat.idents
    WHERE ident = keyword;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Unknown keyword ident: %', keyword;
    END IF;

    RETURN result;
END;
$$ LANGUAGE plpgsql;

-- Lookup entity by unique attribute value
CREATE OR REPLACE FUNCTION mentat.lookup_ref(attr_ident TEXT, value BYTEA, vtype_tag SMALLINT)
RETURNS BIGINT AS $$
DECLARE
    attr_entid BIGINT;
    entity_id BIGINT;
BEGIN
    -- Resolve attribute ident to entid
    attr_entid := mentat.resolve_ident(attr_ident);

    -- Find entity with this unique value
    SELECT e INTO entity_id
    FROM mentat.datoms
    WHERE a = attr_entid
      AND v = value
      AND value_type_tag = vtype_tag
      AND added = TRUE
    LIMIT 1;

    RETURN entity_id;
END;
$$ LANGUAGE plpgsql;

-- Get all datoms for an entity (current state, not history)
CREATE OR REPLACE FUNCTION mentat.entity_datoms(entity_id BIGINT)
RETURNS TABLE(
    attribute BIGINT,
    value BYTEA,
    value_type SMALLINT,
    transaction BIGINT
) AS $$
BEGIN
    RETURN QUERY
    SELECT a, v, value_type_tag, tx
    FROM mentat.datoms
    WHERE e = entity_id
      AND added = TRUE
    ORDER BY a, tx DESC;
END;
$$ LANGUAGE plpgsql;

-- Fulltext search helper
CREATE OR REPLACE FUNCTION mentat.fulltext_search(query TEXT)
RETURNS TABLE(
    rowid BIGINT,
    text_value TEXT,
    rank REAL
) AS $$
BEGIN
    RETURN QUERY
    SELECT
        f.rowid,
        f.text_value,
        ts_rank(f.search_vector, websearch_to_tsquery('english', query))::REAL as rank
    FROM mentat.fulltext f
    WHERE f.search_vector @@ websearch_to_tsquery('english', query)
    ORDER BY rank DESC;
END;
$$ LANGUAGE plpgsql;

-- Check if an attribute has the :db/index property
CREATE OR REPLACE FUNCTION mentat.is_indexed(attr_entid BIGINT)
RETURNS BOOLEAN AS $$
DECLARE
    result BOOLEAN;
BEGIN
    SELECT indexed INTO result
    FROM mentat.schema
    WHERE entid = attr_entid;

    RETURN COALESCE(result, FALSE);
END;
$$ LANGUAGE plpgsql;

-- Check if an attribute has the :db/unique property
CREATE OR REPLACE FUNCTION mentat.is_unique(attr_entid BIGINT)
RETURNS BOOLEAN AS $$
DECLARE
    result mentat.unique_type;
BEGIN
    SELECT unique_constraint INTO result
    FROM mentat.schema
    WHERE entid = attr_entid;

    RETURN result IS NOT NULL;
END;
$$ LANGUAGE plpgsql;

-- Get the value type for an attribute
CREATE OR REPLACE FUNCTION mentat.attribute_value_type(attr_entid BIGINT)
RETURNS mentat.value_type AS $$
DECLARE
    result mentat.value_type;
BEGIN
    SELECT value_type INTO result
    FROM mentat.schema
    WHERE entid = attr_entid;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attribute % not found', attr_entid;
    END IF;

    RETURN result;
END;
$$ LANGUAGE plpgsql;
-- Bootstrap data for pg_mentat
-- Initialize default partitions and core schema attributes

-- Initialize default partitions
-- Based on mentat's default partition map
-- Note: next_entid is kept for metadata but actual allocation uses sequences.
INSERT INTO mentat.partitions (name, start_entid, end_entid, next_entid, allow_excision) VALUES
    ('db.part/db', 0, 10000, 100, FALSE),
    ('db.part/user', 10000, 1000000, 10000, FALSE),
    ('db.part/tx', 1000000, 2000000, 1000001, FALSE);

-- Core schema attributes
-- These correspond to mentat's built-in :db/* attributes
INSERT INTO mentat.schema (entid, ident, value_type, cardinality, unique_constraint, indexed, fulltext, component, no_history) VALUES
    -- Schema definition attributes
    (10, ':db/ident', 'keyword', 'one', 'identity', TRUE, FALSE, FALSE, FALSE),
    (11, ':db/valueType', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (12, ':db/cardinality', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (13, ':db/unique', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (14, ':db/index', 'boolean', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (15, ':db/fulltext', 'boolean', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (16, ':db/component', 'boolean', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (17, ':db/noHistory', 'boolean', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (18, ':db/isComponent', 'boolean', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (19, ':db/doc', 'string', 'one', NULL, FALSE, FALSE, FALSE, FALSE),

    -- Transaction attributes
    (50, ':db/txInstant', 'instant', 'one', NULL, TRUE, FALSE, FALSE, FALSE),

    -- Partition attributes
    (60, ':db.install/partition', 'ref', 'many', NULL, FALSE, FALSE, FALSE, FALSE),
    (61, ':db.install/attribute', 'ref', 'many', NULL, FALSE, FALSE, FALSE, FALSE),

    -- Value type references
    (70, ':db.type/ref', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (71, ':db.type/keyword', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (72, ':db.type/long', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (73, ':db.type/double', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (74, ':db.type/string', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (75, ':db.type/boolean', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (76, ':db.type/instant', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (77, ':db.type/uuid', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (78, ':db.type/bytes', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),

    -- Cardinality references
    (80, ':db.cardinality/one', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (81, ':db.cardinality/many', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),

    -- Unique references
    (82, ':db.unique/value', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (83, ':db.unique/identity', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),

    -- Partition entities
    (90, ':db.part/db', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (91, ':db.part/user', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE),
    (92, ':db.part/tx', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE);

-- Populate idents cache with core attributes
INSERT INTO mentat.idents (ident, entid)
SELECT ident, entid FROM mentat.schema
WHERE entid < 100;

-- Advance sequences past bootstrap-allocated IDs.
-- db.part/db bootstrap uses entids up to 92, so advance to 100.
SELECT setval('mentat.partition_db_seq', 100, false);
