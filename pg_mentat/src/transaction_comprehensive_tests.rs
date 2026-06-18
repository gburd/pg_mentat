// Comprehensive transaction tests covering all operation types, formats,
// error cases, and edge conditions.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._test_raises_error(stmt TEXT) RETURNS BOOLEAN
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN false;
             EXCEPTION WHEN OTHERS THEN
                 RETURN true;
             END;
             $$"
        ).expect("create helper");
    }

    fn raises_error(sql: &str) -> bool {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<bool>(&format!(
            "SELECT mentat._test_raises_error('{}')", escaped
        )).expect("raises_error call").unwrap_or(false)
    }

    fn setup_tx_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :tx/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :tx/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :tx/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :tx/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"u\" :db/ident :tx/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"c\" :db/ident :tx/code :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/value}
                {:db/id \"r\" :db/ident :tx/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :tx/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("tx schema");
    }

    // ========================================================================
    // Transaction Report Format
    // ========================================================================

    #[pg_test]
    fn test_tx_report_has_tempids() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e1\" :tx/name \"test\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"].is_object(), "Report should have tempids");
        assert!(j["tempids"]["e1"].as_i64().is_some(), "e1 tempid should be assigned");
    }

    #[pg_test]
    fn test_tx_report_tempids_unique_per_tx() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"a\" :tx/name \"first\"]
                [:db/add \"b\" :tx/name \"second\"]
                [:db/add \"c\" :tx/name \"third\"]
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

    #[pg_test]
    fn test_tx_report_no_tempids_for_existing() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tx/name \"first\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Update existing entity (no tempid needed)
        let r2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add {} :tx/val 42]]'::TEXT)", eid
        )).expect("tx").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        let tempids = j2["tempids"].as_object();
        // Should have no tempids (or empty map)
        assert!(
            tempids.map_or(true, |m| m.is_empty()),
            "Update of existing entity should not generate tempids"
        );
    }

    // ========================================================================
    // db/add operations
    // ========================================================================

    #[pg_test]
    fn test_tx_add_vector_form() {
        setup(); setup_tx_schema();
        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :tx/name \"vector\"]]'::TEXT)",
        ).expect("vector add");
    }

    #[pg_test]
    fn test_tx_add_map_form() {
        setup(); setup_tx_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :tx/name \"map\"}]'::TEXT)",
        ).expect("map add");
    }

    #[pg_test]
    fn test_tx_add_map_with_multiple_attrs() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tx/name \"multi\" :tx/val 42 :tx/flag true :tx/dbl 3.14}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Verify all four attributes
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = true", eid
        )).expect("q").expect("NULL");
        assert!(count >= 4, "Should have at least 4 datoms");
    }

    #[pg_test]
    fn test_tx_add_mixed_vector_and_map() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"e1\" :tx/name \"from-map\"}
                [:db/add \"e2\" :tx/name \"from-vector\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e1"].as_i64().is_some());
        assert!(j["tempids"]["e2"].as_i64().is_some());
    }

    // ========================================================================
    // db/retract operations
    // ========================================================================

    #[pg_test]
    fn test_tx_retract_specific_value() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tx/name \"retractme\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :tx/name \"retractme\"]]'::TEXT)", eid
        )).expect("retract");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tx/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_tx_retract_wrong_value_is_noop() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tx/name \"keep\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Retract a different value (should be no-op or error)
        let _result = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/retract {} :tx/name \"wrong\"]]'::TEXT)", eid
        ));
        // Whether it errors or succeeds, original should remain
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tx/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "keep");
    }

    // ========================================================================
    // db/retractEntity operations
    // ========================================================================

    #[pg_test]
    fn test_tx_retract_entity_basic() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :tx/name \"doomed\" :tx/val 1 :tx/flag false}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid
        )).expect("retract entity");

        // All attributes should be gone
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tx/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_tx_retract_entity_with_many() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :tx/name \"tagged\"]
                [:db/add \"e\" :tx/tags \"t1\"]
                [:db/add \"e\" :tx/tags \"t2\"]
                [:db/add \"e\" :tx/tags \"t3\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid
        )).expect("retract entity");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :tx/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tags = v["result"].as_array().expect("arr");
        assert_eq!(tags.len(), 0);
    }

    // ========================================================================
    // CAS (Compare-And-Swap) operations
    // ========================================================================

    #[pg_test]
    fn test_tx_cas_success() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tx/val 10]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :tx/val 10 20]]'::TEXT)", eid
        )).expect("CAS should succeed");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tx/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 20);
    }

    #[pg_test]
    fn test_tx_cas_failure_wrong_old() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tx/val 10]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        assert!(
            raises_error(&format!(
                "SELECT mentat_transact('[[:db/cas {} :tx/val 99 20]]'::TEXT)", eid
            )),
            "CAS with wrong old value should fail"
        );

        // Original value should be preserved
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tx/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 10);
    }

    #[pg_test]
    fn test_tx_cas_from_nil() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tx/name \"no-val\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // CAS from nil (attribute not yet set)
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :tx/val nil 42]]'::TEXT)", eid
        )).expect("CAS from nil should succeed");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tx/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 42);
    }

    #[pg_test]
    fn test_tx_cas_sequential() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tx/val 0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Chain 10 CAS operations
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/cas {} :tx/val {} {}]]'::TEXT)", eid, i, i + 1
            )).expect("CAS chain");
        }

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tx/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 10);
    }

    // ========================================================================
    // Upsert operations
    // ========================================================================

    #[pg_test]
    fn test_tx_upsert_identity_creates_then_updates() {
        setup(); setup_tx_schema();

        // First: creates new entity
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :tx/uid \"user-1\" :tx/name \"Original\"}]'::TEXT)",
        ).expect("create");

        // Second: upserts (same uid = same entity)
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :tx/uid \"user-1\" :tx/name \"Updated\"}]'::TEXT)",
        ).expect("upsert");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :tx/uid \"user-1\"] [?e :tx/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Updated");
    }

    #[pg_test]
    fn test_tx_upsert_does_not_create_duplicate() {
        setup(); setup_tx_schema();

        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :tx/uid \"uid-dup\" :tx/val 1}]'::TEXT)",
        ).expect("create");

        for i in 2..=10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"u\" :tx/uid \"uid-dup\" :tx/val {}}}]'::TEXT)", i
            )).expect("upsert");
        }

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tx/uid')
             AND v_text = 'uid-dup' AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 1, "Upsert should not create duplicates");
    }

    // ========================================================================
    // Unique value constraint
    // ========================================================================

    #[pg_test]
    fn test_tx_unique_value_rejects_duplicate() {
        setup(); setup_tx_schema();

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e1\" :tx/code \"CODE-1\"]]'::TEXT)",
        ).expect("first");

        assert!(
            raises_error("SELECT mentat_transact('[[:db/add \"e2\" :tx/code \"CODE-1\"]]'::TEXT)"),
            "Duplicate unique value should be rejected"
        );
    }

    #[pg_test]
    fn test_tx_unique_value_allows_different() {
        setup(); setup_tx_schema();

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e1\" :tx/code \"CODE-A\"]
                [:db/add \"e2\" :tx/code \"CODE-B\"]
            ]'::TEXT)",
        ).expect("different codes should work");
    }

    // ========================================================================
    // Transaction atomicity
    // ========================================================================

    #[pg_test]
    fn test_tx_atomicity_all_or_nothing() {
        setup(); setup_tx_schema();

        let before = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE added = true",
        ).expect("q").expect("NULL");

        // Mix valid and invalid ops
        assert!(
            raises_error("SELECT mentat_transact('[
                [:db/add \"good\" :tx/name \"good\"]
                [:db/add \"bad\" :tx/nonexistent \"bad\"]
            ]'::TEXT)")
        );

        let after = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE added = true",
        ).expect("q").expect("NULL");
        assert_eq!(before, after, "Failed tx should not change DB");
    }

    #[pg_test]
    fn test_tx_atomicity_unique_violation_rolls_back() {
        setup(); setup_tx_schema();

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"existing\" :tx/code \"TAKEN\"]]'::TEXT)",
        ).expect("setup");

        let before = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE added = true",
        ).expect("q").expect("NULL");

        assert!(
            raises_error("SELECT mentat_transact('[
                [:db/add \"new1\" :tx/name \"new1\"]
                [:db/add \"new2\" :tx/code \"TAKEN\"]
            ]'::TEXT)")
        );

        let after = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE added = true",
        ).expect("q").expect("NULL");
        assert_eq!(before, after, "Unique violation should roll back entire tx");
    }

    // ========================================================================
    // Sequential transactions
    // ========================================================================

    #[pg_test]
    fn test_tx_sequential_50_transactions() {
        setup(); setup_tx_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tx/val 0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for i in 1..=50 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :tx/val {}]]'::TEXT)", eid, i
            )).expect("update");
        }

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tx/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 50);
    }

    // ========================================================================
    // Batch transactions
    // ========================================================================

    #[pg_test]
    fn test_tx_batch_100_entities_map() {
        setup(); setup_tx_schema();
        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :tx/name \"entity-{i}\" :tx/val {i}}}",
                i = i
            ));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n")
        )).expect("batch").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("tempids").len(), 100);
    }

    #[pg_test]
    fn test_tx_batch_100_entities_vector() {
        setup(); setup_tx_schema();
        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!("[:db/add \"v{i}\" :tx/name \"vec-{i}\"]", i = i));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n")
        )).expect("batch vector");
    }

    #[pg_test]
    fn test_tx_batch_mixed_ops() {
        setup(); setup_tx_schema();

        // Create some entities first
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"a\" :tx/name \"Alice\" ]
                [:db/add \"a\" :tx/val 10]
                [:db/add \"b\" :tx/name \"Bob\"]
                [:db/add \"b\" :tx/val 20]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("a");
        let b = j["tempids"]["b"].as_i64().expect("b");

        // Mixed: update a, retract b, add c
        Spi::run(&format!(
            "SELECT mentat_transact('[
                [:db/add {} :tx/val 11]
                [:db/retract {} :tx/name \"Bob\"]
                [:db/add \"c\" :tx/name \"Carol\"]
            ]'::TEXT)", a, b
        )).expect("mixed ops");

        // Verify a updated
        let qa = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tx/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", a
        )).expect("q").expect("NULL");
        let va: serde_json::Value = serde_json::from_str(&qa).expect("parse");
        assert_eq!(va["result"].as_i64().expect("v"), 11);

        // Verify b name retracted
        let qb = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :tx/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", b
        )).expect("q").expect("NULL");
        let vb: serde_json::Value = serde_json::from_str(&qb).expect("parse");
        assert!(vb["result"].is_null());

        // Verify c created. Bind ?n via the value pattern, then constrain it,
        // so the find variable is actually bound (a bare value-constant pattern
        // leaves ?n unbound).
        let qc = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :tx/name ?n] [?e :tx/name \"Carol\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let vc: serde_json::Value = serde_json::from_str(&qc).expect("parse");
        assert_eq!(vc["result"].as_str().expect("s"), "Carol");
    }

    // ========================================================================
    // Error handling
    // ========================================================================

    #[pg_test]
    fn test_tx_error_invalid_edn() {
        setup();
        assert!(
            raises_error("SELECT mentat_transact('not valid edn'::TEXT)")
        );
    }

    #[pg_test]
    fn test_tx_error_not_a_vector() {
        setup();
        assert!(
            raises_error("SELECT mentat_transact('{:not \"a vector\"}'::TEXT)")
        );
    }

    #[pg_test]
    fn test_tx_error_unknown_attribute() {
        setup();
        assert!(
            raises_error("SELECT mentat_transact('[[:db/add \"e\" :nonexistent/attr \"val\"]]'::TEXT)")
        );
    }

    #[pg_test]
    fn test_tx_error_type_mismatch() {
        setup(); setup_tx_schema();
        assert!(
            raises_error("SELECT mentat_transact('[[:db/add \"e\" :tx/val \"not-a-long\"]]'::TEXT)")
        );
    }

    #[pg_test]
    fn test_tx_error_empty_assertion() {
        setup();
        assert!(
            raises_error("SELECT mentat_transact('[[:db/add]]'::TEXT)")
        );
    }

    #[pg_test]
    fn test_tx_error_too_few_args() {
        setup();
        assert!(
            raises_error("SELECT mentat_transact('[[:db/add \"e\"]]'::TEXT)")
        );
    }

    // ========================================================================
    // Edge cases
    // ========================================================================

    #[pg_test]
    fn test_tx_empty_transaction() {
        setup();
        // Empty transaction should not error
        let _result = Spi::get_one::<String>(
            "SELECT mentat_transact('[]'::TEXT)",
        );
    }

    #[pg_test]
    fn test_tx_idempotent_add() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tx/name \"idem\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Re-assert exact same fact
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :tx/name \"idem\"]]'::TEXT)", eid
        )).expect("idempotent add");

        // Should still have exactly 1 active datom
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':tx/name')
             AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_tx_ref_tempid_resolution() {
        setup(); setup_tx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"parent\" :tx/name \"parent\"]
                [:db/add \"child\" :tx/name \"child\"]
                [:db/add \"child\" :tx/ref \"parent\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let parent = j["tempids"]["parent"].as_i64().expect("parent");
        let child = j["tempids"]["child"].as_i64().expect("child");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :tx/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", child
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("ref"), parent);
    }
}
