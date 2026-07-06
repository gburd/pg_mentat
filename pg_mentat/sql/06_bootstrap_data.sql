-- Bootstrap data for pg_mentat
-- Initialize default partitions and core schema attributes

-- Initialize default partitions
-- Based on mentat's default partition map
-- Note: next_entid is kept for metadata but actual allocation uses sequences.
INSERT INTO mentat.partitions (name, start_entid, end_entid, next_entid, allow_excision) VALUES
    ('db.part/db',   0,             1000000,       100,            FALSE),
    ('db.part/user', 1000000,       1000000000000, 1000000,        FALSE),
    ('db.part/tx',   1000000000000, 2000000000000, 1000000000001,  FALSE)
ON CONFLICT (name) DO NOTHING;

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
    (92, ':db.part/tx', 'ref', 'one', NULL, FALSE, FALSE, FALSE, FALSE)
ON CONFLICT (entid) DO NOTHING;

-- Populate idents cache with core attributes
INSERT INTO mentat.idents (ident, entid)
SELECT ident, entid FROM mentat.schema
WHERE entid < 100
ON CONFLICT (ident) DO NOTHING;

-- The partition sequences are created at their band floor by the
-- CREATE SEQUENCE ... START WITH statements (02_tables.sql / lib.rs), and the
-- bootstrap schema entids (10-92) are explicit, below the db sequence floor of
-- 100. No setval is needed here, and a setval would be harmful if this ran on
-- a store whose sequences had already advanced (it would rewind them).
