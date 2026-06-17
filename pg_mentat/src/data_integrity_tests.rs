// Data integrity tests: verifying that data stored via transactions
// is faithfully retrieved through queries, and that database constraints
// hold under various operations.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_di_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :di/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :di/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :di/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"b\" :db/ident :di/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :di/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :di/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :di/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"u\" :db/ident :di/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"uv\" :db/ident :di/uval :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/value}
            ]'::TEXT)",
        ).expect("di schema");
    }

    // ========================================================================
    // Write-then-read verification per type (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_di_string_roundtrip() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :di/name \"hello world\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/name ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "hello world");
    }

    #[pg_test]
    fn test_di_long_roundtrip() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :di/val 123456789]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 123456789);
    }

    #[pg_test]
    fn test_di_double_roundtrip() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :di/dbl 2.71828]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/dbl ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") - 2.71828).abs() < 0.0001);
    }

    #[pg_test]
    fn test_di_boolean_true_roundtrip() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :di/flag true]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/flag ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), true);
    }

    #[pg_test]
    fn test_di_boolean_false_roundtrip() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :di/flag false]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/flag ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_di_keyword_roundtrip() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :di/kw :status/active]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/kw ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("status/active"));
    }

    #[pg_test]
    fn test_di_string_100_chars() {
        setup(); setup_di_schema();
        let s = "a".repeat(100);
        Spi::run(&format!("SELECT mentat_transact('[[:db/add \"e\" :di/name \"{}\"]]'::TEXT)", s)).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/name ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s").len(), 100);
    }

    #[pg_test]
    fn test_di_string_1000_chars() {
        setup(); setup_di_schema();
        let s = "b".repeat(1000);
        Spi::run(&format!("SELECT mentat_transact('[[:db/add \"e\" :di/name \"{}\"]]'::TEXT)", s)).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/name ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s").len(), 1000);
    }

    #[pg_test]
    fn test_di_negative_long_roundtrip() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :di/val -987654321]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), -987654321);
    }

    #[pg_test]
    fn test_di_negative_double_roundtrip() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :di/dbl -99.99]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/dbl ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") - (-99.99)).abs() < 0.01);
    }

    // ========================================================================
    // Cardinality-one replacement integrity (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_di_replace_string_no_old() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :di/name \"v1\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :di/name \"v2\"]]'::TEXT)", eid)).expect("replace");

        // Append-only: "exactly 1 active datom" is a current-state property,
        // checked against the projection (the log retains v1's assertion).
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.current_text WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':di/name')", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1, "Should have exactly 1 current datom after replacement");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :di/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "v2");
    }

    #[pg_test]
    fn test_di_replace_long_no_old() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :di/val 1]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :di/val 2]]'::TEXT)", eid)).expect("replace");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :di/val 3]]'::TEXT)", eid)).expect("replace");

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.current_long WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':di/val')", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_di_replace_10x_only_latest() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :di/val 0]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=10 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :di/val {}]]'::TEXT)", eid, i)).expect("replace");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :di/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 10);
    }

    // ========================================================================
    // Cardinality-many accumulation integrity (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_di_many_tags_accumulate() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :di/name \"tagged\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :di/tags \"a\"] [:db/add {} :di/tags \"b\"] [:db/add {} :di/tags \"c\"]]'::TEXT)", eid, eid, eid)).expect("tags");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :di/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_di_many_no_duplicates() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :di/name \"dup\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for _ in 0..5 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :di/tags \"same\"]]'::TEXT)", eid)).expect("tag");
        }
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':di/tags') AND v_text = 'same' AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_di_many_retract_one_keeps_others() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :di/name \"multi\"] [:db/add \"e\" :di/tags \"x\"] [:db/add \"e\" :di/tags \"y\"] [:db/add \"e\" :di/tags \"z\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :di/tags \"y\"]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :di/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tags: Vec<&str> = v["result"].as_array().expect("arr").iter().map(|t| t.as_str().expect("s")).collect();
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"x"));
        assert!(tags.contains(&"z"));
    }

    #[pg_test]
    fn test_di_many_retract_all_empty() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :di/name \"all\"] [:db/add \"e\" :di/tags \"a\"] [:db/add \"e\" :di/tags \"b\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :di/tags \"a\"] [:db/retract {} :di/tags \"b\"]]'::TEXT)", eid, eid)).expect("retract all");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :di/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    // ========================================================================
    // Unique constraint integrity (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_di_unique_identity_upsert() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :di/uid \"U1\" :di/name \"v1\"}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :di/uid \"U1\" :di/name \"v2\"}]'::TEXT)").expect("upsert");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':di/uid') AND v_text = 'U1' AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_di_unique_identity_different_values() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e1\" :di/uid \"U2\" :di/name \"Alice\"}]'::TEXT)").expect("c1");
        Spi::run("SELECT mentat_transact('[{:db/id \"e2\" :di/uid \"U3\" :di/name \"Bob\"}]'::TEXT)").expect("c2");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':di/uid') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 2);
    }

    #[pg_test]
    fn test_di_unique_identity_preserves_attrs_on_upsert() {
        setup(); setup_di_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :di/uid \"U4\" :di/name \"orig\" :di/val 42}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :di/uid \"U4\" :di/name \"updated\"}]'::TEXT)").expect("upsert");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/uid \"U4\"] [?e :di/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 42);
    }

    // ========================================================================
    // Retract entity integrity (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_di_retract_entity_removes_all_attrs() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :di/name \"doomed\" :di/val 42 :di/flag true :di/kw :temp}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid)).expect("retract");

        // Check each attr
        for attr in &["di/name", "di/val", "di/flag", "di/kw"] {
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :{} ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid, attr
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert!(v["result"].is_null(), "Attr {} should be null after retractEntity", attr);
        }
    }

    #[pg_test]
    fn test_di_retract_entity_with_many() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :di/name \"tagged\"] [:db/add \"e\" :di/tags \"a\"] [:db/add \"e\" :di/tags \"b\"] [:db/add \"e\" :di/tags \"c\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :di/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_di_retract_entity_doesnt_affect_others() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"a\" :di/name \"keep\"} {:db/id \"b\" :di/name \"remove\"}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("a");
        let b = j["tempids"]["b"].as_i64().expect("b");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", b)).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :di/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", a
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "keep");
    }

    // ========================================================================
    // Cross-type entity integrity (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_di_multi_type_entity_roundtrip() {
        setup(); setup_di_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :di/name \"mixed\" :di/val 42 :di/dbl 3.14 :di/flag true :di/kw :test}]'::TEXT)"
        ).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?s ?l ?d ?b ?k :where [?e :di/name ?s] [?e :di/val ?l] [?e :di/dbl ?d] [?e :di/flag ?b] [?e :di/kw ?k]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0][0].as_str().expect("s"), "mixed");
        assert_eq!(results[0][1].as_i64().expect("l"), 42);
    }

    #[pg_test]
    fn test_di_10_entities_all_types() {
        setup(); setup_di_schema();
        let mut ops = Vec::new();
        for i in 0..10 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :di/name \"ent-{i}\" :di/val {i} :di/dbl {d} :di/flag {f} :di/kw :type-{k}}}",
                i = i, d = (i as f64) * 1.5, f = if i % 2 == 0 { "true" } else { "false" }, k = i % 3
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v ?d ?f :where [?e :di/name ?n] [?e :di/val ?v] [?e :di/dbl ?d] [?e :di/flag ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_di_ref_integrity_after_update() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"p\" :di/name \"Parent\"} {:db/id \"c\" :di/name \"Child\" :di/ref \"p\"}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let p = j["tempids"]["p"].as_i64().expect("p");
        let c = j["tempids"]["c"].as_i64().expect("c");
        // Update parent name
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :di/name \"Updated Parent\"]]'::TEXT)", p)).expect("update");
        // Child ref should still work
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :di/ref ?p] [?p :di/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", c
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Updated Parent");
    }

    #[pg_test]
    fn test_di_batch_50_integrity() {
        setup(); setup_di_schema();
        let mut ops = Vec::new();
        for i in 0..50 {
            ops.push(format!("{{:db/id \"e{i}\" :di/name \"item-{i}\" :di/val {i}}}", i = i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch");
        // Verify count
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':di/name') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 50);
        // Verify specific value
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :di/name \"item-25\"] [?e :di/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 25);
    }

    #[pg_test]
    fn test_di_concurrent_attrs_independence() {
        setup(); setup_di_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :di/name \"test\" :di/val 10 :di/flag true}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Update just val, name and flag should remain
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :di/val 20]]'::TEXT)", eid)).expect("update val");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n ?v ?f :where [{e} :di/name ?n] [{e} :di/val ?v] [{e} :di/flag ?f]]'::TEXT, '{{}}'::jsonb)::TEXT", e = eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0][0].as_str().expect("n"), "test");
        assert_eq!(results[0][1].as_i64().expect("v"), 20);
        assert_eq!(results[0][2].as_bool().expect("f"), true);
    }
}
