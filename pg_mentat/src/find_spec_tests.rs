// Exhaustive find-spec tests: relation, scalar, collection, tuple
// across different data shapes.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod find_spec_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_fs_schema_and_data() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :fs/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :fs/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :fs/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :fs/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :fs/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        ).expect("fs schema");

        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e1\" :fs/name \"Alice\" :fs/val 10 :fs/flag true :fs/dbl 1.1}
                {:db/id \"e2\" :fs/name \"Bob\" :fs/val 20 :fs/flag false :fs/dbl 2.2}
                {:db/id \"e3\" :fs/name \"Carol\" :fs/val 30 :fs/flag true :fs/dbl 3.3}
                [:db/add \"e1\" :fs/tags \"t1\"]
                [:db/add \"e1\" :fs/tags \"t2\"]
                [:db/add \"e2\" :fs/tags \"t3\"]
            ]'::TEXT)",
        ).expect("fs data");
    }

    // ========================================================================
    // Relation find-spec (default)
    // ========================================================================

    #[pg_test]
    fn test_fs_relation_single_var() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name :where [?e :fs/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        assert_eq!(results.len(), 3);
        // Each row should be a 1-element array
        for r in results {
            assert_eq!(r.as_array().expect("row").len(), 1);
        }
    }

    #[pg_test]
    fn test_fs_relation_two_vars() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name ?val :where [?e :fs/name ?name] [?e :fs/val ?val]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        assert_eq!(results.len(), 3);
        for r in results {
            assert_eq!(r.as_array().expect("row").len(), 2);
        }
    }

    #[pg_test]
    fn test_fs_relation_three_vars() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name ?val ?flag :where [?e :fs/name ?name] [?e :fs/val ?val] [?e :fs/flag ?flag]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        assert_eq!(results.len(), 3);
        for r in results {
            assert_eq!(r.as_array().expect("row").len(), 3);
        }
    }

    #[pg_test]
    fn test_fs_relation_four_vars() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name ?val ?flag ?dbl :where [?e :fs/name ?name] [?e :fs/val ?val] [?e :fs/flag ?flag] [?e :fs/dbl ?dbl]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        assert_eq!(results.len(), 3);
        for r in results {
            assert_eq!(r.as_array().expect("row").len(), 4);
        }
    }

    #[pg_test]
    fn test_fs_relation_empty() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name :where [?e :fs/name ?name] [?e :fs/val 999]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["results"].as_array().expect("arr").len(), 0);
    }

    // ========================================================================
    // Scalar find-spec
    // ========================================================================

    #[pg_test]
    fn test_fs_scalar_string() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name . :where [?e :fs/name ?name] [?e :fs/name \"Alice\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_str().expect("s"), "Alice");
    }

    #[pg_test]
    fn test_fs_scalar_long() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?val . :where [?e :fs/name \"Bob\"] [?e :fs/val ?val]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_i64().expect("v"), 20);
    }

    #[pg_test]
    fn test_fs_scalar_boolean() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?f . :where [?e :fs/name \"Alice\"] [?e :fs/flag ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_bool().expect("b"), true);
    }

    #[pg_test]
    fn test_fs_scalar_double() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?d . :where [?e :fs/name \"Carol\"] [?e :fs/dbl ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((j["result"].as_f64().expect("d") - 3.3).abs() < 0.01);
    }

    #[pg_test]
    fn test_fs_scalar_no_match() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name . :where [?e :fs/name ?name] [?e :fs/val 999]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(j["result"].is_null());
    }

    // ========================================================================
    // Collection find-spec
    // ========================================================================

    #[pg_test]
    fn test_fs_collection_strings() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :fs/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let arr = j["result"].as_array().expect("arr");
        assert_eq!(arr.len(), 3);
    }

    #[pg_test]
    fn test_fs_collection_longs() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :fs/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let arr = j["result"].as_array().expect("arr");
        assert_eq!(arr.len(), 3);
    }

    #[pg_test]
    fn test_fs_collection_empty() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :fs/name ?name] [?e :fs/val 999]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_fs_collection_with_filter() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :fs/name ?name] [?e :fs/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let arr = j["result"].as_array().expect("arr");
        assert_eq!(arr.len(), 2); // Alice and Carol
    }

    // ========================================================================
    // Tuple find-spec
    // ========================================================================

    #[pg_test]
    fn test_fs_tuple_two_vars() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ?val] :where [?e :fs/name ?name] [?e :fs/name \"Alice\"] [?e :fs/val ?val]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tuple = j["result"].as_array().expect("tuple");
        assert_eq!(tuple.len(), 2);
        assert_eq!(tuple[0].as_str().expect("name"), "Alice");
        assert_eq!(tuple[1].as_i64().expect("val"), 10);
    }

    #[pg_test]
    fn test_fs_tuple_three_vars() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ?val ?flag] :where [?e :fs/name ?name] [?e :fs/name \"Bob\"] [?e :fs/val ?val] [?e :fs/flag ?flag]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tuple = j["result"].as_array().expect("tuple");
        assert_eq!(tuple.len(), 3);
        assert_eq!(tuple[0].as_str().expect("name"), "Bob");
        assert_eq!(tuple[1].as_i64().expect("val"), 20);
        assert_eq!(tuple[2].as_bool().expect("flag"), false);
    }

    #[pg_test]
    fn test_fs_tuple_no_match() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ?val] :where [?e :fs/name ?name] [?e :fs/name \"Nobody\"] [?e :fs/val ?val]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(j["result"].is_null() || j["result"].as_array().map_or(false, |a| a.is_empty()));
    }

    // ========================================================================
    // Entity ID in find
    // ========================================================================

    #[pg_test]
    fn test_fs_entity_id_in_relation() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e ?name :where [?e :fs/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        assert_eq!(results.len(), 3);
        for r in results {
            let row = r.as_array().expect("row");
            assert!(row[0].as_i64().is_some(), "Entity ID should be numeric");
            assert!(row[1].as_str().is_some(), "Name should be string");
        }
    }

    #[pg_test]
    fn test_fs_entity_id_scalar() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :fs/name \"Alice\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(j["result"].as_i64().is_some());
    }

    #[pg_test]
    fn test_fs_entity_id_collection() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?e ...] :where [?e :fs/name _]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let eids = j["result"].as_array().expect("arr");
        assert_eq!(eids.len(), 3);
    }

    // ========================================================================
    // Cardinality-many in find
    // ========================================================================

    #[pg_test]
    fn test_fs_many_in_collection() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :fs/name \"Alice\"] [?e :fs/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_fs_many_in_relation() {
        setup(); setup_fs_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name ?tag :where [?e :fs/name ?name] [?e :fs/tags ?tag]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        // Alice has 2 tags, Bob has 1 tag => 3 rows
        assert_eq!(results.len(), 3);
    }
}
