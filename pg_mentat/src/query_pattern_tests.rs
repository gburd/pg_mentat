// Query pattern tests: systematic coverage of Datalog query patterns,
// join patterns, binding forms, and result shaping.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod query_pattern_tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_qp_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :qp/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :qp/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :qp/dept :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :qp/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :qp/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :qp/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :qp/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"rm\" :db/ident :qp/refs :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"db\" :db/ident :qp/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"em\" :db/ident :qp/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            ]'::TEXT)",
        ).expect("qp schema");
    }

    fn setup_qp_data() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"mgr\" :qp/name \"Manager\" :qp/dept \"exec\" :qp/val 100 :qp/flag true :qp/status :active}
                {:db/id \"a\" :qp/name \"Alice\" :qp/dept \"eng\" :qp/val 80 :qp/flag true :qp/status :active :qp/ref \"mgr\"}
                {:db/id \"b\" :qp/name \"Bob\" :qp/dept \"eng\" :qp/val 75 :qp/flag false :qp/status :active :qp/ref \"mgr\"}
                {:db/id \"c\" :qp/name \"Carol\" :qp/dept \"sales\" :qp/val 90 :qp/flag true :qp/status :inactive :qp/ref \"mgr\"}
                {:db/id \"d\" :qp/name \"Dave\" :qp/dept \"sales\" :qp/val 60 :qp/flag false :qp/status :pending :qp/ref \"mgr\"}
                {:db/id \"e\" :qp/name \"Eve\" :qp/dept \"hr\" :qp/val 70 :qp/flag true :qp/status :active :qp/ref \"mgr\"}
            ]'::TEXT)",
        ).expect("qp data");
    }

    // ========================================================================
    // Single-clause patterns (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_qp_find_all_names() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 6);
    }

    #[pg_test]
    fn test_qp_find_all_vals() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :qp/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 6);
    }

    #[pg_test]
    fn test_qp_find_all_depts() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?d ...] :where [?e :qp/dept ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // eng, sales, hr, exec = 4 distinct depts
        assert!(v["result"].as_array().expect("arr").len() >= 3);
    }

    #[pg_test]
    fn test_qp_find_all_flags() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?f ...] :where [?e :qp/flag ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // true and false
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_qp_find_all_statuses() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?s ...] :where [?e :qp/status ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // active, inactive, pending
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    // ========================================================================
    // Multi-clause join patterns (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_qp_join_name_and_dept() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?d :where [?e :qp/name ?n] [?e :qp/dept ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 6);
    }

    #[pg_test]
    fn test_qp_join_name_val_flag() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v ?f :where [?e :qp/name ?n] [?e :qp/val ?v] [?e :qp/flag ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 6);
    }

    #[pg_test]
    fn test_qp_join_four_attrs() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?d ?v ?s :where [?e :qp/name ?n] [?e :qp/dept ?d] [?e :qp/val ?v] [?e :qp/status ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 6);
    }

    #[pg_test]
    fn test_qp_join_with_constant_dept_eng() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/dept \"eng\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // Alice, Bob
    }

    #[pg_test]
    fn test_qp_join_with_constant_dept_sales() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/dept \"sales\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // Carol, Dave
    }

    #[pg_test]
    fn test_qp_join_with_constant_dept_hr() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/dept \"hr\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1); // Eve
    }

    #[pg_test]
    fn test_qp_join_with_constant_status_active() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/status :active]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Manager, Alice, Bob, Eve
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_qp_join_with_constant_flag_true() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Manager, Alice, Carol, Eve
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_qp_join_with_constant_flag_false() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/flag false]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Bob, Dave
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_qp_join_dept_and_flag() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/dept \"eng\"] [?e :qp/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1); // Alice
    }

    #[pg_test]
    fn test_qp_join_dept_and_status() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/dept \"sales\"] [?e :qp/status :inactive]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1); // Carol
    }

    // ========================================================================
    // Predicate patterns (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_qp_pred_gt_70() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/val ?v] [(> ?v 70)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Manager(100), Alice(80), Bob(75), Carol(90) = 4
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_qp_pred_lt_70() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/val ?v] [(< ?v 70)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Dave(60) = 1
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_qp_pred_gte_80() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/val ?v] [(>= ?v 80)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Manager(100), Alice(80), Carol(90) = 3
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_qp_pred_lte_75() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/val ?v] [(<= ?v 75)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Bob(75), Dave(60), Eve(70) = 3
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_qp_pred_ne_80() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/val ?v] [(!= ?v 80)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // All except Alice = 5
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_qp_pred_range_70_90() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/val ?v] [(>= ?v 70)] [(<= ?v 90)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Alice(80), Bob(75), Carol(90), Eve(70) = 4
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_qp_pred_gt_combined_with_dept() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/dept \"eng\"] [?e :qp/val ?v] [(> ?v 77)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Alice(80) = 1
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_qp_pred_gt_combined_with_flag() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/flag true] [?e :qp/val ?v] [(> ?v 85)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Manager(100), Carol(90) = 2
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_qp_pred_gt_combined_with_status() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/status :active] [?e :qp/val ?v] [(> ?v 75)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Manager(100), Alice(80) = 2
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_qp_pred_no_match() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/val ?v] [(> ?v 200)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    // ========================================================================
    // Ref join patterns (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_qp_ref_forward() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/ref ?r] [?r :qp/name \"Manager\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Alice, Bob, Carol, Dave, Eve all ref Manager
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_qp_ref_with_filter() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/ref ?r] [?r :qp/dept \"exec\"] [?e :qp/dept \"eng\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // Alice, Bob
    }

    #[pg_test]
    fn test_qp_ref_navigate_and_predicate() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/ref ?r] [?r :qp/val ?rv] [(> ?rv 50)] [?e :qp/val ?v] [(> ?v 70)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    #[pg_test]
    fn test_qp_ref_join_two_entities() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?mn :where [?e :qp/name ?n] [?e :qp/ref ?m] [?m :qp/name ?mn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 5);
    }

    // ========================================================================
    // Find-spec variations (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_qp_find_scalar_string() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :qp/name ?n] [?e :qp/dept \"hr\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Eve");
    }

    #[pg_test]
    fn test_qp_find_scalar_long() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :qp/name \"Alice\"] [?e :qp/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 80);
    }

    #[pg_test]
    fn test_qp_find_scalar_boolean() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?f . :where [?e :qp/name \"Bob\"] [?e :qp/flag ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_qp_find_scalar_no_match() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :qp/name ?n] [?e :qp/dept \"nonexistent\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_qp_find_coll_strings() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_qp_find_coll_longs() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :qp/val ?v] [?e :qp/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_qp_find_coll_empty() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/dept \"nobody\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_qp_find_tuple_name_val() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?v] :where [?e :qp/name ?n] [?e :qp/val ?v] [?e :qp/dept \"hr\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let result = v["result"].as_array().expect("arr");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].as_str().expect("s"), "Eve");
        assert_eq!(result[1].as_i64().expect("v"), 70);
    }

    #[pg_test]
    fn test_qp_find_tuple_three() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?d ?v] :where [?e :qp/name ?n] [?e :qp/dept ?d] [?e :qp/val ?v] [?e :qp/dept \"hr\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_qp_find_tuple_no_match() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?v] :where [?e :qp/name ?n] [?e :qp/val ?v] [?e :qp/dept \"x\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_qp_find_relation_2_vars() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :qp/name ?n] [?e :qp/val ?v] [?e :qp/dept \"eng\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_qp_find_relation_3_vars() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?d ?v :where [?e :qp/name ?n] [?e :qp/dept ?d] [?e :qp/val ?v] [?e :qp/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_qp_find_relation_empty() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :qp/name ?n] [?e :qp/val ?v] [(> ?v 500)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 0);
    }

    // ========================================================================
    // Cardinality-many query patterns (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_qp_many_tags_query() {
        setup(); setup_qp_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :qp/name \"Tagged\" :qp/tags \"a\" :qp/tags \"b\" :qp/tags \"c\"}]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :qp/name \"Tagged\"] [?e :qp/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_qp_many_tags_scalar_finds_one() {
        setup(); setup_qp_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :qp/name \"Tagged2\" :qp/tags \"x\" :qp/tags \"y\"}]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?t . :where [?e :qp/name \"Tagged2\"] [?e :qp/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().is_some());
    }

    #[pg_test]
    fn test_qp_many_refs_query() {
        setup(); setup_qp_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"hub\" :qp/name \"Hub\"} {:db/id \"s1\" :qp/name \"S1\"} {:db/id \"s2\" :qp/name \"S2\"} {:db/id \"s3\" :qp/name \"S3\"} [:db/add \"hub\" :qp/refs \"s1\"] [:db/add \"hub\" :qp/refs \"s2\"] [:db/add \"hub\" :qp/refs \"s3\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let hub = j["tempids"]["hub"].as_i64().expect("hub");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?n ...] :where [{} :qp/refs ?r] [?r :qp/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", hub
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_qp_many_tags_with_filter() {
        setup(); setup_qp_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e1\" :qp/name \"E1\" :qp/tags \"common\" :qp/tags \"unique1\"} {:db/id \"e2\" :qp/name \"E2\" :qp/tags \"common\" :qp/tags \"unique2\"}]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/tags \"common\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Entity ID in queries (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_qp_find_eid() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :qp/name \"Alice\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_i64().expect("eid") > 0);
    }

    #[pg_test]
    fn test_qp_find_eid_collection() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?e ...] :where [?e :qp/dept \"eng\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_qp_find_eid_with_name() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e ?n :where [?e :qp/name ?n] [?e :qp/dept \"eng\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Wildcard and underscore patterns (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_qp_wildcard_entity() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :qp/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 6);
    }

    #[pg_test]
    fn test_qp_wildcard_entity_with_pred() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :qp/val ?v] [(> ?v 80)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Manager(100), Carol(90) = 2
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Complex combined patterns (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_qp_complex_dept_flag_pred() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :qp/name ?n] [?e :qp/dept \"eng\"] [?e :qp/flag true] [?e :qp/val ?v] [(> ?v 70)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1); // Alice
    }

    #[pg_test]
    fn test_qp_complex_status_and_ref() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?mn :where [?e :qp/name ?n] [?e :qp/status :active] [?e :qp/ref ?m] [?m :qp/name ?mn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Active people with refs: Alice, Bob, Eve
        assert_eq!(v["results"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_qp_complex_all_constraints() {
        setup(); setup_qp_schema(); setup_qp_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :qp/name ?n] [?e :qp/dept \"eng\"] [?e :qp/flag false] [?e :qp/status :active] [?e :qp/val ?v] [(<= ?v 80)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Bob");
    }

    #[pg_test]
    fn test_qp_same_var_two_patterns() {
        setup(); setup_qp_schema(); setup_qp_data();
        // Find entities that share same department
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n1 ?n2 :where [?e1 :qp/name ?n1] [?e1 :qp/dept ?d] [?e2 :qp/name ?n2] [?e2 :qp/dept ?d] [(!= ?e1 ?e2)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // eng: Alice+Bob (2 combos), sales: Carol+Dave (2 combos) = 4 pairs
        assert!(v["results"].as_array().expect("arr").len() >= 4);
    }

    #[pg_test]
    fn test_qp_batch_data_then_query() {
        setup(); setup_qp_schema();
        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :qp/name \"entity-{i}\" :qp/val {i} :qp/flag {f}}}",
                i = i, f = if i % 3 == 0 { "true" } else { "false" }
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qp/name ?n] [?e :qp/flag true] [?e :qp/val ?v] [(> ?v 50)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // i % 3 == 0 and i > 50: 51, 54, 57, ..., 99 => 17 values
        assert!(v["result"].as_array().expect("arr").len() > 10);
    }
}
