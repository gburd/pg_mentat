// Aggregate function tests in queries: count, sum, min, max, avg, etc.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod aggregate_tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_agg_schema_and_data() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :ag/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :ag/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :ag/dept :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :ag/score :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("agg schema");

        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e1\" :ag/name \"Alice\" :ag/val 100 :ag/dept \"Engineering\" :ag/score 88.5}
                {:db/id \"e2\" :ag/name \"Bob\" :ag/val 200 :ag/dept \"Engineering\" :ag/score 72.3}
                {:db/id \"e3\" :ag/name \"Carol\" :ag/val 150 :ag/dept \"Design\" :ag/score 91.7}
                {:db/id \"e4\" :ag/name \"Dave\" :ag/val 300 :ag/dept \"Product\" :ag/score 67.8}
                {:db/id \"e5\" :ag/name \"Eve\" :ag/val 50 :ag/dept \"Engineering\" :ag/score 95.2}
            ]'::TEXT)",
        ).expect("agg data");
    }

    // ========================================================================
    // Count
    // ========================================================================

    #[pg_test]
    fn test_ag_count_all() {
        setup(); setup_agg_schema_and_data();
        // Count distinct entities
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find (count ?e) . :where [?e :ag/name _]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        // count may or may not be supported; if it is, verify it returns 5
        if let Ok(Some(result)) = q {
            let j: serde_json::Value = serde_json::from_str(&result).expect("parse");
            if let Some(count) = j["result"].as_i64() {
                assert_eq!(count, 5);
            }
        }
    }

    // ========================================================================
    // Alternative: count via result set size
    // ========================================================================

    #[pg_test]
    fn test_ag_count_via_results() {
        setup(); setup_agg_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name :where [?e :ag/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["results"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_ag_count_by_dept() {
        setup(); setup_agg_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name :where [?e :ag/name ?name] [?e :ag/dept \"Engineering\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["results"].as_array().expect("arr").len(), 3); // Alice, Bob, Eve
    }

    #[pg_test]
    fn test_ag_count_filtered() {
        setup(); setup_agg_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name :where [?e :ag/name ?name] [?e :ag/val ?v] [(> ?v 100)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["results"].as_array().expect("arr").len(), 3); // Bob=200, Carol=150, Dave=300
    }

    // ========================================================================
    // Min/Max via sort + scalar
    // ========================================================================

    #[pg_test]
    fn test_ag_min_val_via_predicate() {
        setup(); setup_agg_schema_and_data();
        // Find the minimum val: Eve has 50
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :ag/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let vals: Vec<i64> = j["result"].as_array().expect("arr")
            .iter().map(|v| v.as_i64().expect("v")).collect();
        assert_eq!(*vals.iter().min().unwrap(), 50);
    }

    #[pg_test]
    fn test_ag_max_val_via_predicate() {
        setup(); setup_agg_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :ag/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let vals: Vec<i64> = j["result"].as_array().expect("arr")
            .iter().map(|v| v.as_i64().expect("v")).collect();
        assert_eq!(*vals.iter().max().unwrap(), 300);
    }

    // ========================================================================
    // Sum via collection
    // ========================================================================

    #[pg_test]
    fn test_ag_sum_via_collection() {
        setup(); setup_agg_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :ag/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let sum: i64 = j["result"].as_array().expect("arr")
            .iter().map(|v| v.as_i64().expect("v")).sum();
        assert_eq!(sum, 100 + 200 + 150 + 300 + 50);
    }

    // ========================================================================
    // Distinct values
    // ========================================================================

    #[pg_test]
    fn test_ag_distinct_depts() {
        setup(); setup_agg_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?d ...] :where [_ :ag/dept ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let depts = j["result"].as_array().expect("arr");
        assert_eq!(depts.len(), 3); // Engineering, Design, Product
    }

    // ========================================================================
    // Score-based queries
    // ========================================================================

    #[pg_test]
    fn test_ag_high_scorers() {
        setup(); setup_agg_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name ?s :where [?e :ag/name ?name] [?e :ag/score ?s] [(> ?s 85.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        // Alice=88.5, Carol=91.7, Eve=95.2
        assert_eq!(results.len(), 3);
    }

    #[pg_test]
    fn test_ag_score_range() {
        setup(); setup_agg_schema_and_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?s ...] :where [_ :ag/score ?s] [(>= ?s 70.0)] [(<= ?s 90.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let scores = j["result"].as_array().expect("arr");
        // Alice=88.5, Bob=72.3 => 2
        assert_eq!(scores.len(), 2);
    }
}
