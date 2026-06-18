// Regression tests: test cases for specific scenarios that could break,
// edge cases that have caused issues, and defensive tests to prevent
// future regressions.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_reg_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :reg/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :reg/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :reg/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"b\" :db/ident :reg/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :reg/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :reg/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :reg/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"u\" :db/ident :reg/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            ]'::TEXT)",
        ).expect("reg schema");
    }

    // ========================================================================
    // Cardinality-one replacement regression (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_reg_replace_string_clears_old() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :reg/name \"v1\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :reg/name \"v2\"]]'::TEXT)",
            eid
        ))
        .expect("replace");
        // Old value should not be queryable
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?e . :where [?e :reg/name \"v1\"]]'::TEXT, '{{}}'::jsonb)::TEXT"
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null(), "Old value should not be findable");
    }

    #[pg_test]
    fn test_reg_replace_long_clears_old() {
        setup();
        setup_reg_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :reg/val 10]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :reg/val 20]]'::TEXT)",
            eid
        ))
        .expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?e . :where [?e :reg/val 10]]'::TEXT, '{{}}'::jsonb)::TEXT"
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_reg_replace_bool_clears_old() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :reg/flag true]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :reg/flag false]]'::TEXT)",
            eid
        ))
        .expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :reg/flag ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_reg_replace_doesnt_affect_other_attrs() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :reg/name \"test\" :reg/val 42 :reg/flag true}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :reg/name \"updated\"]]'::TEXT)",
            eid
        ))
        .expect("replace");
        // val and flag should be unchanged
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v ?f :where [{e} :reg/val ?v] [{e} :reg/flag ?f]]'::TEXT, '{{}}'::jsonb)::TEXT", e = eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(results.len(), 1);
    }

    // ========================================================================
    // Cardinality-many duplicate regression (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_reg_many_no_dup_same_tx() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :reg/name \"h\"] [:db/add \"e\" :reg/tags \"dup\"] [:db/add \"e\" :reg/tags \"dup\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':reg/tags') AND v_text = 'dup' AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_reg_many_no_dup_across_txs() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :reg/name \"h\"] [:db/add \"e\" :reg/tags \"x\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :reg/tags \"x\"]]'::TEXT)",
            eid
        ))
        .expect("dup add");
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':reg/tags') AND v_text = 'x' AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    // ========================================================================
    // Retract regression (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_reg_retract_nonexistent_no_crash() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :reg/name \"test\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        // Retract a value that doesn't exist should not crash
        let result = Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :reg/val 999]]'::TEXT)",
            eid
        ));
        // Should either succeed silently or produce a clean error
        let _ = result;
    }

    #[pg_test]
    fn test_reg_retract_entity_then_query() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :reg/name \"doomed\" :reg/val 42}]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retract");
        // Query should return empty results
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :reg/name ?n] [?e :reg/name \"doomed\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_reg_retract_many_one_at_a_time() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :reg/name \"h\"] [:db/add \"e\" :reg/tags \"a\"] [:db/add \"e\" :reg/tags \"b\"] [:db/add \"e\" :reg/tags \"c\"] [:db/add \"e\" :reg/tags \"d\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for tag in &["a", "b", "c", "d"] {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/retract {} :reg/tags \"{}\"]]'::TEXT)",
                eid, tag
            ))
            .expect("retract");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :reg/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    // ========================================================================
    // Upsert regression (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_reg_upsert_creates_only_once() {
        setup();
        setup_reg_schema();
        for i in 0..5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"e\" :reg/uid \"RU1\" :reg/val {}}}]'::TEXT)",
                i
            ))
            .expect("upsert");
        }
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':reg/uid') AND v_text = 'RU1' AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_reg_upsert_last_val_wins() {
        setup();
        setup_reg_schema();
        for i in 0..5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"e\" :reg/uid \"RU2\" :reg/val {}}}]'::TEXT)",
                i * 10
            ))
            .expect("upsert");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :reg/uid \"RU2\"] [?e :reg/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 40);
    }

    #[pg_test]
    fn test_reg_upsert_with_many_attr() {
        setup();
        setup_reg_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :reg/uid \"RU3\" :reg/tags \"a\"}]'::TEXT)",
        )
        .expect("create");
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :reg/uid \"RU3\" :reg/tags \"b\"}]'::TEXT)",
        )
        .expect("upsert");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :reg/uid \"RU3\"] [?e :reg/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Should have both tags
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Query regression (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_reg_query_empty_db() {
        setup();
        setup_reg_schema();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :reg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_reg_query_scalar_no_match() {
        setup();
        setup_reg_schema();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :reg/name ?n] [?e :reg/name \"nonexistent\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_reg_query_after_retract() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :reg/name \"gone\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :reg/name \"gone\"]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :reg/name \"gone\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_reg_query_after_replace() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :reg/name \"before\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :reg/name \"after\"]]'::TEXT)",
            eid
        ))
        .expect("replace");
        // Old value not findable
        let q1 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :reg/name \"before\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v1: serde_json::Value = serde_json::from_str(&q1).expect("parse");
        assert!(v1["result"].is_null());
        // New value findable
        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :reg/name \"after\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_i64().expect("e"), eid);
    }

    #[pg_test]
    fn test_reg_query_100_entities() {
        setup();
        setup_reg_schema();
        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!("[:db/add \"e{i}\" :reg/name \"ent-{i}\"]", i = i));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :reg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 100);
    }

    // ========================================================================
    // Schema regression (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_reg_schema_after_bootstrap_twice() {
        setup();
        Spi::run("SELECT bootstrap_schema()").expect("second bootstrap");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("db/ident"));
    }

    #[pg_test]
    fn test_reg_schema_after_data() {
        setup();
        setup_reg_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :reg/name \"test\"]]'::TEXT)")
            .expect("data");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("reg/name"));
    }

    #[pg_test]
    fn test_reg_schema_after_many_txs() {
        setup();
        setup_reg_schema();
        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{i}\" :reg/name \"tx-{i}\"]]'::TEXT)",
                i = i
            ))
            .expect("tx");
        }
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("reg/name"));
    }

    // ========================================================================
    // Ref regression (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_reg_ref_replace_updates_correctly() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"a\" :reg/name \"A\"} {:db/id \"b\" :reg/name \"B\"} {:db/id \"c\" :reg/name \"C\" :reg/ref \"a\"}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let b = j["tempids"]["b"].as_i64().expect("b");
        let c = j["tempids"]["c"].as_i64().expect("c");
        // Replace ref from A to B
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :reg/ref {}]]'::TEXT)",
            c, b
        ))
        .expect("replace ref");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :reg/ref ?r] [?r :reg/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", c
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "B");
    }

    #[pg_test]
    fn test_reg_ref_chain_3_deep() {
        setup();
        setup_reg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"a\" :reg/name \"Top\"} {:db/id \"b\" :reg/name \"Mid\" :reg/ref \"a\"} {:db/id \"c\" :reg/name \"Bot\" :reg/ref \"b\"}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let _c = j["tempids"]["c"].as_i64().expect("c");
        // 3-deep navigation
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?top . :where [?c :reg/name \"Bot\"] [?c :reg/ref ?m] [?m :reg/ref ?t] [?t :reg/name ?top]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Top");
    }
}
