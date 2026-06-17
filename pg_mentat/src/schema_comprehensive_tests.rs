// Comprehensive schema tests: attribute definitions, modifications,
// constraints, and schema introspection.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._test_raises_error(stmt TEXT) RETURNS BOOLEAN
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN false;
             EXCEPTION WHEN OTHERS THEN
                 RETURN true;
             END;
             $$"
        ).expect("create helper");
    }

    fn raises_error(sql: &str) -> bool {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<bool>(&format!(
            "SELECT mentat._test_raises_error('{}')", escaped
        )).expect("raises_error call").unwrap_or(false)
    }

    // ========================================================================
    // Value type definitions
    // ========================================================================

    #[pg_test]
    fn test_sc_define_string_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/s1 :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("define");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sc/s1 \"hello\"]]'::TEXT)").expect("use");
    }

    #[pg_test]
    fn test_sc_define_long_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/l1 :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("define");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sc/l1 42]]'::TEXT)").expect("use");
    }

    #[pg_test]
    fn test_sc_define_double_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/d1 :db/valueType :db.type/double :db/cardinality :db.cardinality/one}]'::TEXT)").expect("define");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sc/d1 3.14]]'::TEXT)").expect("use");
    }

    #[pg_test]
    fn test_sc_define_boolean_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/b1 :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}]'::TEXT)").expect("define");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sc/b1 true]]'::TEXT)").expect("use");
    }

    #[pg_test]
    fn test_sc_define_ref_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/r1 :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}]'::TEXT)").expect("define");
        Spi::run("SELECT mentat_transact('[[:db/add \"p\" :db/ident :sc/dummy] [:db/add \"e\" :sc/r1 \"p\"]]'::TEXT)").expect("use");
    }

    #[pg_test]
    fn test_sc_define_keyword_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/k1 :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}]'::TEXT)").expect("define");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sc/k1 :active]]'::TEXT)").expect("use");
    }

    #[pg_test]
    fn test_sc_define_instant_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/i1 :db/valueType :db.type/instant :db/cardinality :db.cardinality/one}]'::TEXT)").expect("define");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sc/i1 #inst \"2024-01-01T00:00:00Z\"]]'::TEXT)").expect("use");
    }

    #[pg_test]
    fn test_sc_define_uuid_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/u1 :db/valueType :db.type/uuid :db/cardinality :db.cardinality/one}]'::TEXT)").expect("define");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sc/u1 #uuid \"550e8400-e29b-41d4-a716-446655440000\"]]'::TEXT)").expect("use");
    }

    #[pg_test]
    fn test_sc_define_bytes_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/y1 :db/valueType :db.type/bytes :db/cardinality :db.cardinality/one}]'::TEXT)").expect("define");
    }

    // ========================================================================
    // Cardinality
    // ========================================================================

    #[pg_test]
    fn test_sc_cardinality_one_replaces() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/co :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :sc/co \"first\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :sc/co \"second\"]]'::TEXT)", eid
        )).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :sc/co ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "second");
    }

    #[pg_test]
    fn test_sc_cardinality_many_accumulates() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/cm :db/valueType :db.type/string :db/cardinality :db.cardinality/many}]'::TEXT)").expect("schema");
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :sc/cm \"a\"] [:db/add \"e\" :sc/cm \"b\"] [:db/add \"e\" :sc/cm \"c\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :sc/cm ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    // ========================================================================
    // Unique constraints
    // ========================================================================

    #[pg_test]
    fn test_sc_unique_identity_enables_upsert() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :sc/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            {:db/id \"b\" :db/ident :sc/uval :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
        ]'::TEXT)").expect("schema");

        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :sc/uid \"U1\" :sc/uval 10}]'::TEXT)").expect("first");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :sc/uid \"U1\" :sc/uval 20}]'::TEXT)").expect("upsert");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':sc/uid')
             AND v_text = 'U1' AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_sc_unique_value_rejects_dup() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/uv :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/value}]'::TEXT)").expect("schema");

        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :sc/uv \"unique-val\"]]'::TEXT)").expect("first");
        assert!(
            raises_error("SELECT mentat_transact('[[:db/add \"e2\" :sc/uv \"unique-val\"]]'::TEXT)"),
            "Duplicate unique value should be rejected"
        );
    }

    // ========================================================================
    // Schema + data in same tx
    // ========================================================================

    #[pg_test]
    fn test_sc_schema_and_data_same_tx() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"attr\" :db/ident :sc/combo :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            [:db/add \"e1\" :sc/combo \"first\"]
            [:db/add \"e2\" :sc/combo \"second\"]
        ]'::TEXT)").expect("schema+data");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :sc/combo ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Multiple attributes in one tx
    // ========================================================================

    #[pg_test]
    fn test_sc_define_20_attrs_one_tx() {
        setup();
        let mut ops = Vec::new();
        for i in 0..20 {
            ops.push(format!(
                "{{:db/id \"a{i}\" :db/ident :sc.bulk/attr-{i} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}",
                i = i
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n")
        )).expect("bulk schema");

        // Verify all 20 are usable
        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{i}\" :sc.bulk/attr-{i} \"val-{i}\"]]'::TEXT)",
                i = i
            )).expect(&format!("use attr-{}", i));
        }
    }

    // ========================================================================
    // Schema introspection (mentat_schema)
    // ========================================================================

    #[pg_test]
    fn test_sc_mentat_schema_returns_json() {
        setup();
        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema query")
            .expect("NULL");
        let _json: serde_json::Value = serde_json::from_str(&result).expect("Should be valid JSON");
    }

    #[pg_test]
    fn test_sc_mentat_schema_contains_bootstrap_attrs() {
        setup();
        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema query")
            .expect("NULL");
        assert!(result.contains("db/ident"), "Should contain :db/ident");
        assert!(result.contains("db/valueType"), "Should contain :db/valueType");
        assert!(result.contains("db/cardinality"), "Should contain :db/cardinality");
    }

    #[pg_test]
    fn test_sc_mentat_schema_contains_user_attrs() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/introspect :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");

        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema query")
            .expect("NULL");
        assert!(result.contains("sc/introspect"), "Schema should contain user-defined attribute");
    }

    // ========================================================================
    // Schema error handling
    // ========================================================================

    #[pg_test]
    fn test_sc_error_missing_value_type() {
        setup();
        // A schema attribute without :db/valueType is incomplete and must not
        // be installed. Attempt it inside the error-catching helper (a raised
        // error would otherwise abort the test transaction), then assert no
        // such attribute exists in mentat.schema.
        let _ = raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/bad1 :db/cardinality :db.cardinality/one}]'::TEXT)");
        let installed = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.schema WHERE ident = ':sc/bad1'",
        )
        .expect("count")
        .expect("NULL");
        assert_eq!(installed, 0, "Schema attribute without valueType must not be installed");
    }

    #[pg_test]
    fn test_sc_error_missing_cardinality() {
        setup();
        // A schema attribute without :db/cardinality is incomplete and must not
        // be installed. Attempt it inside the error-catching helper, then
        // assert no such attribute exists in mentat.schema.
        let _ = raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/bad2 :db/valueType :db.type/string}]'::TEXT)");
        let installed = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.schema WHERE ident = ':sc/bad2'",
        )
        .expect("count")
        .expect("NULL");
        assert_eq!(installed, 0, "Schema attribute without cardinality must not be installed");
    }

    #[pg_test]
    fn test_sc_error_invalid_value_type() {
        setup();
        assert!(
            raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/bad3 :db/valueType :db.type/invalid :db/cardinality :db.cardinality/one}]'::TEXT)"),
            "Invalid value type should fail"
        );
    }

    #[pg_test]
    fn test_sc_error_invalid_cardinality() {
        setup();
        assert!(
            raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/bad4 :db/valueType :db.type/string :db/cardinality :db.cardinality/invalid}]'::TEXT)"),
            "Invalid cardinality should fail"
        );
    }

    // ========================================================================
    // Optional schema properties
    // ========================================================================

    #[pg_test]
    fn test_sc_doc_string() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/documented :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/doc \"A documented attribute\"}]'::TEXT)").expect("schema with doc");
    }

    #[pg_test]
    fn test_sc_index() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/indexed :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/index true}]'::TEXT)").expect("schema with index");
    }

    #[pg_test]
    fn test_sc_fulltext() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/ftext :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/fulltext true}]'::TEXT)").expect("schema with fulltext");
    }

    #[pg_test]
    fn test_sc_component() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/comp :db/valueType :db.type/ref :db/cardinality :db.cardinality/many :db/isComponent true}]'::TEXT)").expect("schema with component");
    }

    #[pg_test]
    fn test_sc_no_history() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sc/nohist :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/noHistory true}]'::TEXT)").expect("schema with noHistory");
    }

    // ========================================================================
    // Sequential schema definitions
    // ========================================================================

    #[pg_test]
    fn test_sc_sequential_schema_definitions() {
        setup();
        for i in 0..30 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"a\" :db/ident :sc.seq/attr-{i} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}]'::TEXT)",
                i = i
            )).expect(&format!("define attr {}", i));
        }

        // Verify all 30 exist in schema
        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        for i in 0..30 {
            assert!(
                result.contains(&format!("sc.seq/attr-{}", i)),
                "attr-{} missing from schema",
                i
            );
        }
    }

    // ========================================================================
    // All properties combined
    // ========================================================================

    #[pg_test]
    fn test_sc_all_properties() {
        setup();
        Spi::run("SELECT mentat_transact('[{
            :db/id \"a\"
            :db/ident :sc/all-props
            :db/valueType :db.type/string
            :db/cardinality :db.cardinality/one
            :db/unique :db.unique/identity
            :db/index true
            :db/doc \"Attribute with all properties\"
            :db/noHistory true
        }]'::TEXT)").expect("all properties");

        // Should be usable
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sc/all-props \"test\"]]'::TEXT)").expect("use");
    }
}
