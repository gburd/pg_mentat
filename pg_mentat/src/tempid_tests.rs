// Exhaustive tempid tests: allocation, resolution, cross-referencing,
// reuse within transactions.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_tempid_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :ti/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :ti/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"r\" :db/ident :ti/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"rm\" :db/ident :ti/refs :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        ).expect("tempid schema");
    }

    // ========================================================================
    // Basic tempid allocation
    // ========================================================================

    #[pg_test]
    fn test_ti_single_tempid() {
        setup(); setup_tempid_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :ti/name \"single\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e"].as_i64().is_some());
    }

    #[pg_test]
    fn test_ti_multiple_distinct_tempids() {
        setup(); setup_tempid_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"a\" :ti/name \"alpha\"]
                [:db/add \"b\" :ti/name \"beta\"]
                [:db/add \"c\" :ti/name \"gamma\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("a");
        let b = j["tempids"]["b"].as_i64().expect("b");
        let c = j["tempids"]["c"].as_i64().expect("c");
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    // ========================================================================
    // Same tempid = same entity within tx
    // ========================================================================

    #[pg_test]
    fn test_ti_same_tempid_same_entity() {
        setup(); setup_tempid_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :ti/name \"person\"]
                [:db/add \"e\" :ti/val 42]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Both attributes should be on the same entity
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n ?v :where [{e} :ti/name ?n] [{e} :ti/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            e = eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(results.len(), 1);
    }

    // ========================================================================
    // Cross-reference via tempids
    // ========================================================================

    #[pg_test]
    fn test_ti_cross_ref_single() {
        setup(); setup_tempid_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"parent\" :ti/name \"parent\"]
                [:db/add \"child\" :ti/name \"child\"]
                [:db/add \"child\" :ti/ref \"parent\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let parent = j["tempids"]["parent"].as_i64().expect("parent");
        let child = j["tempids"]["child"].as_i64().expect("child");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :ti/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", child
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("ref"), parent);
    }

    #[pg_test]
    fn test_ti_cross_ref_chain() {
        setup(); setup_tempid_schema();
        // A -> B -> C
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"a\" :ti/name \"A\"]
                [:db/add \"b\" :ti/name \"B\"]
                [:db/add \"c\" :ti/name \"C\"]
                [:db/add \"b\" :ti/ref \"a\"]
                [:db/add \"c\" :ti/ref \"b\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("a");
        let b = j["tempids"]["b"].as_i64().expect("b");
        let c = j["tempids"]["c"].as_i64().expect("c");

        // C -> B
        let q1 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :ti/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", c
        )).expect("q").expect("NULL");
        let v1: serde_json::Value = serde_json::from_str(&q1).expect("parse");
        assert_eq!(v1["result"].as_i64().expect("ref"), b);

        // B -> A
        let q2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :ti/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", b
        )).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_i64().expect("ref"), a);
    }

    #[pg_test]
    fn test_ti_cross_ref_fan_out() {
        setup(); setup_tempid_schema();
        // Hub -> 5 spokes via refs-many
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"hub\" :ti/name \"hub\"]
                [:db/add \"s0\" :ti/name \"s0\"]
                [:db/add \"s1\" :ti/name \"s1\"]
                [:db/add \"s2\" :ti/name \"s2\"]
                [:db/add \"s3\" :ti/name \"s3\"]
                [:db/add \"s4\" :ti/name \"s4\"]
                [:db/add \"hub\" :ti/refs \"s0\"]
                [:db/add \"hub\" :ti/refs \"s1\"]
                [:db/add \"hub\" :ti/refs \"s2\"]
                [:db/add \"hub\" :ti/refs \"s3\"]
                [:db/add \"hub\" :ti/refs \"s4\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let hub = j["tempids"]["hub"].as_i64().expect("hub");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?r ...] :where [{} :ti/refs ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", hub
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    // ========================================================================
    // Tempid naming patterns
    // ========================================================================

    #[pg_test]
    fn test_ti_short_names() {
        setup(); setup_tempid_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"x\" :ti/name \"short\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["x"].as_i64().is_some());
    }

    #[pg_test]
    fn test_ti_long_names() {
        setup(); setup_tempid_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"a-very-long-tempid-name-for-testing\" :ti/name \"long\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["a-very-long-tempid-name-for-testing"].as_i64().is_some());
    }

    #[pg_test]
    fn test_ti_numeric_string_names() {
        setup(); setup_tempid_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"1\" :ti/name \"one\"]
                [:db/add \"2\" :ti/name \"two\"]
                [:db/add \"99\" :ti/name \"ninety-nine\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["1"].as_i64().is_some());
        assert!(j["tempids"]["2"].as_i64().is_some());
        assert!(j["tempids"]["99"].as_i64().is_some());
    }

    #[pg_test]
    fn test_ti_hyphenated_names() {
        setup(); setup_tempid_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"my-entity\" :ti/name \"hyphen\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["my-entity"].as_i64().is_some());
    }

    // ========================================================================
    // Map form tempids
    // ========================================================================

    #[pg_test]
    fn test_ti_map_form_tempid() {
        setup(); setup_tempid_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"mapped\" :ti/name \"from-map\" :ti/val 10}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["mapped"].as_i64().is_some());
    }

    #[pg_test]
    fn test_ti_map_form_cross_ref() {
        setup(); setup_tempid_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"p\" :ti/name \"parent-map\"}
                {:db/id \"c\" :ti/name \"child-map\" :ti/ref \"p\"}
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let p = j["tempids"]["p"].as_i64().expect("p");
        let c = j["tempids"]["c"].as_i64().expect("c");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :ti/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", c
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("ref"), p);
    }

    // ========================================================================
    // Large tempid batches
    // ========================================================================

    #[pg_test]
    fn test_ti_100_tempids() {
        setup(); setup_tempid_schema();
        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!("[:db/add \"t{}\" :ti/name \"entity-{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n")
        )).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let tempids = j["tempids"].as_object().expect("tempids");
        assert_eq!(tempids.len(), 100);

        // All IDs should be unique
        let mut ids: Vec<i64> = tempids.values().map(|v| v.as_i64().expect("id")).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 100);
    }

    #[pg_test]
    fn test_ti_200_tempids_with_refs() {
        setup(); setup_tempid_schema();
        // Create 100 parents and 100 children each referencing a parent
        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!("[:db/add \"p{}\" :ti/name \"parent-{}\"]", i, i));
        }
        for i in 0..100 {
            ops.push(format!("[:db/add \"c{}\" :ti/name \"child-{}\"]", i, i));
            ops.push(format!("[:db/add \"c{}\" :ti/ref \"p{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n")
        )).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let tempids = j["tempids"].as_object().expect("tempids");
        assert_eq!(tempids.len(), 200);
    }

    // ========================================================================
    // Tempid across separate txs
    // ========================================================================

    #[pg_test]
    fn test_ti_same_name_different_txs_different_entities() {
        setup(); setup_tempid_schema();
        let r1 = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :ti/name \"tx1\"]]'::TEXT)",
        ).expect("tx1").expect("NULL");
        let j1: serde_json::Value = serde_json::from_str(&r1).expect("parse");
        let eid1 = j1["tempids"]["e"].as_i64().expect("eid1");

        let r2 = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :ti/name \"tx2\"]]'::TEXT)",
        ).expect("tx2").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        let eid2 = j2["tempids"]["e"].as_i64().expect("eid2");

        assert_ne!(eid1, eid2, "Same tempid name in different txs should create different entities");
    }
}
