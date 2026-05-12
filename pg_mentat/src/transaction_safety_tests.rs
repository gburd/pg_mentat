// Transaction safety tests: advisory locks, serialization failure handling,
// and isolation behavior from Task #2.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :safety/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :safety/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"c\" :db/ident :safety/counter :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("safety schema");
    }

    // ========================================================================
    // Advisory lock behavior
    // ========================================================================

    #[pg_test]
    fn test_advisory_lock_acquired_during_transaction() {
        setup();
        setup_schema();

        // Run a transaction -- the advisory lock should be acquired and released
        // within the transaction. If this succeeds, the lock mechanism works.
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :safety/name \"lock-test\"]]'::TEXT)")
            .expect("transact with advisory lock");

        // Run another -- if locks weren't released, this would deadlock
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :safety/name \"lock-test-2\"]]'::TEXT)")
            .expect("second transact should not deadlock");
    }

    #[pg_test]
    fn test_sequential_transactions_produce_ordered_tx_ids() {
        setup();
        setup_schema();

        let mut tx_ids: Vec<i64> = Vec::new();

        for i in 0..10 {
            let result = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :safety/name \"seq-{}\"]]'::TEXT)",
                i, i
            ))
            .expect("tx")
            .expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&result).expect("parse");
            let tx_id = j["db-after"]["basis-t"].as_i64().expect("basis-t");
            tx_ids.push(tx_id);
        }

        // Verify monotonically increasing
        for i in 1..tx_ids.len() {
            assert!(
                tx_ids[i] > tx_ids[i - 1],
                "tx IDs should be monotonically increasing: {} should be > {}",
                tx_ids[i],
                tx_ids[i - 1]
            );
        }
    }

    #[pg_test]
    fn test_transaction_basis_t_reflects_prior_state() {
        setup();
        setup_schema();

        // First transaction
        let r1 = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :safety/name \"first\"]]'::TEXT)",
        )
        .expect("tx1")
        .expect("NULL");
        let j1: serde_json::Value = serde_json::from_str(&r1).expect("parse");
        let tx1_after = j1["db-after"]["basis-t"].as_i64().expect("tx1 after");

        // Second transaction -- its db-before should be the same as first's db-after
        let r2 = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"f\" :safety/name \"second\"]]'::TEXT)",
        )
        .expect("tx2")
        .expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        let tx2_before = j2["db-before"]["basis-t"].as_i64().expect("tx2 before");

        assert_eq!(
            tx1_after, tx2_before,
            "second tx's db-before ({}) should equal first tx's db-after ({})",
            tx2_before, tx1_after
        );
    }

    // ========================================================================
    // CAS with advisory lock integration
    // ========================================================================

    #[pg_test]
    fn test_cas_sequential_correctness() {
        setup();
        setup_schema();

        // Create a counter entity
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"c\" :safety/counter 0]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["c"].as_i64().expect("eid");

        // Increment counter 20 times using CAS
        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db.fn/cas {} :safety/counter {} {}]]'::TEXT)",
                eid,
                i,
                i + 1
            ))
            .unwrap_or_else(|e| panic!("CAS {} -> {} failed: {}", i, i + 1, e));
        }

        // Verify final value
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :safety/counter ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 20);
    }

    #[pg_test]
    fn test_cas_wrong_old_value_fails_with_advisory_lock() {
        setup();
        setup_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :safety/counter 100]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // CAS with wrong old value -- should fail even with advisory locks
        let result = Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :safety/counter 999 200]]'::TEXT)",
            eid
        ));
        assert!(result.is_err(), "CAS with wrong old value should fail");

        // Value should remain unchanged
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :safety/counter ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 100);
    }

    // ========================================================================
    // Transaction namespace variant tests (Task #5)
    // ========================================================================

    #[pg_test]
    fn test_db_cas_short_namespace_works() {
        setup();
        setup_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :safety/val 1]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Use :db/cas instead of :db.fn/cas
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :safety/val 1 2]]'::TEXT)",
            eid
        ))
        .expect(":db/cas should work");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :safety/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 2);
    }

    #[pg_test]
    fn test_db_fn_retract_entity_namespace_works() {
        setup();
        setup_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :safety/name \"will-be-retracted\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Use :db.fn/retractEntity instead of :db/retractEntity
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect(":db.fn/retractEntity should work");

        // Entity should be retracted
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :safety/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Result should be nil/null since entity was retracted
        assert!(
            v["result"].is_null(),
            "entity should be retracted, result should be null"
        );
    }

    #[pg_test]
    fn test_db_retract_entity_original_namespace_still_works() {
        setup();
        setup_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :safety/name \"retract-me\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Use original :db/retractEntity
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect(":db/retractEntity should still work");
    }

    // ========================================================================
    // Error type for serialization failure
    // ========================================================================

    #[pg_test]
    fn test_serialization_failure_error_format() {
        crate::ensure_extension_loaded();
        // Test that the SerializationFailure error variant formats correctly
        let err = crate::error::MentatError::SerializationFailure {
            message: "test failure".to_string(),
            attempt: 5,
        };
        let msg = format!("{}", err);
        assert!(
            msg.contains("serialization-failure"),
            "error should contain serialization-failure: {}",
            msg
        );
        assert!(
            msg.contains("attempt 5"),
            "error should contain attempt number: {}",
            msg
        );
    }

    #[pg_test]
    fn test_serialization_failure_error_code() {
        crate::ensure_extension_loaded();
        let err = crate::error::MentatError::SerializationFailure {
            message: "test".to_string(),
            attempt: 1,
        };
        assert_eq!(err.error_code(), ":db.error/serialization-failure");
    }
}
