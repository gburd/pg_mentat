// Exhaustive predicate tests in queries: comparison operators,
// type-specific predicates, combined predicates.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_pred_schema_and_data() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :pd/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :pd/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :pd/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :pd/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :pd/score :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("pred schema");

        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e0\" :pd/name \"Zero\" :pd/val 0 :pd/dbl 0.0 :pd/flag false :pd/score 0}
                {:db/id \"e1\" :pd/name \"One\" :pd/val 1 :pd/dbl 1.1 :pd/flag true :pd/score 10}
                {:db/id \"e2\" :pd/name \"Two\" :pd/val 2 :pd/dbl 2.2 :pd/flag false :pd/score 20}
                {:db/id \"e5\" :pd/name \"Five\" :pd/val 5 :pd/dbl 5.5 :pd/flag true :pd/score 50}
                {:db/id \"e10\" :pd/name \"Ten\" :pd/val 10 :pd/dbl 10.0 :pd/flag false :pd/score 100}
                {:db/id \"e20\" :pd/name \"Twenty\" :pd/val 20 :pd/dbl 20.2 :pd/flag true :pd/score 200}
                {:db/id \"e100\" :pd/name \"Hundred\" :pd/val 100 :pd/dbl 100.0 :pd/flag true :pd/score 1000}
                {:db/id \"en1\" :pd/name \"NegOne\" :pd/val -1 :pd/dbl -1.1 :pd/flag false :pd/score -10}
            ]'::TEXT)",
        ).expect("pred data");
    }

    // ========================================================================
    // Greater than
    // ========================================================================

    #[pg_test]
    fn test_pd_gt_long_basic() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(> ?v 5)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Ten=10, Twenty=20, Hundred=100
        assert_eq!(names.len(), 3);
    }

    #[pg_test]
    fn test_pd_gt_long_zero() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(> ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // 1, 2, 5, 10, 20, 100 => 6 matches
        assert_eq!(names.len(), 6);
    }

    #[pg_test]
    fn test_pd_gt_long_negative() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(> ?v -2)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // All 8 entities have val > -2 except none
        assert_eq!(names.len(), 8);
    }

    // ========================================================================
    // Less than
    // ========================================================================

    #[pg_test]
    fn test_pd_lt_long_basic() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(< ?v 5)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // NegOne=-1, Zero=0, One=1, Two=2 => 4
        assert_eq!(names.len(), 4);
    }

    #[pg_test]
    fn test_pd_lt_none_match() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(< ?v -100)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        assert_eq!(names.len(), 0);
    }

    // ========================================================================
    // Greater than or equal
    // ========================================================================

    #[pg_test]
    fn test_pd_gte_long() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(>= ?v 10)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Ten=10, Twenty=20, Hundred=100 => 3
        assert_eq!(names.len(), 3);
    }

    // ========================================================================
    // Less than or equal
    // ========================================================================

    #[pg_test]
    fn test_pd_lte_long() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(<= ?v 2)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // NegOne=-1, Zero=0, One=1, Two=2 => 4
        assert_eq!(names.len(), 4);
    }

    // ========================================================================
    // Not equal
    // ========================================================================

    #[pg_test]
    fn test_pd_ne_long() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(!= ?v 5)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // All except Five => 7
        assert_eq!(names.len(), 7);
    }

    #[pg_test]
    fn test_pd_ne_string() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [(!= ?n \"Zero\")]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        assert_eq!(names.len(), 7);
    }

    // ========================================================================
    // Combined predicates
    // ========================================================================

    #[pg_test]
    fn test_pd_range_between() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(>= ?v 2)] [(<= ?v 20)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Two=2, Five=5, Ten=10, Twenty=20 => 4
        assert_eq!(names.len(), 4);
    }

    #[pg_test]
    fn test_pd_combined_val_and_flag() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(> ?v 0)] [?e :pd/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // One(1,true), Five(5,true), Twenty(20,true), Hundred(100,true) => 4
        assert_eq!(names.len(), 4);
    }

    #[pg_test]
    fn test_pd_combined_two_attrs() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [?e :pd/score ?s] [(> ?v 1)] [(> ?s 30)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Five(5,50), Ten(10,100), Twenty(20,200), Hundred(100,1000) => 4
        assert_eq!(names.len(), 4);
    }

    // ========================================================================
    // Double predicates
    // ========================================================================

    #[pg_test]
    fn test_pd_gt_double() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/dbl ?d] [(> ?d 5.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Five=5.5, Ten=10.0, Twenty=20.2, Hundred=100.0 => 4
        assert_eq!(names.len(), 4);
    }

    #[pg_test]
    fn test_pd_lt_double() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/dbl ?d] [(< ?d 2.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // NegOne=-1.1, Zero=0.0, One=1.1 => 3
        assert_eq!(names.len(), 3);
    }

    // ========================================================================
    // All entities match
    // ========================================================================

    #[pg_test]
    fn test_pd_all_match() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(> ?v -1000)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_array().expect("arr").len(), 8);
    }

    // ========================================================================
    // No entities match
    // ========================================================================

    #[pg_test]
    fn test_pd_none_match() {
        setup();
        setup_pred_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :pd/name ?n] [?e :pd/val ?v] [(> ?v 1000)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_array().expect("arr").len(), 0);
    }
}
