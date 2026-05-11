// Idempotency tests: verifying that repeated operations produce
// consistent, predictable results.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod idempotency_tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_idem_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :id/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :id/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :id/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"f\" :db/ident :id/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :id/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"u\" :db/ident :id/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            ]'::TEXT)",
        ).expect("idem schema");
    }

    // ========================================================================
    // Cardinality-one: same value add is idempotent
    // ========================================================================

    #[pg_test]
    fn test_id_string_add_same_value_10x() {
        setup(); setup_idem_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :id/name \"same\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for _ in 0..10 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :id/name \"same\"]]'::TEXT)", eid)).expect("idem");
        }

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':id/name') AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1, "Should have exactly 1 active datom after idempotent adds");
    }

    #[pg_test]
    fn test_id_long_add_same_value_10x() {
        setup(); setup_idem_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :id/val 42]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for _ in 0..10 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :id/val 42]]'::TEXT)", eid)).expect("idem");
        }

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':id/val') AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_id_bool_add_same_value_10x() {
        setup(); setup_idem_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :id/flag true]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for _ in 0..10 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :id/flag true]]'::TEXT)", eid)).expect("idem");
        }

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':id/flag') AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_id_double_add_same_value_10x() {
        setup(); setup_idem_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :id/dbl 3.14]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for _ in 0..10 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :id/dbl 3.14]]'::TEXT)", eid)).expect("idem");
        }

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':id/dbl') AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    // ========================================================================
    // Cardinality-many: same value add is idempotent
    // ========================================================================

    #[pg_test]
    fn test_id_many_add_same_value_10x() {
        setup(); setup_idem_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :id/name \"holder\"] [:db/add \"e\" :id/tags \"tag\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for _ in 0..10 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :id/tags \"tag\"]]'::TEXT)", eid)).expect("idem");
        }

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':id/tags') AND v_text = 'tag' AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1, "Duplicate many adds should be idempotent");
    }

    // ========================================================================
    // Upsert idempotency
    // ========================================================================

    #[pg_test]
    fn test_id_upsert_same_data_10x() {
        setup(); setup_idem_schema();
        for _ in 0..10 {
            Spi::run(
                "SELECT mentat_transact('[{:db/id \"e\" :id/uid \"U1\" :id/name \"Same\" :id/val 42}]'::TEXT)",
            ).expect("upsert");
        }

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':id/uid') AND v_text = 'U1' AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 1, "10 identical upserts should produce 1 entity");
    }

    // ========================================================================
    // Query idempotency (same query, same result)
    // ========================================================================

    #[pg_test]
    fn test_id_query_returns_same_result_50x() {
        setup(); setup_idem_schema();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e1\" :id/name \"Alice\" :id/val 10}
                {:db/id \"e2\" :id/name \"Bob\" :id/val 20}
            ]'::TEXT)",
        ).expect("data");

        let mut results = Vec::new();
        for _ in 0..50 {
            let q = Spi::get_one::<String>(
                "SELECT mentat_query('[:find [?n ...] :where [?e :id/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
            ).expect("q").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
            let mut names: Vec<String> = j["result"].as_array().expect("arr")
                .iter().map(|v| v.as_str().expect("s").to_string()).collect();
            names.sort();
            results.push(names);
        }

        // All 50 results should be identical
        for i in 1..50 {
            assert_eq!(results[0], results[i], "Query {} returned different results", i);
        }
    }

    // ========================================================================
    // Schema definition idempotency
    // ========================================================================

    #[pg_test]
    fn test_id_bootstrap_schema_idempotent() {
        setup();
        // Call bootstrap_schema again
        Spi::run("SELECT mentat.bootstrap_schema()").expect("second bootstrap");

        // Should still work
        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(result.contains("db/ident"));
    }

    // ========================================================================
    // Transaction report consistency
    // ========================================================================

    #[pg_test]
    fn test_id_tempids_consistent_within_tx() {
        setup(); setup_idem_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :id/name \"Test\"]
                [:db/add \"e\" :id/val 42]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let tempids = j["tempids"].as_object().expect("tempids");
        // "e" should appear only once in tempids
        assert_eq!(tempids.len(), 1);
        assert!(tempids.contains_key("e"));
    }

    // ========================================================================
    // Retract-then-add cycle idempotency
    // ========================================================================

    #[pg_test]
    fn test_id_retract_readd_cycle_5x() {
        setup(); setup_idem_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :id/val 42]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for _ in 0..5 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :id/val 42]]'::TEXT)", eid)).expect("retract");
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :id/val 42]]'::TEXT)", eid)).expect("readd");
        }

        // Should still have exactly 1 active datom
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':id/val') AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :id/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 42);
    }

    // ========================================================================
    // Empty transaction idempotency
    // ========================================================================

    #[pg_test]
    fn test_id_empty_tx_10x() {
        setup();
        for _ in 0..10 {
            let _r = Spi::get_one::<String>("SELECT mentat_transact('[]'::TEXT)");
        }
        // Should not crash
    }
}
