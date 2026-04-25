/// Bootstrap schema for pg_mentat extension
use pgrx::prelude::*;

/// Bootstrap the core Mentat schema.
/// This function initializes the core schema attributes (:db/ident, :db/valueType, etc.)
/// and should be called automatically when the extension is created.
#[pg_extern]
pub fn bootstrap_schema() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Spi::run(
        r#"
        INSERT INTO mentat.schema (entid, ident, value_type, cardinality, unique_constraint, indexed) VALUES
            -- Core schema attributes
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

            -- Value type enum entities (used as values for :db/valueType)
            (20, ':db.type/ref', 'ref', 'one', NULL, false),
            (21, ':db.type/keyword', 'ref', 'one', NULL, false),
            (22, ':db.type/long', 'ref', 'one', NULL, false),
            (23, ':db.type/double', 'ref', 'one', NULL, false),
            (24, ':db.type/string', 'ref', 'one', NULL, false),
            (25, ':db.type/boolean', 'ref', 'one', NULL, false),
            (26, ':db.type/instant', 'ref', 'one', NULL, false),
            (27, ':db.type/uuid', 'ref', 'one', NULL, false),
            (28, ':db.type/bytes', 'ref', 'one', NULL, false),

            -- Cardinality enum entities (used as values for :db/cardinality)
            (30, ':db.cardinality/one', 'ref', 'one', NULL, false),
            (31, ':db.cardinality/many', 'ref', 'one', NULL, false),

            -- Unique constraint enum entities (used as values for :db/unique)
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
            -- Value type enums
            (':db.type/ref', 20),
            (':db.type/keyword', 21),
            (':db.type/long', 22),
            (':db.type/double', 23),
            (':db.type/string', 24),
            (':db.type/boolean', 25),
            (':db.type/instant', 26),
            (':db.type/uuid', 27),
            (':db.type/bytes', 28),
            -- Cardinality enums
            (':db.cardinality/one', 30),
            (':db.cardinality/many', 31),
            -- Unique constraint enums
            (':db.unique/value', 32),
            (':db.unique/identity', 33)
        ON CONFLICT (ident) DO NOTHING;

        -- Bootstrap datoms in the datoms table so queries can find them.
        -- a=1 is :db/ident (keyword type_tag=8, stored in v_keyword)
        -- a=2 is :db/valueType (ref type_tag=0, stored in v_ref as entity ID)
        -- a=3 is :db/cardinality (ref type_tag=0, stored in v_ref as entity ID)
        -- tx=1000000 is the bootstrap transaction.

        -- :db/ident datoms (a=1, keyword stored in v_keyword)
        INSERT INTO mentat.datoms (e, a, value_type_tag, v_keyword, tx, added) VALUES
            (1,  1, 8, 'db/ident',            1000000, true),
            (2,  1, 8, 'db/valueType',        1000000, true),
            (3,  1, 8, 'db/cardinality',      1000000, true),
            (4,  1, 8, 'db/unique',            1000000, true),
            (5,  1, 8, 'db/doc',               1000000, true),
            (6,  1, 8, 'db/isComponent',       1000000, true),
            (7,  1, 8, 'db/fulltext',          1000000, true),
            (8,  1, 8, 'db/index',             1000000, true),
            (9,  1, 8, 'db/noHistory',         1000000, true),
            (10, 1, 8, 'db/txInstant',         1000000, true),
            (20, 1, 8, 'db.type/ref',          1000000, true),
            (21, 1, 8, 'db.type/keyword',      1000000, true),
            (22, 1, 8, 'db.type/long',         1000000, true),
            (23, 1, 8, 'db.type/double',       1000000, true),
            (24, 1, 8, 'db.type/string',       1000000, true),
            (25, 1, 8, 'db.type/boolean',      1000000, true),
            (26, 1, 8, 'db.type/instant',      1000000, true),
            (27, 1, 8, 'db.type/uuid',         1000000, true),
            (28, 1, 8, 'db.type/bytes',        1000000, true),
            (30, 1, 8, 'db.cardinality/one',   1000000, true),
            (31, 1, 8, 'db.cardinality/many',  1000000, true),
            (32, 1, 8, 'db.unique/value',      1000000, true),
            (33, 1, 8, 'db.unique/identity',   1000000, true);

        -- :db/valueType datoms (a=2, ref stored in v_ref as entity ID)
        INSERT INTO mentat.datoms (e, a, value_type_tag, v_ref, tx, added) VALUES
            -- Entity 1 (:db/ident) -> :db.type/keyword (entity 21)
            (1,  2, 0, 21, 1000000, true),
            -- Entity 2 (:db/valueType) -> :db.type/ref (entity 20)
            (2,  2, 0, 20, 1000000, true),
            -- Entity 3 (:db/cardinality) -> :db.type/ref (entity 20)
            (3,  2, 0, 20, 1000000, true),
            -- Entity 4 (:db/unique) -> :db.type/ref (entity 20)
            (4,  2, 0, 20, 1000000, true),
            -- Entity 5 (:db/doc) -> :db.type/string (entity 24)
            (5,  2, 0, 24, 1000000, true),
            -- Entity 6 (:db/isComponent) -> :db.type/boolean (entity 25)
            (6,  2, 0, 25, 1000000, true),
            -- Entity 7 (:db/fulltext) -> :db.type/boolean (entity 25)
            (7,  2, 0, 25, 1000000, true),
            -- Entity 8 (:db/index) -> :db.type/boolean (entity 25)
            (8,  2, 0, 25, 1000000, true),
            -- Entity 9 (:db/noHistory) -> :db.type/boolean (entity 25)
            (9,  2, 0, 25, 1000000, true),
            -- Entity 10 (:db/txInstant) -> :db.type/instant (entity 26)
            (10, 2, 0, 26, 1000000, true);

        -- :db/cardinality datoms (a=3, ref stored in v_ref as entity ID)
        -- All core attrs have cardinality :db.cardinality/one (entity 30)
        INSERT INTO mentat.datoms (e, a, value_type_tag, v_ref, tx, added) VALUES
            (1,  3, 0, 30, 1000000, true),
            (2,  3, 0, 30, 1000000, true),
            (3,  3, 0, 30, 1000000, true),
            (4,  3, 0, 30, 1000000, true),
            (5,  3, 0, 30, 1000000, true),
            (6,  3, 0, 30, 1000000, true),
            (7,  3, 0, 30, 1000000, true),
            (8,  3, 0, 30, 1000000, true),
            (9,  3, 0, 30, 1000000, true),
            (10, 3, 0, 30, 1000000, true);
        "#,
    )?;
    Ok(())
}
