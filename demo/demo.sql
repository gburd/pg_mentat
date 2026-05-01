-- pg_mentat demo: Mentat Datalog database for PostgreSQL
-- This script runs automatically when the Docker container starts for the first time.

-- ============================================================================
-- 1. Create the extension (registers Rust functions and creates mentat schema)
-- ============================================================================
CREATE EXTENSION pg_mentat;

-- ============================================================================
-- 2. Bootstrap the Mentat storage layer
--    (The extension_sql! macro content from lib.rs is not always included in
--     the pgrx-generated migration, so we run it explicitly here.)
-- ============================================================================

-- Enum types for schema metadata
DO $$ BEGIN
    CREATE TYPE mentat.value_type AS ENUM (
        'ref', 'boolean', 'instant', 'long', 'double', 'string', 'keyword', 'uuid', 'bytes'
    );
EXCEPTION WHEN duplicate_object THEN null;
END $$;

DO $$ BEGIN
    CREATE TYPE mentat.unique_type AS ENUM ('value', 'identity');
EXCEPTION WHEN duplicate_object THEN null;
END $$;

DO $$ BEGIN
    CREATE TYPE mentat.cardinality_type AS ENUM ('one', 'many');
EXCEPTION WHEN duplicate_object THEN null;
END $$;

-- Core storage tables
CREATE TABLE IF NOT EXISTS mentat.datoms (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BYTEA NOT NULL,
    value_type_tag SMALLINT NOT NULL,
    tx BIGINT NOT NULL,
    added BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE IF NOT EXISTS mentat.schema (
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

CREATE TABLE IF NOT EXISTS mentat.idents (
    ident TEXT PRIMARY KEY,
    entid BIGINT UNIQUE NOT NULL
);

CREATE TABLE IF NOT EXISTS mentat.partitions (
    name TEXT PRIMARY KEY,
    start_entid BIGINT NOT NULL,
    end_entid BIGINT NOT NULL,
    next_entid BIGINT NOT NULL,
    allow_excision BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS mentat.transactions (
    tx BIGINT PRIMARY KEY,
    tx_instant TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- EAVT, AEVT, AVET, VAET index pattern
CREATE INDEX IF NOT EXISTS idx_datoms_eavt ON mentat.datoms (e, a, value_type_tag, v, tx);
CREATE INDEX IF NOT EXISTS idx_datoms_aevt ON mentat.datoms (a, e, value_type_tag, v, tx);
CREATE INDEX IF NOT EXISTS idx_datoms_avet ON mentat.datoms (a, value_type_tag, v, e, tx);
CREATE INDEX IF NOT EXISTS idx_datoms_vaet ON mentat.datoms (v, a, e, tx) WHERE value_type_tag = 0;
CREATE INDEX IF NOT EXISTS idx_datoms_tx ON mentat.datoms (tx);

-- Full-text search support
CREATE TABLE IF NOT EXISTS mentat.fulltext (
    text_value TEXT NOT NULL,
    search_vector TSVECTOR
);
CREATE INDEX IF NOT EXISTS idx_fulltext_search ON mentat.fulltext USING GIN (search_vector);

CREATE OR REPLACE FUNCTION mentat.fulltext_update_trigger() RETURNS trigger AS $$
BEGIN
    NEW.search_vector := to_tsvector('english', NEW.text_value);
    RETURN NEW;
END; $$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS fulltext_update ON mentat.fulltext;
CREATE TRIGGER fulltext_update BEFORE INSERT OR UPDATE ON mentat.fulltext
    FOR EACH ROW EXECUTE FUNCTION mentat.fulltext_update_trigger();

-- Partition initialization
INSERT INTO mentat.partitions (name, start_entid, end_entid, next_entid, allow_excision) VALUES
    ('db.part/db', 0, 10000, 100, FALSE),
    ('db.part/user', 10000, 1000000, 10000, FALSE),
    ('db.part/tx', 1000000, 2000000, 1000001, FALSE)
ON CONFLICT (name) DO NOTHING;

INSERT INTO mentat.transactions (tx, tx_instant)
VALUES (1000000, '2025-01-01T00:00:00Z')
ON CONFLICT (tx) DO NOTHING;

-- Sequences for lock-free entity ID allocation
CREATE SEQUENCE IF NOT EXISTS mentat.partition_db_seq START WITH 100 CACHE 10;
CREATE SEQUENCE IF NOT EXISTS mentat.partition_user_seq START WITH 10000 CACHE 100;
CREATE SEQUENCE IF NOT EXISTS mentat.partition_tx_seq START WITH 1000001 CACHE 100;

-- PL/pgSQL helper functions (sequence-based, lock-free)
CREATE OR REPLACE FUNCTION mentat.allocate_entid(partition_name TEXT)
RETURNS BIGINT AS $$
BEGIN
    CASE partition_name
        WHEN 'db.part/db' THEN RETURN nextval('mentat.partition_db_seq');
        WHEN 'db.part/user' THEN RETURN nextval('mentat.partition_user_seq');
        WHEN 'db.part/tx' THEN RETURN nextval('mentat.partition_tx_seq');
        ELSE RAISE EXCEPTION 'Partition % not found', partition_name;
    END CASE;
END; $$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION mentat.resolve_ident(keyword TEXT)
RETURNS BIGINT AS $$
BEGIN
    RETURN (SELECT entid FROM mentat.idents WHERE ident = keyword);
END; $$ LANGUAGE plpgsql;

-- Bootstrap schema data (core attributes and ident mappings)
INSERT INTO mentat.schema (entid, ident, value_type, cardinality, unique_constraint, indexed) VALUES
    (1, ':db/ident', 'keyword', 'one', 'identity', true),
    (2, ':db/valueType', 'ref', 'one', NULL, false),
    (3, ':db/cardinality', 'ref', 'one', NULL, false),
    (4, ':db/unique', 'ref', 'one', NULL, false),
    (5, ':db/doc', 'string', 'one', NULL, false),
    (6, ':db/isComponent', 'boolean', 'one', NULL, false),
    (7, ':db/fulltext', 'boolean', 'one', NULL, false),
    (8, ':db/index', 'boolean', 'one', NULL, false),
    (9, ':db/noHistory', 'boolean', 'one', NULL, false),
    (10, ':db/txInstant', 'instant', 'one', NULL, true),
    (20, ':db.type/ref', 'ref', 'one', NULL, false),
    (21, ':db.type/keyword', 'ref', 'one', NULL, false),
    (22, ':db.type/long', 'ref', 'one', NULL, false),
    (23, ':db.type/double', 'ref', 'one', NULL, false),
    (24, ':db.type/string', 'ref', 'one', NULL, false),
    (25, ':db.type/boolean', 'ref', 'one', NULL, false),
    (26, ':db.type/instant', 'ref', 'one', NULL, false),
    (27, ':db.type/uuid', 'ref', 'one', NULL, false),
    (28, ':db.type/bytes', 'ref', 'one', NULL, false),
    (30, ':db.cardinality/one', 'ref', 'one', NULL, false),
    (31, ':db.cardinality/many', 'ref', 'one', NULL, false),
    (32, ':db.unique/value', 'ref', 'one', NULL, false),
    (33, ':db.unique/identity', 'ref', 'one', NULL, false)
ON CONFLICT (entid) DO NOTHING;

INSERT INTO mentat.idents (ident, entid) VALUES
    (':db/ident', 1),
    (':db/valueType', 2),
    (':db/cardinality', 3),
    (':db/unique', 4),
    (':db/doc', 5),
    (':db/isComponent', 6),
    (':db/fulltext', 7),
    (':db/index', 8),
    (':db/noHistory', 9),
    (':db/txInstant', 10),
    (':db.type/ref', 20),
    (':db.type/keyword', 21),
    (':db.type/long', 22),
    (':db.type/double', 23),
    (':db.type/string', 24),
    (':db.type/boolean', 25),
    (':db.type/instant', 26),
    (':db.type/uuid', 27),
    (':db.type/bytes', 28),
    (':db.cardinality/one', 30),
    (':db.cardinality/many', 31),
    (':db.unique/value', 32),
    (':db.unique/identity', 33)
ON CONFLICT (ident) DO NOTHING;

-- Bootstrap datoms (so queries can find schema attributes)
INSERT INTO mentat.datoms (e, a, v, value_type_tag, tx, added) VALUES
    -- :db/ident datoms (a=1, keyword type_tag=8)
    (1,  1, 'db/ident'::bytea,            8, 1000000, true),
    (2,  1, 'db/valueType'::bytea,        8, 1000000, true),
    (3,  1, 'db/cardinality'::bytea,      8, 1000000, true),
    (4,  1, 'db/unique'::bytea,            8, 1000000, true),
    (5,  1, 'db/doc'::bytea,               8, 1000000, true),
    (6,  1, 'db/isComponent'::bytea,       8, 1000000, true),
    (7,  1, 'db/fulltext'::bytea,          8, 1000000, true),
    (8,  1, 'db/index'::bytea,             8, 1000000, true),
    (9,  1, 'db/noHistory'::bytea,         8, 1000000, true),
    (10, 1, 'db/txInstant'::bytea,         8, 1000000, true),
    (20, 1, 'db.type/ref'::bytea,          8, 1000000, true),
    (21, 1, 'db.type/keyword'::bytea,      8, 1000000, true),
    (22, 1, 'db.type/long'::bytea,         8, 1000000, true),
    (23, 1, 'db.type/double'::bytea,       8, 1000000, true),
    (24, 1, 'db.type/string'::bytea,       8, 1000000, true),
    (25, 1, 'db.type/boolean'::bytea,      8, 1000000, true),
    (26, 1, 'db.type/instant'::bytea,      8, 1000000, true),
    (27, 1, 'db.type/uuid'::bytea,         8, 1000000, true),
    (28, 1, 'db.type/bytes'::bytea,        8, 1000000, true),
    (30, 1, 'db.cardinality/one'::bytea,   8, 1000000, true),
    (31, 1, 'db.cardinality/many'::bytea,  8, 1000000, true),
    (32, 1, 'db.unique/value'::bytea,      8, 1000000, true),
    (33, 1, 'db.unique/identity'::bytea,   8, 1000000, true),
    -- :db/valueType datoms (a=2, ref type_tag=0, LE i64 entity IDs)
    (1,  2, E'\\x1500000000000000'::bytea, 0, 1000000, true),
    (2,  2, E'\\x1400000000000000'::bytea, 0, 1000000, true),
    (3,  2, E'\\x1400000000000000'::bytea, 0, 1000000, true),
    (4,  2, E'\\x1400000000000000'::bytea, 0, 1000000, true),
    (5,  2, E'\\x1800000000000000'::bytea, 0, 1000000, true),
    (6,  2, E'\\x1900000000000000'::bytea, 0, 1000000, true),
    (7,  2, E'\\x1900000000000000'::bytea, 0, 1000000, true),
    (8,  2, E'\\x1900000000000000'::bytea, 0, 1000000, true),
    (9,  2, E'\\x1900000000000000'::bytea, 0, 1000000, true),
    (10, 2, E'\\x1a00000000000000'::bytea, 0, 1000000, true),
    -- :db/cardinality datoms (a=3, ref type_tag=0, all cardinality/one = entity 30 = 0x1e)
    (1,  3, E'\\x1e00000000000000'::bytea, 0, 1000000, true),
    (2,  3, E'\\x1e00000000000000'::bytea, 0, 1000000, true),
    (3,  3, E'\\x1e00000000000000'::bytea, 0, 1000000, true),
    (4,  3, E'\\x1e00000000000000'::bytea, 0, 1000000, true),
    (5,  3, E'\\x1e00000000000000'::bytea, 0, 1000000, true),
    (6,  3, E'\\x1e00000000000000'::bytea, 0, 1000000, true),
    (7,  3, E'\\x1e00000000000000'::bytea, 0, 1000000, true),
    (8,  3, E'\\x1e00000000000000'::bytea, 0, 1000000, true),
    (9,  3, E'\\x1e00000000000000'::bytea, 0, 1000000, true),
    (10, 3, E'\\x1e00000000000000'::bytea, 0, 1000000, true);

-- Grant permissions
GRANT USAGE ON SCHEMA mentat TO PUBLIC;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA mentat TO PUBLIC;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA mentat TO PUBLIC;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA mentat TO PUBLIC;

-- ============================================================================
-- 3. Define schema attributes
-- ============================================================================

-- Define person attributes (name, age, email)
SELECT mentat_transact('[
  [:db/add "name-attr" :db/ident :person/name]
  [:db/add "name-attr" :db/valueType :db.type/string]
  [:db/add "name-attr" :db/cardinality :db.cardinality/one]
  [:db/add "age-attr" :db/ident :person/age]
  [:db/add "age-attr" :db/valueType :db.type/long]
  [:db/add "age-attr" :db/cardinality :db.cardinality/one]
  [:db/add "email-attr" :db/ident :person/email]
  [:db/add "email-attr" :db/valueType :db.type/string]
  [:db/add "email-attr" :db/cardinality :db.cardinality/one]
]'::TEXT);

-- ============================================================================
-- 3. Add sample data
-- ============================================================================

-- Add some people
SELECT mentat_transact('[
  [:db/add "alice" :person/name "Alice"]
  [:db/add "alice" :person/age 30]
  [:db/add "alice" :person/email "alice@example.com"]
]'::TEXT);

SELECT mentat_transact('[
  [:db/add "bob" :person/name "Bob"]
  [:db/add "bob" :person/age 25]
  [:db/add "bob" :person/email "bob@example.com"]
]'::TEXT);

SELECT mentat_transact('[
  [:db/add "carol" :person/name "Carol"]
  [:db/add "carol" :person/age 35]
  [:db/add "carol" :person/email "carol@example.com"]
]'::TEXT);

-- ============================================================================
-- 4. Query examples
-- ============================================================================

-- Find all people (entity ID and name)
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]'::TEXT,
  '{}'::jsonb
);

-- Find a person by age using :in parameter binding
SELECT mentat_query(
  '[:find ?name :in ?age :where [?e :person/age ?age] [?e :person/name ?name]]'::TEXT,
  '{"inputs": [30]}'::jsonb
);

-- Find people older than 28
SELECT mentat_query(
  '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] [(> ?age 28)]]'::TEXT,
  '{}'::jsonb
);

-- View the full schema
SELECT mentat_schema();
