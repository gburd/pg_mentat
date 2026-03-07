use pgrx::prelude::*;

pgrx::pg_module_magic!();

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
    pub use crate::functions::entity::*;
    pub use crate::functions::pull::*;
    pub use crate::functions::query::*;
    pub use crate::functions::schema::*;
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

            -- EAVT, AEVT, AVET, VAET index pattern (simplified for tests)
            CREATE INDEX IF NOT EXISTS idx_datoms_eavt ON mentat.datoms (e, a, value_type_tag, v, tx);
            CREATE INDEX IF NOT EXISTS idx_datoms_aevt ON mentat.datoms (a, e, value_type_tag, v, tx);
            CREATE INDEX IF NOT EXISTS idx_datoms_avet ON mentat.datoms (a, value_type_tag, v, e, tx);
            CREATE INDEX IF NOT EXISTS idx_datoms_vaet ON mentat.datoms (v, a, e, tx) WHERE value_type_tag = 0;
            CREATE INDEX IF NOT EXISTS idx_datoms_tx ON mentat.datoms (tx);

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

            INSERT INTO mentat.transactions (tx, tx_instant)
            VALUES (1000000, '2025-01-01T00:00:00Z')
            ON CONFLICT (tx) DO NOTHING;

            -- PL/pgSQL helper functions for transaction processing
            CREATE OR REPLACE FUNCTION mentat.allocate_entid(partition_name TEXT)
            RETURNS BIGINT AS $$
            DECLARE new_entid BIGINT;
            BEGIN
                UPDATE mentat.partitions
                SET next_entid = next_entid + 1
                WHERE name = partition_name
                RETURNING next_entid - 1 INTO new_entid;
                IF NOT FOUND THEN
                    RAISE EXCEPTION 'Partition % not found', partition_name;
                END IF;
                RETURN new_entid;
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
            -- Each entity with an ident gets a (:db/ident, keyword) datom.
            -- a=1 is :db/ident, value_type_tag=8 is keyword, tx=1000000 is bootstrap tx.
            INSERT INTO mentat.datoms (e, a, v, value_type_tag, tx, added) VALUES
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
                (33, 1, 'db.unique/identity'::bytea,   8, 1000000, true);
            "#,
        )?;
        Ok(())
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

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"person1\" :person/name \"Alice\"]
                 [:db/add \"person1\" :person/age 30]]
            '::TEXT)",
        )
        .expect("Transaction failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e :in $ ?name :where [?e :person/name ?name]]'::TEXT,
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
        Spi::run(
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
        .expect("Transaction 1 failed");

        let tx1 = Spi::get_one::<i64>("SELECT MAX(tx) FROM mentat.datoms")
            .expect("Failed to get tx1")
            .expect("tx1 is null");

        Spi::run("SELECT mentat_transact('[[:db/add \"p1\" :person/age 26]]'::TEXT)")
            .expect("Transaction 2 failed");

        let tx2 = Spi::get_one::<i64>(&format!(
            "SELECT MAX(tx) FROM mentat.datoms WHERE tx > {}", tx1
        ))
            .expect("Failed to get tx2")
            .expect("tx2 is null");

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"p1\" :person/age 27]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 30]]
            '::TEXT)",
        )
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

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"status-attr\" :db/ident :person/status]
                 [:db/add \"status-attr\" :db/valueType :db.type/string]
                 [:db/add \"status-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/status \"active\"]]
            '::TEXT)",
        )
        .expect("Transaction 1 failed");

        Spi::run(
            "SELECT mentat_transact('[[:db/retract \"p1\" :person/status \"active\"]]'::TEXT)",
        )
        .expect("Retraction failed");

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
                 :in $
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

        Spi::run(
            "SELECT mentat_transact('
                [[:db/add \"age-attr\" :db/ident :person/age]
                 [:db/add \"age-attr\" :db/valueType :db.type/long]
                 [:db/add \"age-attr\" :db/cardinality :db.cardinality/one]
                 [:db/add \"p1\" :person/name \"Alice\"]
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
                 (or [[?p :person/role \"admin\"]]
                     [[?p :person/role \"moderator\"]])]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected array");

        assert_eq!(results.len(), 2, "Expected 2 results (admin and moderator)");
    }

    #[pg_test]
    fn test_pg_rule_bind() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

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
}
