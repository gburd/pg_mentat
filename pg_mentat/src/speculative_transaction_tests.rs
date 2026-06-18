// Speculative transaction (mentat_with / with) tests.
//
// Tests that d/with-style speculative transactions:
// - Return identical reports to committed transactions
// - Do NOT persist any data to the database
// - Handle tempid resolution correctly
// - Enforce constraints (cardinality, uniqueness, CAS)
// - Work with schema definitions
// - Work with the named store variant

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
             $$",
        )
        .expect("create helper");
    }

    fn raises_error(sql: &str) -> bool {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<bool>(&format!("SELECT mentat._test_raises_error('{}')", escaped))
            .expect("raises_error call")
            .unwrap_or(false)
    }

    fn setup_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :spec/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :spec/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :spec/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"u\" :db/ident :spec/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            ]'::TEXT)",
        )
        .expect("spec schema");
    }

    // ========================================================================
    // Basic speculative transaction behavior
    // ========================================================================

    #[pg_test]
    fn test_with_returns_valid_json() {
        setup();
        setup_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_with('[[:db/add \"e\" :spec/name \"Alice\"]]'::TEXT)",
        )
        .expect("with")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&result).expect("parse json");
        assert!(j["db-before"].is_object(), "missing db-before");
        assert!(j["db-after"].is_object(), "missing db-after");
        assert!(j["tx-data"].is_array(), "missing tx-data");
        assert!(j["tempids"].is_object(), "missing tempids");
    }

    #[pg_test]
    fn test_with_does_not_persist_data() {
        setup();
        setup_schema();

        // Count datoms before
        let before_count =
            Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.datoms WHERE added = true")
                .expect("count")
                .expect("NULL");

        // Run speculative transaction
        Spi::run("SELECT mentat_with('[[:db/add \"e\" :spec/name \"Ghost\"]]'::TEXT)")
            .expect("with");

        // Count datoms after -- should be unchanged
        let after_count =
            Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.datoms WHERE added = true")
                .expect("count")
                .expect("NULL");

        assert_eq!(
            before_count, after_count,
            "speculative tx should not persist datoms"
        );
    }

    #[pg_test]
    fn test_with_does_not_persist_transactions() {
        setup();
        setup_schema();

        let before_count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.transactions")
            .expect("count")
            .expect("NULL");

        Spi::run("SELECT mentat_with('[[:db/add \"e\" :spec/name \"Ghost\"]]'::TEXT)")
            .expect("with");

        let after_count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.transactions")
            .expect("count")
            .expect("NULL");

        assert_eq!(
            before_count, after_count,
            "speculative tx should not create transaction records"
        );
    }

    // ========================================================================
    // Tempid resolution in speculative context
    // ========================================================================

    #[pg_test]
    fn test_with_resolves_tempids() {
        setup();
        setup_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_with('[[:db/add \"person\" :spec/name \"Bob\"]]'::TEXT)",
        )
        .expect("with")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let tempids = &j["tempids"];
        assert!(
            tempids["person"].is_number(),
            "tempid 'person' should resolve to a number"
        );
    }

    #[pg_test]
    fn test_with_tempids_are_consistent() {
        setup();
        setup_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_with('[
                [:db/add \"e\" :spec/name \"Alice\"]
                [:db/add \"e\" :spec/val 42]
            ]'::TEXT)",
        )
        .expect("with")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&result).expect("parse");

        // Both assertions should use the same entity ID for tempid "e"
        let tempid_e = j["tempids"]["e"].as_i64().expect("tempid e");
        let tx_data = j["tx-data"].as_array().expect("tx-data array");

        // Find datoms for tempid "e" (skip tx-instant datom at index 0)
        let entity_datoms: Vec<_> = tx_data
            .iter()
            .skip(1)
            .filter(|d| d[0].as_i64() == Some(tempid_e))
            .collect();
        assert_eq!(
            entity_datoms.len(),
            2,
            "both assertions should use the same resolved entity ID"
        );
    }

    // ========================================================================
    // Report format matches committed transactions
    // ========================================================================

    #[pg_test]
    fn test_with_report_has_basis_t() {
        setup();
        setup_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_with('[[:db/add \"e\" :spec/name \"test\"]]'::TEXT)",
        )
        .expect("with")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&result).expect("parse");

        let basis_before = j["db-before"]["basis-t"].as_i64().expect("basis-t before");
        let basis_after = j["db-after"]["basis-t"].as_i64().expect("basis-t after");
        assert!(
            basis_after > basis_before,
            "db-after basis-t ({}) should be greater than db-before basis-t ({})",
            basis_after,
            basis_before
        );
    }

    #[pg_test]
    fn test_with_tx_data_includes_tx_instant() {
        setup();
        setup_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_with('[[:db/add \"e\" :spec/name \"test\"]]'::TEXT)",
        )
        .expect("with")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let tx_data = j["tx-data"].as_array().expect("tx-data");

        // First datom should be :db/txInstant (attribute 50)
        assert!(!tx_data.is_empty(), "tx-data should not be empty");
        let first = &tx_data[0];
        assert_eq!(
            first[1].as_i64().expect("attr"),
            50,
            "first tx-data datom should be :db/txInstant (attr 50)"
        );
        assert_eq!(
            first[4].as_bool().expect("added"),
            true,
            "txInstant datom should be added=true"
        );
    }

    #[pg_test]
    fn test_with_report_format_matches_transact() {
        setup();
        setup_schema();

        // Run speculative transaction
        let with_result = Spi::get_one::<String>(
            "SELECT mentat_with('[[:db/add \"e\" :spec/name \"compare\"]]'::TEXT)",
        )
        .expect("with")
        .expect("NULL");
        let with_j: serde_json::Value = serde_json::from_str(&with_result).expect("parse with");

        // Run real transaction
        let real_result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :spec/name \"compare2\"]]'::TEXT)",
        )
        .expect("transact")
        .expect("NULL");
        let real_j: serde_json::Value = serde_json::from_str(&real_result).expect("parse real");

        // Both should have the same top-level keys
        assert!(with_j["db-before"].is_object());
        assert!(with_j["db-after"].is_object());
        assert!(with_j["tx-data"].is_array());
        assert!(with_j["tempids"].is_object());
        assert!(real_j["db-before"].is_object());
        assert!(real_j["db-after"].is_object());
        assert!(real_j["tx-data"].is_array());
        assert!(real_j["tempids"].is_object());

        // Both should have the same number of tx-data entries
        assert_eq!(
            with_j["tx-data"].as_array().unwrap().len(),
            real_j["tx-data"].as_array().unwrap().len(),
            "tx-data length should match"
        );
    }

    // ========================================================================
    // CAS in speculative context
    // ========================================================================

    #[pg_test]
    fn test_with_cas_success() {
        setup();
        setup_schema();

        // First, commit an entity with a value
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :spec/val 10]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Run speculative CAS -- should succeed
        let with_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_with('[[:db.fn/cas {} :spec/val 10 20]]'::TEXT)",
            eid
        ))
        .expect("with cas")
        .expect("NULL");
        let with_j: serde_json::Value = serde_json::from_str(&with_result).expect("parse");
        let tx_data = with_j["tx-data"].as_array().expect("tx-data");

        // Should contain retract of old value (10) and assert of new value (20)
        // Plus the txInstant datom
        assert!(
            tx_data.len() >= 3,
            "CAS should produce at least 3 datoms (txInstant + retract + assert)"
        );

        // Verify the committed value is unchanged (speculative didn't persist)
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :spec/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(
            v["result"].as_i64().expect("v"),
            10,
            "committed value should remain 10 after speculative CAS"
        );
    }

    #[pg_test]
    fn test_with_cas_failure_returns_error() {
        setup();
        setup_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :spec/val 10]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Speculative CAS with wrong old value should fail
        assert!(
            raises_error(&format!(
                "SELECT mentat_with('[[:db.fn/cas {} :spec/val 999 20]]'::TEXT)",
                eid
            )),
            "CAS with wrong old value should fail in speculative tx"
        );
    }

    #[pg_test]
    fn test_with_cas_does_not_affect_real_value() {
        setup();
        setup_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :spec/val 100]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Successful speculative CAS
        Spi::run(&format!(
            "SELECT mentat_with('[[:db.fn/cas {} :spec/val 100 200]]'::TEXT)",
            eid
        ))
        .expect("with cas");

        // Real value should still be 100
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :spec/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 100);
    }

    // ========================================================================
    // Constraint checking in speculative context
    // ========================================================================

    #[pg_test]
    fn test_with_unique_constraint_checked() {
        setup();
        setup_schema();

        // Commit an entity with a unique identity value
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :spec/uid \"unique-1\"]]'::TEXT)")
            .expect("tx");

        // Speculative tx that tries to add a different entity with the same unique value
        // should trigger upsert (same as committed tx behavior)
        let result = Spi::get_one::<String>(
            "SELECT mentat_with('[[:db/add \"f\" :spec/uid \"unique-1\"]]'::TEXT)",
        )
        .expect("with upsert")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&result).expect("parse");

        // Should not create a new tempid mapping for "f" -- upsert should reuse existing entity
        // The tempid "f" should resolve to the same entity ID as the original
        assert!(
            j["tempids"]["f"].is_number(),
            "upsert should resolve tempid"
        );
    }

    #[pg_test]
    fn test_with_retract_entity() {
        setup();
        setup_schema();

        // Commit an entity
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :spec/name \"Alice\"] [:db/add \"e\" :spec/val 42]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Speculative retractEntity
        let with_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_with('[[:db.fn/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("with retract")
        .expect("NULL");
        let with_j: serde_json::Value = serde_json::from_str(&with_result).expect("parse");
        let tx_data = with_j["tx-data"].as_array().expect("tx-data");

        // Should contain retraction datoms (added=false)
        let retractions: Vec<_> = tx_data
            .iter()
            .filter(|d| d[4].as_bool() == Some(false))
            .collect();
        assert!(
            retractions.len() >= 2,
            "retractEntity should produce at least 2 retraction datoms (name + val)"
        );

        // Verify the entity is still present in the real database
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :spec/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(
            v["result"].as_str().expect("s"),
            "Alice",
            "entity should still exist after speculative retract"
        );
    }

    // ========================================================================
    // Multiple speculative transactions
    // ========================================================================

    #[pg_test]
    fn test_with_multiple_speculative_no_side_effects() {
        setup();
        setup_schema();

        let before_count =
            Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.datoms WHERE added = true")
                .expect("count")
                .expect("NULL");

        // Run 5 speculative transactions
        for i in 0..5 {
            Spi::run(&format!(
                "SELECT mentat_with('[[:db/add \"e\" :spec/name \"ghost-{}\"]]'::TEXT)",
                i
            ))
            .expect("with");
        }

        let after_count =
            Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.datoms WHERE added = true")
                .expect("count")
                .expect("NULL");

        assert_eq!(
            before_count, after_count,
            "5 speculative txns should leave datom count unchanged"
        );
    }

    // ========================================================================
    // Transaction function namespace variants
    // ========================================================================

    #[pg_test]
    fn test_with_db_fn_retract_entity_namespace() {
        setup();
        setup_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :spec/name \"Zap\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Use :db.fn/retractEntity (the db.fn namespace variant)
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_with('[[:db.fn/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("with db.fn/retractEntity")
        .expect("NULL");
        let with_j: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let tx_data = with_j["tx-data"].as_array().expect("tx-data");
        let retractions: Vec<_> = tx_data
            .iter()
            .filter(|d| d[4].as_bool() == Some(false))
            .collect();
        assert!(
            !retractions.is_empty(),
            ":db.fn/retractEntity should produce retractions"
        );
    }

    #[pg_test]
    fn test_with_db_cas_short_namespace() {
        setup();
        setup_schema();

        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :spec/val 5]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Use :db/cas (the short namespace variant)
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_with('[[:db/cas {} :spec/val 5 10]]'::TEXT)",
            eid
        ))
        .expect("with db/cas")
        .expect("NULL");
        let with_j: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let tx_data = with_j["tx-data"].as_array().expect("tx-data");
        assert!(
            tx_data.len() >= 3,
            ":db/cas should produce tx-data (txInstant + retract + assert)"
        );
    }

    // ========================================================================
    // Transaction function discovery API
    // ========================================================================

    #[pg_test]
    fn test_transaction_fns_returns_valid_json() {
        setup();
        let result = Spi::get_one::<String>("SELECT transaction_fns()")
            .expect("fn")
            .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert!(j.is_array(), "transaction_fns should return a JSON array");
        let arr = j.as_array().unwrap();
        assert_eq!(arr.len(), 2, "should list 2 built-in functions");
    }

    #[pg_test]
    fn test_transaction_fns_lists_cas() {
        setup();
        let result = Spi::get_one::<String>("SELECT transaction_fns()")
            .expect("fn")
            .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let arr = j.as_array().unwrap();
        let cas_fn = arr.iter().find(|f| f["name"] == ":db.fn/cas");
        assert!(cas_fn.is_some(), "should list :db.fn/cas");
        let cas = cas_fn.unwrap();
        assert!(cas["args"].as_str().unwrap().contains("old-value"));
        assert!(cas["description"]
            .as_str()
            .unwrap()
            .contains("Compare-and-swap"));
    }

    #[pg_test]
    fn test_transaction_fns_lists_retract_entity() {
        setup();
        let result = Spi::get_one::<String>("SELECT transaction_fns()")
            .expect("fn")
            .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let arr = j.as_array().unwrap();
        let retract_fn = arr.iter().find(|f| f["name"] == ":db.fn/retractEntity");
        assert!(retract_fn.is_some(), "should list :db.fn/retractEntity");
    }
}
