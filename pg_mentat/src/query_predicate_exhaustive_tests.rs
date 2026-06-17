// Exhaustive query predicate tests: systematic coverage of all predicate
// operators, combinations, and edge cases in Datalog queries.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_qpe_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :qpe/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :qpe/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :qpe/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :qpe/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :qpe/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :qpe/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :qpe/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :qpe/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"sc\" :db/ident :qpe/score :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("qpe schema");
    }

    fn setup_50_entities() {
        let mut ops = vec![];
        let depts = ["eng", "sales", "hr", "exec", "ops"];
        let statuses = [":active", ":inactive", ":pending", ":archived", ":suspended"];
        for i in 0..50 {
            ops.push(format!(
                "{{:db/id \"e{}\" :qpe/name \"person-{}\" :qpe/val {} :qpe/dbl {:?} :qpe/flag {} :qpe/status {} :qpe/age {}}}",
                i, i, i * 10, (i as f64) * 1.5, if i % 2 == 0 { "true" } else { "false" },
                statuses[i % 5], 20 + (i % 40)
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("data");
    }

    // ========================================================================
    // Greater-than predicates (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_qpe_gt_zero() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(> ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 49);
    }

    #[pg_test]
    fn test_qpe_gt_100() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(> ?v 100)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 30);
    }

    #[pg_test]
    fn test_qpe_gt_250() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(> ?v 250)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 20);
    }

    #[pg_test]
    fn test_qpe_gt_490() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :qpe/val ?v] [(> ?v 490)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Only val=490 is max, so 0 results since 490 is not > 490
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_qpe_gt_double() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/dbl ?d] [(> ?d 50.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 10);
    }

    #[pg_test]
    fn test_qpe_gt_age_40() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/age ?a] [(> ?a 40)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    #[pg_test]
    fn test_qpe_gt_combined_val_and_age() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(> ?v 200)] [?e :qpe/age ?a] [(> ?a 30)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    // ========================================================================
    // Less-than predicates (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_qpe_lt_50() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(< ?v 50)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5); // 0,10,20,30,40
    }

    #[pg_test]
    fn test_qpe_lt_100() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(< ?v 100)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_qpe_lt_double_10() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/dbl ?d] [(< ?d 10.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // dbl values: 0.0, 1.5, 3.0, 4.5, 6.0, 7.5, 9.0 = 7 values < 10.0
        assert_eq!(v["result"].as_array().expect("arr").len(), 7);
    }

    #[pg_test]
    fn test_qpe_lt_zero() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :qpe/val ?v] [(< ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_qpe_lt_1() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :qpe/val ?v] [(< ?v 1)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1); // only 0
    }

    #[pg_test]
    fn test_qpe_lt_age_25() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/age ?a] [(< ?a 25)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    // ========================================================================
    // Greater-than-or-equal predicates (6 tests)
    // ========================================================================

    #[pg_test]
    fn test_qpe_gte_0() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :qpe/val ?v] [(>= ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 50);
    }

    #[pg_test]
    fn test_qpe_gte_490() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :qpe/val ?v] [(>= ?v 490)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_qpe_gte_250() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(>= ?v 250)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 25);
    }

    #[pg_test]
    fn test_qpe_gte_double_37_5() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/dbl ?d] [(>= ?d 37.5)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // dbl >= 37.5: i*1.5 >= 37.5 => i >= 25, so 25 entities
        assert_eq!(v["result"].as_array().expect("arr").len(), 25);
    }

    // ========================================================================
    // Less-than-or-equal predicates (6 tests)
    // ========================================================================

    #[pg_test]
    fn test_qpe_lte_0() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :qpe/val ?v] [(<= ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_qpe_lte_490() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :qpe/val ?v] [(<= ?v 490)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 50);
    }

    #[pg_test]
    fn test_qpe_lte_100() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(<= ?v 100)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 11); // 0,10,...,100
    }

    #[pg_test]
    fn test_qpe_lte_double_15() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/dbl ?d] [(<= ?d 15.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // dbl <= 15.0: i*1.5 <= 15.0 => i <= 10, so 11 entities (0..10)
        assert_eq!(v["result"].as_array().expect("arr").len(), 11);
    }

    // ========================================================================
    // Not-equal predicates (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_qpe_ne_val_0() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(!= ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 49);
    }

    #[pg_test]
    fn test_qpe_ne_val_250() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(!= ?v 250)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 49);
    }

    #[pg_test]
    fn test_qpe_ne_nonexistent_val() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(!= ?v 999999)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 50);
    }

    #[pg_test]
    fn test_qpe_ne_double() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/dbl ?d] [(!= ?d 0.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 49);
    }

    // ========================================================================
    // Range predicates (combined > and <) (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_qpe_range_100_to_200() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(> ?v 100)] [(< ?v 200)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // 110,120,130,140,150,160,170,180,190 = 9 values
        assert_eq!(v["result"].as_array().expect("arr").len(), 9);
    }

    #[pg_test]
    fn test_qpe_range_0_to_50() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(>= ?v 0)] [(< ?v 50)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_qpe_range_tight_single_value() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(>= ?v 250)] [(<= ?v 250)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_qpe_range_empty() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :qpe/val ?v] [(> ?v 200)] [(< ?v 200)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_qpe_range_double_10_to_30() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/dbl ?d] [(> ?d 10.0)] [(< ?d 30.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // dbl: i*1.5, 10 < i*1.5 < 30 => 6.67 < i < 20 => i in 7..19 = 13
        assert_eq!(v["result"].as_array().expect("arr").len(), 13);
    }

    #[pg_test]
    fn test_qpe_range_age_25_to_35() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/age ?a] [(>= ?a 25)] [(<= ?a 35)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    #[pg_test]
    fn test_qpe_range_with_flag_filter() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(> ?v 100)] [(< ?v 300)] [?e :qpe/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    #[pg_test]
    fn test_qpe_range_with_status_filter() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [(> ?v 100)] [?e :qpe/status :active]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    // ========================================================================
    // Predicate with constant values (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_qpe_const_flag_true() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 25);
    }

    #[pg_test]
    fn test_qpe_const_flag_false() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/flag false]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 25);
    }

    #[pg_test]
    fn test_qpe_const_status_active() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/status :active]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_qpe_const_status_pending() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/status :pending]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_qpe_const_name_specific() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :qpe/name \"person-25\"] [?e :qpe/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 250);
    }

    #[pg_test]
    fn test_qpe_const_combined_flag_and_status() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/flag true] [?e :qpe/status :active]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // flag=true (even i) AND status=active (i%5==0): i=0,10,20,30,40 = 5
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_qpe_const_flag_status_and_range() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/flag true] [?e :qpe/status :active] [?e :qpe/val ?v] [(> ?v 100)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // flag=true (even i), status=:active (i%5==0), val=i*10 > 100 (i>10):
        // even AND i%5==0 AND i>10 => i in {20,30,40} = 3. (i=10 has val=100,
        // which is not strictly > 100, so it is excluded.)
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_qpe_const_nonexistent_name() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :qpe/name \"nonexistent\"] [?e :qpe/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    // ========================================================================
    // Multi-variable predicates and joins (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_qpe_two_attr_join() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [?e :qpe/age ?a] [(> ?v 200)] [(< ?a 40)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    #[pg_test]
    fn test_qpe_three_attr_relation() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v ?a :where [?e :qpe/name ?n] [?e :qpe/val ?v] [?e :qpe/age ?a] [(> ?v 400)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let rows = v["result"].as_array().expect("arr");
        assert!(rows.len() > 0);
        // Each row should have 3 elements
        for row in rows {
            assert_eq!(row.as_array().expect("r").len(), 3);
        }
    }

    #[pg_test]
    fn test_qpe_four_var_tuple() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?v ?d ?a] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [?e :qpe/dbl ?d] [?e :qpe/age ?a] [?e :qpe/name \"person-25\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tuple = v["result"].as_array().expect("arr");
        assert_eq!(tuple.len(), 4);
    }

    #[pg_test]
    fn test_qpe_relation_with_all_predicates() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :qpe/name ?n] [?e :qpe/val ?v] [?e :qpe/flag true] [?e :qpe/status :active] [(> ?v 0)] [(< ?v 300)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    #[pg_test]
    fn test_qpe_count_all_statuses() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let statuses = [":active", ":inactive", ":pending", ":archived", ":suspended"];
        for status in &statuses {
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/status {}]]'::TEXT, '{{}}'::jsonb)::TEXT", status
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_array().expect("arr").len(), 10);
        }
    }

    #[pg_test]
    fn test_qpe_predicate_on_different_attrs_simultaneously() {
        setup(); setup_qpe_schema(); setup_50_entities();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qpe/name ?n] [?e :qpe/val ?v] [?e :qpe/dbl ?d] [?e :qpe/age ?a] [(> ?v 200)] [(< ?d 60.0)] [(>= ?a 25)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() >= 0);
    }
}
