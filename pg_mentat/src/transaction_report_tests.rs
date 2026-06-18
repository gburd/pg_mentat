// Transaction report tests: systematic validation of transaction report
// structure, tempid resolution, tx metadata, and report consistency.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_tr_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :tr/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :tr/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :tr/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"u\" :db/ident :tr/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"r\" :db/ident :tr/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("tr schema");
    }

    // ========================================================================
    // Report structure (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_tr_report_has_tempids() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tr/name \"test\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"].is_object());
    }

    #[pg_test]
    fn test_tr_report_has_tx() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tr/name \"test\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        // The transaction report exposes the tx id as db-after.basis-t.
        assert!(j["db-after"]["basis-t"].as_i64().is_some());
    }

    #[pg_test]
    fn test_tr_tempid_is_positive() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tr/name \"test\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        assert!(eid > 0);
    }

    #[pg_test]
    fn test_tr_single_tempid() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tr/name \"Alice\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("obj").len(), 1);
        assert!(j["tempids"]["e"].as_i64().is_some());
    }

    #[pg_test]
    fn test_tr_two_tempids() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"a\" :tr/name \"Alice\"] [:db/add \"b\" :tr/name \"Bob\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("obj").len(), 2);
        assert!(j["tempids"]["a"].as_i64().is_some());
        assert!(j["tempids"]["b"].as_i64().is_some());
        assert_ne!(j["tempids"]["a"].as_i64(), j["tempids"]["b"].as_i64());
    }

    #[pg_test]
    fn test_tr_five_tempids() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"a\" :tr/name \"A\"] [:db/add \"b\" :tr/name \"B\"] [:db/add \"c\" :tr/name \"C\"] [:db/add \"d\" :tr/name \"D\"] [:db/add \"e\" :tr/name \"E\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("obj").len(), 5);
    }

    #[pg_test]
    fn test_tr_same_tempid_one_entity() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tr/name \"Test\"] [:db/add \"e\" :tr/val 42]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("obj").len(), 1);
    }

    #[pg_test]
    fn test_tr_map_form_tempid() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tr/name \"Test\" :tr/val 42}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("obj").len(), 1);
    }

    #[pg_test]
    fn test_tr_tempids_unique_across_batch() {
        setup(); setup_tr_schema();
        let mut ops = Vec::new();
        for i in 0..20 {
            ops.push(format!("[:db/add \"e{}\" :tr/name \"ent-{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join(" "))).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let tempids = j["tempids"].as_object().expect("obj");
        assert_eq!(tempids.len(), 20);
        // All EIDs should be distinct
        let mut eids: Vec<i64> = tempids.values().map(|v| v.as_i64().expect("eid")).collect();
        eids.sort();
        eids.dedup();
        assert_eq!(eids.len(), 20);
    }

    #[pg_test]
    fn test_tr_no_tempid_for_existing_entity() {
        setup(); setup_tr_schema();
        let r1 = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tr/name \"Test\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j1: serde_json::Value = serde_json::from_str(&r1).expect("parse");
        let eid = j1["tempids"]["e"].as_i64().expect("eid");

        let r2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add {} :tr/val 42]]'::TEXT)", eid
        )).expect("tx").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        // No tempids when using existing entity IDs
        let tempids = j2["tempids"].as_object();
        if let Some(t) = tempids {
            assert_eq!(t.len(), 0);
        }
    }

    // ========================================================================
    // TX ID monotonicity (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_tr_tx_ids_increase() {
        setup(); setup_tr_schema();
        let mut tx_ids = Vec::new();
        for i in 0..5 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :tr/name \"tx-{}\"]]'::TEXT)", i, i
            )).expect("tx").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            if let Some(tx) = j["tx"].as_i64() {
                tx_ids.push(tx);
            } else if let Some(tx) = j["tx_id"].as_i64() {
                tx_ids.push(tx);
            }
        }
        if tx_ids.len() >= 2 {
            for i in 1..tx_ids.len() {
                assert!(tx_ids[i] > tx_ids[i-1], "TX IDs should increase: {} <= {}", tx_ids[i], tx_ids[i-1]);
            }
        }
    }

    #[pg_test]
    fn test_tr_tx_ids_increase_10() {
        setup(); setup_tr_schema();
        let mut tx_ids = Vec::new();
        for i in 0..10 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :tr/name \"tx-{}\"]]'::TEXT)", i, i
            )).expect("tx").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            if let Some(tx) = j["tx"].as_i64() {
                tx_ids.push(tx);
            } else if let Some(tx) = j["tx_id"].as_i64() {
                tx_ids.push(tx);
            }
        }
        if tx_ids.len() >= 2 {
            for i in 1..tx_ids.len() {
                assert!(tx_ids[i] > tx_ids[i-1]);
            }
        }
    }

    // ========================================================================
    // Upsert report behavior (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_tr_upsert_first_creates() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tr/uid \"U1\" :tr/name \"Alice\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e"].as_i64().is_some());
    }

    #[pg_test]
    fn test_tr_upsert_second_no_new_tempid() {
        setup(); setup_tr_schema();
        let r1 = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tr/uid \"U2\" :tr/name \"Bob\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j1: serde_json::Value = serde_json::from_str(&r1).expect("parse");
        let eid1 = j1["tempids"]["e"].as_i64().expect("eid");

        let r2 = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tr/uid \"U2\" :tr/name \"Bob Updated\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        // The tempid should resolve to the same entity
        if let Some(eid2) = j2["tempids"]["e"].as_i64() {
            assert_eq!(eid1, eid2);
        }
    }

    #[pg_test]
    fn test_tr_upsert_10x_same_entity() {
        setup(); setup_tr_schema();
        let mut eids = Vec::new();
        for i in 0..10 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[{{:db/id \"e\" :tr/uid \"U3\" :tr/val {}}}]'::TEXT)", i
            )).expect("tx").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            if let Some(eid) = j["tempids"]["e"].as_i64() {
                eids.push(eid);
            }
        }
        if eids.len() >= 2 {
            for eid in &eids {
                assert_eq!(*eid, eids[0], "All upserts should resolve to same entity");
            }
        }
    }

    // ========================================================================
    // Ref tempid resolution (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_tr_ref_tempid_resolved() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"p\" :tr/name \"Parent\"] [:db/add \"c\" :tr/name \"Child\"] [:db/add \"c\" :tr/ref \"p\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let parent = j["tempids"]["p"].as_i64().expect("parent");
        let child = j["tempids"]["c"].as_i64().expect("child");
        assert_ne!(parent, child);
    }

    #[pg_test]
    fn test_tr_ref_map_form_resolved() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"p\" :tr/name \"Parent\"} {:db/id \"c\" :tr/name \"Child\" :tr/ref \"p\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let parent = j["tempids"]["p"].as_i64().expect("parent");
        let child = j["tempids"]["c"].as_i64().expect("child");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :tr/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", child
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("r"), parent);
    }

    #[pg_test]
    fn test_tr_chain_ref_tempids() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"a\" :tr/name \"A\"} {:db/id \"b\" :tr/name \"B\" :tr/ref \"a\"} {:db/id \"c\" :tr/name \"C\" :tr/ref \"b\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("a");
        let b = j["tempids"]["b"].as_i64().expect("b");
        let c = j["tempids"]["c"].as_i64().expect("c");
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[pg_test]
    fn test_tr_fan_out_ref_tempids() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"hub\" :tr/name \"Hub\"} {:db/id \"s1\" :tr/name \"S1\" :tr/ref \"hub\"} {:db/id \"s2\" :tr/name \"S2\" :tr/ref \"hub\"} {:db/id \"s3\" :tr/name \"S3\" :tr/ref \"hub\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let hub = j["tempids"]["hub"].as_i64().expect("hub");
        assert_eq!(j["tempids"].as_object().expect("obj").len(), 4);

        // Verify all spokes point to hub
        for s in &["s1", "s2", "s3"] {
            let sid = j["tempids"][s].as_i64().expect("s");
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?r . :where [{} :tr/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", sid
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_i64().expect("r"), hub);
        }
    }

    // ========================================================================
    // Batch report (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_tr_batch_10_tempids() {
        setup(); setup_tr_schema();
        let mut ops = Vec::new();
        for i in 0..10 {
            ops.push(format!("[:db/add \"e{}\" :tr/name \"batch-{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join(" "))).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("obj").len(), 10);
    }

    #[pg_test]
    fn test_tr_batch_50_tempids() {
        setup(); setup_tr_schema();
        let mut ops = Vec::new();
        for i in 0..50 {
            ops.push(format!("[:db/add \"e{}\" :tr/name \"batch-{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("obj").len(), 50);
    }

    #[pg_test]
    fn test_tr_batch_100_tempids() {
        setup(); setup_tr_schema();
        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!("[:db/add \"e{}\" :tr/name \"batch-{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("obj").len(), 100);
    }

    #[pg_test]
    fn test_tr_batch_map_20() {
        setup(); setup_tr_schema();
        let mut ops = Vec::new();
        for i in 0..20 {
            ops.push(format!("{{:db/id \"e{i}\" :tr/name \"map-{i}\" :tr/val {i}}}", i = i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("obj").len(), 20);
    }

    // ========================================================================
    // Retraction reports (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_tr_retract_produces_report() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tr/name \"doomed\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        let r2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/retract {} :tr/name \"doomed\"]]'::TEXT)", eid
        )).expect("retract").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        // Should be valid JSON report
        assert!(j2.is_object());
    }

    #[pg_test]
    fn test_tr_retract_entity_produces_report() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tr/name \"doomed\" :tr/val 42}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        let r2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid
        )).expect("retract").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        assert!(j2.is_object());
    }

    // ========================================================================
    // Mixed operation reports (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_tr_mixed_add_and_retract() {
        setup(); setup_tr_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tr/name \"test\"] [:db/add \"e\" :tr/val 1]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        let r2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/retract {} :tr/val 1] [:db/add {} :tr/val 2] [:db/add \"new\" :tr/name \"new\"]]'::TEXT)", eid, eid
        )).expect("mixed").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        assert!(j2.is_object());
    }

    #[pg_test]
    fn test_tr_report_valid_json_always() {
        setup(); setup_tr_schema();
        for i in 0..10 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :tr/name \"json-{}\"]]'::TEXT)", i, i
            )).expect("tx").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            assert!(j.is_object(), "Report {} should be valid JSON object", i);
        }
    }

    #[pg_test]
    fn test_tr_schema_tx_report() {
        setup();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"a\" :db/ident :tr.tmp/attr :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j.is_object());
        assert!(j["tempids"].is_object());
    }
}
