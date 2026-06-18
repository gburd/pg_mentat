// Transaction lifecycle tests: comprehensive coverage of transaction
// behavior including ordering, atomicity, report structure, and multi-step workflows.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_tl_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :tl/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :tl/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :tl/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :tl/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :tl/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :tl/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :tl/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"u\" :db/ident :tl/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            ]'::TEXT)",
        ).expect("tl schema");
    }

    // ========================================================================
    // Transaction report structure (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_tl_report_has_tempids() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tl/name \"test\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"].is_object());
        assert!(j["tempids"]["e"].is_number());
    }

    #[pg_test]
    fn test_tl_report_has_tx_id() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tl/name \"test\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        // The transaction report exposes the tx id as db-after.basis-t.
        assert!(j["db-after"]["basis-t"].is_number());
    }

    #[pg_test]
    fn test_tl_report_tempid_positive() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tl/name \"test\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        assert!(eid > 0);
    }

    #[pg_test]
    fn test_tl_report_multiple_tempids() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"a\" :tl/name \"A\"] [:db/add \"b\" :tl/name \"B\"] [:db/add \"c\" :tl/name \"C\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 3);
    }

    #[pg_test]
    fn test_tl_report_same_tempid_one_entity() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tl/name \"test\"] [:db/add \"e\" :tl/val 42]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 1);
    }

    #[pg_test]
    fn test_tl_report_10_tempids() {
        setup(); setup_tl_schema();
        let mut ops = vec![];
        for i in 0..10 {
            ops.push(format!("[:db/add \"e{}\" :tl/name \"entity-{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n")
        )).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 10);
    }

    #[pg_test]
    fn test_tl_report_50_tempids() {
        setup(); setup_tl_schema();
        let mut ops = vec![];
        for i in 0..50 {
            ops.push(format!("[:db/add \"e{}\" :tl/name \"entity-{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n")
        )).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 50);
    }

    #[pg_test]
    fn test_tl_report_map_form_tempids() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tl/name \"test\" :tl/val 42}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e"].is_number());
    }

    #[pg_test]
    fn test_tl_report_valid_json() {
        setup(); setup_tl_schema();
        for i in 0..10 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :tl/val {}]]'::TEXT)", i, i
            )).expect("tx").expect("NULL");
            let j: Result<serde_json::Value, _> = serde_json::from_str(&r);
            assert!(j.is_ok(), "Transaction {} report should be valid JSON", i);
        }
    }

    #[pg_test]
    fn test_tl_report_existing_entity_no_tempid() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tl/name \"test\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let r2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add {} :tl/val 42]]'::TEXT)", eid
        )).expect("tx").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        assert_eq!(j2["tempids"].as_object().expect("t").len(), 0);
    }

    // ========================================================================
    // TX ID monotonicity (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_tl_tx_id_increases_3() {
        setup(); setup_tl_schema();
        let mut tx_ids = vec![];
        for i in 0..3 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :tl/val {}]]'::TEXT)", i, i
            )).expect("tx").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let tx = j["db-after"]["basis-t"].as_i64().expect("tx");
            tx_ids.push(tx);
        }
        for w in tx_ids.windows(2) {
            assert!(w[1] > w[0], "TX IDs should increase monotonically");
        }
    }

    #[pg_test]
    fn test_tl_tx_id_increases_10() {
        setup(); setup_tl_schema();
        let mut tx_ids = vec![];
        for i in 0..10 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :tl/val {}]]'::TEXT)", i, i
            )).expect("tx").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let tx = j["db-after"]["basis-t"].as_i64().expect("tx");
            tx_ids.push(tx);
        }
        for w in tx_ids.windows(2) {
            assert!(w[1] > w[0]);
        }
    }

    #[pg_test]
    fn test_tl_tx_id_increases_25() {
        setup(); setup_tl_schema();
        let mut tx_ids = vec![];
        for i in 0..25 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :tl/val {}]]'::TEXT)", i, i
            )).expect("tx").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let tx = j["db-after"]["basis-t"].as_i64().expect("tx");
            tx_ids.push(tx);
        }
        for w in tx_ids.windows(2) {
            assert!(w[1] > w[0]);
        }
    }

    #[pg_test]
    fn test_tl_tx_id_unique_after_20() {
        setup(); setup_tl_schema();
        let mut tx_ids = vec![];
        for i in 0..20 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :tl/name \"n-{}\"]]'::TEXT)", i, i
            )).expect("tx").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let tx = j["db-after"]["basis-t"].as_i64().expect("tx");
            tx_ids.push(tx);
        }
        let unique: std::collections::HashSet<_> = tx_ids.iter().collect();
        assert_eq!(unique.len(), 20, "All TX IDs should be unique");
    }

    // ========================================================================
    // Multi-step workflows (12 tests)
    // ========================================================================

    #[pg_test]
    fn test_tl_create_read_update_delete() {
        setup(); setup_tl_schema();
        // Create
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tl/name \"alice\" :tl/val 100}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        // Read
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 100);
        // Update
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :tl/val 200]]'::TEXT)", eid)).expect("update");
        // Read again
        let q2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_i64().expect("v"), 200);
        // Delete
        Spi::run(&format!("SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid)).expect("delete");
        let q3 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v3: serde_json::Value = serde_json::from_str(&q3).expect("parse");
        assert!(v3["result"].is_null());
    }

    #[pg_test]
    fn test_tl_workflow_5_entities_crud() {
        setup(); setup_tl_schema();
        // Create 5
        let mut ids = vec![];
        for i in 0..5 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[{{:db/id \"e{}\" :tl/name \"entity-{}\" :tl/val {}}}]'::TEXT)", i, i, i * 10
            )).expect("tx").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            ids.push(j["tempids"][&format!("e{}", i)].as_i64().expect("eid"));
        }
        // Update all
        for (i, &eid) in ids.iter().enumerate() {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :tl/val {}]]'::TEXT)", eid, (i + 1) * 100)).expect("update");
        }
        // Verify
        for (i, &eid) in ids.iter().enumerate() {
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_i64().expect("v"), ((i + 1) * 100) as i64);
        }
        // Delete first 3
        for &eid in &ids[..3] {
            Spi::run(&format!("SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid)).expect("delete");
        }
        // Verify remaining
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :tl/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_tl_status_machine_workflow() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"task\" :tl/name \"my-task\" :tl/status :draft}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["task"].as_i64().expect("eid");
        let states = [":review", ":approved", ":in-progress", ":testing", ":deployed"];
        for state in &states {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :tl/status {}]]'::TEXT)", eid, state
            )).expect("transition");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?s . :where [{} :tl/status ?s]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("deployed"));
    }

    #[pg_test]
    fn test_tl_tag_accumulation_workflow() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"doc\" :tl/name \"document\" :tl/tags \"initial\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["doc"].as_i64().expect("eid");
        // Add tags one by one
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :tl/tags \"tag-{}\"]]'::TEXT)", eid, i
            )).expect("add tag");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :tl/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 11); // initial + 10
    }

    #[pg_test]
    fn test_tl_batch_then_filter() {
        setup(); setup_tl_schema();
        let mut ops = vec![];
        for i in 0..100 {
            ops.push(format!(
                "{{:db/id \"e{}\" :tl/name \"item-{}\" :tl/val {} :tl/flag {}}}",
                i, i, i, if i % 3 == 0 { "true" } else { "false" }
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :tl/flag true] [?e :tl/name ?n] [?e :tl/val ?v] [(> ?v 50)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    #[pg_test]
    fn test_tl_upsert_workflow_10_rounds() {
        setup(); setup_tl_schema();
        for round in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:tl/uid \"user-001\" :tl/val {} :tl/status :round-{}}}]'::TEXT)",
                round * 100, round
            )).expect("upsert");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :tl/uid \"user-001\"] [?e :tl/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 900);
    }

    #[pg_test]
    fn test_tl_ref_chain_then_query() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"a\" :tl/name \"root\" :tl/val 1}
                {:db/id \"b\" :tl/name \"mid\" :tl/val 2 :tl/ref \"a\"}
                {:db/id \"c\" :tl/name \"leaf\" :tl/val 3 :tl/ref \"b\"}
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let _ = r;
        // Two-hop navigation
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?c :tl/name \"leaf\"] [?c :tl/ref ?b] [?b :tl/ref ?a] [?a :tl/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "root");
    }

    #[pg_test]
    fn test_tl_interleaved_schema_and_data() {
        setup(); setup_tl_schema();
        // Add more schema
        Spi::run("SELECT mentat_transact('[{:db/id \"x\" :db/ident :tl/extra :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        // Use new and existing attrs
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :tl/name \"test\" :tl/extra \"bonus\"}]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?x . :where [?e :tl/name \"test\"] [?e :tl/extra ?x]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "bonus");
    }

    #[pg_test]
    fn test_tl_retract_and_readd() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tl/name \"ephemeral\" :tl/val 42}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        // Retract the value
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :tl/val 42]]'::TEXT)", eid)).expect("retract");
        // Re-add a different value
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :tl/val 99]]'::TEXT)", eid)).expect("readd");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 99);
    }

    #[pg_test]
    fn test_tl_50_entities_update_all_then_retract_half() {
        setup(); setup_tl_schema();
        let mut ops = vec![];
        for i in 0..50 {
            ops.push(format!("{{:db/id \"e{}\" :tl/name \"item-{}\" :tl/val {}}}", i, i, i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n")
        )).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        // Update all
        let mut updates = vec![];
        for i in 0..50 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            updates.push(format!("[:db/add {} :tl/val {}]", eid, (i + 1) * 1000));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", updates.join("\n"))).expect("update");
        // Retract first 25
        let mut retracts = vec![];
        for i in 0..25 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            retracts.push(format!("[:db/retractEntity {}]", eid));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", retracts.join("\n"))).expect("retract");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :tl/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 25);
    }

    #[pg_test]
    fn test_tl_schema_then_100_data_txs() {
        setup(); setup_tl_schema();
        for i in 0..100 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :tl/name \"n{}\" :tl/val {}]]'::TEXT)", i, i, i
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :tl/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 100);
    }

    #[pg_test]
    fn test_tl_mixed_add_retract_same_tx() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tl/name \"original\" :tl/val 10}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        // In one tx: retract old name, add new name, add new entity with ref
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :tl/name \"original\"] [:db/add {} :tl/name \"updated\"] [:db/add \"new\" :tl/name \"linked\" :tl/ref {}]]'::TEXT)",
            eid, eid, eid
        )).expect("mixed tx");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :tl/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "updated");
    }

    // ========================================================================
    // Concurrent-like sequential patterns (6 tests)
    // ========================================================================

    #[pg_test]
    fn test_tl_rapid_fire_same_entity_50() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tl/val 0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=50 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :tl/val {}]]'::TEXT)", eid, i)).expect("update");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 50);
    }

    #[pg_test]
    fn test_tl_alternating_entities_20() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"a\" :tl/val 0] [:db/add \"b\" :tl/val 0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("eid");
        let b = j["tempids"]["b"].as_i64().expect("eid");
        for i in 1..=20 {
            if i % 2 == 0 {
                Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :tl/val {}]]'::TEXT)", a, i)).expect("a");
            } else {
                Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :tl/val {}]]'::TEXT)", b, i)).expect("b");
            }
        }
        let qa = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", a
        )).expect("q").expect("NULL");
        let va: serde_json::Value = serde_json::from_str(&qa).expect("parse");
        assert_eq!(va["result"].as_i64().expect("v"), 20);
        let qb = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", b
        )).expect("q").expect("NULL");
        let vb: serde_json::Value = serde_json::from_str(&qb).expect("parse");
        assert_eq!(vb["result"].as_i64().expect("v"), 19);
    }

    #[pg_test]
    fn test_tl_write_read_write_read_10x() {
        setup(); setup_tl_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tl/val 0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=10 {
            // Write
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :tl/val {}]]'::TEXT)", eid, i * 10)).expect("write");
            // Read
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_i64().expect("v"), (i * 10) as i64);
        }
    }

    #[pg_test]
    fn test_tl_batch_create_sequential_query() {
        setup(); setup_tl_schema();
        let mut ops = vec![];
        for i in 0..30 {
            ops.push(format!("{{:db/id \"e{}\" :tl/name \"entity-{}\" :tl/val {}}}", i, i, i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch");
        for i in 0..30 {
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [?e :tl/name \"entity-{}\"] [?e :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", i
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_i64().expect("v"), i as i64);
        }
    }

    #[pg_test]
    fn test_tl_create_and_link_across_txs() {
        setup(); setup_tl_schema();
        let mut eids = vec![];
        for i in 0..5 {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :tl/name \"node-{}\"]]'::TEXT)", i, i
            )).expect("tx").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            eids.push(j["tempids"][&format!("e{}", i)].as_i64().expect("eid"));
        }
        // Link chain across separate transactions
        for i in 1..5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :tl/ref {}]]'::TEXT)", eids[i], eids[i - 1]
            )).expect("link");
        }
        // Verify chain
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :tl/ref ?p] [?p :tl/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", eids[4]
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "node-3");
    }

    #[pg_test]
    fn test_tl_bulk_upsert_then_verify() {
        setup(); setup_tl_schema();
        // Create 20 entities via upsert
        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:tl/uid \"user-{}\" :tl/name \"User {}\" :tl/val {}}}]'::TEXT)", i, i, i * 5
            )).expect("create");
        }
        // Update all via upsert
        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:tl/uid \"user-{}\" :tl/val {}}}]'::TEXT)", i, (i + 1) * 100
            )).expect("update");
        }
        // Verify each
        for i in 0..20 {
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [?e :tl/uid \"user-{}\"] [?e :tl/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", i
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_i64().expect("v"), ((i + 1) * 100) as i64);
        }
    }
}
