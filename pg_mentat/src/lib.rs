use pgrx::prelude::*;

pgrx::pg_module_magic!();

// Initialize the mentat schema during CREATE EXTENSION
extension_sql!(
    r#"
    CREATE SCHEMA IF NOT EXISTS mentat;

    -- Define enum types
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

    -- Datoms table partitioned by value_type_tag for partition pruning
    CREATE TABLE IF NOT EXISTS mentat.datoms (
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

        -- Ensure exactly one value column is populated per row
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

    -- Create partitions for each value type
    CREATE TABLE IF NOT EXISTS mentat.datoms_ref PARTITION OF mentat.datoms FOR VALUES IN (0);
    CREATE TABLE IF NOT EXISTS mentat.datoms_bool PARTITION OF mentat.datoms FOR VALUES IN (1);
    CREATE TABLE IF NOT EXISTS mentat.datoms_long PARTITION OF mentat.datoms FOR VALUES IN (2);
    CREATE TABLE IF NOT EXISTS mentat.datoms_double PARTITION OF mentat.datoms FOR VALUES IN (3);
    CREATE TABLE IF NOT EXISTS mentat.datoms_instant PARTITION OF mentat.datoms FOR VALUES IN (4);
    CREATE TABLE IF NOT EXISTS mentat.datoms_text PARTITION OF mentat.datoms FOR VALUES IN (7);
    CREATE TABLE IF NOT EXISTS mentat.datoms_keyword PARTITION OF mentat.datoms FOR VALUES IN (8);
    CREATE TABLE IF NOT EXISTS mentat.datoms_uuid PARTITION OF mentat.datoms FOR VALUES IN (10);
    CREATE TABLE IF NOT EXISTS mentat.datoms_bytes PARTITION OF mentat.datoms FOR VALUES IN (11);
    CREATE TABLE IF NOT EXISTS mentat.datoms_default PARTITION OF mentat.datoms DEFAULT;

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

    -- Core datom indexes (reduced from 22 to 8 for write throughput)
    -- EAVT: Entity-centric lookups (partial on added=TRUE)
    CREATE INDEX IF NOT EXISTS idx_datoms_eavt ON mentat.datoms (e, a, value_type_tag, tx) WHERE added = TRUE;
    -- AEVT: Attribute-centric scans (partial on added=TRUE)
    CREATE INDEX IF NOT EXISTS idx_datoms_aevt ON mentat.datoms (a, e, value_type_tag, tx) WHERE added = TRUE;
    -- TX: Transaction history lookups
    CREATE INDEX IF NOT EXISTS idx_datoms_tx ON mentat.datoms (tx DESC);
    -- VAET: Reverse ref lookups (refs only)
    CREATE INDEX IF NOT EXISTS idx_datoms_vaet ON mentat.datoms (v_ref, a, e, tx) WHERE added = TRUE AND value_type_tag = 0;

    -- Type-specific AVET indexes (only high-frequency types)
    CREATE INDEX IF NOT EXISTS idx_datoms_avet_ref ON mentat.datoms (a, v_ref, e, tx) WHERE added = TRUE AND value_type_tag = 0;
    CREATE INDEX IF NOT EXISTS idx_datoms_avet_long ON mentat.datoms (a, v_long, e, tx) WHERE added = TRUE AND value_type_tag = 2;
    CREATE INDEX IF NOT EXISTS idx_datoms_avet_text ON mentat.datoms (a, v_text, e, tx) WHERE added = TRUE AND value_type_tag = 7;
    CREATE INDEX IF NOT EXISTS idx_datoms_avet_keyword ON mentat.datoms (a, v_keyword, e, tx) WHERE added = TRUE AND value_type_tag = 8;

    -- Configure autovacuum for high-churn temporal workloads
    -- Vacuum at 5% dead tuples (vs default 20%) to prevent index bloat from retractions
    ALTER TABLE mentat.datoms SET (
        autovacuum_vacuum_scale_factor = 0.05,
        autovacuum_analyze_scale_factor = 0.02
    );

    -- Full-text search support table
    CREATE TABLE IF NOT EXISTS mentat.fulltext (
        text_value TEXT NOT NULL,
        search_vector TSVECTOR
    );
    CREATE INDEX IF NOT EXISTS idx_fulltext_search ON mentat.fulltext USING GIN (search_vector);

    -- Trigger to auto-update search vector
    CREATE OR REPLACE FUNCTION mentat.fulltext_update_trigger() RETURNS trigger AS $$
    BEGIN
        NEW.search_vector := to_tsvector('english', NEW.text_value);
        RETURN NEW;
    END; $$ LANGUAGE plpgsql;

    DROP TRIGGER IF EXISTS fulltext_update ON mentat.fulltext;
    CREATE TRIGGER fulltext_update BEFORE INSERT OR UPDATE ON mentat.fulltext
        FOR EACH ROW EXECUTE FUNCTION mentat.fulltext_update_trigger();

    INSERT INTO mentat.partitions (name, start_entid, end_entid, next_entid, allow_excision) VALUES
        ('db.part/db', 0, 10000, 100, FALSE),
        ('db.part/user', 10000, 1000000, 10000, FALSE),
        ('db.part/tx', 1000000, 2000000, 1000001, FALSE)
    ON CONFLICT (name) DO NOTHING;

    -- Sequences for lock-free entity ID allocation (replaces UPDATE-based locking)
    -- CACHE 100 pre-allocates IDs per connection for high concurrency
    CREATE SEQUENCE IF NOT EXISTS mentat.partition_db_seq START WITH 100 CACHE 10;
    CREATE SEQUENCE IF NOT EXISTS mentat.partition_user_seq START WITH 10000 CACHE 100;
    CREATE SEQUENCE IF NOT EXISTS mentat.partition_tx_seq START WITH 1000001 CACHE 100;

    INSERT INTO mentat.transactions (tx, tx_instant)
    VALUES (1000000, '2025-01-01T00:00:00Z')
    ON CONFLICT (tx) DO NOTHING;

    -- PL/pgSQL helper functions for transaction processing
    -- allocate_entid uses sequences for lock-free concurrent ID allocation
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
"#,
    name = "bootstrap_schema",
);

mod cache;
#[cfg(any(test, feature = "pg_test"))]
mod cache_tests;
#[cfg(any(test, feature = "pg_test"))]
mod concurrency_tests;
#[cfg(any(test, feature = "pg_test"))]
mod typed_value_tests;
#[cfg(any(test, feature = "pg_test"))]
mod transact_unit_tests;
#[cfg(any(test, feature = "pg_test"))]
mod query_edge_tests;
#[cfg(any(test, feature = "pg_test"))]
mod pull_tests;
#[cfg(any(test, feature = "pg_test"))]
mod temporal_tests;
#[cfg(any(test, feature = "pg_test"))]
mod security_tests;
#[cfg(any(test, feature = "pg_test"))]
mod error_regression_tests;
#[cfg(any(test, feature = "pg_test"))]
mod schema_operation_tests;
#[cfg(any(test, feature = "pg_test"))]
mod entity_tests;
#[cfg(any(test, feature = "pg_test"))]
mod batch_operation_tests;
#[cfg(any(test, feature = "pg_test"))]
mod property_tests;
#[cfg(any(test, feature = "pg_test"))]
mod value_type_exhaustive_tests;
#[cfg(any(test, feature = "pg_test"))]
mod query_comprehensive_tests;
#[cfg(any(test, feature = "pg_test"))]
mod transaction_comprehensive_tests;
#[cfg(any(test, feature = "pg_test"))]
mod schema_comprehensive_tests;
#[cfg(any(test, feature = "pg_test"))]
mod cardinality_tests;
#[cfg(any(test, feature = "pg_test"))]
mod upsert_tests;
#[cfg(any(test, feature = "pg_test"))]
mod retraction_tests;
#[cfg(any(test, feature = "pg_test"))]
mod tempid_tests;
#[cfg(any(test, feature = "pg_test"))]
mod lookup_ref_tests;
#[cfg(any(test, feature = "pg_test"))]
mod predicate_tests;
#[cfg(any(test, feature = "pg_test"))]
mod aggregate_tests;
#[cfg(any(test, feature = "pg_test"))]
mod find_spec_tests;
#[cfg(any(test, feature = "pg_test"))]
mod input_parameter_tests;
#[cfg(any(test, feature = "pg_test"))]
mod history_tests;
#[cfg(any(test, feature = "pg_test"))]
mod boundary_value_tests;
#[cfg(any(test, feature = "pg_test"))]
mod cross_entity_tests;
#[cfg(any(test, feature = "pg_test"))]
mod stress_scale_tests;
#[cfg(any(test, feature = "pg_test"))]
mod idempotency_tests;
#[cfg(any(test, feature = "pg_test"))]
mod multi_transaction_workflow_tests;
#[cfg(any(test, feature = "pg_test"))]
mod schema_evolution_tests;
#[cfg(any(test, feature = "pg_test"))]
mod parameterized_value_tests;
#[cfg(any(test, feature = "pg_test"))]
mod query_pattern_tests;
#[cfg(any(test, feature = "pg_test"))]
mod transaction_report_tests;
#[cfg(any(test, feature = "pg_test"))]
mod data_integrity_tests;
#[cfg(any(test, feature = "pg_test"))]
mod schema_introspection_tests;
#[cfg(any(test, feature = "pg_test"))]
mod cas_tests;
#[cfg(any(test, feature = "pg_test"))]
mod pull_api_tests;
#[cfg(any(test, feature = "pg_test"))]
mod regression_tests;
#[cfg(any(test, feature = "pg_test"))]
mod bootstrap_tests;
#[cfg(any(test, feature = "pg_test"))]
mod namespace_tests;
#[cfg(any(test, feature = "pg_test"))]
mod mixed_operation_tests;
#[cfg(any(test, feature = "pg_test"))]
mod edge_case_query_tests;
pub mod error;
mod functions;
mod operators;
mod planner;
mod types;

// Export EdnValue at root for internal use
pub use types::edn::EdnValue;

#[pg_schema]
mod mentat {
    use pgrx::prelude::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// EdnValue is a PostgreSQL custom type that wraps Mentat's EDN Value.
    /// Uses CBOR for binary storage and custom EDN text I/O functions.
    #[derive(
        Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PostgresType, PostgresEq,
    )]
    pub struct EdnValue {
        #[serde(serialize_with = "serialize_edn", deserialize_with = "deserialize_edn")]
        pub(crate) inner: edn::Value,
    }

    fn serialize_edn<S>(value: &edn::Value, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let edn_text = format!("{}", value);
        serializer.serialize_str(&edn_text)
    }

    fn deserialize_edn<'de, D>(deserializer: D) -> Result<edn::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let edn_text = String::deserialize(deserializer)?;
        let value_and_span = edn::parse::value(&edn_text).map_err(serde::de::Error::custom)?;
        Ok(value_and_span.without_spans())
    }

    /// Initialize the pg_mentat extension
    #[pg_extern]
    fn initialize_schema() -> Result<(), Box<dyn std::error::Error>> {
        Spi::run(
            r#"
            CREATE TABLE IF NOT EXISTS mentat_datoms (
                e BIGINT NOT NULL,
                a BIGINT NOT NULL,
                v mentat.EdnValue NOT NULL,
                tx BIGINT NOT NULL,
                added BOOLEAN NOT NULL DEFAULT TRUE
            );

            CREATE INDEX IF NOT EXISTS idx_mentat_eavt
                ON mentat_datoms (e, a, v, tx);
            CREATE INDEX IF NOT EXISTS idx_mentat_aevt
                ON mentat_datoms (a, e, v, tx);
            CREATE INDEX IF NOT EXISTS idx_mentat_avet
                ON mentat_datoms (a, v, e, tx);
            CREATE INDEX IF NOT EXISTS idx_mentat_vaet
                ON mentat_datoms (v, a, e, tx);
        "#,
        )?;
        Ok(())
    }

    // Re-export all extension functions into the mentat schema
    #[allow(unused_imports)]
    pub use crate::functions::entity::*;
    #[allow(unused_imports)]
    pub use crate::functions::pull::*;
    #[allow(unused_imports)]
    pub use crate::functions::query::*;
    #[allow(unused_imports)]
    pub use crate::functions::schema::*;
    #[allow(unused_imports)]
    pub use crate::functions::stats::*;
    #[allow(unused_imports)]
    pub use crate::functions::transact::*;
}

