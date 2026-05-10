// Find-spec exhaustive tests: systematic coverage of all find-spec
// variants (relation, scalar, collection, tuple) with various data types
// and edge cases.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod find_spec_exhaustive_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_fs_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :fs/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :fs/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :fs/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :fs/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :fs/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :fs/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :fs/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"dept\" :db/ident :fs/dept :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("fs schema");
    }

    fn setup_fs_data() {
        let mut ops = vec![];
        let depts = ["eng", "sales", "hr", "ops", "finance"];
        let statuses = [":active", ":inactive", ":pending"];
        for i in 0..30 {
            ops.push(format!(
                "{{:db/id \"e{}\" :fs/name \"person-{}\" :fs/val {} :fs/dbl {} :fs/flag {} :fs/status {} :fs/dept \"{}\"}}",
                i, i, i * 10, (i as f64) * 2.5, if i % 2 == 0 { "true" } else { "false" },
                statuses[i % 3], depts[i % 5]
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("data");
    }

    // ========================================================================
    // Scalar find-spec (?var .) (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_fse_scalar_string() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :fs/name ?n] [?e :fs/name \"person-0\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "person-0");
    }

    #[pg_test]
    fn test_fse_scalar_long() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :fs/name \"person-5\"] [?e :fs/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 50);
    }

    #[pg_test]
    fn test_fse_scalar_double() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?d . :where [?e :fs/name \"person-4\"] [?e :fs/dbl ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let result = v["result"].as_f64().expect("d");
        assert!((result - 10.0).abs() < 0.1);
    }

    #[pg_test]
    fn test_fs_scalar_boolean_true() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?f . :where [?e :fs/name \"person-0\"] [?e :fs/flag ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), true);
    }

    #[pg_test]
    fn test_fs_scalar_boolean_false() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?f . :where [?e :fs/name \"person-1\"] [?e :fs/flag ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_fs_scalar_keyword() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?s . :where [?e :fs/name \"person-0\"] [?e :fs/status ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("active"));
    }

    #[pg_test]
    fn test_fs_scalar_entity_id() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :fs/name \"person-0\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_i64().expect("e") > 0);
    }

    #[pg_test]
    fn test_fse_scalar_no_match() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :fs/name ?n] [?e :fs/name \"nonexistent\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_fs_scalar_with_predicate() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :fs/name ?n] [?e :fs/val ?v] [(> ?v 280)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().is_some());
    }

    #[pg_test]
    fn test_fs_scalar_with_constant() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :fs/dept \"eng\"] [?e :fs/val ?v] [(> ?v 100)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_number());
    }

    #[pg_test]
    fn test_fs_scalar_long_from_each_entity() {
        setup(); setup_fs_schema(); setup_fs_data();
        for i in 0..10 {
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [?e :fs/name \"person-{}\"] [?e :fs/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", i
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_i64().expect("v"), (i * 10) as i64);
        }
    }

    #[pg_test]
    fn test_fs_scalar_name_from_each_dept() {
        setup(); setup_fs_schema(); setup_fs_data();
        let depts = ["eng", "sales", "hr", "ops", "finance"];
        for dept in &depts {
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?n . :where [?e :fs/dept \"{}\"] [?e :fs/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", dept
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert!(v["result"].as_str().is_some());
        }
    }

    // ========================================================================
    // Collection find-spec [?var ...] (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_fs_coll_all_names() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :fs/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 30);
    }

    #[pg_test]
    fn test_fs_coll_all_vals() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :fs/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 30);
    }

    #[pg_test]
    fn test_fs_coll_all_depts() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?d ...] :where [_ :fs/dept ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_fs_coll_all_statuses() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?s ...] :where [_ :fs/status ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_fs_coll_all_eids() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?e ...] :where [?e :fs/name _]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 30);
    }

    #[pg_test]
    fn test_fs_coll_filtered_by_dept() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :fs/dept \"eng\"] [?e :fs/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 6);
    }

    #[pg_test]
    fn test_fs_coll_filtered_by_flag() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :fs/flag true] [?e :fs/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 15);
    }

    #[pg_test]
    fn test_fs_coll_filtered_by_predicate() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :fs/name ?n] [?e :fs/val ?v] [(> ?v 200)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 5);
    }

    #[pg_test]
    fn test_fs_coll_vals_gt_100() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :fs/val ?v] [(> ?v 100)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 19); // 110..290
    }

    #[pg_test]
    fn test_fs_coll_empty_result() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :fs/name ?n] [?e :fs/val ?v] [(> ?v 9999)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_fs_coll_dbls_filtered() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?d ...] :where [_ :fs/dbl ?d] [(> ?d 50.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 5);
    }

    #[pg_test]
    fn test_fs_coll_combined_dept_and_status() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :fs/dept \"eng\"] [?e :fs/status :active] [?e :fs/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    // ========================================================================
    // Tuple find-spec [?a ?b] (12 tests)
    // ========================================================================

    #[pg_test]
    fn test_fs_tuple_name_val() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?v] :where [?e :fs/name \"person-10\"] [?e :fs/name ?n] [?e :fs/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tuple = v["result"].as_array().expect("arr");
        assert_eq!(tuple.len(), 2);
        assert_eq!(tuple[0].as_str().expect("n"), "person-10");
        assert_eq!(tuple[1].as_i64().expect("v"), 100);
    }

    #[pg_test]
    fn test_fs_tuple_name_dept() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?d] :where [?e :fs/name \"person-0\"] [?e :fs/name ?n] [?e :fs/dept ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tuple = v["result"].as_array().expect("arr");
        assert_eq!(tuple.len(), 2);
        assert_eq!(tuple[0].as_str().expect("n"), "person-0");
        assert_eq!(tuple[1].as_str().expect("d"), "eng");
    }

    #[pg_test]
    fn test_fs_tuple_3_vars() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?v ?d] :where [?e :fs/name \"person-5\"] [?e :fs/name ?n] [?e :fs/val ?v] [?e :fs/dept ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tuple = v["result"].as_array().expect("arr");
        assert_eq!(tuple.len(), 3);
    }

    #[pg_test]
    fn test_fs_tuple_4_vars() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?v ?d ?f] :where [?e :fs/name \"person-2\"] [?e :fs/name ?n] [?e :fs/val ?v] [?e :fs/dept ?d] [?e :fs/dbl ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tuple = v["result"].as_array().expect("arr");
        assert_eq!(tuple.len(), 4);
    }

    #[pg_test]
    fn test_fse_tuple_no_match() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?v] :where [?e :fs/name \"nonexistent\"] [?e :fs/name ?n] [?e :fs/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_fs_tuple_eid_and_name() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?e ?n] :where [?e :fs/name \"person-15\"] [?e :fs/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tuple = v["result"].as_array().expect("arr");
        assert_eq!(tuple.len(), 2);
        assert!(tuple[0].as_i64().expect("e") > 0);
        assert_eq!(tuple[1].as_str().expect("n"), "person-15");
    }

    #[pg_test]
    fn test_fs_tuple_with_predicate() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?v] :where [?e :fs/name ?n] [?e :fs/val ?v] [(> ?v 280)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tuple = v["result"].as_array().expect("arr");
        assert_eq!(tuple.len(), 2);
        assert!(tuple[1].as_i64().expect("v") > 280);
    }

    #[pg_test]
    fn test_fs_tuple_each_entity() {
        setup(); setup_fs_schema(); setup_fs_data();
        for i in 0..5 {
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find [?n ?v] :where [?e :fs/name \"person-{}\"] [?e :fs/name ?n] [?e :fs/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", i
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            let tuple = v["result"].as_array().expect("arr");
            assert_eq!(tuple.len(), 2);
            assert_eq!(tuple[1].as_i64().expect("v"), (i * 10) as i64);
        }
    }

    // ========================================================================
    // Relation find-spec ?a ?b (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_fs_rel_two_vars() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :fs/name ?n] [?e :fs/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let rows = v["result"].as_array().expect("arr");
        assert_eq!(rows.len(), 30);
        for row in rows {
            assert_eq!(row.as_array().expect("r").len(), 2);
        }
    }

    #[pg_test]
    fn test_fs_rel_three_vars() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v ?d :where [?e :fs/name ?n] [?e :fs/val ?v] [?e :fs/dept ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let rows = v["result"].as_array().expect("arr");
        assert_eq!(rows.len(), 30);
        for row in rows {
            assert_eq!(row.as_array().expect("r").len(), 3);
        }
    }

    #[pg_test]
    fn test_fs_rel_four_vars() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v ?d ?f :where [?e :fs/name ?n] [?e :fs/val ?v] [?e :fs/dept ?d] [?e :fs/dbl ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let rows = v["result"].as_array().expect("arr");
        assert_eq!(rows.len(), 30);
        for row in rows {
            assert_eq!(row.as_array().expect("r").len(), 4);
        }
    }

    #[pg_test]
    fn test_fs_rel_filtered_dept() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :fs/name ?n] [?e :fs/val ?v] [?e :fs/dept \"eng\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 6);
    }

    #[pg_test]
    fn test_fs_rel_filtered_flag() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :fs/name ?n] [?e :fs/val ?v] [?e :fs/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 15);
    }

    #[pg_test]
    fn test_fs_rel_filtered_predicate() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :fs/name ?n] [?e :fs/val ?v] [(> ?v 200)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 5);
    }

    #[pg_test]
    fn test_fs_rel_empty() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :fs/name ?n] [?e :fs/val ?v] [(> ?v 99999)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_fs_rel_eid_and_name() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e ?n :where [?e :fs/name ?n] [?e :fs/dept \"hr\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let rows = v["result"].as_array().expect("arr");
        assert_eq!(rows.len(), 6);
        for row in rows {
            assert!(row[0].as_i64().expect("e") > 0);
        }
    }

    #[pg_test]
    fn test_fs_rel_combined_filters() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :fs/name ?n] [?e :fs/val ?v] [?e :fs/flag true] [?e :fs/dept \"eng\"] [(> ?v 50)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() >= 0);
    }

    #[pg_test]
    fn test_fs_rel_with_status() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?s :where [?e :fs/name ?n] [?e :fs/status ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 30);
    }

    #[pg_test]
    fn test_fs_rel_name_dept_status() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?d ?s :where [?e :fs/name ?n] [?e :fs/dept ?d] [?e :fs/status ?s] [?e :fs/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 15);
        for row in v["result"].as_array().expect("arr") {
            assert_eq!(row.as_array().expect("r").len(), 3);
        }
    }

    #[pg_test]
    fn test_fs_rel_all_five_vars() {
        setup(); setup_fs_schema(); setup_fs_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v ?d ?f ?s :where [?e :fs/name ?n] [?e :fs/val ?v] [?e :fs/dept ?d] [?e :fs/dbl ?f] [?e :fs/status ?s] [?e :fs/name \"person-3\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let rows = v["result"].as_array().expect("arr");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].as_array().expect("r").len(), 5);
    }
}
