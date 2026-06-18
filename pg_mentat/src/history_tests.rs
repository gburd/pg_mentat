// History and temporal query tests: as-of, since, history queries.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_hist_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :hi/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :hi/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :hi/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        ).expect("hist schema");
    }

    // Helper: create entity and return (eid, tx1_id)
    fn create_entity_with_tx() -> (i64, i64) {
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :hi/name \"Test\"] [:db/add \"e\" :hi/val 1]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let tx = j.get("tx").and_then(|t| t.as_i64())
            .or_else(|| j.get("db-after").and_then(|t| t.as_i64()))
            .unwrap_or(0);
        (eid, tx)
    }

    // ========================================================================
    // Basic history via datoms table
    // ========================================================================

    #[pg_test]
    fn test_hi_datoms_record_assertion() {
        setup(); setup_hist_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :hi/val 42]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = true", eid
        )).expect("q").expect("NULL");
        assert!(count >= 1, "Should have assertion datoms");
    }

    #[pg_test]
    fn test_hi_datoms_record_retraction() {
        setup(); setup_hist_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :hi/val 42]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :hi/val 42]]'::TEXT)", eid
        )).expect("retract");

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = false", eid
        )).expect("q").expect("NULL");
        assert!(count >= 1, "Should have retraction datoms");
    }

    // ========================================================================
    // Multiple updates create history
    // ========================================================================

    #[pg_test]
    fn test_hi_multiple_updates_history() {
        setup(); setup_hist_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :hi/val 1]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for i in 2..=5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :hi/val {}]]'::TEXT)", eid, i
            )).expect("update");
        }

        // Current value should be 5
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :hi/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 5);

        // Should have multiple datoms (assertions + retractions) for this e/a
        let total = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':hi/val')", eid
        )).expect("q").expect("NULL");
        // 5 assertions + 4 retractions = 9 total datoms
        assert!(total >= 5, "Should have at least 5 datoms for 5 updates, got {}", total);
    }

    // ========================================================================
    // TX ordering
    // ========================================================================

    #[pg_test]
    fn test_hi_tx_ids_increasing() {
        setup(); setup_hist_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :hi/val 1]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Get tx IDs for successive updates
        let mut tx_ids = Vec::new();
        for i in 2..=10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :hi/val {}]]'::TEXT)", eid, i
            )).expect("update");
        }

        // Get all tx IDs for this entity
        let txs = Spi::get_one::<String>(&format!(
            "SELECT json_agg(DISTINCT tx ORDER BY tx)::text FROM mentat.datoms WHERE e = {}", eid
        ));
        if let Ok(Some(result)) = txs {
            let arr: Vec<i64> = serde_json::from_str(&result).unwrap_or_default();
            for window in arr.windows(2) {
                assert!(window[1] > window[0], "TX IDs should be increasing");
            }
            tx_ids = arr;
        }
        assert!(tx_ids.len() >= 2, "Should have multiple distinct tx IDs");
    }

    // ========================================================================
    // Cardinality-many history
    // ========================================================================

    #[pg_test]
    fn test_hi_many_add_history() {
        setup(); setup_hist_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :hi/name \"tagged\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Add tags one by one
        for i in 0..5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :hi/tags \"tag-{}\"]]'::TEXT)", eid, i
            )).expect("add tag");
        }

        // All 5 should be active
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :hi/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_hi_many_retract_history() {
        setup(); setup_hist_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :hi/name \"pruned\"]
                [:db/add \"e\" :hi/tags \"keep\"]
                [:db/add \"e\" :hi/tags \"remove\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :hi/tags \"remove\"]]'::TEXT)", eid
        )).expect("retract");

        // Should have 1 active tag
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :hi/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);

        // Should have retraction datom for "remove"
        let retracted = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':hi/tags')
             AND v_text = 'remove' AND added = false", eid
        )).expect("q").expect("NULL");
        assert_eq!(retracted, 1);
    }

    // ========================================================================
    // Entity lifecycle history
    // ========================================================================

    #[pg_test]
    fn test_hi_entity_lifecycle() {
        setup(); setup_hist_schema();

        // Create
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :hi/name \"Lifecycle\" :hi/val 0}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Update 5 times
        for i in 1..=5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :hi/val {}]]'::TEXT)", eid, i
            )).expect("update");
        }

        // Retract entity
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid
        )).expect("retract");

        // Entity should be gone
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :hi/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());

        // But history should exist
        let total = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {}", eid
        )).expect("q").expect("NULL");
        assert!(total >= 10, "Should have extensive history, got {}", total);
    }

    // ========================================================================
    // History with different value types
    // ========================================================================

    #[pg_test]
    fn test_hi_string_update_history() {
        setup(); setup_hist_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :hi/name \"v1\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for i in 2..=5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :hi/name \"v{}\"]]'::TEXT)", eid, i
            )).expect("update");
        }

        // Current value
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :hi/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "v5");

        // All historical values should be in datoms
        let history_count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':hi/name')", eid
        )).expect("q").expect("NULL");
        // 5 assertions + 4 retractions = 9
        assert!(history_count >= 5, "Should have at least 5 history datoms, got {}", history_count);
    }

    // ========================================================================
    // Concurrent entity history isolation
    // ========================================================================

    #[pg_test]
    fn test_hi_two_entities_independent_history() {
        setup(); setup_hist_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"a\" :hi/val 0] [:db/add \"b\" :hi/val 0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("a");
        let b = j["tempids"]["b"].as_i64().expect("b");

        // Update a 10 times, b 5 times
        for i in 1..=10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :hi/val {}]]'::TEXT)", a, i
            )).expect("update a");
        }
        for i in 1..=5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :hi/val {}]]'::TEXT)", b, i * 100
            )).expect("update b");
        }

        // a should be 10
        let qa = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :hi/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", a
        )).expect("q").expect("NULL");
        let va: serde_json::Value = serde_json::from_str(&qa).expect("parse");
        assert_eq!(va["result"].as_i64().expect("v"), 10);

        // b should be 500
        let qb = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :hi/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", b
        )).expect("q").expect("NULL");
        let vb: serde_json::Value = serde_json::from_str(&qb).expect("parse");
        assert_eq!(vb["result"].as_i64().expect("v"), 500);
    }
}