#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {
        // Initialize extension for testing
    }

    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![]
    }
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;
    use pgrx::datum::DatumWithOid;

    // ============================================================================
    // Test Helper Functions
    // ============================================================================

    /// Initialize a test database with the pg_mentat schema.
    fn setup_test_db() -> Result<(), Box<dyn std::error::Error>> {
        Spi::run(
            r#"
            CREATE SCHEMA IF NOT EXISTS mentat;

            -- Define enum types
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

            -- Datoms table partitioned by value_type_tag
            CREATE TABLE IF NOT EXISTS mentat.datoms (
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

            CREATE TABLE IF NOT EXISTS mentat.datoms_ref PARTITION OF mentat.datoms FOR VALUES IN (0);
            CREATE TABLE IF NOT EXISTS mentat.datoms_bool PARTITION OF mentat.datoms FOR VALUES IN (1);
            CREATE TABLE IF NOT EXISTS mentat.datoms_long PARTITION OF mentat.datoms FOR VALUES IN (2);
            CREATE TABLE IF NOT EXISTS mentat.datoms_double PARTITION OF mentat.datoms FOR VALUES IN (3);
            CREATE TABLE IF NOT EXISTS mentat.datoms_instant PARTITION OF mentat.datoms FOR VALUES IN (4);
            CREATE TABLE IF NOT EXISTS mentat.datoms_text PARTITION OF mentat.datoms FOR VALUES IN (7);
            CREATE TABLE IF NOT EXISTS mentat.datoms_keyword PARTITION OF mentat.datoms FOR VALUES IN (8);
            CREATE TABLE IF NOT EXISTS mentat.datoms_uuid PARTITION OF mentat.datoms FOR VALUES IN (10);
            CREATE TABLE IF NOT EXISTS mentat.datoms_bytes PARTITION OF mentat.datoms FOR VALUES IN (11);
            CREATE TABLE IF NOT EXISTS mentat.datoms_default PARTITION OF mentat.datoms DEFAULT;

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

            -- Core datom indexes (reduced from 22 to 8 for write throughput)
            CREATE INDEX IF NOT EXISTS idx_datoms_eavt ON mentat.datoms (e, a, value_type_tag, tx) WHERE added = TRUE;
            CREATE INDEX IF NOT EXISTS idx_datoms_aevt ON mentat.datoms (a, e, value_type_tag, tx) WHERE added = TRUE;
            CREATE INDEX IF NOT EXISTS idx_datoms_tx ON mentat.datoms (tx DESC);
            CREATE INDEX IF NOT EXISTS idx_datoms_vaet ON mentat.datoms (v_ref, a, e, tx) WHERE added = TRUE AND value_type_tag = 0;

            -- Type-specific AVET indexes (only high-frequency types)
            CREATE INDEX IF NOT EXISTS idx_datoms_avet_ref ON mentat.datoms (a, v_ref, e, tx) WHERE added = TRUE AND value_type_tag = 0;
            CREATE INDEX IF NOT EXISTS idx_datoms_avet_long ON mentat.datoms (a, v_long, e, tx) WHERE added = TRUE AND value_type_tag = 2;
            CREATE INDEX IF NOT EXISTS idx_datoms_avet_text ON mentat.datoms (a, v_text, e, tx) WHERE added = TRUE AND value_type_tag = 7;
            CREATE INDEX IF NOT EXISTS idx_datoms_avet_keyword ON mentat.datoms (a, v_keyword, e, tx) WHERE added = TRUE AND value_type_tag = 8;

            -- Full-text search support table
            CREATE TABLE IF NOT EXISTS mentat.fulltext (
                text_value TEXT NOT NULL,
                search_vector TSVECTOR
            );
            CREATE INDEX IF NOT EXISTS idx_fulltext_search ON mentat.fulltext USING GIN (search_vector);

            -- Trigger to auto-update search vector
            CREATE OR REPLACE FUNCTION mentat.fulltext_update_trigger() RETURNS trigger AS $$
            BEGIN
                NEW.search_vector := to_tsvector('english', NEW.text_value);
                RETURN NEW;
            END; $$ LANGUAGE plpgsql;

            DROP TRIGGER IF EXISTS fulltext_update ON mentat.fulltext;
            CREATE TRIGGER fulltext_update BEFORE INSERT OR UPDATE ON mentat.fulltext
                FOR EACH ROW EXECUTE FUNCTION mentat.fulltext_update_trigger();

            INSERT INTO mentat.partitions (name, start_entid, end_entid, next_entid, allow_excision) VALUES
                ('db.part/db', 0, 10000, 100, FALSE),
                ('db.part/user', 10000, 1000000, 10000, FALSE),
                ('db.part/tx', 1000000, 2000000, 1000001, FALSE)
            ON CONFLICT (name) DO NOTHING;

            -- Sequences for lock-free entity ID allocation
            CREATE SEQUENCE IF NOT EXISTS mentat.partition_db_seq START WITH 100 CACHE 10;
            CREATE SEQUENCE IF NOT EXISTS mentat.partition_user_seq START WITH 10000 CACHE 100;
            CREATE SEQUENCE IF NOT EXISTS mentat.partition_tx_seq START WITH 1000001 CACHE 100;

            INSERT INTO mentat.transactions (tx, tx_instant)
            VALUES (1000000, '2025-01-01T00:00:00Z')
            ON CONFLICT (tx) DO NOTHING;

            -- PL/pgSQL helper functions for transaction processing
            -- allocate_entid uses sequences for lock-free concurrent ID allocation
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
            "#,
        )?;
        Ok(())
    }

    /// Bootstrap the core Mentat schema.
    fn bootstrap_schema() -> Result<(), Box<dyn std::error::Error>> {
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

    /// Define common person attributes (:person/name, :person/age, :person/parent,
    /// :person/status) via mentat_transact. Must be called after setup_test_db()
    /// and bootstrap_schema().
    fn setup_person_schema() {
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"name-attr\" :db/ident :person/name]
                 [:db/add \"name-attr\" :db/valueType :db.type/string]
                 [:db/add \"name-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"age-attr\" :db/ident :person/age]
                 [:db/add \"age-attr\" :db/valueType :db.type/long]
                 [:db/add \"age-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"parent-attr\" :db/ident :person/parent]
                 [:db/add \"parent-attr\" :db/valueType :db.type/ref]
                 [:db/add \"parent-attr\" :db/cardinality :db.cardinality/many]
                 [:db/add \"status-attr\" :db/ident :person/status]
                 [:db/add \"status-attr\" :db/valueType :db.type/string]
                 [:db/add \"status-attr\" :db/cardinality :db.cardinality/one]]
            '::TEXT)",
        )
        .expect("Failed to setup person schema");
    }

    // ============================================================================
    // EDN Type Tests
    // ============================================================================

    #[pg_test]
    fn test_edn_roundtrip_boolean() {
        let result = Spi::get_one::<String>("SELECT edn_out(edn_in('true'))")
            .expect("Failed to execute query")
            .expect("Query returned NULL");
        assert!(result.contains("true"));
    }

    #[pg_test]
    fn test_edn_roundtrip_integer() {
        let result = Spi::get_one::<String>("SELECT edn_out(edn_in('42'))")
            .expect("Failed to execute query")
            .expect("Query returned NULL");
        assert!(result.contains("42"));
    }

    #[pg_test]
    fn test_edn_roundtrip_string() {
        let result = Spi::get_one::<String>("SELECT edn_out(edn_in('\"hello\"'))")
            .expect("Failed to execute query")
            .expect("Query returned NULL");
        assert!(result.contains("hello"));
    }

    #[pg_test]
    fn test_edn_roundtrip_vector() {
        let result = Spi::get_one::<String>("SELECT edn_out(edn_in('[1 2 3]'))")
            .expect("Failed to execute query")
            .expect("Query returned NULL");
        assert!(result.contains("1"));
    }

    #[pg_test]
    fn test_edn_roundtrip_map() {
        let result = Spi::get_one::<String>("SELECT edn_out(edn_in('{:name \"Alice\" :age 30}'))")
            .expect("Failed to execute query")
            .expect("Query returned NULL");
        assert!(result.contains("Alice"));
    }

    // ============================================================================
    // Query Tests (11 tests)
    // ============================================================================

    #[pg_test]
    fn test_pg_rel() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?x ?ident :where [?x :db/ident ?ident]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        assert!(json.get("columns").is_some(), "Missing columns");
        assert!(json.get("results").is_some(), "Missing results");

        let results = json["results"].as_array().expect("results should be array");

        assert!(
            results.len() >= 10,
            "Expected at least 10 schema idents, got {}",
            results.len()
        );

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            assert_eq!(row_arr.len(), 2, "Expected 2 values per row");
        }
    }

    #[pg_test]
    fn test_pg_failing_scalar() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?x . :where [?x :db/fulltext true]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        assert!(
            json["result"].is_null(),
            "Expected null for failing scalar query"
        );
    }

    #[pg_test]
    fn test_pg_scalar() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?ident . :where [1 :db/ident ?ident]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let keyword = json["result"].as_str().expect("Expected string result");

        assert_eq!(keyword, ":db/ident", "Expected :db/ident");
    }

    #[pg_test]
    fn test_pg_tuple() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find [?ident ?type] :where [1 :db/ident ?ident] [1 :db/valueType ?type]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let tuple = json["result"].as_array().expect("Expected array result");

        assert_eq!(tuple.len(), 2, "Expected 2-tuple");
        assert_eq!(
            tuple[0].as_str().expect("First element should be string"),
            ":db/ident"
        );
    }

    #[pg_test]
    fn test_pg_coll() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find [?ident ...] :where [?e :db/ident ?ident]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let coll = json["result"].as_array().expect("Expected array result");

        assert!(coll.len() >= 10, "Expected at least 10 idents");

        for elem in coll {
            assert!(elem.is_string(), "Collection element should be string");
        }
    }

    #[pg_test]
    fn test_pg_query_with_inputs() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"person1\" :person/name \"Alice\"]
                 [:db/add \"person1\" :person/age 30]]
            '::TEXT)",
        )
        .expect("Transaction failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e :in ?name :where [?e :person/name ?name]]'::TEXT,
                '{\"inputs\": [\"Alice\"]}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 1, "Expected 1 result");
    }

    #[pg_test]
    fn test_pg_multi_clause() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?ident ?type
                  :where
                  [?e :db/ident ?ident]
                  [?e :db/valueType ?type]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(results.len() >= 5, "Expected at least 5 results");

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            assert_eq!(row_arr.len(), 3);
        }
    }

    #[pg_test]
    fn test_pg_query_not() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e
                  :where
                  [?e :db/ident]
                  (not [?e :db/fulltext true])]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(
            results.len() >= 8,
            "Expected at least 8 non-fulltext attributes"
        );
    }

    #[pg_test]
    fn test_pg_query_or() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e
                  :where
                  (or [?e :db/ident :db/ident]
                      [?e :db/ident :db/valueType])]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 2, "Expected exactly 2 results");
    }

    #[pg_test]
    fn test_pg_query_order() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?ident
                  :where [?e :db/ident ?ident]
                  :order (asc ?e)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        let mut prev_id: i64 = 0;
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let current_id = row_arr[0].as_i64().expect("First element should be int");
            assert!(current_id > prev_id, "Results should be ordered ascending");
            prev_id = current_id;
        }
    }

    #[pg_test]
    fn test_pg_query_limit() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?ident
                  :where [?e :db/ident ?ident]
                  :limit 5]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 5, "Expected exactly 5 results due to limit");
    }

    // ============================================================================
    // Time-Travel Tests (7 tests)
    // ============================================================================

    fn setup_temporal_data() -> (i64, i64, i64) {
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"name-attr\" :db/ident :person/name]
                 [:db/add \"name-attr\" :db/valueType :db.type/string]
                 [:db/add \"name-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"age-attr\" :db/ident :person/age]
                 [:db/add \"age-attr\" :db/valueType :db.type/long]
                 [:db/add \"age-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 25]]
            '::TEXT)",
        )
        .expect("Transaction 1 failed")
        .expect("Transaction 1 returned NULL");

        // Extract Alice's entity ID from the tempid map in the tx report
        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("Failed to parse tx report");
        let alice_eid = tx_report["tempids"]["p1"]
            .as_i64()
            .expect("Failed to get Alice's entity ID from tempids");

        let tx1 = Spi::get_one::<i64>("SELECT MAX(tx) FROM mentat.datoms")
            .expect("Failed to get tx1")
            .expect("tx1 is null");

        // Use Alice's actual entity ID to update her age
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :person/age 26]]'::TEXT)",
            alice_eid
        ))
        .expect("Transaction 2 failed");

        let tx2 = Spi::get_one::<i64>(&format!(
            "SELECT MAX(tx) FROM mentat.datoms WHERE tx > {}",
            tx1
        ))
        .expect("Failed to get tx2")
        .expect("tx2 is null");

        Spi::run(&format!(
            "SELECT mentat_transact('
                [[:db/add {} :person/age 27]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 30]]
            '::TEXT)",
            alice_eid
        ))
        .expect("Transaction 3 failed");

        let tx3 = Spi::get_one::<i64>("SELECT MAX(tx) FROM mentat.datoms")
            .expect("Failed to get tx3")
            .expect("tx3 is null");

        (tx1, tx2, tx3)
    }

    #[pg_test]
    fn test_pg_as_of() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        let (tx1, tx2, _tx3) = setup_temporal_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?age .
                 :where
                 [?p :person/name \"Alice\"]
                 [?p :person/age ?age]]
            '::TEXT, '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("as-of tx1 query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let age = json["result"].as_i64().expect("Age should be integer");
        assert_eq!(age, 25, "Age at tx1 should be 25");

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?age .
                 :where
                 [?p :person/name \"Alice\"]
                 [?p :person/age ?age]]'::TEXT, '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx2
        ))
        .expect("as-of tx2 query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let age = json["result"].as_i64().expect("Age should be integer");
        assert_eq!(age, 26, "Age at tx2 should be 26");
    }

    #[pg_test]
    fn test_pg_since() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        let (tx1, _tx2, _tx3) = setup_temporal_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?e ?a ?v ?tx ?added
                 :where
                 [?e ?a ?v ?tx ?added]]'::TEXT, '{{\"since\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("since query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(results.len() > 0, "Should have datoms since tx1");

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let tx = row_arr[3].as_i64().expect("TX should be integer");
            assert!(tx > tx1, "All transactions should be > tx1");
        }
    }

    #[pg_test]
    fn test_pg_history() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        let (_tx1, _tx2, _tx3) = setup_temporal_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?age ?tx ?added
                 :where
                 [?p :person/name \"Alice\"]
                 [?p :person/age ?age ?tx ?added]]'::TEXT, '{\"history\": true}'::jsonb)::TEXT",
        )
        .expect("history query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(
            results.len() >= 3,
            "Should have at least 3 age datoms (assertions)"
        );

        let ages: Vec<i64> = results
            .iter()
            .map(|row| {
                let row_arr = row.as_array().expect("Row should be array");
                row_arr[0].as_i64().expect("Age should be integer")
            })
            .collect();

        assert!(ages.contains(&25), "Should contain age 25");
        assert!(ages.contains(&26), "Should contain age 26");
        assert!(ages.contains(&27), "Should contain age 27");
    }

    #[pg_test]
    fn test_pg_as_of_future_entity() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        let (tx1, _tx2, _tx3) = setup_temporal_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?age .
                 :where
                 [?p :person/name \"Bob\"]
                 [?p :person/age ?age]]'::TEXT, '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("as-of query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        assert!(json["result"].is_null(), "Bob should not exist at tx1");
    }

    #[pg_test]
    fn test_pg_history_retraction() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Insert the initial data
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/status \"active\"]]
            '::TEXT)",
        )
        .expect("Transaction 1 failed");

        // Look up the entity ID for the retraction (tempids don't carry across transactions)
        let entity_id = Spi::get_one::<i64>(
            "SELECT e FROM mentat.datoms d \
             JOIN mentat.idents i ON d.a = i.entid \
             WHERE i.ident = ':person/name' \
             AND d.v_text = 'Alice' \
             AND d.added = true \
             LIMIT 1",
        )
        .expect("Failed to find entity")
        .expect("Entity not found");

        // Retract using the actual entity ID
        let retract_tx = format!(
            "SELECT mentat_transact('[[:db/retract {} :person/status \"active\"]]'::TEXT)",
            entity_id
        );
        Spi::run(&retract_tx).expect("Retraction failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?status ?tx ?added
                 :where
                 [?p :person/name \"Alice\"]
                 [?p :person/status ?status ?tx ?added]]'::TEXT, '{\"history\": true}'::jsonb)::TEXT",
        )
        .expect("history query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 2, "Should have assertion and retraction");

        let has_assertion = results.iter().any(|row| {
            let row_arr = row.as_array().unwrap();
            row_arr[2].as_bool().unwrap() == true
        });

        let has_retraction = results.iter().any(|row| {
            let row_arr = row.as_array().unwrap();
            row_arr[2].as_bool().unwrap() == false
        });

        assert!(has_assertion, "Should have assertion");
        assert!(has_retraction, "Should have retraction");
    }

    #[pg_test]
    fn test_pg_as_of_complex() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        let (tx1, _tx2, tx3) = setup_temporal_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find (count ?p)
                 :where
                 [?p :person/name ?name]]'::TEXT, '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("as-of count query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let count = json["result"].as_i64().expect("Count should be integer");
        assert_eq!(count, 1, "Only Alice should exist at tx1");

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find (count ?p)
                 :where
                 [?p :person/name ?name]]'::TEXT, '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx3
        ))
        .expect("as-of count query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let count = json["result"].as_i64().expect("Count should be integer");
        assert_eq!(count, 2, "Both Alice and Bob should exist at tx3");
    }

    #[pg_test]
    fn test_pg_tx_metadata() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_temporal_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?tx ?instant
                 :where
                 [?tx :db/txInstant ?instant]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("tx metadata query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(results.len() >= 3, "Should have at least 3 transactions");

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            assert!(row_arr[1].is_string(), "Timestamp should be string");
        }
    }

    // ============================================================================
    // Rules Tests (8 tests)
    // ============================================================================

    fn setup_family_schema() {
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"parent\" :db/ident :family/parent]
                 [:db/add \"parent\" :db/valueType :db.type/ref]
                 [:db/add \"parent\" :db/cardinality :db.cardinality/many]
                 [:db/add \"child\" :db/ident :family/child]
                 [:db/add \"child\" :db/valueType :db.type/ref]
                 [:db/add \"child\" :db/cardinality :db.cardinality/many]
                 [:db/add \"name\" :db/ident :person/name]
                 [:db/add \"name\" :db/valueType :db.type/string]
                 [:db/add \"name\" :db/cardinality :db.cardinality/one]]
            '::TEXT)",
        )
        .expect("Failed to create family schema");
    }

    fn setup_family_data() {
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"grandma\" :person/name \"Grandma\"]
                 [:db/add \"mom\" :person/name \"Mom\"]
                 [:db/add \"dad\" :person/name \"Dad\"]
                 [:db/add \"child1\" :person/name \"Alice\"]
                 [:db/add \"child2\" :person/name \"Bob\"]
                 [:db/add \"grandma\" :family/child \"mom\"]
                 [:db/add \"mom\" :family/child \"child1\"]
                 [:db/add \"mom\" :family/child \"child2\"]
                 [:db/add \"dad\" :family/child \"child1\"]
                 [:db/add \"dad\" :family/child \"child2\"]]
            '::TEXT)",
        )
        .expect("Failed to insert family data");
    }

    #[pg_test]
    fn test_pg_simple_rule() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_family_schema();
        setup_family_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?parent-name ?child-name
                 :where
                 [?p :family/child ?c]
                 [?p :person/name ?parent-name]
                 [?c :person/name ?child-name]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(results.len() >= 3, "Expected at least 3 parent-child pairs");
    }

    #[pg_test]
    fn test_pg_recursive_rule() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_family_schema();
        setup_family_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?ancestor-name ?descendant-name
                 :with
                 [[(ancestor ?a ?d)
                   [?a :family/child ?d]]
                  [(ancestor ?a ?d)
                   [?a :family/child ?x]
                   (ancestor ?x ?d)]]
                 :where
                 (ancestor ?anc ?desc)
                 [?anc :person/name ?ancestor-name]
                 [?desc :person/name ?descendant-name]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Recursive query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(
            results.len() >= 2,
            "Expected at least 2 ancestor relationships"
        );

        let has_grandma_to_alice = results.iter().any(|row| {
            let row_arr = row.as_array().unwrap();
            row_arr[0].as_str() == Some("Grandma") && row_arr[1].as_str() == Some("Alice")
        });

        assert!(
            has_grandma_to_alice,
            "Should find Grandma -> Alice relationship"
        );
    }

    #[pg_test]
    fn test_pg_rule_multi_clause() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_family_schema();
        setup_family_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?sib1-name ?sib2-name
                 :where
                 [?p :family/child ?s1]
                 [?p :family/child ?s2]
                 [(< ?s1 ?s2)]
                 [?s1 :person/name ?sib1-name]
                 [?s2 :person/name ?sib2-name]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(results.len() >= 1, "Expected at least 1 sibling pair");
    }

    #[pg_test]
    fn test_pg_rule_with_predicates() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 25]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 30]
                 [:db/add \"p3\" :person/name \"Charlie\"]
                 [:db/add \"p3\" :person/age 35]]
            '::TEXT)",
        )
        .expect("Failed to insert age data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name ?age
                 :where
                 [?p :person/name ?name]
                 [?p :person/age ?age]
                 [(> ?age 28)]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 2, "Expected 2 people over 28");

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let age = row_arr[1].as_i64().expect("Age should be integer");
            assert!(age > 28, "Age should be > 28");
        }
    }

    #[pg_test]
    fn test_pg_rule_negation() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_family_schema();
        setup_family_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where
                 [?p :person/name ?name]
                 (not [?p :family/child _])]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 2, "Expected 2 non-parents");
    }

    #[pg_test]
    fn test_pg_rule_aggregation() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_family_schema();
        setup_family_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?parent-name (count ?child)
                 :where
                 [?p :family/child ?child]
                 [?p :person/name ?parent-name]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(
            results.len() >= 2,
            "Expected at least 2 parents with child counts"
        );

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let count = row_arr[1].as_i64();
            assert!(count.is_some(), "Count should be numeric");
            assert!(count.unwrap() > 0, "Count should be positive");
        }
    }

    #[pg_test]
    fn test_pg_rule_or() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"role-attr\" :db/ident :person/role]
                 [:db/add \"role-attr\" :db/valueType :db.type/string]
                 [:db/add \"role-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/role \"admin\"]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/role \"user\"]
                 [:db/add \"p3\" :person/name \"Charlie\"]
                 [:db/add \"p3\" :person/role \"moderator\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name ?role
                 :where
                 [?p :person/name ?name]
                 [?p :person/role ?role]
                 (or [?p :person/role \"admin\"]
                     [?p :person/role \"moderator\"])]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 2, "Expected 2 results (admin and moderator)");
    }

    // ============================================================================
    // OR Clause Edge Case Tests
    // ============================================================================

    /// OR-only query (no base patterns): each branch independently produces
    /// results that are unioned together with set deduplication.
    #[pg_test]
    fn test_pg_or_only_no_base_patterns() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Query schema attributes using OR with no shared base patterns
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e
                  :where
                  (or [?e :db/ident :db/ident]
                      [?e :db/ident :db/doc])]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");
        assert_eq!(results.len(), 2, "Expected 2 results from OR-only query");
    }

    /// OR with variable bindings: ?name must be consistently bound across
    /// branches so that the UNION columns align.
    #[pg_test]
    fn test_pg_or_variable_binding_consistency() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"role-attr\" :db/ident :person/role]
                 [:db/add \"role-attr\" :db/valueType :db.type/string]
                 [:db/add \"role-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/role \"admin\"]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/role \"user\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        // The shared ?p variable binds consistently across OR branches
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where
                 [?p :person/name ?name]
                 (or [?p :person/role \"admin\"]
                     [?p :person/role \"user\"])]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");
        assert_eq!(
            results.len(),
            2,
            "Expected 2 results with consistent variable binding"
        );

        let names: Vec<&str> = results
            .iter()
            .map(|r| r.as_array().unwrap()[0].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Alice"), "Should contain Alice");
        assert!(names.contains(&"Bob"), "Should contain Bob");
    }

    /// OR with AND branches: (or (and [?e :a1 v1] [?e :a2 v2]) [?e :a3 v3])
    #[pg_test]
    fn test_pg_or_with_and_branches() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"role-attr\" :db/ident :person/role]
                 [:db/add \"role-attr\" :db/valueType :db.type/string]
                 [:db/add \"role-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 25]
                 [:db/add \"p1\" :person/role \"admin\"]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 30]
                 [:db/add \"p2\" :person/role \"user\"]
                 [:db/add \"p3\" :person/name \"Charlie\"]
                 [:db/add \"p3\" :person/age 35]
                 [:db/add \"p3\" :person/role \"moderator\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        // OR with AND: match (admin AND name=Alice) OR (moderator)
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where
                 [?p :person/name ?name]
                 (or (and [?p :person/role \"admin\"]
                          [?p :person/name \"Alice\"])
                     [?p :person/role \"moderator\"])]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");
        assert_eq!(
            results.len(),
            2,
            "Expected 2 results: Alice (admin) and Charlie (moderator)"
        );

        let names: Vec<&str> = results
            .iter()
            .map(|r| r.as_array().unwrap()[0].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Alice"), "Should contain Alice (via and-branch)");
        assert!(
            names.contains(&"Charlie"),
            "Should contain Charlie (via simple branch)"
        );
    }

    /// OR deduplication: when both branches match the same entity, the result
    /// set should contain it only once (Datalog set semantics).
    #[pg_test]
    fn test_pg_or_deduplication() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // :db/ident attribute has entid 1; querying for it by two different
        // values that both resolve to the same entity should deduplicate.
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e
                  :where
                  (or [?e :db/ident :db/ident]
                      [?e :db/ident :db/ident])]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");
        assert_eq!(
            results.len(),
            1,
            "Duplicate results from OR branches should be deduplicated"
        );
    }

    /// OR with multiple find variables: verify column alignment across branches.
    #[pg_test]
    fn test_pg_or_multiple_find_vars() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"status-attr\" :db/ident :person/status]
                 [:db/add \"status-attr\" :db/valueType :db.type/string]
                 [:db/add \"status-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/status \"active\"]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/status \"inactive\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name ?status
                 :where
                 [?p :person/name ?name]
                 [?p :person/status ?status]
                 (or [?p :person/status \"active\"]
                     [?p :person/status \"inactive\"])]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");
        assert_eq!(
            results.len(),
            2,
            "Expected 2 results with multiple find variables"
        );

        // Verify each row has 2 columns with correct types
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            assert_eq!(row_arr.len(), 2, "Each row should have 2 columns");
            assert!(row_arr[0].is_string(), "Name should be a string");
            assert!(row_arr[1].is_string(), "Status should be a string");
        }
    }

    /// OR that matches nothing in one branch: only the matching branch
    /// should contribute results.
    #[pg_test]
    fn test_pg_or_one_branch_empty() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"role-attr\" :db/ident :person/role]
                 [:db/add \"role-attr\" :db/valueType :db.type/string]
                 [:db/add \"role-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/role \"admin\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        // Second branch matches nothing (no "superadmin" role exists)
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where
                 [?p :person/name ?name]
                 (or [?p :person/role \"admin\"]
                     [?p :person/role \"superadmin\"])]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");
        assert_eq!(
            results.len(),
            1,
            "Only the matching branch should produce results"
        );
        assert_eq!(
            results[0].as_array().unwrap()[0].as_str().unwrap(),
            "Alice"
        );
    }

    /// Test that string predicates with quotes are properly escaped (SQL injection prevention).
    #[pg_test]
    fn test_pg_predicate_string_with_quotes() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"O''Brien\"]
                 [:db/add \"p2\" :person/name \"Alice\"]
                 [:db/add \"p3\" :person/name \"Bob\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        // Test that string predicate with quotes works correctly
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where
                 [?p :person/name ?name]
                 [(= ?name \"O''Brien\")]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");
        assert_eq!(results.len(), 1, "Should find O'Brien");
        assert_eq!(
            results[0].as_array().unwrap()[0].as_str().unwrap(),
            "O'Brien",
            "Name with quote should match correctly"
        );
    }

    #[pg_test]
    fn test_pg_rule_bind() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 25]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 30]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name ?double-age
                 :where
                 [?p :person/name ?name]
                 [?p :person/age ?age]
                 [(* ?age 2) ?double-age]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 2, "Expected 2 results");

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let double_age = row_arr[1].as_i64().expect("Double age should be integer");
            assert!(double_age >= 50, "Doubled age should be at least 50");
        }
    }

    // ============================================================================
    // Full-Text Search Tests (7 tests)
    // ============================================================================

    fn setup_fts_schema() {
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"person-name\" :db/ident :person/name]
                 [:db/add \"person-name\" :db/valueType :db.type/string]
                 [:db/add \"person-name\" :db/cardinality :db.cardinality/one]
                 [:db/add \"person-name\" :db/fulltext true]
                 [:db/add \"person-name\" :db/index true]
                 [:db/add \"article-content\" :db/ident :article/content]
                 [:db/add \"article-content\" :db/valueType :db.type/string]
                 [:db/add \"article-content\" :db/cardinality :db.cardinality/one]
                 [:db/add \"article-content\" :db/fulltext true]
                 [:db/add \"article-content\" :db/index true]]
            '::TEXT)",
        )
        .expect("Failed to create FTS schema");
    }

    #[pg_test]
    fn test_pg_fulltext_basic() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice Johnson\"]
                 [:db/add \"p2\" :person/name \"Bob Smith\"]
                 [:db/add \"p3\" :person/name \"Alice Smith\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?name ?score
                  :where
                  [(fulltext $ :person/name \"Alice\") [[?e ?name _ ?score]]]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("FTS query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 2, "Expected 2 results for 'Alice'");

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let name = row_arr[1].as_str().expect("Name should be string");
            assert!(name.contains("Alice"), "Result should contain 'Alice'");

            let score = row_arr[2].as_f64();
            assert!(score.is_some(), "Score should be numeric");
        }
    }

    #[pg_test]
    fn test_pg_fulltext_multi_term() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"a1\" :article/content \"The quick brown fox jumps over the lazy dog\"]
                 [:db/add \"a2\" :article/content \"A quick study of foxes in the wild\"]
                 [:db/add \"a3\" :article/content \"Dogs are better than cats\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?content
                  :where
                  [(fulltext $ :article/content \"quick fox\") [[?e ?content _ _]]]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("FTS query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(results.len() >= 1, "Expected at least 1 result");

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let content = row_arr[1].as_str().expect("Content should be string");
            assert!(
                content.contains("quick") || content.contains("fox"),
                "Result should contain 'quick' or 'fox'"
            );
        }
    }

    #[pg_test]
    fn test_pg_fulltext_non_fts_attribute() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?val
                  :where
                  [(fulltext $ :db/ident \"test\") [[?e ?val _ _]]]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query should succeed but return no results");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(
            results.len(),
            0,
            "Expected no results for non-FTS attribute"
        );
    }

    #[pg_test]
    fn test_pg_fulltext_scoring() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p2\" :person/name \"Alice Alice Alice\"]
                 [:db/add \"p3\" :person/name \"Alice and Bob\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?name ?score
                  :where
                  [(fulltext $ :person/name \"Alice\") [[?e ?name _ ?score]]]
                  :order (desc ?score)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("FTS query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 3, "Expected 3 results");

        let mut prev_score = f64::INFINITY;
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let score = row_arr[2].as_f64().expect("Score should be numeric");
            assert!(score <= prev_score, "Scores should be descending");
            prev_score = score;
        }
    }

    #[pg_test]
    fn test_pg_fulltext_special_chars() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"a1\" :article/content \"Hello, World! This is a test.\"]
                 [:db/add \"a2\" :article/content \"Testing: one-two-three\"]
                 [:db/add \"a3\" :article/content \"C++ programming\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?content
                  :where
                  [(fulltext $ :article/content \"test\") [[?e ?content _ _]]]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("FTS query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert!(results.len() >= 1, "Expected at least 1 result");
    }

    #[pg_test]
    fn test_pg_fulltext_phrase() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"a1\" :article/content \"quick brown fox\"]
                 [:db/add \"a2\" :article/content \"brown quick fox\"]
                 [:db/add \"a3\" :article/content \"the quick brown fox jumps\"]]
            '::TEXT)",
        )
        .expect("Failed to insert test data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?content
                  :where
                  [(fulltext $ :article/content \"\\\"quick brown\\\"\") [[?e ?content _ _]]]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("FTS query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let content = row_arr[1].as_str().expect("Content should be string");
            assert!(
                content.contains("quick brown"),
                "Should contain exact phrase"
            );
        }
    }

    #[pg_test]
    fn test_pg_fulltext_empty_query() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?content
                  :where
                  [(fulltext $ :article/content \"\") [[?e ?content _ _]]]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Empty query should succeed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 0, "Empty query should return no results");
    }

    // ============================================================================
    // Retract Entity Test
    // ============================================================================

    #[pg_test]
    fn test_db_retract_entity() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema
        let schema_tx = r#"[
            {:db/ident :person/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
            {:db/ident :person/age
             :db/valueType :db.type/long
             :db/cardinality :db.cardinality/one}
            {:db/ident :person/email
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Create entity with multiple attributes
        let data_tx = r#"[
            {:db/id "alice"
             :person/name "Alice"
             :person/age 30
             :person/email "alice@example.com"}
        ]"#;

        let tx_result =
            Spi::get_one_with_args::<String>("SELECT mentat_transact($1)", &[DatumWithOid::from(data_tx)])
                .expect("Data transaction failed")
                .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse transaction result");
        let tempids = tx_json["tempids"].as_object().expect("Missing tempids");
        let alice_eid = tempids["alice"].as_i64().expect("Missing alice tempid");

        // Verify entity exists with all attributes
        let query_before = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?name ?age ?email
                      :where
                      [?e :person/name ?name]
                      [?e :person/age ?age]
                      [?e :person/email ?email]
                      [(= ?e {})]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_before_json: serde_json::Value =
            serde_json::from_str(&query_before).expect("Failed to parse query result");
        let results_before = query_before_json["results"]
            .as_array()
            .expect("Expected results array");

        assert_eq!(
            results_before.len(),
            1,
            "Expected one result before retraction"
        );

        // Retract the entire entity
        let retract_tx = format!(r#"[[:db/retractEntity {}]]"#, alice_eid);

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(retract_tx.as_str())],
        )
        .expect("Retract entity transaction failed");

        // Verify entity no longer has any attributes
        let query_after = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?name
                      :where
                      [?e :person/name ?name]
                      [(= ?e {})]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_after_json: serde_json::Value =
            serde_json::from_str(&query_after).expect("Failed to parse query result");
        let results_after = query_after_json["results"]
            .as_array()
            .expect("Expected results array");

        assert_eq!(
            results_after.len(),
            0,
            "Expected no results after retractEntity"
        );

        // Verify retractions are recorded in history
        let history_query = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find (count ?a)
                      :where
                      [?e ?a _ _ false]
                      [(= ?e {})]]'::TEXT,
                    '{{\"history\": true}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("History query failed")
        .expect("History query returned NULL");

        let history_json: serde_json::Value =
            serde_json::from_str(&history_query).expect("Failed to parse history result");
        let retraction_count = history_json["result"]
            .as_i64()
            .expect("Expected retraction count");

        assert_eq!(
            retraction_count, 3,
            "Expected 3 retractions (name, age, email)"
        );
    }

    // ============================================================================
    // Type Tag Consistency Test (Critical Bug Fix)
    // ============================================================================

    #[pg_test]
    fn test_ref_type_tag_consistency() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with ref attribute
        let schema_tx = r#"[
            {:db/ident :person/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
            {:db/ident :person/friend
             :db/valueType :db.type/ref
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Transact two entities where one references the other
        let data_tx = r#"[
            {:db/id "alice"
             :person/name "Alice"}
            {:db/id "bob"
             :person/name "Bob"
             :person/friend "alice"}
        ]"#;

        let tx_result =
            Spi::get_one_with_args::<String>("SELECT mentat_transact($1)", &[DatumWithOid::from(data_tx)])
                .expect("Data transaction failed")
                .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse transaction result");
        let tempids = tx_json["tempids"].as_object().expect("Missing tempids");
        let alice_eid = tempids["alice"].as_i64().expect("Missing alice tempid");
        let bob_eid = tempids["bob"].as_i64().expect("Missing bob tempid");

        // Test 1: Query the ref value - should return alice's entity ID
        let query_result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?friend :where [?e :person/name \"Bob\"] [?e :person/friend ?friend]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let query_results = query_json["results"]
            .as_array()
            .expect("Expected results array");

        assert_eq!(query_results.len(), 1, "Expected exactly one result");
        let friend_eid = query_results[0][0]
            .as_i64()
            .expect("Friend should be integer");
        assert_eq!(
            friend_eid, alice_eid,
            "Query should return Alice's entity ID as Bob's friend"
        );

        // Test 2: Pull Bob's entity - should include :person/friend with correct entity ID
        let pull_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[* {{:person/friend [*]}}]', {})",
            bob_eid
        ))
        .expect("Pull failed")
        .expect("Pull returned NULL");

        let pull_json: serde_json::Value =
            serde_json::from_str(&pull_result).expect("Failed to parse pull result");

        // Verify Bob's data
        assert_eq!(
            pull_json[":db/id"].as_i64(),
            Some(bob_eid),
            "Pull should return Bob's entity ID"
        );
        assert_eq!(
            pull_json[":person/name"].as_str(),
            Some("Bob"),
            "Pull should return Bob's name"
        );

        // Verify the ref attribute is correctly decoded (this is the critical test!)
        // With map spec [* {:person/friend [*]}], the ref is followed and sub-pulled
        let friend_obj = pull_json[":person/friend"]
            .as_object()
            .expect(":person/friend should be decoded as object via map spec");
        let friend_ref = friend_obj[":db/id"]
            .as_i64()
            .expect(":person/friend :db/id should be an integer entity ID");
        assert_eq!(
            friend_ref, alice_eid,
            "Pull should decode ref type correctly with type tag 0 (not tag 5)"
        );

        // Test 3: Verify ref is stored with correct type tag in database
        let type_tag_result = Spi::get_one_with_args::<i16>(
            "SELECT value_type_tag FROM mentat.datoms
             WHERE e = $1 AND a = (SELECT entid FROM mentat.schema WHERE ident = ':person/friend')
             AND added = true",
            &[DatumWithOid::from(bob_eid)],
        )
        .expect("Type tag query failed")
        .expect("No type tag found");

        assert_eq!(
            type_tag_result, 0,
            "Ref values should be stored with type tag 0 in database"
        );
    }

    #[pg_test]
    fn test_ref_round_trip_entity() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with ref attribute
        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(
                r#"[
                {:db/ident :item/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/ident :item/link
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/one}
            ]"#,
            )],
        )
        .expect("Schema transaction failed");

        // Transact entities with a ref between them
        let tx_result = Spi::get_one_with_args::<String>(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(
                r#"[
                {:db/id "target" :item/name "Target"}
                {:db/id "source" :item/name "Source" :item/link "target"}
            ]"#,
            )],
        )
        .expect("Data transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let tempids = tx_json["tempids"].as_object().expect("Missing tempids");
        let target_eid = tempids["target"].as_i64().expect("Missing target tempid");
        let source_eid = tempids["source"].as_i64().expect("Missing source tempid");

        // Test: mentat_entity should correctly decode ref (type_tag=0) as an integer
        let entity_result = Spi::get_one::<String>(
            &format!("SELECT mentat_entity({})", source_eid),
        )
        .expect("Entity query failed")
        .expect("Entity returned NULL");

        let entity_json: serde_json::Value =
            serde_json::from_str(&entity_result).expect("Failed to parse entity result");

        assert_eq!(
            entity_json[":item/name"].as_str(),
            Some("Source"),
            "Entity should return the name"
        );
        let link_ref = entity_json[":item/link"]
            .as_i64()
            .expect(":item/link should be decoded as integer entity ID (type_tag=0)");
        assert_eq!(
            link_ref, target_eid,
            "Entity should decode ref correctly with type tag 0"
        );
    }

    // ============================================================================
    // Lookup Ref Tests
    // ============================================================================

    #[pg_test]
    fn test_lookup_ref_in_transaction() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with a unique email attribute
        let schema_tx = r#"[
            {:db/ident :person/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
            {:db/ident :person/email
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one
             :db/unique :db.unique/identity}
            {:db/ident :person/age
             :db/valueType :db.type/long
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Create an entity with a unique email
        let data_tx = r#"[
            {:db/id "alice"
             :person/name "Alice"
             :person/email "alice@example.com"
             :person/age 25}
        ]"#;

        let tx_result =
            Spi::get_one_with_args::<String>("SELECT mentat_transact($1)", &[DatumWithOid::from(data_tx)])
                .expect("Data transaction failed")
                .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // Use lookup ref to update Alice's age via her unique email
        let update_tx = r#"[
            [:db/add [:person/email "alice@example.com"] :person/age 30]
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(update_tx)],
        )
        .expect("Lookup ref transaction failed");

        // Verify the update happened on the correct entity
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?age .
                      :where
                      [{} :person/age ?age]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let age = query_json["result"]
            .as_i64()
            .expect("Age should be integer");

        assert_eq!(age, 30, "Lookup ref should have updated Alice's age to 30");
    }

    #[pg_test]
    fn test_lookup_ref_in_map_entity() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with unique email attribute
        let schema_tx = r#"[
            {:db/ident :person/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
            {:db/ident :person/email
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one
             :db/unique :db.unique/identity}
            {:db/ident :person/age
             :db/valueType :db.type/long
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Create an entity with a unique email
        let data_tx = r#"[
            {:db/id "alice"
             :person/name "Alice"
             :person/email "alice@example.com"
             :person/age 25}
        ]"#;

        let tx_result =
            Spi::get_one_with_args::<String>("SELECT mentat_transact($1)", &[DatumWithOid::from(data_tx)])
                .expect("Data transaction failed")
                .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // Use lookup ref in map entity form with :db/id
        let update_tx = r#"[
            {:db/id [:person/email "alice@example.com"]
             :person/age 31}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(update_tx)],
        )
        .expect("Map-form lookup ref transaction failed");

        // Verify the update happened on the correct entity
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?age .
                      :where
                      [{} :person/age ?age]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let age = query_json["result"]
            .as_i64()
            .expect("Age should be integer");

        assert_eq!(age, 31, "Map-form lookup ref should have updated Alice's age to 31");
    }

    #[pg_test]
    fn test_lookup_ref_as_ref_value() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with unique email and ref attribute
        let schema_tx = r#"[
            {:db/ident :person/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
            {:db/ident :person/email
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one
             :db/unique :db.unique/identity}
            {:db/ident :person/friend
             :db/valueType :db.type/ref
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Create Alice
        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(r#"[
                {:db/id "alice"
                 :person/name "Alice"
                 :person/email "alice@example.com"}
            ]"#)],
        )
        .expect("Alice transaction failed");

        // Create Bob with :person/friend pointing to Alice via lookup ref
        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(r#"[
                {:db/id "bob"
                 :person/name "Bob"
                 :person/email "bob@example.com"
                 :person/friend [:person/email "alice@example.com"]}
            ]"#)],
        )
        .expect("Bob transaction with lookup ref as ref value failed");

        // Verify Bob's friend is Alice by querying for Alice's name via the ref
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?friend-name .
                  :where [?bob :person/name \"Bob\"]
                         [?bob :person/friend ?friend]
                         [?friend :person/name ?friend-name]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let json: serde_json::Value =
            serde_json::from_str(&result).expect("Failed to parse query result");
        let friend_name = json["result"].as_str().expect("Expected string result");

        assert_eq!(
            friend_name, "Alice",
            "Lookup ref as ref value should resolve to Alice"
        );
    }

    #[pg_test]
    fn test_lookup_ref_with_unique_value() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with :db.unique/value (not identity)
        let schema_tx = r#"[
            {:db/ident :product/sku
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one
             :db/unique :db.unique/value}
            {:db/ident :product/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Create a product
        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(r#"[
                {:db/id "widget"
                 :product/sku "WIDGET-001"
                 :product/name "Widget"}
            ]"#)],
        )
        .expect("Product transaction failed");

        // Use lookup ref with :db.unique/value attribute to update the product name
        let update_tx = r#"[
            [:db/add [:product/sku "WIDGET-001"] :product/name "Super Widget"]
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(update_tx)],
        )
        .expect("Lookup ref with unique/value should succeed");

        // Verify the update
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name .
                  :where [?p :product/sku \"WIDGET-001\"]
                         [?p :product/name ?name]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let json: serde_json::Value =
            serde_json::from_str(&result).expect("Failed to parse query result");
        let name = json["result"].as_str().expect("Expected string result");

        assert_eq!(
            name, "Super Widget",
            "Lookup ref with :db.unique/value should resolve correctly"
        );
    }

    #[pg_test]
    fn test_lookup_ref_not_found() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with unique email
        let schema_tx = r#"[
            {:db/ident :person/email
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one
             :db/unique :db.unique/identity}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Try to use lookup ref for non-existent entity - should fail
        let bad_tx = r#"[
            [:db/add [:person/email "nobody@example.com"] :person/email "new@example.com"]
        ]"#;

        let result = Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(bad_tx)],
        );

        assert!(
            result.is_err(),
            "Lookup ref for non-existent entity should fail"
        );
    }

    #[pg_test]
    fn test_lookup_ref_non_unique_attribute_fails() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema WITHOUT unique constraint
        let schema_tx = r#"[
            {:db/ident :person/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Create an entity
        Spi::run("SELECT mentat_transact('[[:db/add \"p1\" :person/name \"Alice\"]]'::TEXT)")
            .expect("Data transaction failed");

        // Try to use lookup ref with non-unique attribute - should fail
        let bad_tx = r#"[
            [:db/add [:person/name "Alice"] :person/name "Bob"]
        ]"#;

        let result = Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(bad_tx)],
        );

        assert!(
            result.is_err(),
            "Lookup ref with non-unique attribute should fail"
        );
    }

    // ============================================================================
    // Lookup Ref in Query :in Bindings Tests
    // ============================================================================

    #[pg_test]
    fn test_lookup_ref_in_query_entity_input() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with a unique email attribute
        let schema_tx = r#"[
            {:db/ident :person/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
            {:db/ident :person/email
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one
             :db/unique :db.unique/identity}
            {:db/ident :person/age
             :db/valueType :db.type/long
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Create an entity
        let data_tx = r#"[
            {:db/id "alice"
             :person/name "Alice"
             :person/email "alice@example.com"
             :person/age 30}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(data_tx)],
        )
        .expect("Data transaction failed");

        // Use lookup ref as :in binding for entity position
        // Query: find the name of the person with email "alice@example.com"
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name .
                  :in ?person
                  :where [?person :person/name ?name]]'::TEXT,
                '{\"inputs\": [[ \":person/email\", \"alice@example.com\"]]}'::jsonb
            )::TEXT",
        )
        .expect("Query with lookup ref input failed")
        .expect("Query returned NULL");

        let json: serde_json::Value =
            serde_json::from_str(&result).expect("Failed to parse query result");
        let name = json["result"].as_str().expect("Expected string result");

        assert_eq!(
            name, "Alice",
            "Lookup ref in :in binding should resolve to the correct entity"
        );
    }

    #[pg_test]
    fn test_lookup_ref_in_query_value_input() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with unique email and ref attribute
        let schema_tx = r#"[
            {:db/ident :person/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
            {:db/ident :person/email
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one
             :db/unique :db.unique/identity}
            {:db/ident :person/friend
             :db/valueType :db.type/ref
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Create two entities: Alice and Bob, with Bob being Alice's friend
        let data_tx = r#"[
            {:db/id "alice"
             :person/name "Alice"
             :person/email "alice@example.com"}
            {:db/id "bob"
             :person/name "Bob"
             :person/email "bob@example.com"
             :person/friend "alice"}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(data_tx)],
        )
        .expect("Data transaction failed");

        // Use lookup ref in value position: find who has Alice as a friend
        // The :in variable ?alice binds to a value-position (ref type) via lookup ref
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name .
                  :in ?alice
                  :where [?e :person/friend ?alice]
                         [?e :person/name ?name]]'::TEXT,
                '{\"inputs\": [[ \":person/email\", \"alice@example.com\"]]}'::jsonb
            )::TEXT",
        )
        .expect("Query with lookup ref value input failed")
        .expect("Query returned NULL");

        let json: serde_json::Value =
            serde_json::from_str(&result).expect("Failed to parse query result");
        let name = json["result"].as_str().expect("Expected string result");

        assert_eq!(
            name, "Bob",
            "Lookup ref in value-position :in binding should find referencing entity"
        );
    }

    // ============================================================================
    // Transaction Isolation Tests (CRITICAL FIX - Marco Slot Review)
    // ============================================================================

    #[pg_test]
    fn test_transaction_rollback_on_error() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema
        let schema_result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/ident :person/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            ]')::TEXT"
        );
        assert!(schema_result.is_ok(), "Schema transaction should succeed");

        // Count initial datoms
        let initial_count =
            Spi::get_one::<i64>("SELECT COUNT(*)::BIGINT FROM mentat.datoms WHERE added = true")
                .expect("Count query failed")
                .expect("Count returned NULL");

        // Attempt transaction with invalid data (type mismatch on age)
        // This should ROLLBACK completely, leaving no partial data
        let bad_tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"alice\" :person/name \"Alice\" :person/age 30}
                {:db/id \"bob\" :person/name \"Bob\" :person/age \"thirty\"}
            ]')::TEXT",
        );

        // Transaction should fail due to type mismatch
        assert!(
            bad_tx_result.is_err(),
            "Transaction with invalid data type should fail"
        );

        // CRITICAL: Verify NO datoms were inserted (rollback worked)
        let final_count =
            Spi::get_one::<i64>("SELECT COUNT(*)::BIGINT FROM mentat.datoms WHERE added = true")
                .expect("Count query failed")
                .expect("Count returned NULL");

        assert_eq!(
            initial_count, final_count,
            "Datom count unchanged after failed transaction proves ROLLBACK worked"
        );

        // Verify Alice was not partially inserted
        let alice_check = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find (count ?e) . :where [?e :person/name \"Alice\"]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let alice_json: serde_json::Value =
            serde_json::from_str(&alice_check).expect("Failed to parse query result");
        let alice_count = alice_json["result"].as_i64().unwrap_or(0);

        assert_eq!(alice_count, 0, "Alice should not exist after rollback");
    }

    #[pg_test]
    fn test_transaction_commits_on_success() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema
        Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]')::TEXT"
        ).expect("Schema transaction should succeed");

        // Valid transaction should commit fully
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"alice\" :person/name \"Alice\"}
                {:db/id \"bob\" :person/name \"Bob\"}
            ]')::TEXT",
        )
        .expect("Transaction should succeed")
        .expect("Transaction returned NULL");

        // Verify transaction report
        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("Failed to parse transaction report");
        assert!(tx_report["tx-id"].is_number(), "Should have tx-id");

        // Verify both entities committed
        let count_result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find (count ?e) . :where [?e :person/name]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let count_json: serde_json::Value =
            serde_json::from_str(&count_result).expect("Failed to parse count result");
        let count = count_json["result"].as_i64().expect("Expected count");

        assert_eq!(count, 2, "Both entities should be committed");
    }

    // ============================================================================
    // :db/retract Comprehensive Tests
    // ============================================================================

    #[pg_test]
    fn test_db_retract_specific_value() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Step 1: Add datoms with :db/add
        let tx1_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Bob\"]
                 [:db/add \"p1\" :person/age 25]
                 [:db/add \"p1\" :person/status \"active\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction 1 failed")
        .expect("Transaction 1 returned NULL");

        let tx1_json: serde_json::Value =
            serde_json::from_str(&tx1_result).expect("Failed to parse tx1 result");
        let bob_eid = tx1_json["tempids"]["p1"]
            .as_i64()
            .expect("Missing p1 tempid");

        // Verify all attributes are present before retraction
        let query_before = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?status
                      :where
                      [{} :person/status ?status]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            bob_eid
        ))
        .expect("Query before retraction failed")
        .expect("Query returned NULL");

        let before_json: serde_json::Value =
            serde_json::from_str(&query_before).expect("Failed to parse");
        let results_before = before_json["results"].as_array().expect("Expected array");
        assert_eq!(
            results_before.len(),
            1,
            "Should find status before retraction"
        );
        assert_eq!(results_before[0][0].as_str().unwrap(), "active");

        // Step 2: Retract a specific value with :db/retract
        let retract_tx = format!(
            "SELECT mentat_transact('[[:db/retract {} :person/status \"active\"]]'::TEXT)::TEXT",
            bob_eid
        );
        Spi::run(&retract_tx).expect("Retraction failed");

        // Step 3: Normal query should NOT find the retracted datom
        let query_after = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?status
                      :where
                      [{} :person/status ?status]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            bob_eid
        ))
        .expect("Query after retraction failed")
        .expect("Query returned NULL");

        let after_json: serde_json::Value =
            serde_json::from_str(&query_after).expect("Failed to parse");
        let results_after = after_json["results"].as_array().expect("Expected array");
        assert_eq!(
            results_after.len(),
            0,
            "Should NOT find status after retraction"
        );

        // Non-retracted attributes should still be present
        let name_query = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?name
                      :where
                      [{} :person/name ?name]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            bob_eid
        ))
        .expect("Name query failed")
        .expect("Query returned NULL");

        let name_json: serde_json::Value =
            serde_json::from_str(&name_query).expect("Failed to parse");
        let name_results = name_json["results"].as_array().expect("Expected array");
        assert_eq!(
            name_results.len(),
            1,
            "Non-retracted name should still be present"
        );
        assert_eq!(name_results[0][0].as_str().unwrap(), "Bob");

        // Step 4: History query should show both assertion and retraction
        let history_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?status ?tx ?added
                      :where
                      [{} :person/status ?status ?tx ?added]]'::TEXT,
                    '{{\"history\": true}}' ::jsonb
                )::TEXT",
            bob_eid
        ))
        .expect("History query failed")
        .expect("History query returned NULL");

        let history_json: serde_json::Value =
            serde_json::from_str(&history_result).expect("Failed to parse");
        let history_results = history_json["results"].as_array().expect("Expected array");

        assert_eq!(
            history_results.len(),
            2,
            "History should contain both assertion and retraction"
        );

        let has_assertion = history_results
            .iter()
            .any(|row| row.as_array().unwrap()[2].as_bool().unwrap() == true);
        let has_retraction = history_results
            .iter()
            .any(|row| row.as_array().unwrap()[2].as_bool().unwrap() == false);

        assert!(
            has_assertion,
            "History should include the assertion (added=true)"
        );
        assert!(
            has_retraction,
            "History should include the retraction (added=false)"
        );
    }

    // ============================================================================
    // Cardinality-Many Tests
    // ============================================================================

    /// Helper: define a cardinality-many string attribute :person/tag
    fn setup_cardinality_many_schema() {
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"name-attr\" :db/ident :person/name]
                 [:db/add \"name-attr\" :db/valueType :db.type/string]
                 [:db/add \"name-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"tag-attr\" :db/ident :person/tag]
                 [:db/add \"tag-attr\" :db/valueType :db.type/string]
                 [:db/add \"tag-attr\" :db/cardinality :db.cardinality/many]
                 [:db/add \"friend-attr\" :db/ident :person/friends]
                 [:db/add \"friend-attr\" :db/valueType :db.type/ref]
                 [:db/add \"friend-attr\" :db/cardinality :db.cardinality/many]]
            '::TEXT)",
        )
        .expect("Failed to setup cardinality-many schema");
    }

    #[pg_test]
    fn test_cardinality_many_multiple_values() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_cardinality_many_schema();

        // Add multiple tags to one entity
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/tag \"friendly\"]
                 [:db/add \"alice\" :person/tag \"smart\"]
                 [:db/add \"alice\" :person/tag \"tall\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // Query should return all three tags
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?tag
                      :where
                      [{} :person/tag ?tag]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let results = query_json["results"]
            .as_array()
            .expect("Expected results array");

        assert_eq!(
            results.len(),
            3,
            "Should have 3 tags for cardinality-many attribute"
        );

        let tags: Vec<&str> = results
            .iter()
            .map(|r| r.as_array().unwrap()[0].as_str().unwrap())
            .collect();

        assert!(tags.contains(&"friendly"), "Should contain 'friendly'");
        assert!(tags.contains(&"smart"), "Should contain 'smart'");
        assert!(tags.contains(&"tall"), "Should contain 'tall'");
    }

    #[pg_test]
    fn test_cardinality_many_across_transactions() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_cardinality_many_schema();

        // First transaction: add initial tags
        let tx1_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/tag \"friendly\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction 1 failed")
        .expect("Transaction 1 returned NULL");

        let tx1_json: serde_json::Value =
            serde_json::from_str(&tx1_result).expect("Failed to parse tx1 result");
        let alice_eid = tx1_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // Second transaction: add more tags using entity ID
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :person/tag \"smart\"]]'::TEXT)",
            alice_eid
        ))
        .expect("Transaction 2 failed");

        // Third transaction: add yet another tag
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :person/tag \"tall\"]]'::TEXT)",
            alice_eid
        ))
        .expect("Transaction 3 failed");

        // Query should return all three tags from different transactions
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?tag
                      :where
                      [{} :person/tag ?tag]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let results = query_json["results"]
            .as_array()
            .expect("Expected results array");

        assert_eq!(
            results.len(),
            3,
            "Should have 3 tags across multiple transactions"
        );

        let tags: Vec<&str> = results
            .iter()
            .map(|r| r.as_array().unwrap()[0].as_str().unwrap())
            .collect();

        assert!(tags.contains(&"friendly"), "Should contain 'friendly'");
        assert!(tags.contains(&"smart"), "Should contain 'smart'");
        assert!(tags.contains(&"tall"), "Should contain 'tall'");
    }

    #[pg_test]
    fn test_cardinality_many_retract_single_value() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_cardinality_many_schema();

        // Add multiple tags
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/tag \"friendly\"]
                 [:db/add \"alice\" :person/tag \"smart\"]
                 [:db/add \"alice\" :person/tag \"tall\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // Retract just one value
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :person/tag \"smart\"]]'::TEXT)",
            alice_eid
        ))
        .expect("Retraction failed");

        // Query should return only the two remaining tags
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?tag
                      :where
                      [{} :person/tag ?tag]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let results = query_json["results"]
            .as_array()
            .expect("Expected results array");

        assert_eq!(results.len(), 2, "Should have 2 tags after retracting one");

        let tags: Vec<&str> = results
            .iter()
            .map(|r| r.as_array().unwrap()[0].as_str().unwrap())
            .collect();

        assert!(
            tags.contains(&"friendly"),
            "Should still contain 'friendly'"
        );
        assert!(tags.contains(&"tall"), "Should still contain 'tall'");
        assert!(
            !tags.contains(&"smart"),
            "Should NOT contain retracted 'smart'"
        );
    }

    #[pg_test]
    fn test_cardinality_many_idempotent_assertion() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_cardinality_many_schema();

        // Add a tag
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/tag \"friendly\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // Assert the same value again -- should be idempotent (no duplicate)
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :person/tag \"friendly\"]]'::TEXT)",
            alice_eid
        ))
        .expect("Idempotent assertion failed");

        // Should still have exactly one "friendly" tag, not two
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?tag
                      :where
                      [{} :person/tag ?tag]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let results = query_json["results"]
            .as_array()
            .expect("Expected results array");

        assert_eq!(
            results.len(),
            1,
            "Idempotent assertion should not create duplicates"
        );
    }

    #[pg_test]
    fn test_cardinality_many_ref_type() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_cardinality_many_schema();

        // Create entities and add multiple ref values (friends)
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"bob\" :person/name \"Bob\"]
                 [:db/add \"charlie\" :person/name \"Charlie\"]
                 [:db/add \"alice\" :person/friends \"bob\"]
                 [:db/add \"alice\" :person/friends \"charlie\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // Query all friends of Alice
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?friend-name
                      :where
                      [{} :person/friends ?f]
                      [?f :person/name ?friend-name]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let results = query_json["results"]
            .as_array()
            .expect("Expected results array");

        assert_eq!(results.len(), 2, "Alice should have 2 friends");

        let friend_names: Vec<&str> = results
            .iter()
            .map(|r| r.as_array().unwrap()[0].as_str().unwrap())
            .collect();

        assert!(friend_names.contains(&"Bob"), "Should contain 'Bob'");
        assert!(
            friend_names.contains(&"Charlie"),
            "Should contain 'Charlie'"
        );
    }

    #[pg_test]
    fn test_cardinality_many_pull() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_cardinality_many_schema();

        // Add entity with cardinality-many values
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/tag \"friendly\"]
                 [:db/add \"alice\" :person/tag \"smart\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // Pull should return cardinality-many as an array
        let pull_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:person/name :person/tag]', {})",
            alice_eid
        ))
        .expect("Pull failed")
        .expect("Pull returned NULL");

        let pull_json: serde_json::Value =
            serde_json::from_str(&pull_result).expect("Failed to parse pull result");

        assert_eq!(
            pull_json[":person/name"].as_str(),
            Some("Alice"),
            "Name should be a single string (cardinality one)"
        );

        let tags = pull_json[":person/tag"]
            .as_array()
            .expect("Tags should be an array (cardinality many)");

        assert_eq!(tags.len(), 2, "Should have 2 tags");

        let tag_strs: Vec<&str> = tags.iter().map(|t| t.as_str().unwrap()).collect();

        assert!(tag_strs.contains(&"friendly"), "Should contain 'friendly'");
        assert!(tag_strs.contains(&"smart"), "Should contain 'smart'");
    }

    #[pg_test]
    fn test_cardinality_many_history() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_cardinality_many_schema();

        // Add tags across transactions
        let tx1_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/tag \"friendly\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction 1 failed")
        .expect("Transaction 1 returned NULL");

        let tx1_json: serde_json::Value =
            serde_json::from_str(&tx1_result).expect("Failed to parse tx1 result");
        let alice_eid = tx1_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :person/tag \"smart\"]]'::TEXT)",
            alice_eid
        ))
        .expect("Transaction 2 failed");

        // Retract one tag
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :person/tag \"friendly\"]]'::TEXT)",
            alice_eid
        ))
        .expect("Retraction failed");

        // History should show all assertions and retractions
        let history_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?tag ?added
                      :where
                      [{} :person/tag ?tag _ ?added]]'::TEXT,
                    '{{\"history\": true}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("History query failed")
        .expect("History query returned NULL");

        let history_json: serde_json::Value =
            serde_json::from_str(&history_result).expect("Failed to parse history result");
        let results = history_json["results"]
            .as_array()
            .expect("Expected results array");

        // Should have: "friendly" added, "smart" added, "friendly" retracted = 3 entries
        assert_eq!(
            results.len(),
            3,
            "History should show 3 datoms (2 assertions + 1 retraction)"
        );

        let assertions: Vec<&str> = results
            .iter()
            .filter(|r| r.as_array().unwrap()[1].as_bool().unwrap())
            .map(|r| r.as_array().unwrap()[0].as_str().unwrap())
            .collect();

        let retractions: Vec<&str> = results
            .iter()
            .filter(|r| !r.as_array().unwrap()[1].as_bool().unwrap())
            .map(|r| r.as_array().unwrap()[0].as_str().unwrap())
            .collect();

        assert_eq!(assertions.len(), 2, "Should have 2 assertions");
        assert!(
            assertions.contains(&"friendly"),
            "Assertions should include 'friendly'"
        );
        assert!(
            assertions.contains(&"smart"),
            "Assertions should include 'smart'"
        );
        assert_eq!(retractions.len(), 1, "Should have 1 retraction");
        assert_eq!(
            retractions[0], "friendly",
            "Retraction should be 'friendly'"
        );
    }

    #[pg_test]
    fn test_cardinality_one_vs_many_semantics() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_cardinality_many_schema();

        // Add entity with both cardinality-one (name) and cardinality-many (tag)
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/tag \"friendly\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // Update cardinality-one (should replace) and add cardinality-many (should accumulate)
        Spi::run(&format!(
            "SELECT mentat_transact('
                [[:db/add {} :person/name \"Alicia\"]
                 [:db/add {} :person/tag \"smart\"]]
            '::TEXT)",
            alice_eid, alice_eid
        ))
        .expect("Update transaction failed");

        // cardinality-one: name should be replaced
        let name_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?name .
                      :where
                      [{} :person/name ?name]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Name query failed")
        .expect("Name query returned NULL");

        let name_json: serde_json::Value =
            serde_json::from_str(&name_result).expect("Failed to parse name result");
        assert_eq!(
            name_json["result"].as_str(),
            Some("Alicia"),
            "Cardinality-one name should be replaced"
        );

        // cardinality-many: both tags should be present
        let tag_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                    '[:find ?tag
                      :where
                      [{} :person/tag ?tag]]'::TEXT,
                    '{{}}' ::jsonb
                )::TEXT",
            alice_eid
        ))
        .expect("Tag query failed")
        .expect("Tag query returned NULL");

        let tag_json: serde_json::Value =
            serde_json::from_str(&tag_result).expect("Failed to parse tag result");
        let tag_results = tag_json["results"]
            .as_array()
            .expect("Expected results array");

        assert_eq!(
            tag_results.len(),
            2,
            "Cardinality-many should accumulate both tags"
        );

        let tags: Vec<&str> = tag_results
            .iter()
            .map(|r| r.as_array().unwrap()[0].as_str().unwrap())
            .collect();

        assert!(tags.contains(&"friendly"), "Should still have 'friendly'");
        assert!(tags.contains(&"smart"), "Should have new 'smart'");
    }

    // ============================================================================
    // :db.fn/cas (Compare-And-Swap) Tests
    // ============================================================================

    #[pg_test]
    fn test_cas_success_cardinality_one() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Create entity with a name
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/age 25]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // CAS: change name from "Alice" to "Alicia" (should succeed)
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :person/name \"Alice\" \"Alicia\"]]'::TEXT)",
            alice_eid
        ))
        .expect("CAS transaction should succeed");

        // Verify the name was updated
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                '[:find ?name .
                  :where [{} :person/name ?name]]'::TEXT,
                '{{}}' ::jsonb
            )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        assert_eq!(
            query_json["result"].as_str(),
            Some("Alicia"),
            "CAS should have updated name to Alicia"
        );

        // Verify age was not affected
        let age_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                '[:find ?age .
                  :where [{} :person/age ?age]]'::TEXT,
                '{{}}' ::jsonb
            )::TEXT",
            alice_eid
        ))
        .expect("Age query failed")
        .expect("Age query returned NULL");

        let age_json: serde_json::Value =
            serde_json::from_str(&age_result).expect("Failed to parse age result");
        assert_eq!(
            age_json["result"].as_i64(),
            Some(25),
            "Age should not be affected by CAS on name"
        );
    }

    #[pg_test]
    fn test_cas_failure_wrong_old_value() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Create entity
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // CAS with wrong old value: expect "Bob" but actual is "Alice"
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :person/name \"Bob\" \"Charlie\"]]'::TEXT)::TEXT",
            alice_eid
        ));

        assert!(result.is_err(), "CAS should fail when old value doesn't match");

        // Verify value was NOT changed
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                '[:find ?name .
                  :where [{} :person/name ?name]]'::TEXT,
                '{{}}' ::jsonb
            )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        assert_eq!(
            query_json["result"].as_str(),
            Some("Alice"),
            "Value should remain unchanged after failed CAS"
        );
    }

    #[pg_test]
    fn test_cas_nil_old_value_success() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Create entity with name but no age
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // CAS with nil old value: attribute doesn't exist yet (should succeed)
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :person/age nil 30]]'::TEXT)",
            alice_eid
        ))
        .expect("CAS with nil old value should succeed when attribute has no value");

        // Verify the age was set
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                '[:find ?age .
                  :where [{} :person/age ?age]]'::TEXT,
                '{{}}' ::jsonb
            )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        assert_eq!(
            query_json["result"].as_i64(),
            Some(30),
            "CAS with nil should have set age to 30"
        );
    }

    #[pg_test]
    fn test_cas_nil_old_value_failure() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Create entity WITH an age already set
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/age 25]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // CAS with nil old value should fail because age already exists
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :person/age nil 30]]'::TEXT)::TEXT",
            alice_eid
        ));

        assert!(
            result.is_err(),
            "CAS with nil old value should fail when attribute already has a value"
        );
    }

    #[pg_test]
    fn test_cas_integer_values() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Create entity with age 25
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/age 25]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // CAS: change age from 25 to 26
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :person/age 25 26]]'::TEXT)",
            alice_eid
        ))
        .expect("CAS on integer should succeed");

        // Verify age updated
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                '[:find ?age .
                  :where [{} :person/age ?age]]'::TEXT,
                '{{}}' ::jsonb
            )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        assert_eq!(
            query_json["result"].as_i64(),
            Some(26),
            "CAS should have updated age to 26"
        );
    }

    #[pg_test]
    fn test_cas_rollback_on_failure() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Create entity
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]
                 [:db/add \"alice\" :person/age 25]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // Transaction with a valid add followed by a failing CAS
        // The entire transaction should be rolled back
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add {} :person/name \"Updated\"] \
             [:db.fn/cas {} :person/age 999 30]]'::TEXT)::TEXT",
            alice_eid, alice_eid
        ));

        assert!(result.is_err(), "Transaction with failing CAS should be rolled back");

        // Verify name was NOT changed (rollback)
        let query_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                '[:find ?name .
                  :where [{} :person/name ?name]]'::TEXT,
                '{{}}' ::jsonb
            )::TEXT",
            alice_eid
        ))
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        assert_eq!(
            query_json["result"].as_str(),
            Some("Alice"),
            "Name should remain unchanged after rolled-back CAS transaction"
        );
    }

    #[pg_test]
    fn test_cas_history_shows_retraction_and_assertion() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Create entity
        let tx_result = Spi::get_one::<String>(
            "SELECT mentat_transact('
                [[:db/add \"alice\" :person/name \"Alice\"]]
            '::TEXT)::TEXT",
        )
        .expect("Transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse tx result");
        let alice_eid = tx_json["tempids"]["alice"]
            .as_i64()
            .expect("Missing alice tempid");

        // CAS: change name from Alice to Alicia
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :person/name \"Alice\" \"Alicia\"]]'::TEXT)",
            alice_eid
        ))
        .expect("CAS should succeed");

        // History should show both the original assertion and the CAS retraction+assertion
        let history_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query(
                '[:find ?name ?added
                  :where [{} :person/name ?name _ ?added]]'::TEXT,
                '{{\"history\": true}}' ::jsonb
            )::TEXT",
            alice_eid
        ))
        .expect("History query failed")
        .expect("History query returned NULL");

        let history_json: serde_json::Value =
            serde_json::from_str(&history_result).expect("Failed to parse history result");
        let results = history_json["results"]
            .as_array()
            .expect("Expected results array");

        // Should have: "Alice" added (original), "Alice" retracted (CAS), "Alicia" added (CAS)
        assert_eq!(
            results.len(),
            3,
            "History should show 3 datoms: original assert, CAS retract, CAS assert"
        );

        let alice_added = results
            .iter()
            .any(|r| {
                let row = r.as_array().unwrap();
                row[0].as_str() == Some("Alice") && row[1].as_bool() == Some(true)
            });
        let alice_retracted = results
            .iter()
            .any(|r| {
                let row = r.as_array().unwrap();
                row[0].as_str() == Some("Alice") && row[1].as_bool() == Some(false)
            });
        let alicia_added = results
            .iter()
            .any(|r| {
                let row = r.as_array().unwrap();
                row[0].as_str() == Some("Alicia") && row[1].as_bool() == Some(true)
            });

        assert!(alice_added, "History should show Alice was asserted");
        assert!(alice_retracted, "History should show Alice was retracted by CAS");
        assert!(alicia_added, "History should show Alicia was asserted by CAS");
    }

    // ============================================================================
    // Prepared Statement Cache Tests
    // ============================================================================

    #[pg_test]
    fn test_stmt_cache_stats_initial() {
        // Cache starts empty
        let result = Spi::get_one::<String>("SELECT mentat_stmt_cache_clear()::TEXT")
            .expect("Cache clear failed")
            .expect("Cache clear returned NULL");
        assert_eq!(result, "ok");

        let stats_str = Spi::get_one::<String>("SELECT mentat_stmt_cache_stats()::TEXT")
            .expect("Stats query failed")
            .expect("Stats returned NULL");
        let stats: serde_json::Value =
            serde_json::from_str(&stats_str).expect("Failed to parse stats JSON");
        assert_eq!(stats["size"], 0, "Cache should start empty after clear");
        assert_eq!(stats["total_hits"], 0, "No hits after clear");
    }

    #[pg_test]
    fn test_stmt_cache_populates_on_query() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Clear cache first
        Spi::get_one::<String>("SELECT mentat_stmt_cache_clear()::TEXT")
            .expect("Cache clear failed");

        // Run a query - should create a cache entry
        Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?x ?ident :where [?x :db/ident ?ident]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let stats_str = Spi::get_one::<String>("SELECT mentat_stmt_cache_stats()::TEXT")
            .expect("Stats query failed")
            .expect("Stats returned NULL");
        let stats: serde_json::Value =
            serde_json::from_str(&stats_str).expect("Failed to parse stats JSON");
        assert_eq!(stats["size"], 1, "Cache should have 1 entry after first query");
        assert_eq!(stats["total_hits"], 0, "No hits yet (first execution was a miss)");
    }

    #[pg_test]
    fn test_stmt_cache_hits_on_repeated_query() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Clear cache first
        Spi::get_one::<String>("SELECT mentat_stmt_cache_clear()::TEXT")
            .expect("Cache clear failed");

        let query = "SELECT mentat_query(
            '[:find ?x ?ident :where [?x :db/ident ?ident]]'::TEXT,
            '{}'::jsonb
        )::TEXT";

        // First execution: cache miss
        Spi::get_one::<String>(query).expect("Query 1 failed");

        // Second execution: cache hit
        Spi::get_one::<String>(query).expect("Query 2 failed");

        // Third execution: another cache hit
        Spi::get_one::<String>(query).expect("Query 3 failed");

        let stats_str = Spi::get_one::<String>("SELECT mentat_stmt_cache_stats()::TEXT")
            .expect("Stats query failed")
            .expect("Stats returned NULL");
        let stats: serde_json::Value =
            serde_json::from_str(&stats_str).expect("Failed to parse stats JSON");
        assert_eq!(stats["size"], 1, "Same query pattern should produce 1 cache entry");
        assert_eq!(stats["total_hits"], 2, "Should have 2 cache hits (queries 2 and 3)");
    }

    #[pg_test]
    fn test_stmt_cache_different_queries_separate_entries() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Clear cache first
        Spi::get_one::<String>("SELECT mentat_stmt_cache_clear()::TEXT")
            .expect("Cache clear failed");

        // Query 1: find ident
        Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?ident . :where [1 :db/ident ?ident]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query 1 failed");

        // Query 2: find valueType
        Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?type . :where [1 :db/valueType ?type]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query 2 failed");

        let stats_str = Spi::get_one::<String>("SELECT mentat_stmt_cache_stats()::TEXT")
            .expect("Stats query failed")
            .expect("Stats returned NULL");
        let stats: serde_json::Value =
            serde_json::from_str(&stats_str).expect("Failed to parse stats JSON");
        assert_eq!(stats["size"], 2, "Different queries should produce separate cache entries");
    }

    #[pg_test]
    fn test_stmt_cache_clear_resets() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Run some queries to populate cache
        Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?x ?ident :where [?x :db/ident ?ident]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        // Verify cache is non-empty
        let stats_str = Spi::get_one::<String>("SELECT mentat_stmt_cache_stats()::TEXT")
            .expect("Stats query failed")
            .expect("Stats returned NULL");
        let stats: serde_json::Value =
            serde_json::from_str(&stats_str).expect("Failed to parse stats JSON");
        assert!(stats["size"].as_u64().expect("size should be int") > 0, "Cache should be non-empty");

        // Clear and verify
        Spi::get_one::<String>("SELECT mentat_stmt_cache_clear()::TEXT")
            .expect("Cache clear failed");

        let stats_str = Spi::get_one::<String>("SELECT mentat_stmt_cache_stats()::TEXT")
            .expect("Stats query failed")
            .expect("Stats returned NULL");
        let stats: serde_json::Value =
            serde_json::from_str(&stats_str).expect("Failed to parse stats JSON");
        assert_eq!(stats["size"], 0, "Cache should be empty after clear");
        assert_eq!(stats["total_hits"], 0, "Hits should be zero after clear");
    }

    #[pg_test]
    fn test_stmt_cache_correct_results_after_cache_hit() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Clear cache
        Spi::get_one::<String>("SELECT mentat_stmt_cache_clear()::TEXT")
            .expect("Cache clear failed");

        let query = "SELECT mentat_query(
            '[:find ?ident . :where [1 :db/ident ?ident]]'::TEXT,
            '{}'::jsonb
        )::TEXT";

        // First execution (cache miss)
        let result1_str = Spi::get_one::<String>(query)
            .expect("Query 1 failed")
            .expect("Query 1 returned NULL");
        let result1: serde_json::Value =
            serde_json::from_str(&result1_str).expect("Failed to parse result 1");

        // Second execution (cache hit)
        let result2_str = Spi::get_one::<String>(query)
            .expect("Query 2 failed")
            .expect("Query 2 returned NULL");
        let result2: serde_json::Value =
            serde_json::from_str(&result2_str).expect("Failed to parse result 2");

        // Results should be identical
        assert_eq!(result1, result2, "Cached query should produce identical results");
        assert_eq!(
            result1["result"].as_str().expect("Expected string result"),
            ":db/ident",
            "Both should return :db/ident"
        );
    }

    // ============================================================================
    // EDN Type Round-Trip Tests (CRITICAL for v1.0)
    // ============================================================================

    #[pg_test]
    fn test_double_type_round_trip() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with double attribute
        let schema_tx = r#"[
            {:db/ident :measurement/value
             :db/valueType :db.type/double
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Test transact with double value
        let data_tx = r#"[
            {:db/id "m1"
             :measurement/value 3.14159}
            {:db/id "m2"
             :measurement/value 2.71828}
            {:db/id "m3"
             :measurement/value 0.0}
            {:db/id "m4"
             :measurement/value -1.5}
        ]"#;

        let tx_result = Spi::get_one_with_args::<String>(
            "SELECT mentat_transact($1)::TEXT",
            &[DatumWithOid::from(data_tx)],
        )
        .expect("Data transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse transaction result");
        let m1_eid = tx_json["tempids"]["m1"]
            .as_i64()
            .expect("Missing m1 tempid");

        // Test query filtering on double
        let query_result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?val
                  :where [?e :measurement/value ?val]
                         [(> ?val 3.0)]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let results = query_json["results"]
            .as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 1, "Should find one value > 3.0");
        let val = results[0][1].as_f64().expect("Value should be double");
        assert!((val - 3.14159).abs() < 0.00001, "Double value should match");

        // Test pull API returns correct format
        let pull_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:measurement/value]', {})::TEXT",
            m1_eid
        ))
        .expect("Pull failed")
        .expect("Pull returned NULL");

        let pull_json: serde_json::Value =
            serde_json::from_str(&pull_result).expect("Failed to parse pull result");
        let pull_val = pull_json["measurement/value"]
            .as_f64()
            .expect("Pull should return double");
        assert!((pull_val - 3.14159).abs() < 0.00001, "Pull double value should match");

        // Test entity API returns correct format
        let entity_result = Spi::get_one::<String>(&format!("SELECT mentat_entity({})::TEXT", m1_eid))
            .expect("Entity failed")
            .expect("Entity returned NULL");

        let entity_json: serde_json::Value =
            serde_json::from_str(&entity_result).expect("Failed to parse entity result");
        let entity_val = entity_json["measurement/value"]
            .as_f64()
            .expect("Entity should return double");
        assert!(
            (entity_val - 3.14159).abs() < 0.00001,
            "Entity double value should match"
        );
    }

    #[pg_test]
    fn test_instant_type_round_trip() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with instant attribute
        let schema_tx = r#"[
            {:db/ident :event/timestamp
             :db/valueType :db.type/instant
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Test transact with instant value (RFC3339 format)
        let data_tx = r#"[
            {:db/id "e1"
             :event/timestamp "2024-01-15T10:30:00Z"}
            {:db/id "e2"
             :event/timestamp "2024-01-15T14:45:00Z"}
            {:db/id "e3"
             :event/timestamp "1970-01-01T00:00:00Z"}
        ]"#;

        let tx_result = Spi::get_one_with_args::<String>(
            "SELECT mentat_transact($1)::TEXT",
            &[DatumWithOid::from(data_tx)],
        )
        .expect("Data transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse transaction result");
        let e1_eid = tx_json["tempids"]["e1"]
            .as_i64()
            .expect("Missing e1 tempid");

        // Test query filtering on instant
        let query_result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?ts
                  :where [?e :event/timestamp ?ts]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let results = query_json["results"]
            .as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 3, "Should find all three timestamps");
        let ts = results[0][1].as_str().expect("Timestamp should be string");
        assert!(
            ts.contains("2024-01-15") || ts.contains("1970-01-01"),
            "Timestamp should be in ISO format"
        );

        // Test pull API returns correct format
        let pull_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:event/timestamp]', {})::TEXT",
            e1_eid
        ))
        .expect("Pull failed")
        .expect("Pull returned NULL");

        let pull_json: serde_json::Value =
            serde_json::from_str(&pull_result).expect("Failed to parse pull result");
        let pull_ts = pull_json["event/timestamp"]
            .as_str()
            .expect("Pull should return instant as string");
        assert!(
            pull_ts.contains("2024-01-15T10:30:00"),
            "Pull instant should match"
        );

        // Test entity API returns correct format
        let entity_result = Spi::get_one::<String>(&format!("SELECT mentat_entity({})::TEXT", e1_eid))
            .expect("Entity failed")
            .expect("Entity returned NULL");

        let entity_json: serde_json::Value =
            serde_json::from_str(&entity_result).expect("Failed to parse entity result");
        let entity_ts = entity_json["event/timestamp"]
            .as_str()
            .expect("Entity should return instant as string");
        assert!(
            entity_ts.contains("2024-01-15T10:30:00"),
            "Entity instant should match"
        );
    }

    #[pg_test]
    fn test_uuid_type_round_trip() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with uuid attribute
        let schema_tx = r#"[
            {:db/ident :session/id
             :db/valueType :db.type/uuid
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Test transact with uuid value
        let test_uuid = "550e8400-e29b-41d4-a716-446655440000";
        let data_tx = format!(
            r#"[
            {{:db/id "s1"
             :session/id "{}"}}
            {{:db/id "s2"
             :session/id "123e4567-e89b-12d3-a456-426614174000"}}
        ]"#,
            test_uuid
        );

        let tx_result = Spi::get_one_with_args::<String>(
            "SELECT mentat_transact($1)::TEXT",
            &[DatumWithOid::from(data_tx.as_str())],
        )
        .expect("Data transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse transaction result");
        let s1_eid = tx_json["tempids"]["s1"]
            .as_i64()
            .expect("Missing s1 tempid");

        // Test query filtering on uuid
        let query_result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?id
                  :where [?e :session/id ?id]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let results = query_json["results"]
            .as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 2, "Should find both UUIDs");
        let uuid_val = results[0][1].as_str().expect("UUID should be string");
        assert!(
            uuid_val == test_uuid || uuid_val == "123e4567-e89b-12d3-a456-426614174000",
            "UUID should match one of the inserted values"
        );

        // Test pull API returns correct format
        let pull_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:session/id]', {})::TEXT",
            s1_eid
        ))
        .expect("Pull failed")
        .expect("Pull returned NULL");

        let pull_json: serde_json::Value =
            serde_json::from_str(&pull_result).expect("Failed to parse pull result");
        let pull_uuid = pull_json["session/id"]
            .as_str()
            .expect("Pull should return UUID as string");
        assert_eq!(pull_uuid, test_uuid, "Pull UUID should match");

        // Test entity API returns correct format
        let entity_result = Spi::get_one::<String>(&format!("SELECT mentat_entity({})::TEXT", s1_eid))
            .expect("Entity failed")
            .expect("Entity returned NULL");

        let entity_json: serde_json::Value =
            serde_json::from_str(&entity_result).expect("Failed to parse entity result");
        let entity_uuid = entity_json["session/id"]
            .as_str()
            .expect("Entity should return UUID as string");
        assert_eq!(entity_uuid, test_uuid, "Entity UUID should match");
    }

    #[pg_test]
    fn test_bytes_type_round_trip() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with bytes attribute
        let schema_tx = r#"[
            {:db/ident :file/data
             :db/valueType :db.type/bytes
             :db/cardinality :db.cardinality/one}
        ]"#;

        Spi::run_with_args(
            "SELECT mentat_transact($1)",
            &[DatumWithOid::from(schema_tx)],
        )
        .expect("Schema transaction failed");

        // Test transact with bytes value (base64 encoded)
        // "Hello, World!" in base64
        let test_bytes_b64 = "SGVsbG8sIFdvcmxkIQ==";
        let data_tx = format!(
            r#"[
            {{:db/id "f1"
             :file/data "{}"}}
            {{:db/id "f2"
             :file/data "AQIDBA=="}}
        ]"#,
            test_bytes_b64
        );

        let tx_result = Spi::get_one_with_args::<String>(
            "SELECT mentat_transact($1)::TEXT",
            &[DatumWithOid::from(data_tx.as_str())],
        )
        .expect("Data transaction failed")
        .expect("Transaction returned NULL");

        let tx_json: serde_json::Value =
            serde_json::from_str(&tx_result).expect("Failed to parse transaction result");
        let f1_eid = tx_json["tempids"]["f1"]
            .as_i64()
            .expect("Missing f1 tempid");

        // Test query returns bytes
        let query_result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?data
                  :where [?e :file/data ?data]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let query_json: serde_json::Value =
            serde_json::from_str(&query_result).expect("Failed to parse query result");
        let results = query_json["results"]
            .as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 2, "Should find both byte arrays");
        let bytes_val = results[0][1].as_str().expect("Bytes should be string (base64)");
        assert!(
            bytes_val == test_bytes_b64 || bytes_val == "AQIDBA==",
            "Bytes should match one of the inserted values (base64 encoded)"
        );

        // Test pull API returns correct format
        let pull_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:file/data]', {})::TEXT",
            f1_eid
        ))
        .expect("Pull failed")
        .expect("Pull returned NULL");

        let pull_json: serde_json::Value =
            serde_json::from_str(&pull_result).expect("Failed to parse pull result");
        let pull_bytes = pull_json["file/data"]
            .as_str()
            .expect("Pull should return bytes as base64 string");
        assert_eq!(pull_bytes, test_bytes_b64, "Pull bytes should match");

        // Test entity API returns correct format
        let entity_result = Spi::get_one::<String>(&format!("SELECT mentat_entity({})::TEXT", f1_eid))
            .expect("Entity failed")
            .expect("Entity returned NULL");

        let entity_json: serde_json::Value =
            serde_json::from_str(&entity_result).expect("Failed to parse entity result");
        let entity_bytes = entity_json["file/data"]
            .as_str()
            .expect("Entity should return bytes as base64 string");
        assert_eq!(entity_bytes, test_bytes_b64, "Entity bytes should match");
    }

    // ============================================================================
    // Error Message Quality Tests
    // ============================================================================

    /// Helper to run a SQL statement and return the error message string.
    fn get_error_message(sql: &str) -> String {
        match Spi::get_one::<String>(sql) {
            Err(e) => format!("{e}"),
            Ok(_) => panic!("Expected error from SQL: {sql}"),
        }
    }

    #[pg_test]
    fn test_error_invalid_transaction_not_vector() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        crate::cache::get_cache().invalidate();

        let err = get_error_message(
            "SELECT mentat_transact('42'::TEXT)"
        );
        assert!(
            err.contains(":db.error/invalid-transaction"),
            "Error should contain error code, got: {err}"
        );
        assert!(
            err.contains("vector"),
            "Error should mention 'vector', got: {err}"
        );
    }

    #[pg_test]
    fn test_error_attribute_not_found_includes_available() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        crate::cache::get_cache().invalidate();

        let err = get_error_message(
            "SELECT mentat_transact('[[:db/add \"t\" :nonexistent/attr \"val\"]]'::TEXT)"
        );
        assert!(
            err.contains(":db.error/attribute-not-found"),
            "Error should contain error code, got: {err}"
        );
        assert!(
            err.contains("Available attributes") || err.contains("not found in schema"),
            "Error should list available attributes or indicate schema lookup failure, got: {err}"
        );
    }

    #[pg_test]
    fn test_error_type_mismatch_is_descriptive() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();
        crate::cache::get_cache().invalidate();

        // :person/age is :db.type/long, so passing a string should fail
        let err = get_error_message(
            "SELECT mentat_transact('[[:db/add \"p\" :person/age \"not-a-number\"]]'::TEXT)"
        );
        assert!(
            err.contains(":db.error/wrong-type-for-attribute"),
            "Error should contain type mismatch error code, got: {err}"
        );
        assert!(
            err.contains("person/age"),
            "Error should mention the attribute name, got: {err}"
        );
        assert!(
            err.contains("long"),
            "Error should mention the expected type, got: {err}"
        );
    }

    #[pg_test]
    fn test_error_pull_pattern_not_vector() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        crate::cache::get_cache().invalidate();

        let err = get_error_message(
            "SELECT mentat_pull(':person/name'::TEXT, 1)"
        );
        assert!(
            err.contains(":db.error/invalid-pull-pattern"),
            "Error should contain pull pattern error code, got: {err}"
        );
        assert!(
            err.contains("vector"),
            "Error should suggest vector format, got: {err}"
        );
    }

    #[pg_test]
    fn test_error_unsupported_aggregate() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        crate::cache::get_cache().invalidate();

        let err = get_error_message(
            "SELECT mentat_query(
                '[:find (median ?x) :where [?x :db/ident _]]'::TEXT,
                '{}'::jsonb
            )::TEXT"
        );
        assert!(
            err.contains(":db.error/unsupported-aggregate"),
            "Error should contain aggregate error code, got: {err}"
        );
        assert!(
            err.contains("count") || err.contains("sum"),
            "Error should list valid aggregates, got: {err}"
        );
    }

    #[pg_test]
    fn test_error_batch_unknown_operation() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        crate::cache::get_cache().invalidate();

        let err = get_error_message(
            "SELECT mentat.batch('[[:bogus 123]]'::TEXT)::TEXT"
        );
        assert!(
            err.contains(":db.error/unknown-batch-op") || err.contains(":db.error/invalid-batch-op"),
            "Error should contain batch operation error code, got: {err}"
        );
        assert!(
            err.contains(":query") || err.contains(":transact"),
            "Error should list valid operations, got: {err}"
        );
    }

    // ============================================================================
    // Range Query Regression Tests (BYTEA encoding bug fix verification)
    // ============================================================================

    /// Regression test: numeric range queries must use native BIGINT comparison.
    ///
    /// With the old BYTEA encoding, `[(> ?age 30)]` could produce wrong results
    /// because binary comparison of little-endian bytes doesn't match numeric ordering.
    #[pg_test]
    fn test_range_query_numeric() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Insert people with various ages
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 25]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 35]
                 [:db/add \"p3\" :person/name \"Carol\"]
                 [:db/add \"p3\" :person/age 10]
                 [:db/add \"p4\" :person/name \"Dave\"]
                 [:db/add \"p4\" :person/age 100]
                 [:db/add \"p5\" :person/name \"Eve\"]
                 [:db/add \"p5\" :person/age 2]]
            '::TEXT)",
        )
        .expect("Transaction failed");

        // Test: (> ?age 30) should return Bob(35) and Dave(100), NOT Eve(2)
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name ?age
                  :where
                  [?p :person/name ?name]
                  [?p :person/age ?age]
                  [(> ?age 30)]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");

        // Should have exactly 2 results: Bob(35) and Dave(100)
        assert_eq!(
            results.len(), 2,
            "Expected 2 people with age > 30, got {}: {:?}",
            results.len(), results
        );

        // Verify the names
        let names: Vec<&str> = results.iter()
            .map(|r| r[0].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Bob"), "Bob (age 35) should be in results: {:?}", names);
        assert!(names.contains(&"Dave"), "Dave (age 100) should be in results: {:?}", names);
        // Critical: Eve (age 2) must NOT be in results
        assert!(!names.contains(&"Eve"), "Eve (age 2) should NOT be in results: {:?}", names);
    }

    /// Regression test: numeric less-than comparison.
    ///
    /// With BYTEA encoding, `[(< ?age 10)]` could incorrectly include values
    /// whose binary representation sorts lower but numeric value is higher.
    #[pg_test]
    fn test_range_query_numeric_less_than() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 5]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 10]
                 [:db/add \"p3\" :person/name \"Carol\"]
                 [:db/add \"p3\" :person/age 100]
                 [:db/add \"p4\" :person/name \"Dave\"]
                 [:db/add \"p4\" :person/age 2]]
            '::TEXT)",
        )
        .expect("Transaction failed");

        // Test: (< ?age 10) should return Alice(5) and Dave(2) only
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name ?age
                  :where
                  [?p :person/name ?name]
                  [?p :person/age ?age]
                  [(< ?age 10)]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(
            results.len(), 2,
            "Expected 2 people with age < 10, got {}: {:?}",
            results.len(), results
        );

        let names: Vec<&str> = results.iter()
            .map(|r| r[0].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Alice"), "Alice (age 5) should be in results: {:?}", names);
        assert!(names.contains(&"Dave"), "Dave (age 2) should be in results: {:?}", names);
    }

    /// Regression test: text range queries use lexicographic ordering.
    ///
    /// With BYTEA encoding, text comparison used binary (byte) ordering which
    /// is the same as UTF-8 lexicographic for ASCII. This test verifies the
    /// new TEXT column preserves correct behavior.
    #[pg_test]
    fn test_range_query_text_ordering() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 1]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 2]
                 [:db/add \"p3\" :person/name \"Carol\"]
                 [:db/add \"p3\" :person/age 3]
                 [:db/add \"p4\" :person/name \"Zara\"]
                 [:db/add \"p4\" :person/age 4]]
            '::TEXT)",
        )
        .expect("Transaction failed");

        // Test: names > "Bob" should return Carol and Zara (lexicographic)
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name
                  :where
                  [?p :person/name ?name]
                  [(> ?name \"Bob\")]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(
            results.len(), 2,
            "Expected 2 names > 'Bob', got {}: {:?}",
            results.len(), results
        );

        let names: Vec<&str> = results.iter()
            .map(|r| r[0].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Carol"), "Carol should be > Bob: {:?}", names);
        assert!(names.contains(&"Zara"), "Zara should be > Bob: {:?}", names);
        assert!(!names.contains(&"Alice"), "Alice should NOT be > Bob: {:?}", names);
    }

    /// Regression test: numeric ordering with order-by.
    ///
    /// Verifies that ORDER BY on numeric values uses correct integer ordering,
    /// not BYTEA binary ordering.
    #[pg_test]
    fn test_order_by_numeric() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 2]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 10]
                 [:db/add \"p3\" :person/name \"Carol\"]
                 [:db/add \"p3\" :person/age 100]
                 [:db/add \"p4\" :person/name \"Dave\"]
                 [:db/add \"p4\" :person/age 3]]
            '::TEXT)",
        )
        .expect("Transaction failed");

        // Test: order by age ascending should be 2, 3, 10, 100
        // With BYTEA, binary ordering would give: 2, 3, 10, 100 for small ints
        // but for larger values the ordering breaks.
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name ?age
                  :where
                  [?p :person/name ?name]
                  [?p :person/age ?age]
                  :order (asc ?age)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 4, "Expected 4 results");

        // Verify ordering: ages should be 2, 3, 10, 100
        let ages: Vec<i64> = results.iter()
            .map(|r| r[1].as_i64().unwrap())
            .collect();
        assert_eq!(
            ages, vec![2, 3, 10, 100],
            "Ages should be in ascending numeric order, got: {:?}",
            ages
        );
    }

    /// Regression test: BETWEEN-style range queries with two predicates.
    ///
    /// Tests that combining (> ?age X) and (< ?age Y) works correctly
    /// with native typed columns.
    #[pg_test]
    fn test_range_query_between() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 5]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 15]
                 [:db/add \"p3\" :person/name \"Carol\"]
                 [:db/add \"p3\" :person/age 25]
                 [:db/add \"p4\" :person/name \"Dave\"]
                 [:db/add \"p4\" :person/age 35]
                 [:db/add \"p5\" :person/name \"Eve\"]
                 [:db/add \"p5\" :person/age 45]]
            '::TEXT)",
        )
        .expect("Transaction failed");

        // Test: 10 < age < 40 should return Bob(15), Carol(25), Dave(35)
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name ?age
                  :where
                  [?p :person/name ?name]
                  [?p :person/age ?age]
                  [(> ?age 10)]
                  [(< ?age 40)]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(
            results.len(), 3,
            "Expected 3 people with 10 < age < 40, got {}: {:?}",
            results.len(), results
        );

        let names: Vec<&str> = results.iter()
            .map(|r| r[0].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Bob"), "Bob (15) should be in 10..40: {:?}", names);
        assert!(names.contains(&"Carol"), "Carol (25) should be in 10..40: {:?}", names);
        assert!(names.contains(&"Dave"), "Dave (35) should be in 10..40: {:?}", names);
    }

    /// Regression test: UUID values maintain consistent ordering.
    ///
    /// Verifies that UUID values stored in the native v_uuid column produce
    /// deterministic and consistent ordering via ORDER BY, unlike BYTEA where
    /// the 16-byte representation could produce unexpected sort results.
    #[pg_test]
    fn test_uuid_ordering() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with uuid attribute
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"uuid-attr\" :db/ident :item/id]
                 [:db/add \"uuid-attr\" :db/valueType :db.type/uuid]
                 [:db/add \"uuid-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"name-attr\" :db/ident :item/name]
                 [:db/add \"name-attr\" :db/valueType :db.type/string]
                 [:db/add \"name-attr\" :db/cardinality :db.cardinality/one]]
            '::TEXT)",
        )
        .expect("Schema transaction failed");

        // Insert 3 items with UUIDs that have a known lexicographic order:
        //   "11111111-..." < "55555555-..." < "aaaaaaaa-..."
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"i1\" :item/name \"First\"]
                 [:db/add \"i1\" :item/id #uuid \"55555555-5555-5555-5555-555555555555\"]
                 [:db/add \"i2\" :item/name \"Second\"]
                 [:db/add \"i2\" :item/id #uuid \"11111111-1111-1111-1111-111111111111\"]
                 [:db/add \"i3\" :item/name \"Third\"]
                 [:db/add \"i3\" :item/id #uuid \"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa\"]]
            '::TEXT)",
        )
        .expect("Transaction failed");

        // Query all UUIDs with ORDER BY ascending
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name ?id
                  :where
                  [?e :item/name ?name]
                  [?e :item/id ?id]
                  :order (asc ?id)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 3, "Should find all 3 items");

        // Verify consistent ordering: 11111111 < 55555555 < aaaaaaaa
        let names: Vec<&str> = results.iter()
            .map(|r| r[0].as_str().unwrap())
            .collect();
        assert_eq!(
            names, vec!["Second", "First", "Third"],
            "UUIDs should sort in consistent lexicographic order: 1111 < 5555 < aaaa, got: {:?}",
            names
        );

        // Also verify the UUID values round-trip correctly
        let uuids: Vec<&str> = results.iter()
            .map(|r| r[1].as_str().unwrap())
            .collect();
        assert!(
            uuids[0].starts_with("11111111"),
            "First UUID should be 11111111..., got: {}",
            uuids[0]
        );
        assert!(
            uuids[1].starts_with("55555555"),
            "Second UUID should be 55555555..., got: {}",
            uuids[1]
        );
        assert!(
            uuids[2].starts_with("aaaaaaaa"),
            "Third UUID should be aaaaaaaa..., got: {}",
            uuids[2]
        );
    }

    /// Regression test: timestamp/instant range queries with correct temporal ordering.
    ///
    /// Verifies that instant values stored in the native v_instant TIMESTAMPTZ column
    /// produce correct temporal ordering. With the old BYTEA encoding, the 8-byte
    /// little-endian microsecond representation could produce incorrect ordering
    /// because binary comparison of LE bytes doesn't match numeric ordering.
    #[pg_test]
    fn test_timestamp_ranges() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Define schema with instant attribute
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"ts-attr\" :db/ident :event/timestamp]
                 [:db/add \"ts-attr\" :db/valueType :db.type/instant]
                 [:db/add \"ts-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"label-attr\" :db/ident :event/label]
                 [:db/add \"label-attr\" :db/valueType :db.type/string]
                 [:db/add \"label-attr\" :db/cardinality :db.cardinality/one]]
            '::TEXT)",
        )
        .expect("Schema transaction failed");

        // Insert events with timestamps spanning different years/months
        // These are chosen so that BYTEA LE comparison would fail:
        // epoch microseconds for 2020 vs 2024 differ in higher bytes
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"e1\" :event/label \"Ancient\"]
                 [:db/add \"e1\" :event/timestamp \"1999-06-15T12:00:00Z\"]
                 [:db/add \"e2\" :event/label \"Early\"]
                 [:db/add \"e2\" :event/timestamp \"2020-01-01T00:00:00Z\"]
                 [:db/add \"e3\" :event/label \"Middle\"]
                 [:db/add \"e3\" :event/timestamp \"2022-06-15T12:00:00Z\"]
                 [:db/add \"e4\" :event/label \"Recent\"]
                 [:db/add \"e4\" :event/timestamp \"2024-12-25T18:30:00Z\"]]
            '::TEXT)",
        )
        .expect("Transaction failed");

        // Test ORDER BY timestamp ascending - should be chronological
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?label ?ts
                  :where
                  [?e :event/label ?label]
                  [?e :event/timestamp ?ts]
                  :order (asc ?ts)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 4, "Should find all 4 events");

        // Verify chronological ordering
        let labels: Vec<&str> = results.iter()
            .map(|r| r[0].as_str().unwrap())
            .collect();
        assert_eq!(
            labels, vec!["Ancient", "Early", "Middle", "Recent"],
            "Events should be in chronological order, got: {:?}",
            labels
        );

        // Verify timestamps contain expected date fragments
        let timestamps: Vec<&str> = results.iter()
            .map(|r| r[1].as_str().unwrap())
            .collect();
        assert!(
            timestamps[0].contains("1999"),
            "First timestamp should be 1999, got: {}",
            timestamps[0]
        );
        assert!(
            timestamps[3].contains("2024"),
            "Last timestamp should be 2024, got: {}",
            timestamps[3]
        );

        // Test descending order
        let desc_result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?label ?ts
                  :where
                  [?e :event/label ?label]
                  [?e :event/timestamp ?ts]
                  :order (desc ?ts)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let desc_json: serde_json::Value = serde_json::from_str(&desc_result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");
        let desc_results = desc_json["results"].as_array().expect("Expected array");

        let desc_labels: Vec<&str> = desc_results.iter()
            .map(|r| r[0].as_str().unwrap())
            .collect();
        assert_eq!(
            desc_labels, vec!["Recent", "Middle", "Early", "Ancient"],
            "Descending order should be reverse chronological, got: {:?}",
            desc_labels
        );
    }

    // ============================================================================
    // Range Query Correctness Tests (BYTEA fix validation)
    // ============================================================================
    // These tests verify that numeric and string range queries return correct
    // results using native PostgreSQL types instead of BYTEA encoding.
    // The old BYTEA encoding would cause "2" > "10" (0x32 > 0x31), but with
    // native BIGINT columns, 2 < 10 < 100 as expected.

    #[pg_test]
    fn test_range_queries_numeric_ordering() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Insert persons with ages that would sort incorrectly under BYTEA encoding
        // BYTEA: "2" (0x32) > "10" (0x31 0x30) because 0x32 > 0x31
        // Native BIGINT: 2 < 10 < 100 (correct)
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 2]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 10]
                 [:db/add \"p3\" :person/name \"Charlie\"]
                 [:db/add \"p3\" :person/age 100]
                 [:db/add \"p4\" :person/name \"Diana\"]
                 [:db/add \"p4\" :person/age 5]]
            '::TEXT)",
        )
        .expect("Failed to transact test data");

        // Query: find persons with age < 10 (should return Alice=2 and Diana=5)
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name ?age
                  :where
                  [?e :person/name ?name]
                  [?e :person/age ?age]
                  [(< ?age 10)]
                  :order (asc ?age)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let json: serde_json::Value = serde_json::from_str(&result)
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");

        // With correct numeric ordering: ages 2 and 5 are < 10
        assert_eq!(results.len(), 2, "Expected 2 results for age < 10, got {}", results.len());
        assert_eq!(results[0][0].as_str().unwrap(), "Alice", "First should be Alice (age 2)");
        assert_eq!(results[0][1].as_i64().unwrap(), 2);
        assert_eq!(results[1][0].as_str().unwrap(), "Diana", "Second should be Diana (age 5)");
        assert_eq!(results[1][1].as_i64().unwrap(), 5);

        // Query: ascending order by age should be 2, 5, 10, 100
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name ?age
                  :where
                  [?e :person/name ?name]
                  [?e :person/age ?age]
                  :order (asc ?age)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let json: serde_json::Value = serde_json::from_str(&result)
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");
        let ages: Vec<i64> = results.iter().map(|r| r[1].as_i64().unwrap()).collect();
        assert_eq!(ages, vec![2, 5, 10, 100], "Ages should be in correct numeric order, got {:?}", ages);
    }

    #[pg_test]
    fn test_range_queries_between() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        // Insert values that test boundary conditions
        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"One\"]
                 [:db/add \"p1\" :person/age 1]
                 [:db/add \"p2\" :person/name \"Nine\"]
                 [:db/add \"p2\" :person/age 9]
                 [:db/add \"p3\" :person/name \"Ten\"]
                 [:db/add \"p3\" :person/age 10]
                 [:db/add \"p4\" :person/name \"Eleven\"]
                 [:db/add \"p4\" :person/age 11]
                 [:db/add \"p5\" :person/name \"Hundred\"]
                 [:db/add \"p5\" :person/age 100]
                 [:db/add \"p6\" :person/name \"Thousand\"]
                 [:db/add \"p6\" :person/age 1000]]
            '::TEXT)",
        )
        .expect("Failed to transact test data");

        // Query: 5 <= age <= 50
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name ?age
                  :where
                  [?e :person/name ?name]
                  [?e :person/age ?age]
                  [(>= ?age 5)]
                  [(<= ?age 50)]
                  :order (asc ?age)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let json: serde_json::Value = serde_json::from_str(&result)
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");
        let names: Vec<&str> = results.iter().map(|r| r[0].as_str().unwrap()).collect();
        assert_eq!(
            names, vec!["Nine", "Ten", "Eleven"],
            "Between 5 and 50 should return Nine(9), Ten(10), Eleven(11), got: {:?}",
            names
        );
    }

    #[pg_test]
    fn test_string_ordering() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_person_schema();

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/name \"banana\"]
                 [:db/add \"p1\" :person/age 1]
                 [:db/add \"p2\" :person/name \"apple\"]
                 [:db/add \"p2\" :person/age 2]
                 [:db/add \"p3\" :person/name \"cherry\"]
                 [:db/add \"p3\" :person/age 3]
                 [:db/add \"p4\" :person/name \"date\"]
                 [:db/add \"p4\" :person/age 4]]
            '::TEXT)",
        )
        .expect("Failed to transact test data");

        // Query all names ordered ascending - should be alphabetical
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name
                  :where
                  [?e :person/name ?name]
                  :order (asc ?name)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed")
        .expect("Query returned NULL");

        let json: serde_json::Value = serde_json::from_str(&result)
            .expect("Failed to parse JSON");
        let results = json["results"].as_array().expect("Expected array");
        let names: Vec<&str> = results.iter().map(|r| r[0].as_str().unwrap()).collect();
        assert_eq!(
            names, vec!["apple", "banana", "cherry", "date"],
            "String ordering should be alphabetical, got: {:?}",
            names
        );
    }
}
