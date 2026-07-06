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
--
-- Each sequence is bounded to its partition's band (see the mentat.partitions
-- rows in 06_bootstrap_data.sql) with MINVALUE/MAXVALUE. Bounding matters for
-- correctness, not just tidiness: without an upper bound an exhausted
-- partition silently issues ids that collide with the NEXT partition's space.
-- With MAXVALUE, exhaustion fails loud ("nextval: reached maximum value of
-- sequence") instead of corrupting the entid space. The bands are disjoint and
-- monotonic, and each is large enough (BIGINT) that exhaustion is not a
-- practical concern:
--   db.part/db   [0,          1e6)   schema / bootstrap entities
--   db.part/user [1e6,        1e12)  data entities
--   db.part/tx   [1e12,       2e12)  transactions (one per mentat.t)
CREATE SEQUENCE mentat.partition_db_seq
    START WITH 100      MINVALUE 100        MAXVALUE 999999               CACHE 10;
CREATE SEQUENCE mentat.partition_user_seq
    START WITH 1000000  MINVALUE 1000000    MAXVALUE 999999999999         CACHE 100;
CREATE SEQUENCE mentat.partition_tx_seq
    START WITH 1000000000001 MINVALUE 1000000000000 MAXVALUE 1999999999999 CACHE 100;
