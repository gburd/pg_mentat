/// Bootstrap schema for pg_mentat extension
use pgrx::prelude::*;

/// Bootstrap the core Mentat schema.
/// This function initializes the core schema attributes (:db/ident, :db/valueType, etc.)
/// and should be called automatically when the extension is created.
///
/// Entid numbering MUST match pg_mentat/sql/06_bootstrap_data.sql:
///   10-19: Core schema attributes (:db/ident, :db/valueType, etc.)
///   50:    :db/txInstant
///   60-61: :db.install/partition, :db.install/attribute
///   70-78: :db.type/* value type enum entities
///   80-81: :db.cardinality/one, :db.cardinality/many
///   82-83: :db.unique/value, :db.unique/identity
///   90-92: :db.part/db, :db.part/user, :db.part/tx
#[pg_extern]
pub fn bootstrap_schema() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Spi::run(
        r"
        INSERT INTO mentat.schema (entid, ident, value_type, cardinality, unique_constraint, indexed, fulltext, component, no_history) VALUES
            -- Core schema attributes
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

        INSERT INTO mentat.idents (ident, entid)
        SELECT ident, entid FROM mentat.schema
        WHERE entid < 100
        ON CONFLICT (ident) DO NOTHING;

        -- Initialize default partitions (new disjoint layout; sequences are
        -- created + bounded by the extension_sql! bootstrap in lib.rs).
        INSERT INTO mentat.partitions (name, start_entid, end_entid, next_entid, allow_excision) VALUES
            ('db.part/db',   0,             1000000,       100,            FALSE),
            ('db.part/user', 1000001,       1000000000000, 1000001,        FALSE),
            ('db.part/tx',   1000000000000, 2000000000000, 1000000000001,  FALSE)
        ON CONFLICT (name) DO NOTHING;

        -- NOTE: do NOT setval the partition sequences here. bootstrap_schema()
        -- is idempotent and re-callable (every test setup calls it), so a
        -- setval would REWIND a shared sequence mid-workload and hand out
        -- duplicate entids. The sequences are created at their correct band
        -- floor by the extension_sql! bootstrap (lib.rs) / 02_tables.sql and
        -- only ever advance via nextval.

        -- Bootstrap datoms: EAV facts describing each bootstrap entity.
        -- a=10 is :db/ident (keyword type_tag=8, stored in v_keyword)
        -- a=11 is :db/valueType (ref type_tag=0, stored in v_ref as entity ID)
        -- a=12 is :db/cardinality (ref type_tag=0, stored in v_ref as entity ID)
        -- a=14 is :db/index (boolean type_tag=1, stored in v_bool)
        -- a=13 is :db/unique (ref type_tag=0, stored in v_ref)
        -- tx=1000000 is the bootstrap transaction.

        -- :db/ident datoms (a=10, keyword stored in v_keyword)
        INSERT INTO mentat.datoms (e, a, value_type_tag, v_keyword, tx, added) VALUES
            (10, 10, 8, 'db/ident',               1000000, true),
            (11, 10, 8, 'db/valueType',            1000000, true),
            (12, 10, 8, 'db/cardinality',          1000000, true),
            (13, 10, 8, 'db/unique',               1000000, true),
            (14, 10, 8, 'db/index',                1000000, true),
            (15, 10, 8, 'db/fulltext',             1000000, true),
            (16, 10, 8, 'db/component',            1000000, true),
            (17, 10, 8, 'db/noHistory',            1000000, true),
            (18, 10, 8, 'db/isComponent',          1000000, true),
            (19, 10, 8, 'db/doc',                  1000000, true),
            (50, 10, 8, 'db/txInstant',            1000000, true),
            (60, 10, 8, 'db.install/partition',    1000000, true),
            (61, 10, 8, 'db.install/attribute',    1000000, true),
            (70, 10, 8, 'db.type/ref',             1000000, true),
            (71, 10, 8, 'db.type/keyword',         1000000, true),
            (72, 10, 8, 'db.type/long',            1000000, true),
            (73, 10, 8, 'db.type/double',          1000000, true),
            (74, 10, 8, 'db.type/string',          1000000, true),
            (75, 10, 8, 'db.type/boolean',         1000000, true),
            (76, 10, 8, 'db.type/instant',         1000000, true),
            (77, 10, 8, 'db.type/uuid',            1000000, true),
            (78, 10, 8, 'db.type/bytes',           1000000, true),
            (80, 10, 8, 'db.cardinality/one',      1000000, true),
            (81, 10, 8, 'db.cardinality/many',     1000000, true),
            (82, 10, 8, 'db.unique/value',         1000000, true),
            (83, 10, 8, 'db.unique/identity',      1000000, true),
            (90, 10, 8, 'db.part/db',              1000000, true),
            (91, 10, 8, 'db.part/user',            1000000, true),
            (92, 10, 8, 'db.part/tx',              1000000, true)
        ON CONFLICT DO NOTHING;

        -- :db/valueType datoms (a=11, ref stored in v_ref as entity ID)
        INSERT INTO mentat.datoms (e, a, value_type_tag, v_ref, tx, added) VALUES
            -- Entity 10 (:db/ident) -> :db.type/keyword (entity 71)
            (10, 11, 0, 71, 1000000, true),
            -- Entity 11 (:db/valueType) -> :db.type/ref (entity 70)
            (11, 11, 0, 70, 1000000, true),
            -- Entity 12 (:db/cardinality) -> :db.type/ref (entity 70)
            (12, 11, 0, 70, 1000000, true),
            -- Entity 13 (:db/unique) -> :db.type/ref (entity 70)
            (13, 11, 0, 70, 1000000, true),
            -- Entity 14 (:db/index) -> :db.type/boolean (entity 75)
            (14, 11, 0, 75, 1000000, true),
            -- Entity 15 (:db/fulltext) -> :db.type/boolean (entity 75)
            (15, 11, 0, 75, 1000000, true),
            -- Entity 16 (:db/component) -> :db.type/boolean (entity 75)
            (16, 11, 0, 75, 1000000, true),
            -- Entity 17 (:db/noHistory) -> :db.type/boolean (entity 75)
            (17, 11, 0, 75, 1000000, true),
            -- Entity 18 (:db/isComponent) -> :db.type/boolean (entity 75)
            (18, 11, 0, 75, 1000000, true),
            -- Entity 19 (:db/doc) -> :db.type/string (entity 74)
            (19, 11, 0, 74, 1000000, true),
            -- Entity 50 (:db/txInstant) -> :db.type/instant (entity 76)
            (50, 11, 0, 76, 1000000, true),
            -- Entity 60 (:db.install/partition) -> :db.type/ref (entity 70)
            (60, 11, 0, 70, 1000000, true),
            -- Entity 61 (:db.install/attribute) -> :db.type/ref (entity 70)
            (61, 11, 0, 70, 1000000, true)
        ON CONFLICT DO NOTHING;

        -- :db/cardinality datoms (a=12, ref stored in v_ref as entity ID)
        INSERT INTO mentat.datoms (e, a, value_type_tag, v_ref, tx, added) VALUES
            -- Core attrs: cardinality one (entity 80)
            (10, 12, 0, 80, 1000000, true),
            (11, 12, 0, 80, 1000000, true),
            (12, 12, 0, 80, 1000000, true),
            (13, 12, 0, 80, 1000000, true),
            (14, 12, 0, 80, 1000000, true),
            (15, 12, 0, 80, 1000000, true),
            (16, 12, 0, 80, 1000000, true),
            (17, 12, 0, 80, 1000000, true),
            (18, 12, 0, 80, 1000000, true),
            (19, 12, 0, 80, 1000000, true),
            (50, 12, 0, 80, 1000000, true),
            -- install attrs: cardinality many (entity 81)
            (60, 12, 0, 81, 1000000, true),
            (61, 12, 0, 81, 1000000, true)
        ON CONFLICT DO NOTHING;

        -- :db/unique datoms (a=13, ref stored in v_ref)
        -- :db/ident has unique identity (entity 83)
        INSERT INTO mentat.datoms (e, a, value_type_tag, v_ref, tx, added) VALUES
            (10, 13, 0, 83, 1000000, true)
        ON CONFLICT DO NOTHING;

        -- :db/index datoms (a=14, boolean stored in v_bool)
        -- :db/ident and :db/txInstant are indexed
        INSERT INTO mentat.datoms (e, a, value_type_tag, v_bool, tx, added) VALUES
            (10, 14, 1, true, 1000000, true),
            (50, 14, 1, true, 1000000, true)
        ON CONFLICT DO NOTHING;

        -- Record bootstrap transaction
        INSERT INTO mentat.transactions (tx, tx_instant)
        VALUES (1000000, '1970-01-01T00:00:00Z')
        ON CONFLICT DO NOTHING;
        ",
    )?;

    // Seed the current-state projection with the bootstrap datoms. These are
    // written directly to the datom tables above (bypassing the transact
    // path's maintain_current_projection), so the projection must be seeded
    // here or it will disagree with the log for the built-in attributes.
    // rebuild_current_projection is idempotent and correct-by-construction
    // (latest-tx-wins over the log), so it is safe to call after CREATE
    // EXTENSION's bootstrap and after any re-bootstrap.
    Spi::run("SELECT mentat.rebuild_current_projection(0)")?;

    Ok(())
}
