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
