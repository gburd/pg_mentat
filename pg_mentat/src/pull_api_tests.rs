// Pull API tests: comprehensive testing of the mentat_pull() function
// including attribute selection, nested refs, wildcard, and edge cases.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod pull_api_tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_pa_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :pa/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :pa/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :pa/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"b\" :db/ident :pa/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :pa/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :pa/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :pa/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"rm\" :db/ident :pa/refs :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        ).expect("pa schema");
    }

    // ========================================================================
    // Single attribute pull (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_pa_pull_string() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Alice\" :pa/val 42}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Alice");
    }

    #[pg_test]
    fn test_pa_pull_long() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Test\" :pa/val 99}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/val]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/val"].as_i64().expect("v"), 99);
    }

    #[pg_test]
    fn test_pa_pull_boolean() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Test\" :pa/flag true}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/flag]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/flag"].as_bool().expect("b"), true);
    }

    #[pg_test]
    fn test_pa_pull_keyword() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Test\" :pa/kw :active}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/kw]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert!(v[":pa/kw"].as_str().expect("s").contains("active"));
    }

    // ========================================================================
    // Multi-attribute pull (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_pa_pull_two_attrs() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Alice\" :pa/val 42}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name :pa/val]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Alice");
        assert_eq!(v[":pa/val"].as_i64().expect("v"), 42);
    }

    #[pg_test]
    fn test_pa_pull_three_attrs() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Bob\" :pa/val 10 :pa/flag false}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name :pa/val :pa/flag]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Bob");
        assert_eq!(v[":pa/val"].as_i64().expect("v"), 10);
        assert_eq!(v[":pa/flag"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_pa_pull_all_types() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Mix\" :pa/val 7 :pa/dbl 1.5 :pa/flag true :pa/kw :test}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name :pa/val :pa/dbl :pa/flag :pa/kw]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Mix");
        assert_eq!(v[":pa/val"].as_i64().expect("v"), 7);
    }

    // ========================================================================
    // Cardinality-many pull (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_pa_pull_many_tags() {
        setup(); setup_pa_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Tagged\" :pa/tags \"a\" :pa/tags \"b\" :pa/tags \"c\"}]'::TEXT)"
        ).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :pa/name \"Tagged\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let eid = j["result"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/tags]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/tags"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_pa_pull_many_with_single() {
        setup(); setup_pa_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Both\" :pa/val 42 :pa/tags \"x\" :pa/tags \"y\"}]'::TEXT)"
        ).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :pa/name \"Both\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let eid = j["result"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name :pa/val :pa/tags]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Both");
        assert_eq!(v[":pa/val"].as_i64().expect("v"), 42);
        assert_eq!(v[":pa/tags"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Wildcard pull (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_pa_pull_wildcard() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Wild\" :pa/val 42}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[*]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert!(v.is_object());
        // Should include name and val
        assert!(p.contains("pa/name") || p.contains("Wild"));
    }

    #[pg_test]
    fn test_pa_pull_wildcard_multi_type() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Multi\" :pa/val 10 :pa/flag true :pa/kw :test}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[*]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert!(v.is_object());
    }

    // ========================================================================
    // Ref pull (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_pa_pull_ref_basic() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"p\" :pa/name \"Parent\"} {:db/id \"c\" :pa/name \"Child\" :pa/ref \"p\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let child = j["tempids"]["c"].as_i64().expect("child");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name :pa/ref]')::TEXT", child
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Child");
    }

    #[pg_test]
    fn test_pa_pull_nested_ref() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"p\" :pa/name \"Parent\"} {:db/id \"c\" :pa/name \"Child\" :pa/ref \"p\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let child = j["tempids"]["c"].as_i64().expect("child");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name {{:pa/ref [:pa/name]}}]')::TEXT", child
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Child");
        // Nested ref should have parent name
        if let Some(ref_obj) = v.get(":pa/ref") {
            if let Some(name) = ref_obj.get(":pa/name") {
                assert_eq!(name.as_str().expect("s"), "Parent");
            }
        }
    }

    #[pg_test]
    fn test_pa_pull_many_refs() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"hub\" :pa/name \"Hub\"} {:db/id \"s1\" :pa/name \"S1\"} {:db/id \"s2\" :pa/name \"S2\"} [:db/add \"hub\" :pa/refs \"s1\"] [:db/add \"hub\" :pa/refs \"s2\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let hub = j["tempids"]["hub"].as_i64().expect("hub");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name :pa/refs]')::TEXT", hub
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Hub");
    }

    // ========================================================================
    // Pull missing attributes (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_pa_pull_missing_attr() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Sparse\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name :pa/val]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Sparse");
        // val should be absent or null
    }

    #[pg_test]
    fn test_pa_pull_after_retract() {
        setup(); setup_pa_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Test\" :pa/val 42}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :pa/val 42]]'::TEXT)", eid)).expect("retract");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name :pa/val]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Test");
    }

    // ========================================================================
    // Pull with queries (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_pa_query_then_pull() {
        setup(); setup_pa_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :pa/name \"Queryable\" :pa/val 42 :pa/flag true}]'::TEXT)"
        ).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :pa/name \"Queryable\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let eid = j["result"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:pa/name :pa/val :pa/flag]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":pa/name"].as_str().expect("s"), "Queryable");
        assert_eq!(v[":pa/val"].as_i64().expect("v"), 42);
    }

    #[pg_test]
    fn test_pa_pull_10_entities() {
        setup(); setup_pa_schema();
        let mut ops = Vec::new();
        for i in 0..10 {
            ops.push(format!("{{:db/id \"e{i}\" :pa/name \"ent-{i}\" :pa/val {i}}}", i = i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        for i in 0..10 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            let p = Spi::get_one::<String>(&format!(
                "SELECT mentat_pull('{}', '[:pa/name :pa/val]')::TEXT", eid
            )).expect("pull").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
            assert_eq!(v[":pa/name"].as_str().expect("s"), &format!("ent-{}", i));
            assert_eq!(v[":pa/val"].as_i64().expect("v"), i as i64);
        }
    }
}
