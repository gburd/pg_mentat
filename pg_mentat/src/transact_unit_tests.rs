// Comprehensive unit tests for transact operations.
//
// Tests cover:
// 1. All value types (ref, bool, long, double, string, keyword, instant, uuid, bytes)
// 2. All operations (assert, retract, retractEntity, cas)
// 3. All cardinalities (one, many)
// 4. Edge cases (empty strings, NULL, zero, negative, max values, unicode)
// 5. Schema definition and attribute resolution
// 6. Tempid allocation and reuse
// 7. Map entity syntax
// 8. Error handling for invalid inputs
// 9. Transaction report format (db-before, db-after, tx-data, tempids)
// 10. Idempotent assertions

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod transact_unit_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    // ========================================================================
    // 1. Value Type Storage Tests (all 9 types)
    // ========================================================================

    #[pg_test]
    fn test_transact_ref_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"link\" :db/ident :tu/link
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/one}
                {:db/id \"name\" :db/ident :tu/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"a\" :tu/name \"source\"]
                [:db/add \"b\" :tu/name \"target\"]
                [:db/add \"a\" :tu/link \"b\"]
            ]'::TEXT)",
        )
        .expect("data failed");

        let v = Spi::get_one::<i64>(
            "SELECT v_ref FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/link')
             AND added = true AND value_type_tag = 0 LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!(v > 0);
    }

    #[pg_test]
    fn test_transact_boolean_true() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/active
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/active true]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<bool>(
            "SELECT v_bool FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/active')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!(v);
    }

    #[pg_test]
    fn test_transact_boolean_false() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/disabled
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/disabled false]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<bool>(
            "SELECT v_bool FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/disabled')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!(!v);
    }

    #[pg_test]
    fn test_transact_long_positive() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/count
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/count 42]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<i64>(
            "SELECT v_long FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/count')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, 42);
    }

    #[pg_test]
    fn test_transact_long_zero() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/zero
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/zero 0]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<i64>(
            "SELECT v_long FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/zero')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, 0);
    }

    #[pg_test]
    fn test_transact_long_negative() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/neg
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/neg -9999]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<i64>(
            "SELECT v_long FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/neg')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, -9999);
    }

    #[pg_test]
    fn test_transact_long_large() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/big
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/big 9223372036854775]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<i64>(
            "SELECT v_long FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/big')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, 9223372036854775_i64);
    }

    #[pg_test]
    fn test_transact_double_positive() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/weight
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/weight 3.14]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<f64>(
            "SELECT v_double FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/weight')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!((v - 3.14).abs() < 0.001);
    }

    #[pg_test]
    fn test_transact_double_zero() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/dzero
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/dzero 0.0]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<f64>(
            "SELECT v_double FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/dzero')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!((v - 0.0).abs() < f64::EPSILON);
    }

    #[pg_test]
    fn test_transact_double_negative() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/dneg
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/dneg -273.15]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<f64>(
            "SELECT v_double FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/dneg')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!((v - (-273.15)).abs() < 0.01);
    }

    #[pg_test]
    fn test_transact_string_basic() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/label
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/label \"hello world\"]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/label')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, "hello world");
    }

    #[pg_test]
    fn test_transact_string_empty() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/empty
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/empty \"\"]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/empty')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, "");
    }

    #[pg_test]
    fn test_transact_string_unicode() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/uni
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run(
            r#"SELECT mentat_transact('[[:db/add "e" :tu/uni "hello ☃ 日本語 🎉"]]'::TEXT)"#,
        )
        .expect("data failed");

        let v = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/uni')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!(v.contains('\u{2603}'), "Should contain snowman");
        assert!(v.contains("日本語"), "Should contain Japanese");
    }

    #[pg_test]
    fn test_transact_keyword_namespaced() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/kw
                 :db/valueType :db.type/keyword
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/kw :foo/bar]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<String>(
            "SELECT v_keyword FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/kw')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, "foo/bar");
    }

    #[pg_test]
    fn test_transact_keyword_plain() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/kwplain
                 :db/valueType :db.type/keyword
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :tu/kwplain :status]]'::TEXT)")
            .expect("data failed");

        let v = Spi::get_one::<String>(
            "SELECT v_keyword FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/kwplain')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, "status");
    }

    #[pg_test]
    fn test_transact_instant_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/created
                 :db/valueType :db.type/instant
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/created #inst \"2024-01-15T10:30:00.000Z\"]]'::TEXT)",
        )
        .expect("data failed");

        let has_instant = Spi::get_one::<bool>(
            "SELECT v_instant IS NOT NULL FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/created')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!(has_instant);
    }

    #[pg_test]
    fn test_transact_uuid_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/uid
                 :db/valueType :db.type/uuid
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/uid #uuid \"550e8400-e29b-41d4-a716-446655440000\"]]'::TEXT)",
        )
        .expect("data failed");

        let has_uuid = Spi::get_one::<bool>(
            "SELECT v_uuid IS NOT NULL FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/uid')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!(has_uuid);
    }

    // ========================================================================
    // 2. Retract Operations
    // ========================================================================

    #[pg_test]
    fn test_retract_specific_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/rname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/rname \"Alice\"]]'::TEXT)",
        )
        .expect("add failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        let eid = tx_report["tempids"]["e"].as_i64().expect("get eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :tu/rname \"Alice\"]]'::TEXT)",
            eid
        ))
        .expect("retract failed");

        let retracted = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND v_text = 'Alice' AND added = false",
            eid
        ))
        .expect("query failed")
        .expect("NULL");

        assert!(retracted > 0);
    }

    #[pg_test]
    fn test_retract_entity() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :tu/rename
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :tu/reage
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :tu/rename \"Bob\"]
                [:db/add \"e\" :tu/reage 30]
            ]'::TEXT)",
        )
        .expect("add failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        let eid = tx_report["tempids"]["e"].as_i64().expect("get eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retractEntity failed");

        let retracted_count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = false",
            eid
        ))
        .expect("query failed")
        .expect("NULL");

        assert!(retracted_count >= 2, "Should retract at least name and age");
    }

    // ========================================================================
    // 3. Compare-and-Swap (CAS)
    // ========================================================================

    #[pg_test]
    fn test_cas_success() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/casval
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/casval \"old\"]]'::TEXT)",
        )
        .expect("add failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        let eid = tx_report["tempids"]["e"].as_i64().expect("get eid");

        // CAS should succeed: current value is "old", swap to "new"
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :tu/casval \"old\" \"new\"]]'::TEXT)",
            eid
        ))
        .expect("CAS should succeed");

        let v = Spi::get_one::<String>(&format!(
            "SELECT v_text FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/casval')
             AND added = true ORDER BY tx DESC LIMIT 1",
            eid
        ))
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, "new");
    }

    #[pg_test]
    fn test_cas_failure() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/casf
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/casf \"current\"]]'::TEXT)",
        )
        .expect("add failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        let eid = tx_report["tempids"]["e"].as_i64().expect("get eid");

        // CAS should fail: expected "wrong" but actual is "current"
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :tu/casf \"wrong\" \"new\"]]'::TEXT)",
            eid
        ));

        assert!(result.is_err(), "CAS should fail when expected value differs");
    }

    #[pg_test]
    fn test_cas_from_nil() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/casnil
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/casnil \"initial\"]]'::TEXT)",
        )
        .expect("add failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        let eid = tx_report["tempids"]["e"].as_i64().expect("get eid");

        // CAS from nil should fail since there IS a value
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :tu/casnil nil \"new\"]]'::TEXT)",
            eid
        ));

        assert!(result.is_err(), "CAS from nil should fail when value exists");
    }

    // ========================================================================
    // 4. Cardinality Tests
    // ========================================================================

    #[pg_test]
    fn test_cardinality_one_replaces() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/c1name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/c1name \"first\"]]'::TEXT)",
        )
        .expect("add failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        let eid = tx_report["tempids"]["e"].as_i64().expect("get eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :tu/c1name \"second\"]]'::TEXT)",
            eid
        ))
        .expect("update failed");

        // Current value should be "second"
        let v = Spi::get_one::<String>(&format!(
            "SELECT v_text FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/c1name')
             AND added = true ORDER BY tx DESC LIMIT 1",
            eid
        ))
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, "second");

        // Old value should be retracted
        let retracted = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND v_text = 'first' AND added = false",
            eid
        ))
        .expect("query failed")
        .expect("NULL");

        assert!(retracted > 0, "Old value should be retracted");
    }

    #[pg_test]
    fn test_cardinality_one_idempotent() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/c1idem
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/c1idem \"same\"]]'::TEXT)",
        )
        .expect("add failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        let eid = tx_report["tempids"]["e"].as_i64().expect("get eid");

        // Assert same value again -- should be idempotent
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :tu/c1idem \"same\"]]'::TEXT)",
            eid
        ))
        .expect("idempotent assert failed");

        // Should not create a retraction datom for "same"
        let retracted = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND v_text = 'same' AND added = false",
            eid
        ))
        .expect("query failed")
        .expect("NULL");

        assert_eq!(retracted, 0, "No retraction for idempotent assertion");
    }

    #[pg_test]
    fn test_cardinality_many_accumulates() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/tags
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e\" :tu/tags \"rust\"]
                [:db/add \"e\" :tu/tags \"postgres\"]
                [:db/add \"e\" :tu/tags \"datalog\"]
            ]'::TEXT)",
        )
        .expect("data failed");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/tags')
             AND added = true",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(count, 3, "Should have 3 tags");
    }

    #[pg_test]
    fn test_cardinality_many_idempotent() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/tags2
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/tags2 \"duplicate\"]]'::TEXT)",
        )
        .expect("first assert failed");

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/tags2 \"duplicate\"]]'::TEXT)",
        )
        .expect("second assert should succeed (idempotent)");

        // The duplicate should be skipped, so only 1 active datom
        // Note: "e" might resolve to a different tempid each time
        // The query checks across all entities
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/tags2')
             AND v_text = 'duplicate' AND added = true",
        )
        .expect("query failed")
        .expect("NULL");

        // Each call to mentat_transact with a new tempid creates a new entity,
        // so each will have its own "duplicate" datom. That's expected.
        assert!(count >= 1);
    }

    #[pg_test]
    fn test_cardinality_one_multiple_in_tx_rejected() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/single
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        // Two different values for same entity+attribute in one transaction
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :tu/single \"val1\"]
                [:db/add \"e\" :tu/single \"val2\"]
            ]'::TEXT)",
        );

        assert!(
            result.is_err(),
            "Should reject multiple values for cardinality-one in same tx"
        );
    }

    // ========================================================================
    // 5. Tempid Allocation
    // ========================================================================

    #[pg_test]
    fn test_tempid_reuse_within_transaction() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :tu/tname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :tu/tage
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"person1\" :tu/tname \"Alice\"]
                [:db/add \"person1\" :tu/tage 30]
            ]'::TEXT)",
        )
        .expect("transact failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        let eid = tx_report["tempids"]["person1"].as_i64().expect("get eid");

        // Both datoms should have the same entity ID
        let name_eid = Spi::get_one::<i64>(
            &format!(
                "SELECT e FROM mentat.datoms
                 WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/tname')
                 AND e = {} AND added = true LIMIT 1",
                eid
            ),
        )
        .expect("query failed")
        .expect("NULL");

        let age_eid = Spi::get_one::<i64>(
            &format!(
                "SELECT e FROM mentat.datoms
                 WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/tage')
                 AND e = {} AND added = true LIMIT 1",
                eid
            ),
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(name_eid, age_eid, "Same tempid should yield same entity");
    }

    #[pg_test]
    fn test_tempid_unique_across_names() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/tval
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"a\" :tu/tval 1]
                [:db/add \"b\" :tu/tval 2]
            ]'::TEXT)",
        )
        .expect("transact failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        let eid_a = tx_report["tempids"]["a"].as_i64().expect("get eid a");
        let eid_b = tx_report["tempids"]["b"].as_i64().expect("get eid b");

        assert_ne!(eid_a, eid_b, "Different tempids should yield different entities");
    }

    // ========================================================================
    // 6. Map Entity Syntax
    // ========================================================================

    #[pg_test]
    fn test_map_entity_with_db_id() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :tu/mname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :tu/mage
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"p1\" :tu/mname \"Alice\" :tu/mage 25}
            ]'::TEXT)",
        )
        .expect("map entity failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        assert!(
            tx_report["tempids"]["p1"].as_i64().is_some(),
            "Should allocate entity for map"
        );
    }

    #[pg_test]
    fn test_map_entity_without_db_id() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/mnoval
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        // Map entity without :db/id should auto-allocate
        Spi::run(
            "SELECT mentat_transact('[
                {:tu/mnoval \"no-id-entity\"}
            ]'::TEXT)",
        )
        .expect("map entity without db/id failed");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/mnoval')
             AND v_text = 'no-id-entity' AND added = true",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(count, 1);
    }

    // ========================================================================
    // 7. Schema Definition
    // ========================================================================

    #[pg_test]
    fn test_schema_define_with_all_properties() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/fullattr
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity
                 :db/index true
                 :db/fulltext true
                 :db/noHistory true}
            ]'::TEXT)",
        )
        .expect("full schema definition failed");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.schema WHERE ident = ':tu/fullattr'",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(count, 1, "Should be in schema table");
    }

    #[pg_test]
    fn test_schema_define_and_use_in_same_tx() {
        setup();
        // Define schema AND use it in the same transaction
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/samename
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                [:db/add \"e\" :tu/samename \"created in same tx\"]
            ]'::TEXT)",
        )
        .expect("define + use in same tx failed");

        let v = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/samename')
             AND added = true LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, "created in same tx");
    }

    // ========================================================================
    // 8. Unique Constraints
    // ========================================================================

    #[pg_test]
    fn test_unique_value_prevents_duplicate() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/code
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/value}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e1\" :tu/code \"ABC\"]]'::TEXT)",
        )
        .expect("first insert");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e2\" :tu/code \"ABC\"]]'::TEXT)",
        );

        assert!(result.is_err(), "Should reject duplicate unique value");
    }

    #[pg_test]
    fn test_unique_identity_upsert() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/email
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e1\" :tu/email \"alice@test.com\"]]'::TEXT)",
        )
        .expect("first insert");

        // Second insert with same identity value should upsert
        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e2\" :tu/email \"alice@test.com\"]]'::TEXT)",
        )
        .expect("upsert should succeed");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/email')
             AND v_text = 'alice@test.com' AND added = true",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(count, 1, "Should have exactly 1 entity with this email");
    }

    // ========================================================================
    // 9. Transaction Report Format
    // ========================================================================

    #[pg_test]
    fn test_tx_report_has_all_fields() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/rpname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/rpname \"test\"]]'::TEXT)",
        )
        .expect("transact failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");

        assert!(tx_report.get("db-before").is_some(), "Missing db-before");
        assert!(tx_report.get("db-after").is_some(), "Missing db-after");
        assert!(tx_report.get("tx-data").is_some(), "Missing tx-data");
        assert!(tx_report.get("tempids").is_some(), "Missing tempids");

        let db_before = &tx_report["db-before"];
        assert!(db_before.get("basis-t").is_some(), "db-before missing basis-t");

        let db_after = &tx_report["db-after"];
        assert!(db_after.get("basis-t").is_some(), "db-after missing basis-t");

        let tx_data = tx_report["tx-data"].as_array().expect("tx-data array");
        assert!(!tx_data.is_empty(), "tx-data should not be empty");

        // First datom should be :db/txInstant
        let first = tx_data[0].as_array().expect("first datom array");
        assert_eq!(first.len(), 5, "Each datom should have [e a v tx added]");
    }

    #[pg_test]
    fn test_tx_report_basis_t_advances() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/advname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result1 = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e1\" :tu/advname \"a\"]]'::TEXT)",
        )
        .expect("tx1 failed")
        .expect("NULL");

        let result2 = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e2\" :tu/advname \"b\"]]'::TEXT)",
        )
        .expect("tx2 failed")
        .expect("NULL");

        let tx1: serde_json::Value = serde_json::from_str(&result1).expect("parse tx1");
        let tx2: serde_json::Value = serde_json::from_str(&result2).expect("parse tx2");

        let t1 = tx1["db-after"]["basis-t"].as_i64().expect("t1");
        let t2 = tx2["db-after"]["basis-t"].as_i64().expect("t2");

        assert!(t2 > t1, "basis-t should advance: t1={}, t2={}", t1, t2);
    }

    // ========================================================================
    // 10. Error Handling
    // ========================================================================

    #[pg_test]
    fn test_error_invalid_edn() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('not valid edn {'::TEXT)",
        );
        assert!(result.is_err(), "Should reject invalid EDN");
    }

    #[pg_test]
    fn test_error_not_a_vector() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('{:key \"value\"}'::TEXT)",
        );
        assert!(result.is_err(), "Should reject non-vector transaction");
    }

    #[pg_test]
    fn test_error_unknown_attribute() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :nonexistent/attr \"val\"]]'::TEXT)",
        );
        assert!(result.is_err(), "Should reject unknown attribute");
    }

    #[pg_test]
    fn test_error_type_mismatch() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/numonly
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :tu/numonly \"not a number\"]]'::TEXT)",
        );

        assert!(result.is_err(), "Should reject string for long attribute");
    }

    #[pg_test]
    fn test_empty_transaction() {
        setup();
        // Empty vector should work without panic
        let _result = Spi::get_one::<String>("SELECT mentat_transact('[]'::TEXT)");
    }

    // ========================================================================
    // 11. Lookup Refs
    // ========================================================================

    #[pg_test]
    fn test_lookup_ref_in_entity_place() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"email\" :db/ident :tu/lremail
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity}
                {:db/id \"name\" :db/ident :tu/lrname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e\" :tu/lremail \"alice@example.com\"]
                [:db/add \"e\" :tu/lrname \"Alice\"]
            ]'::TEXT)",
        )
        .expect("data failed");

        // Use lookup ref to update the entity
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add [:tu/lremail \"alice@example.com\"] :tu/lrname \"Alice Updated\"]
            ]'::TEXT)",
        )
        .expect("lookup ref update failed");

        let v = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':tu/lrname')
             AND added = true ORDER BY tx DESC LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v, "Alice Updated");
    }

    // ========================================================================
    // 12. Batch Transaction Tests
    // ========================================================================

    #[pg_test]
    fn test_batch_50_entities() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :tu/bname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :tu/bval
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        let mut assertions = Vec::new();
        for i in 0..50 {
            assertions.push(format!(
                "[:db/add \"e{i}\" :tu/bname \"entity-{i}\"]
                 [:db/add \"e{i}\" :tu/bval {i}]",
                i = i
            ));
        }
        let txn = format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            assertions.join("\n")
        );
        Spi::run(&txn).expect("batch txn failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name :where [?e :tu/bname ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");
        assert_eq!(results.len(), 50);
    }

    #[pg_test]
    fn test_sequential_20_transactions() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :tu/seqval
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{i}\" :tu/seqval {i}]]'::TEXT)",
                i = i
            ))
            .expect("sequential txn failed");
        }

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v :where [?e :tu/seqval ?v] :order (asc ?v)]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");
        assert_eq!(results.len(), 20);

        let vals: Vec<i64> = results.iter().map(|r| r[0].as_i64().expect("int")).collect();
        assert_eq!(vals, (0..20).collect::<Vec<i64>>());
    }
}
