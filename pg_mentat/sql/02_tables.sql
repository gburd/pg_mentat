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
    tx_id BIGINT PRIMARY KEY,
    instant TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Datoms: The core fact table
-- Stores all assertions and retractions
-- Structure: [entity, attribute, value, transaction, added]
CREATE TABLE mentat.datoms (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BYTEA NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE,
    value_type_tag SMALLINT NOT NULL,

    -- Reference to transactions table
    CONSTRAINT fk_datoms_tx FOREIGN KEY (tx)
        REFERENCES mentat.transactions(tx_id),

    -- Reference to schema table for attribute
    CONSTRAINT fk_datoms_attr FOREIGN KEY (a)
        REFERENCES mentat.schema(entid)
);

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
        REFERENCES mentat.transactions(tx_id),
    CONSTRAINT fk_tx_attrs_attr FOREIGN KEY (attr_entid)
        REFERENCES mentat.schema(entid)
);
