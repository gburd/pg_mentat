// Edge case query tests: testing unusual patterns, corner cases,
// and boundary conditions in the query engine.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_eq_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :eq/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :eq/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :eq/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"b\" :db/ident :eq/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :eq/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :eq/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :eq/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("eq schema");
    }

    // ========================================================================
    // Empty result queries (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_eq_empty_relation() {
        setup(); setup_eq_schema();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :eq/name ?n] [?e :eq/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_eq_empty_scalar() {
        setup(); setup_eq_schema();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :eq/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_eq_empty_collection() {
        setup(); setup_eq_schema();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :eq/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_eq_empty_tuple() {
        setup(); setup_eq_schema();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ?v] :where [?e :eq/name ?n] [?e :eq/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_eq_nonexistent_attr_in_query() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :eq/name \"test\"]]'::TEXT)").expect("data");
        // Query for attr that entity doesn't have
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :eq/name \"test\"] [?e :eq/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    // ========================================================================
    // Single entity queries (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_eq_single_entity_all_attrs() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :eq/name \"solo\" :eq/val 42 :eq/dbl 3.14 :eq/flag true :eq/kw :test}]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v ?d ?f ?k :where [?e :eq/name ?n] [?e :eq/val ?v] [?e :eq/dbl ?d] [?e :eq/flag ?f] [?e :eq/kw ?k]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_eq_single_entity_scalar_each_type() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :eq/name \"multi\" :eq/val 99 :eq/flag false}]'::TEXT)").expect("tx");
        // String scalar
        let q1 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :eq/name ?v] [?e :eq/val 99]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v1: serde_json::Value = serde_json::from_str(&q1).expect("parse");
        assert_eq!(v1["result"].as_str().expect("s"), "multi");

        // Long scalar
        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :eq/name \"multi\"] [?e :eq/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_i64().expect("v"), 99);

        // Boolean scalar
        let q3 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :eq/name \"multi\"] [?e :eq/flag ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v3: serde_json::Value = serde_json::from_str(&q3).expect("parse");
        assert_eq!(v3["result"].as_bool().expect("b"), false);
    }

    // ========================================================================
    // Predicate edge cases (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_eq_pred_gt_all_match() {
        setup(); setup_eq_schema();
        let mut ops = Vec::new();
        for i in 100..110 {
            ops.push(format!("[:db/add \"e{i}\" :eq/val {i}]", i = i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :eq/val ?v] [(> ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_eq_pred_gt_none_match() {
        setup(); setup_eq_schema();
        let mut ops = Vec::new();
        for i in 0..10 {
            ops.push(format!("[:db/add \"e{i}\" :eq/val {i}]", i = i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :eq/val ?v] [(> ?v 1000)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_eq_pred_exact_boundary() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :eq/val 10] [:db/add \"e2\" :eq/val 20] [:db/add \"e3\" :eq/val 30]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :eq/val ?v] [(>= ?v 20)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_eq_pred_combined_tight_range() {
        setup(); setup_eq_schema();
        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!("[:db/add \"e{i}\" :eq/val {i}]", i = i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :eq/val ?v] [(>= ?v 50)] [(<= ?v 55)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 6);
    }

    #[pg_test]
    fn test_eq_pred_ne_with_many_values() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :eq/val 1] [:db/add \"e2\" :eq/val 2] [:db/add \"e3\" :eq/val 3] [:db/add \"e4\" :eq/val 4] [:db/add \"e5\" :eq/val 5]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :eq/val ?v] [(!= ?v 3)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    // ========================================================================
    // Join edge cases (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_eq_join_no_matching_entities() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :eq/name \"Alice\"] [:db/add \"e2\" :eq/val 42]]'::TEXT)").expect("data");
        // These are different entities, join should produce nothing
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :eq/name ?n] [?e :eq/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_eq_join_shared_entity() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :eq/name \"Alice\" :eq/val 42}]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :eq/name ?n] [?e :eq/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_eq_join_self_ref() {
        setup(); setup_eq_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :eq/name \"self\"}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :eq/ref {}]]'::TEXT)", eid, eid)).expect("self ref");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :eq/ref ?r] [?r :eq/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "self");
    }

    #[pg_test]
    fn test_eq_two_var_join() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :eq/name \"A\" :eq/val 10} {:db/id \"b\" :eq/name \"B\" :eq/val 10}]'::TEXT)").expect("data");
        // Find entities sharing same val
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n1 ?n2 :where [?e1 :eq/name ?n1] [?e1 :eq/val ?v] [?e2 :eq/name ?n2] [?e2 :eq/val ?v] [(!= ?e1 ?e2)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // A-B and B-A
        assert_eq!(v["results"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Cardinality-many query edge cases (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_eq_many_empty_collection() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :eq/name \"no-tags\"]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :eq/name \"no-tags\"] [?e :eq/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_eq_many_single_value() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :eq/name \"one-tag\"] [:db/add \"e\" :eq/tags \"only\"]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :eq/name \"one-tag\"] [?e :eq/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_eq_many_join_across_entities() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e1\" :eq/name \"E1\" :eq/tags \"shared\" :eq/tags \"unique1\"} {:db/id \"e2\" :eq/name \"E2\" :eq/tags \"shared\" :eq/tags \"unique2\"}]'::TEXT)").expect("data");
        // Find entities with tag "shared"
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :eq/name ?n] [?e :eq/tags \"shared\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_eq_many_after_retract() {
        setup(); setup_eq_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :eq/name \"post-retract\"] [:db/add \"e\" :eq/tags \"keep\"] [:db/add \"e\" :eq/tags \"remove\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :eq/tags \"remove\"]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :eq/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tags: Vec<&str> = v["result"].as_array().expect("arr").iter().map(|t| t.as_str().expect("s")).collect();
        assert_eq!(tags.len(), 1);
        assert!(tags.contains(&"keep"));
    }

    // ========================================================================
    // Query after mutations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_eq_query_reflects_add() {
        setup(); setup_eq_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :eq/name \"before\"]]'::TEXT)").expect("data");
        let q1 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :eq/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v1: serde_json::Value = serde_json::from_str(&q1).expect("parse");
        assert_eq!(v1["result"].as_array().expect("arr").len(), 1);

        Spi::run("SELECT mentat_transact('[[:db/add \"e2\" :eq/name \"after\"]]'::TEXT)").expect("data");
        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :eq/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_eq_query_reflects_retract() {
        setup(); setup_eq_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :eq/name \"doomed\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :eq/name \"doomed\"]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :eq/name \"doomed\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_eq_query_reflects_replace() {
        setup(); setup_eq_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :eq/val 10]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :eq/val 20]]'::TEXT)", eid)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :eq/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 20);
    }

    #[pg_test]
    fn test_eq_query_consistent_after_10_mutations() {
        setup(); setup_eq_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :eq/name \"evolving\" :eq/val 0}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=10 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :eq/val {}]]'::TEXT)", eid, i)).expect("update");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :eq/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 10);
    }
}
