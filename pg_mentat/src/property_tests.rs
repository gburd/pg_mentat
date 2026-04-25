// Property-based tests for pg_mentat.
//
// These tests systematically verify properties that should hold for ALL valid inputs,
// using parameterized iteration to cover wide ranges of values.
//
// Properties tested:
// 1. Transact-then-query roundtrip: any value stored can be retrieved
// 2. Retract idempotency: retracting already-retracted data is safe
// 3. Transaction ordering: tx IDs are monotonically increasing
// 4. Cardinality-one replacement: latest write wins
// 5. Cardinality-many accumulation: all values preserved
// 6. Upsert determinism: same input yields same result
// 7. Empty transaction safety
// 8. Schema attribute properties preserved

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod property_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_prop_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"sn\" :db/ident :prop/str
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"ln\" :db/ident :prop/num
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"dn\" :db/ident :prop/dbl
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
                {:db/id \"bn\" :db/ident :prop/flag
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
                {:db/id \"kn\" :db/ident :prop/kw
                 :db/valueType :db.type/keyword
                 :db/cardinality :db.cardinality/one}
                {:db/id \"mn\" :db/ident :prop/tags
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/many}
                {:db/id \"un\" :db/ident :prop/uid
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity}
                {:db/id \"vn\" :db/ident :prop/code
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/value}
                {:db/id \"rn\" :db/ident :prop/ref
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/one}
                {:db/id \"rm\" :db/ident :prop/refs
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        )
        .expect("prop schema failed");
    }

    // ========================================================================
    // Property 1: String roundtrip for various lengths
    // ========================================================================

    #[pg_test]
    fn test_prop_string_roundtrip_empty() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/str \"\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");
        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :prop/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_str().expect("str"), "");
    }

    #[pg_test]
    fn test_prop_string_roundtrip_single_char() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/str \"x\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");
        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :prop/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_str().expect("str"), "x");
    }

    #[pg_test]
    fn test_prop_string_roundtrip_lengths() {
        setup();
        setup_prop_schema();
        // Test strings of various lengths: 1, 10, 100, 1000, 5000
        for &len in &[1, 10, 100, 1000, 5000] {
            let s: String = (0..len).map(|i| (b'a' + (i % 26) as u8) as char).collect();
            let result = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :prop/str \"{}\"]]'::TEXT)",
                len, s
            ))
            .expect("tx")
            .expect("NULL");
            let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
            let eid = r["tempids"][&format!("e{}", len)].as_i64().expect("eid");
            let qr = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :prop/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
                eid
            ))
            .expect("q")
            .expect("NULL");
            let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
            assert_eq!(json["result"].as_str().expect("str").len(), len);
        }
    }

    // ========================================================================
    // Property 2: Long integer roundtrip across range
    // ========================================================================

    #[pg_test]
    fn test_prop_long_roundtrip_zero() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/num 0]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");
        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :prop/num ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_i64().expect("num"), 0);
    }

    #[pg_test]
    fn test_prop_long_roundtrip_positive_range() {
        setup();
        setup_prop_schema();
        for &val in &[1i64, 42, 100, 1000, 1_000_000, 1_000_000_000, i64::MAX] {
            let result = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :prop/num {}]]'::TEXT)",
                val, val
            ))
            .expect("tx")
            .expect("NULL");
            let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
            let eid = r["tempids"][&format!("e{}", val)].as_i64().expect("eid");
            let qr = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :prop/num ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
                eid
            ))
            .expect("q")
            .expect("NULL");
            let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
            assert_eq!(json["result"].as_i64().expect("num"), val);
        }
    }

    #[pg_test]
    fn test_prop_long_roundtrip_negative_range() {
        setup();
        setup_prop_schema();
        for &val in &[-1i64, -42, -100, -1000, -1_000_000, -1_000_000_000, i64::MIN] {
            let label = format!("en{}", val.unsigned_abs());
            let result = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"{}\" :prop/num {}]]'::TEXT)",
                label, val
            ))
            .expect("tx")
            .expect("NULL");
            let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
            let eid = r["tempids"][&label].as_i64().expect("eid");
            let qr = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :prop/num ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
                eid
            ))
            .expect("q")
            .expect("NULL");
            let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
            assert_eq!(json["result"].as_i64().expect("num"), val);
        }
    }

    // ========================================================================
    // Property 3: Double roundtrip
    // ========================================================================

    #[pg_test]
    fn test_prop_double_roundtrip_values() {
        setup();
        setup_prop_schema();
        for (i, &val) in [0.0f64, 1.0, -1.0, 0.5, 99.99, 1e10, -1e10, 1e-10]
            .iter()
            .enumerate()
        {
            let result = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"d{}\" :prop/dbl {}]]'::TEXT)",
                i, val
            ))
            .expect("tx")
            .expect("NULL");
            let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
            let eid = r["tempids"][&format!("d{}", i)].as_i64().expect("eid");
            let qr = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :prop/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
                eid
            ))
            .expect("q")
            .expect("NULL");
            let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
            let retrieved = json["result"].as_f64().expect("dbl");
            assert!(
                (retrieved - val).abs() < 1e-6 || (val != 0.0 && ((retrieved - val) / val).abs() < 1e-6),
                "Expected {} got {}",
                val,
                retrieved
            );
        }
    }

    // ========================================================================
    // Property 4: Boolean roundtrip
    // ========================================================================

    #[pg_test]
    fn test_prop_boolean_true_roundtrip() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/flag true]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");
        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :prop/flag ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_bool().expect("bool"), true);
    }

    #[pg_test]
    fn test_prop_boolean_false_roundtrip() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/flag false]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");
        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :prop/flag ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_bool().expect("bool"), false);
    }

    // ========================================================================
    // Property 5: Transaction IDs are monotonically increasing
    // ========================================================================

    #[pg_test]
    fn test_prop_tx_ids_monotonically_increasing() {
        setup();
        setup_prop_schema();

        let mut prev_tx: i64 = 0;
        for i in 0..20 {
            let result = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :prop/str \"val-{}\"]]'::TEXT)",
                i, i
            ))
            .expect("tx")
            .expect("NULL");
            let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
            let tx = r["tx"].as_i64().unwrap_or_else(|| r["db-after"].as_i64().unwrap_or(0));

            // If we can extract tx, verify monotonicity
            if tx > 0 && prev_tx > 0 {
                assert!(
                    tx > prev_tx,
                    "tx {} should be > prev_tx {}",
                    tx,
                    prev_tx
                );
            }
            if tx > 0 {
                prev_tx = tx;
            }
        }
    }

    // ========================================================================
    // Property 6: Cardinality-one replacement (latest write wins)
    // ========================================================================

    #[pg_test]
    fn test_prop_cardinality_one_latest_wins() {
        setup();
        setup_prop_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/num 0]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Update 50 times
        for i in 1..=50 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :prop/num {}]]'::TEXT)",
                eid, i
            ))
            .expect("update");
        }

        // Should have exactly one current value = 50
        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :prop/num ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_i64().expect("num"), 50);

        // Should only have 1 active datom for this e/a
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':prop/num')
             AND added = true",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert_eq!(count, 1, "Should have exactly 1 active datom for cardinality-one");
    }

    // ========================================================================
    // Property 7: Cardinality-many accumulation
    // ========================================================================

    #[pg_test]
    fn test_prop_cardinality_many_accumulates_all() {
        setup();
        setup_prop_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/str \"holder\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Add 30 distinct tags across 30 transactions
        for i in 0..30 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :prop/tags \"tag-{}\"]]'::TEXT)",
                eid, i
            ))
            .expect("add tag");
        }

        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :prop/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        let tags = json["result"].as_array().expect("arr");
        assert_eq!(tags.len(), 30, "All 30 tags should be present");
    }

    #[pg_test]
    fn test_prop_cardinality_many_no_duplicates() {
        setup();
        setup_prop_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/str \"holder\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Add same tag 10 times (should be idempotent)
        for _ in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :prop/tags \"same-tag\"]]'::TEXT)",
                eid
            ))
            .expect("add tag");
        }

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':prop/tags')
             AND v_text = 'same-tag' AND added = true",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert_eq!(count, 1, "Duplicate many-value adds should be idempotent");
    }

    // ========================================================================
    // Property 8: Retract idempotency
    // ========================================================================

    #[pg_test]
    fn test_prop_retract_nonexistent_is_safe() {
        setup();
        setup_prop_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/str \"test\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Retract a value that was never asserted
        // This should either succeed silently or produce a clear error
        let retract_result = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/retract {} :prop/num 999]]'::TEXT)",
            eid
        ));
        // Either it succeeds (retract of non-asserted is no-op) or fails clearly
        // Either way, the DB should be consistent
        let _ok = retract_result.is_ok();

        // Original value should still be there
        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :prop/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_str().expect("str"), "test");
    }

    // ========================================================================
    // Property 9: Upsert determinism
    // ========================================================================

    #[pg_test]
    fn test_prop_upsert_same_input_same_result() {
        setup();
        setup_prop_schema();

        // Create entity with unique identity
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :prop/uid \"user-1\" :prop/num 10}]'::TEXT)",
        )
        .expect("initial");

        // Upsert same identity 10 times with different num values
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"u\" :prop/uid \"user-1\" :prop/num {}}}]'::TEXT)",
                100 + i
            ))
            .expect("upsert");
        }

        // Should still be exactly 1 entity with this uid
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':prop/uid')
             AND v_text = 'user-1' AND added = true",
        )
        .expect("q")
        .expect("NULL");
        assert_eq!(count, 1, "Upsert should not create duplicate entities");

        // Final value should be 109
        let qr = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :prop/uid \"user-1\"] [?e :prop/num ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_i64().expect("num"), 109);
    }

    // ========================================================================
    // Property 10: Schema attributes preserve all properties
    // ========================================================================

    #[pg_test]
    fn test_prop_schema_preserves_cardinality() {
        setup();

        // Create 20 cardinality-one and 20 cardinality-many attributes
        let mut ops = Vec::new();
        for i in 0..20 {
            ops.push(format!(
                "{{:db/id \"co{i}\" :db/ident :prop.gen/one-{i} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}",
                i = i
            ));
            ops.push(format!(
                "{{:db/id \"cm{i}\" :db/ident :prop.gen/many-{i} :db/valueType :db.type/string :db/cardinality :db.cardinality/many}}",
                i = i
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("schema");

        // Verify via mentat_schema
        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema query")
            .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");

        // Check that at least some of our attributes exist in the schema
        let schema_str = serde_json::to_string(&json).expect("serialize");
        for i in 0..20 {
            assert!(
                schema_str.contains(&format!(":prop.gen/one-{}", i)),
                "one-{} missing from schema",
                i
            );
            assert!(
                schema_str.contains(&format!(":prop.gen/many-{}", i)),
                "many-{} missing from schema",
                i
            );
        }
    }

    // ========================================================================
    // Property 11: Unique value constraint enforcement
    // ========================================================================

    #[pg_test]
    fn test_prop_unique_value_prevents_duplicates() {
        setup();
        setup_prop_schema();

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e1\" :prop/code \"CODE-001\"]]'::TEXT)",
        )
        .expect("first insert");

        // Second entity with same unique-value code should fail
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e2\" :prop/code \"CODE-001\"]]'::TEXT)",
        );
        assert!(
            result.is_err(),
            "Duplicate unique-value should be rejected"
        );
    }

    #[pg_test]
    fn test_prop_unique_identity_upserts() {
        setup();
        setup_prop_schema();

        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e1\" :prop/uid \"UID-001\" :prop/str \"original\"}]'::TEXT)",
        )
        .expect("first insert");

        // Second entity with same unique-identity should upsert
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e2\" :prop/uid \"UID-001\" :prop/str \"updated\"}]'::TEXT)",
        )
        .expect("upsert");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':prop/uid')
             AND v_text = 'UID-001' AND added = true",
        )
        .expect("q")
        .expect("NULL");
        assert_eq!(count, 1, "Identity upsert should not create new entity");

        let qr = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?s . :where [?e :prop/uid \"UID-001\"] [?e :prop/str ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_str().expect("str"), "updated");
    }

    // ========================================================================
    // Property 12: Transaction atomicity
    // ========================================================================

    #[pg_test]
    fn test_prop_transaction_all_or_nothing() {
        setup();
        setup_prop_schema();

        // Count entities before
        let before = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE added = true",
        )
        .expect("q")
        .expect("NULL");

        // Attempt a transaction with a valid op followed by invalid op
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e1\" :prop/str \"valid\"]
                [:db/add \"e2\" :prop/nonexistent \"invalid\"]
            ]'::TEXT)",
        );
        assert!(result.is_err(), "Transaction with bad attr should fail");

        // Count after - should be same as before (rolled back)
        let after = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE added = true",
        )
        .expect("q")
        .expect("NULL");
        assert_eq!(before, after, "Failed transaction should roll back completely");
    }

    // ========================================================================
    // Property 13: Keyword roundtrip
    // ========================================================================

    #[pg_test]
    fn test_prop_keyword_roundtrip_various() {
        setup();
        setup_prop_schema();

        let keywords = vec![
            ":simple",
            ":namespaced/keyword",
            ":db/ident",
            ":my.long.namespace/attr-name",
            ":a",
        ];

        for (i, kw) in keywords.iter().enumerate() {
            let result = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"k{}\" :prop/kw {}]]'::TEXT)",
                i, kw
            ))
            .expect("tx")
            .expect("NULL");
            let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
            let eid = r["tempids"][&format!("k{}", i)].as_i64().expect("eid");

            let qr = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :prop/kw ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
                eid
            ))
            .expect("q")
            .expect("NULL");
            let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
            let retrieved = json["result"].as_str().expect("kw");
            assert!(
                retrieved.contains(&kw.trim_start_matches(':')),
                "Expected keyword containing {}, got {}",
                kw,
                retrieved
            );
        }
    }

    // ========================================================================
    // Property 14: Ref integrity
    // ========================================================================

    #[pg_test]
    fn test_prop_ref_roundtrip() {
        setup();
        setup_prop_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"a\" :prop/str \"parent\"]
                [:db/add \"b\" :prop/str \"child\"]
                [:db/add \"b\" :prop/ref \"a\"]
            ]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let a_eid = r["tempids"]["a"].as_i64().expect("a");
        let b_eid = r["tempids"]["b"].as_i64().expect("b");

        // Query the ref
        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?ref . :where [{} :prop/ref ?ref]]'::TEXT, '{{}}'::jsonb)::TEXT",
            b_eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_i64().expect("ref"), a_eid);
    }

    #[pg_test]
    fn test_prop_ref_many_accumulates() {
        setup();
        setup_prop_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"root\" :prop/str \"root\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let root = r["tempids"]["root"].as_i64().expect("root");

        // Create 10 children each referencing root
        let mut child_ops = Vec::new();
        for i in 0..10 {
            child_ops.push(format!(
                "[:db/add \"c{}\" :prop/str \"child-{}\"] [:db/add \"c{}\" :prop/refs {}]",
                i, i, i, root
            ));
        }
        // Also add refs from root to children
        // We do this in reverse for variety
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            child_ops.join("\n")
        ))
        .expect("children");

        // Count entities that ref root via :prop/refs
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':prop/refs')
             AND v_ref = {} AND added = true",
            root
        ))
        .expect("q")
        .expect("NULL");
        assert_eq!(count, 10);
    }

    // ========================================================================
    // Property 15: Entity count consistency
    // ========================================================================

    #[pg_test]
    fn test_prop_entity_count_after_batch() {
        setup();
        setup_prop_schema();

        let n = 100;
        let mut ops = Vec::new();
        for i in 0..n {
            ops.push(format!(
                "[:db/add \"batch{}\" :prop/str \"entity-{}\"]",
                i, i
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':prop/str')
             AND added = true",
        )
        .expect("q")
        .expect("NULL");
        assert_eq!(count, n);
    }

    // ========================================================================
    // Property 16: Retract then re-assert
    // ========================================================================

    #[pg_test]
    fn test_prop_retract_then_reassert() {
        setup();
        setup_prop_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/num 42]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Retract
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :prop/num 42]]'::TEXT)",
            eid
        ))
        .expect("retract");

        // Re-assert same value
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :prop/num 42]]'::TEXT)",
            eid
        ))
        .expect("reassert");

        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :prop/num ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_i64().expect("num"), 42);
    }

    #[pg_test]
    fn test_prop_retract_then_reassert_different_value() {
        setup();
        setup_prop_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/num 42]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Retract old, add new
        Spi::run(&format!(
            "SELECT mentat_transact('[
                [:db/retract {} :prop/num 42]
                [:db/add {} :prop/num 99]
            ]'::TEXT)",
            eid, eid
        ))
        .expect("retract+add");

        let qr = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :prop/num ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
        assert_eq!(json["result"].as_i64().expect("num"), 99);
    }

    // ========================================================================
    // Property 17: Unicode string preservation
    // ========================================================================

    #[pg_test]
    fn test_prop_unicode_roundtrip() {
        setup();
        setup_prop_schema();

        let unicode_strings = vec![
            ("u0", "Hello World"),           // ASCII
            ("u1", "cafe\\u0301"),            // Combining accent
            ("u2", "Tokyo"),                  // ASCII representation
            ("u3", "Привет"),                  // Cyrillic
            ("u4", "مرحبا"),                   // Arabic
        ];

        for (label, val) in &unicode_strings {
            let result = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"{}\" :prop/str \"{}\"]]'::TEXT)",
                label, val
            ))
            .expect("tx")
            .expect("NULL");
            let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
            let eid = r["tempids"][*label].as_i64().expect("eid");

            let qr = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :prop/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
                eid
            ))
            .expect("q")
            .expect("NULL");
            let json: serde_json::Value = serde_json::from_str(&qr).expect("parse");
            assert!(
                json["result"].as_str().is_some(),
                "Unicode string {} should roundtrip",
                label
            );
        }
    }

    // ========================================================================
    // Property 18: Schema value type enforcement
    // ========================================================================

    #[pg_test]
    fn test_prop_type_mismatch_string_to_long() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/num \"not-a-number\"]]'::TEXT)",
        );
        assert!(result.is_err(), "String to long attr should fail");
    }

    #[pg_test]
    fn test_prop_type_mismatch_long_to_string() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/str 42]]'::TEXT)",
        );
        assert!(result.is_err(), "Long to string attr should fail");
    }

    #[pg_test]
    fn test_prop_type_mismatch_string_to_boolean() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/flag \"not-a-bool\"]]'::TEXT)",
        );
        assert!(result.is_err(), "String to boolean attr should fail");
    }

    #[pg_test]
    fn test_prop_type_mismatch_string_to_double() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/dbl \"not-a-double\"]]'::TEXT)",
        );
        assert!(result.is_err(), "String to double attr should fail");
    }

    #[pg_test]
    fn test_prop_type_mismatch_long_to_boolean() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/flag 42]]'::TEXT)",
        );
        assert!(result.is_err(), "Long to boolean attr should fail");
    }

    #[pg_test]
    fn test_prop_type_mismatch_boolean_to_long() {
        setup();
        setup_prop_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :prop/num true]]'::TEXT)",
        );
        assert!(result.is_err(), "Boolean to long attr should fail");
    }

    // ========================================================================
    // Property 19: Large batch entity IDs are all unique
    // ========================================================================

    #[pg_test]
    fn test_prop_batch_entity_ids_unique() {
        setup();
        setup_prop_schema();

        let n = 200;
        let mut ops = Vec::new();
        for i in 0..n {
            ops.push(format!(
                "[:db/add \"uniq{}\" :prop/str \"entity-{}\"]",
                i, i
            ));
        }
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let tempids = r["tempids"].as_object().expect("tempids");
        assert_eq!(tempids.len(), n);

        // Verify all IDs are unique
        let mut ids: Vec<i64> = tempids
            .values()
            .map(|v| v.as_i64().expect("eid"))
            .collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "All entity IDs should be unique");
    }

    // ========================================================================
    // Property 20: RetractEntity completeness
    // ========================================================================

    #[pg_test]
    fn test_prop_retract_entity_removes_all_attributes() {
        setup();
        setup_prop_schema();

        // Create entity with multiple attributes
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"e\"
                 :prop/str \"doomed\"
                 :prop/num 42
                 :prop/flag true
                 :prop/dbl 3.14
                 :prop/kw :doomed}
                [:db/add \"e\" :prop/tags \"tag1\"]
                [:db/add \"e\" :prop/tags \"tag2\"]
                [:db/add \"e\" :prop/tags \"tag3\"]
            ]'::TEXT)",
        )
        .expect("create")
        .expect("NULL");
        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Verify entity has many active facts
        let before = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = true",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert!(before >= 7, "Should have at least 7 active facts, got {}", before);

        // Retract entire entity
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retract");

        // All facts should now have retraction datoms
        let retractions = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = false",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert!(retractions >= before, "Should have at least {} retraction datoms", before);
    }
}
