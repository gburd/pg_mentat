// Schema evolution tests: adding attributes over time, using new attributes
// with existing data, and schema metadata changes.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    // ========================================================================
    // Add attributes incrementally
    // ========================================================================

    #[pg_test]
    fn test_se_add_attr_after_data() {
        setup();
        // Define initial schema and data
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :se/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema v1");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :se/name \"Alice\"]]'::TEXT)").expect("data v1");

        // Add new attribute
        Spi::run("SELECT mentat_transact('[{:db/id \"b\" :db/ident :se/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema v2");

        // Use new attribute with existing entity
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :se/name \"Alice\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let eid = j["result"].as_i64().expect("eid");

        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :se/age 30]]'::TEXT)", eid)).expect("add age");

        let q2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n ?a :where [{e} :se/name ?n] [{e} :se/age ?a]]'::TEXT, '{{}}'::jsonb)::TEXT", e = eid
        )).expect("q").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(j2["results"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_se_add_5_attrs_sequentially() {
        setup();
        for i in 0..5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"a\" :db/ident :se.seq/attr-{i} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}]'::TEXT)",
                i = i
            )).expect("add attr");

            // Immediately use it
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{i}\" :se.seq/attr-{i} \"val-{i}\"]]'::TEXT)",
                i = i
            )).expect("use attr");
        }

        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        for i in 0..5 {
            assert!(result.contains(&format!("se.seq/attr-{}", i)), "attr-{} should exist", i);
        }
    }

    // ========================================================================
    // Add cardinality-many after cardinality-one data exists
    // ========================================================================

    #[pg_test]
    fn test_se_add_many_attr_later() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :se/item :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :se/item \"item1\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Add a cardinality-many attribute
        Spi::run("SELECT mentat_transact('[{:db/id \"b\" :db/ident :se/labels :db/valueType :db.type/string :db/cardinality :db.cardinality/many}]'::TEXT)").expect("add many attr");

        // Use it on existing entity
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :se/labels \"urgent\"] [:db/add {} :se/labels \"bug\"]]'::TEXT)", eid, eid
        )).expect("add labels");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?l ...] :where [{} :se/labels ?l]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Add unique constraint on new attribute
    // ========================================================================

    #[pg_test]
    fn test_se_add_unique_attr_later() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :se/gen-name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema v1");

        // Add data
        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :se/gen-name \"Alice\"] [:db/add \"e2\" :se/gen-name \"Bob\"]]'::TEXT)").expect("data");

        // Add unique identity attribute
        Spi::run("SELECT mentat_transact('[{:db/id \"b\" :db/ident :se/gen-email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}]'::TEXT)").expect("schema v2");

        // Use it for upsert
        Spi::run("SELECT mentat_transact('[{:db/id \"u\" :se/gen-email \"alice@test.com\" :se/gen-name \"Alice Updated\"}]'::TEXT)").expect("first upsert");
        Spi::run("SELECT mentat_transact('[{:db/id \"u\" :se/gen-email \"alice@test.com\" :se/gen-name \"Alice v3\"}]'::TEXT)").expect("second upsert");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':se/gen-email') AND v_text = 'alice@test.com' AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 1, "Upserts should produce exactly 1 entity");
    }

    // ========================================================================
    // Multiple schema transactions then data
    // ========================================================================

    #[pg_test]
    fn test_se_three_schema_txs_then_data() {
        setup();

        // Schema TX 1
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :se.m/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("s1");

        // Schema TX 2
        Spi::run("SELECT mentat_transact('[{:db/id \"b\" :db/ident :se.m/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("s2");

        // Schema TX 3
        Spi::run("SELECT mentat_transact('[{:db/id \"c\" :db/ident :se.m/active :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}]'::TEXT)").expect("s3");

        // Data using all three
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :se.m/name \"Test\" :se.m/age 25 :se.m/active true}]'::TEXT)",
        ).expect("data");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?a ?f :where [?e :se.m/name ?n] [?e :se.m/age ?a] [?e :se.m/active ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    // ========================================================================
    // Schema with all types added incrementally
    // ========================================================================

    #[pg_test]
    fn test_se_all_types_incremental() {
        setup();
        let types = vec![
            ("str", "string"),
            ("lng", "long"),
            ("dbl", "double"),
            ("boo", "boolean"),
            ("kw", "keyword"),
            ("ref", "ref"),
            ("ins", "instant"),
            ("uid", "uuid"),
        ];

        for (suffix, vtype) in &types {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"a\" :db/ident :se.inc/{} :db/valueType :db.type/{} :db/cardinality :db.cardinality/one}}]'::TEXT)",
                suffix, vtype
            )).expect(&format!("add {} attr", vtype));
        }

        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        for (suffix, _) in &types {
            assert!(result.contains(&format!("se.inc/{}", suffix)), "{} should be in schema", suffix);
        }
    }

    // ========================================================================
    // Schema + data interleaved
    // ========================================================================

    #[pg_test]
    fn test_se_interleaved_schema_and_data() {
        setup();

        // Round 1: define + use
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :se.il/r1 :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema 1");
        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :se.il/r1 \"round1\"]]'::TEXT)").expect("data 1");

        // Round 2: define + use
        Spi::run("SELECT mentat_transact('[{:db/id \"b\" :db/ident :se.il/r2 :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema 2");
        Spi::run("SELECT mentat_transact('[[:db/add \"e2\" :se.il/r2 42]]'::TEXT)").expect("data 2");

        // Round 3: define + use
        Spi::run("SELECT mentat_transact('[{:db/id \"c\" :db/ident :se.il/r3 :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema 3");
        Spi::run("SELECT mentat_transact('[[:db/add \"e3\" :se.il/r3 true]]'::TEXT)").expect("data 3");

        // Verify all exist
        let q1 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :se.il/r1 ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v1: serde_json::Value = serde_json::from_str(&q1).expect("parse");
        assert_eq!(v1["result"].as_str().expect("s"), "round1");

        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :se.il/r2 ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_i64().expect("v"), 42);

        let q3 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :se.il/r3 ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v3: serde_json::Value = serde_json::from_str(&q3).expect("parse");
        assert_eq!(v3["result"].as_bool().expect("b"), true);
    }

    // ========================================================================
    // Schema namespace organization
    // ========================================================================

    #[pg_test]
    fn test_se_multiple_namespaces() {
        setup();
        // Define attributes in different namespaces
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :user/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            {:db/id \"b\" :db/ident :user/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            {:db/id \"c\" :db/ident :product/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            {:db/id \"d\" :db/ident :product/price :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
            {:db/id \"e\" :db/ident :order/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
            {:db/id \"f\" :db/ident :order/total :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
        ]'::TEXT)").expect("multi-ns schema");

        // Use them
        Spi::run("SELECT mentat_transact('[
            {:db/id \"u\" :user/name \"Alice\" :user/email \"alice@test.com\"}
            {:db/id \"p\" :product/name \"Widget\" :product/price 9.99}
            {:db/id \"o\" :order/status :pending :order/total 999}
        ]'::TEXT)").expect("data");

        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(result.contains("user/name"));
        assert!(result.contains("product/name"));
        assert!(result.contains("order/status"));
    }
}
