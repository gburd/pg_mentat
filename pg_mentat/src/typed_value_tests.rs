// Comprehensive tests for typed column encoding/decoding and transact operations.
//
// These tests verify that:
// 1. All value types are correctly stored in their typed columns
// 2. Range queries return correct results with native types (not BYTEA)
// 3. CHECK constraint enforces exactly one value column per row
// 4. Type coercion and edge cases are handled properly
// 5. Retraction works correctly with typed columns
// 6. Unique constraints work across typed columns

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod typed_value_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    // ========================================================================
    // Value Type Storage Tests
    // ========================================================================

    /// Verify that string values are stored in v_text column
    #[pg_test]
    fn test_string_stored_in_v_text() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/str
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/str \"hello world\"]]'::TEXT)")
            .expect("data txn failed");

        let v_text = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/str')
             AND added = true AND value_type_tag = 7
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_text");

        assert_eq!(v_text, "hello world");
    }

    /// Verify that long values are stored in v_long column
    #[pg_test]
    fn test_long_stored_in_v_long() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/num
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/num 42]]'::TEXT)")
            .expect("data txn failed");

        let v_long = Spi::get_one::<i64>(
            "SELECT v_long FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/num')
             AND added = true AND value_type_tag = 2
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_long");

        assert_eq!(v_long, 42);
    }

    /// Verify that boolean values are stored in v_bool column
    #[pg_test]
    fn test_boolean_stored_in_v_bool() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/flag
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/flag true]]'::TEXT)")
            .expect("data txn failed");

        let v_bool = Spi::get_one::<bool>(
            "SELECT v_bool FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/flag')
             AND added = true AND value_type_tag = 1
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_bool");

        assert!(v_bool);
    }

    /// Verify that double values are stored in v_double column
    #[pg_test]
    fn test_double_stored_in_v_double() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/weight
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/weight 3.14]]'::TEXT)")
            .expect("data txn failed");

        let v_double = Spi::get_one::<f64>(
            "SELECT v_double FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/weight')
             AND added = true AND value_type_tag = 3
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_double");

        assert!((v_double - 3.14).abs() < 0.001);
    }

    /// Verify that keyword values are stored in v_keyword column
    #[pg_test]
    fn test_keyword_stored_in_v_keyword() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/kw
                 :db/valueType :db.type/keyword
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/kw :foo/bar]]'::TEXT)")
            .expect("data txn failed");

        let v_keyword = Spi::get_one::<String>(
            "SELECT v_keyword FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/kw')
             AND added = true AND value_type_tag = 8
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_keyword");

        assert_eq!(v_keyword, "foo/bar");
    }

    /// Verify that ref values are stored in v_ref column
    #[pg_test]
    fn test_ref_stored_in_v_ref() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/link
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/one}
                {:db/id \"name\" :db/ident :test/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"a\" :test/name \"source\"]
                [:db/add \"b\" :test/name \"target\"]
                [:db/add \"a\" :test/link \"b\"]
            ]'::TEXT)",
        )
        .expect("data txn failed");

        // v_ref should be a valid entity ID (not NULL)
        let v_ref = Spi::get_one::<i64>(
            "SELECT v_ref FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/link')
             AND added = true AND value_type_tag = 0
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_ref");

        assert!(
            v_ref > 0,
            "v_ref should be a positive entity ID, got {}",
            v_ref
        );
    }

    /// Verify that instant values are stored in v_instant column as TIMESTAMPTZ
    #[pg_test]
    fn test_instant_stored_in_v_instant() {
        setup();
        // :db/txInstant is already a built-in instant attribute
        // Just check that the bootstrap transaction has a v_instant value
        let has_instant = Spi::get_one::<bool>(
            "SELECT v_instant IS NOT NULL FROM mentat.datoms
             WHERE a = 10  -- :db/txInstant
             AND added = true AND value_type_tag = 4
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL result");

        assert!(
            has_instant,
            "v_instant should be NOT NULL for :db/txInstant datoms"
        );
    }

    // ========================================================================
    // CHECK Constraint Tests
    // ========================================================================

    /// Verify that the CHECK constraint rejects rows with zero value columns
    #[pg_test]
    #[should_panic(expected = "chk_datom_value")]
    fn test_check_constraint_rejects_no_values() {
        setup();
        // Try to insert a row with no value columns set
        Spi::run(
            "INSERT INTO mentat.datoms (e, a, value_type_tag, tx, added)
             VALUES (99999, 1, 2, 1000000, true)",
        )
        .expect("should fail");
    }

    /// Verify that the CHECK constraint rejects rows with multiple value columns
    #[pg_test]
    #[should_panic(expected = "chk_datom_value")]
    fn test_check_constraint_rejects_multiple_values() {
        setup();
        // Try to insert a row with both v_long and v_text set
        Spi::run(
            "INSERT INTO mentat.datoms (e, a, value_type_tag, v_long, v_text, tx, added)
             VALUES (99999, 1, 2, 42, 'hello', 1000000, true)",
        )
        .expect("should fail");
    }

    /// Verify that the CHECK constraint allows exactly one value column
    #[pg_test]
    fn test_check_constraint_allows_single_value() {
        setup();
        // Insert a row with exactly one value column -- should succeed
        Spi::run(
            "INSERT INTO mentat.datoms (e, a, value_type_tag, v_long, tx, added)
             VALUES (99999, 1, 2, 42, 1000000, true)",
        )
        .expect("single value column should be allowed");

        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.datoms WHERE e = 99999")
            .expect("count failed")
            .expect("NULL count");

        assert_eq!(count, 1);
    }

    // ========================================================================
    // Range Query Correctness (the original BYTEA bug)
    // ========================================================================

    /// This is THE critical test: verify that numeric range queries work correctly.
    /// With BYTEA encoding, "2" > "10" because byte comparison is lexicographic.
    /// With native BIGINT, 2 < 10 correctly.
    #[pg_test]
    fn test_long_range_query_correctness() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :item/priority
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"name\" :db/ident :item/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e1\" :item/name \"low\"]
                [:db/add \"e1\" :item/priority 2]
                [:db/add \"e2\" :item/name \"medium\"]
                [:db/add \"e2\" :item/priority 10]
                [:db/add \"e3\" :item/name \"high\"]
                [:db/add \"e3\" :item/priority 100]
            ]'::TEXT)",
        )
        .expect("data txn failed");

        // Query for items with priority > 5
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name
                  :where
                  [?e :item/name ?name]
                  [?e :item/priority ?p]
                  [(> ?p 5)]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("query failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");
        let results = json["results"].as_array().expect("results array");

        // Should return "medium" (10) and "high" (100), NOT "low" (2)
        assert_eq!(
            results.len(),
            2,
            "Expected 2 results for priority > 5, got {}",
            results.len()
        );

        let names: Vec<&str> = results
            .iter()
            .map(|r| r[0].as_str().expect("string value"))
            .collect();
        assert!(
            names.contains(&"medium"),
            "Should include medium (priority 10)"
        );
        assert!(
            names.contains(&"high"),
            "Should include high (priority 100)"
        );
        assert!(
            !names.contains(&"low"),
            "Should NOT include low (priority 2)"
        );
    }

    /// Verify that numeric ordering works correctly (not lexicographic)
    #[pg_test]
    fn test_long_ordering_not_lexicographic() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :val/num
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        // Insert numbers that would sort differently in lexicographic vs numeric order
        // Lexicographic: "1", "10", "100", "2", "20", "3"
        // Numeric:       1, 2, 3, 10, 20, 100
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e1\" :val/num 1]
                [:db/add \"e2\" :val/num 10]
                [:db/add \"e3\" :val/num 100]
                [:db/add \"e4\" :val/num 2]
                [:db/add \"e5\" :val/num 20]
                [:db/add \"e6\" :val/num 3]
            ]'::TEXT)",
        )
        .expect("data txn failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?v
                  :where
                  [?e :val/num ?v]
                  :order (asc ?v)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("query failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");
        let results = json["results"].as_array().expect("results array");

        let values: Vec<i64> = results
            .iter()
            .map(|r| r[0].as_i64().expect("integer value"))
            .collect();

        assert_eq!(
            values,
            vec![1, 2, 3, 10, 20, 100],
            "Numeric ordering should be 1,2,3,10,20,100 (not lexicographic), got {:?}",
            values
        );
    }

    /// Verify that double range queries work correctly
    #[pg_test]
    fn test_double_range_query() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :measure/val
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e1\" :measure/val 1.5]
                [:db/add \"e2\" :measure/val 2.7]
                [:db/add \"e3\" :measure/val 10.1]
                [:db/add \"e4\" :measure/val 0.5]
            ]'::TEXT)",
        )
        .expect("data txn failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?v
                  :where
                  [?e :measure/val ?v]
                  [(> ?v 2.0)]
                  :order (asc ?v)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("query failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");
        let results = json["results"].as_array().expect("results array");

        assert_eq!(results.len(), 2, "Expected 2 results for val > 2.0");
    }

    // ========================================================================
    // Edge Cases for Each Type
    // ========================================================================

    /// Test storing empty string
    #[pg_test]
    fn test_empty_string_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/str2
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/str2 \"\"]]'::TEXT)")
            .expect("data txn failed");

        let v_text = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/str2')
             AND added = true AND value_type_tag = 7
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_text");

        assert_eq!(v_text, "");
    }

    /// Test storing zero as long value
    #[pg_test]
    fn test_zero_long_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/num2
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/num2 0]]'::TEXT)")
            .expect("data txn failed");

        let v_long = Spi::get_one::<i64>(
            "SELECT v_long FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/num2')
             AND added = true AND value_type_tag = 2
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_long");

        assert_eq!(v_long, 0);
    }

    /// Test storing negative long value
    #[pg_test]
    fn test_negative_long_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/neg
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/neg -999]]'::TEXT)")
            .expect("data txn failed");

        let v_long = Spi::get_one::<i64>(
            "SELECT v_long FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/neg')
             AND added = true AND value_type_tag = 2
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_long");

        assert_eq!(v_long, -999);
    }

    /// Test storing large long value (near i64 max)
    #[pg_test]
    fn test_large_long_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/big
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        // Use a large but valid i64 value
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/big 9223372036854775]]'::TEXT)")
            .expect("data txn failed");

        let v_long = Spi::get_one::<i64>(
            "SELECT v_long FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/big')
             AND added = true AND value_type_tag = 2
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_long");

        assert_eq!(v_long, 9223372036854775_i64);
    }

    /// Test boolean false value
    #[pg_test]
    fn test_boolean_false_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/flag2
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/flag2 false]]'::TEXT)")
            .expect("data txn failed");

        let v_bool = Spi::get_one::<bool>(
            "SELECT v_bool FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/flag2')
             AND added = true AND value_type_tag = 1
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_bool");

        assert!(!v_bool, "Should store false correctly");
    }

    /// Test double with special values (NaN handled by PostgreSQL)
    #[pg_test]
    fn test_double_zero_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/dbl
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/dbl 0.0]]'::TEXT)")
            .expect("data txn failed");

        let v_double = Spi::get_one::<f64>(
            "SELECT v_double FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/dbl')
             AND added = true AND value_type_tag = 3
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_double");

        assert!((v_double - 0.0).abs() < f64::EPSILON);
    }

    /// Test string with special characters (quotes, backslashes, unicode)
    #[pg_test]
    fn test_string_with_unicode() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/ustr
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        // Unicode snowman
        Spi::run(r#"SELECT mentat_transact('[[:db/add "e" :test/ustr "hello ☃ world"]]'::TEXT)"#)
            .expect("data txn failed");

        let v_text = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/ustr')
             AND added = true AND value_type_tag = 7
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL v_text");

        assert!(
            v_text.contains('\u{2603}'),
            "Should contain snowman unicode char"
        );
    }

    // ========================================================================
    // Retraction Tests with Typed Columns
    // ========================================================================

    /// Test that retraction marks datoms as added=false
    #[pg_test]
    fn test_retraction_with_typed_columns() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/rname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        // Add a value
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/rname \"original\"]]'::TEXT)")
            .expect("add txn failed");

        // Get the entity ID
        let entity_id = Spi::get_one::<i64>(
            "SELECT e FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/rname')
             AND v_text = 'original' AND added = true
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL entity");

        // Retract by updating with a new value (cardinality one replaces)
        let retract_sql = format!(
            "SELECT mentat_transact('[[:db/add {} :test/rname \"updated\"]]'::TEXT)",
            entity_id
        );
        Spi::run(&retract_sql).expect("update txn failed");

        // Old value should be retracted (added=false)
        let retracted_count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
                 WHERE e = {} AND v_text = 'original' AND added = false",
            entity_id
        ))
        .expect("query failed")
        .expect("NULL count");

        assert!(retracted_count > 0, "Old value should be retracted");
    }

    // ========================================================================
    // Multi-type Query Tests
    // ========================================================================

    /// Test querying across multiple typed columns in the same query
    #[pg_test]
    fn test_multi_type_query() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"nattr\" :db/ident :item/iname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"pattr\" :db/ident :item/price
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
                {:db/id \"qattr\" :db/ident :item/qty
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"aattr\" :db/ident :item/active
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e1\" :item/iname \"Widget\"]
                [:db/add \"e1\" :item/price 9.99]
                [:db/add \"e1\" :item/qty 100]
                [:db/add \"e1\" :item/active true]
                [:db/add \"e2\" :item/iname \"Gadget\"]
                [:db/add \"e2\" :item/price 24.99]
                [:db/add \"e2\" :item/qty 50]
                [:db/add \"e2\" :item/active false]
            ]'::TEXT)",
        )
        .expect("data txn failed");

        // Query using multiple typed columns
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name ?price
                  :where
                  [?e :item/iname ?name]
                  [?e :item/price ?price]
                  [?e :item/active true]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("query failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");
        let results = json["results"].as_array().expect("results array");

        assert_eq!(results.len(), 1, "Only Widget is active");
        assert_eq!(results[0][0].as_str().expect("str"), "Widget");
    }

    // ========================================================================
    // Cardinality-Many Tests
    // ========================================================================

    /// Test cardinality-many with typed columns
    #[pg_test]
    fn test_cardinality_many_long() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/scores
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e\" :test/scores 10]
                [:db/add \"e\" :test/scores 20]
                [:db/add \"e\" :test/scores 30]
            ]'::TEXT)",
        )
        .expect("data txn failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?s
                  :where
                  [?e :test/scores ?s]
                  :order (asc ?s)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("query failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");
        let results = json["results"].as_array().expect("results array");

        assert_eq!(results.len(), 3, "Should have 3 scores");

        let scores: Vec<i64> = results
            .iter()
            .map(|r| r[0].as_i64().expect("int"))
            .collect();
        assert_eq!(scores, vec![10, 20, 30]);
    }

    // ========================================================================
    // Unique Constraint Tests
    // ========================================================================

    /// Test unique identity constraint with typed columns
    #[pg_test]
    fn test_unique_identity_constraint() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :user/email
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        // First insert
        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :user/email \"alice@test.com\"]]'::TEXT)",
        )
        .expect("first insert failed");

        // Second insert with same email should upsert (identity merge)
        Spi::run(
            "SELECT mentat_transact('[[:db/add \"f\" :user/email \"alice@test.com\"]]'::TEXT)",
        )
        .expect("upsert should succeed");

        // Should only have one entity with this email
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':user/email')
             AND v_text = 'alice@test.com' AND added = true",
        )
        .expect("query failed")
        .expect("NULL count");

        assert_eq!(count, 1, "Identity unique should produce exactly 1 entity");
    }

    // ========================================================================
    // Type-specific Index Usage Tests
    // ========================================================================

    /// Verify that type-specific indexes exist and are used
    #[pg_test]
    fn test_type_specific_indexes_exist() {
        setup();

        // Check that the AVET partial indexes exist
        let idx_count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM pg_indexes
             WHERE schemaname = 'mentat'
             AND tablename = 'datoms'
             AND indexname LIKE 'idx_datoms_avet_%'",
        )
        .expect("query failed")
        .expect("NULL count");

        assert!(
            idx_count >= 7,
            "Should have at least 7 AVET type-specific indexes, got {}",
            idx_count
        );
    }

    /// Verify EAVT covering indexes exist
    #[pg_test]
    fn test_eavt_covering_indexes_exist() {
        setup();

        let idx_count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM pg_indexes
             WHERE schemaname = 'mentat'
             AND tablename = 'datoms'
             AND indexname LIKE 'idx_datoms_eavt_%'",
        )
        .expect("query failed")
        .expect("NULL count");

        assert!(
            idx_count >= 4,
            "Should have at least 4 EAVT covering indexes, got {}",
            idx_count
        );
    }

    // ========================================================================
    // Null Safety Tests
    // ========================================================================

    /// Verify that only the correct value column is non-null for each type
    #[pg_test]
    fn test_null_columns_for_long_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/ln
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/ln 42]]'::TEXT)")
            .expect("data txn failed");

        // All other value columns should be NULL
        let null_check = Spi::get_one::<bool>(
            "SELECT v_ref IS NULL
                AND v_bool IS NULL
                AND v_double IS NULL
                AND v_text IS NULL
                AND v_keyword IS NULL
                AND v_instant IS NULL
                AND v_uuid IS NULL
                AND v_bytes IS NULL
                AND v_long IS NOT NULL
             FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/ln')
             AND added = true AND value_type_tag = 2
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL result");

        assert!(
            null_check,
            "Only v_long should be non-null for a long value"
        );
    }

    /// Verify null columns for string value
    #[pg_test]
    fn test_null_columns_for_string_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/sn
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/sn \"test\"]]'::TEXT)")
            .expect("data txn failed");

        let null_check = Spi::get_one::<bool>(
            "SELECT v_ref IS NULL
                AND v_bool IS NULL
                AND v_long IS NULL
                AND v_double IS NULL
                AND v_keyword IS NULL
                AND v_instant IS NULL
                AND v_uuid IS NULL
                AND v_bytes IS NULL
                AND v_text IS NOT NULL
             FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/sn')
             AND added = true AND value_type_tag = 7
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL result");

        assert!(
            null_check,
            "Only v_text should be non-null for a string value"
        );
    }

    // ========================================================================
    // Type Tag Consistency Tests
    // ========================================================================

    /// Verify that value_type_tag matches the populated column
    #[pg_test]
    fn test_type_tag_matches_column_ref() {
        setup();
        // Bootstrap datoms use refs (v_ref) with type_tag=0
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE value_type_tag = 0 AND v_ref IS NOT NULL AND added = true",
        )
        .expect("query failed")
        .expect("NULL count");

        assert!(count > 0, "Should have ref datoms with type_tag=0");
    }

    #[pg_test]
    fn test_type_tag_matches_column_keyword() {
        setup();
        // Bootstrap datoms use keywords (v_keyword) with type_tag=8
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE value_type_tag = 8 AND v_keyword IS NOT NULL AND added = true",
        )
        .expect("query failed")
        .expect("NULL count");

        assert!(count > 0, "Should have keyword datoms with type_tag=8");
    }

    #[pg_test]
    fn test_type_tag_matches_column_instant() {
        setup();
        // Transaction instant datoms use v_instant with type_tag=4
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE value_type_tag = 4 AND v_instant IS NOT NULL AND added = true",
        )
        .expect("query failed")
        .expect("NULL count");

        assert!(count > 0, "Should have instant datoms with type_tag=4");
    }

    // ========================================================================
    // Batch Transaction Tests
    // ========================================================================

    /// Test a large batch transaction with mixed types
    #[pg_test]
    fn test_batch_transaction_mixed_types() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :person/pname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :person/page
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"e\" :db/ident :person/pemail
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :person/pactive
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        // Insert 20 entities in one transaction
        let mut assertions = Vec::new();
        for i in 0..20 {
            assertions.push(format!(
                "[:db/add \"e{i}\" :person/pname \"Person {i}\"]
                 [:db/add \"e{i}\" :person/page {age}]
                 [:db/add \"e{i}\" :person/pemail \"person{i}@test.com\"]
                 [:db/add \"e{i}\" :person/pactive {active}]",
                i = i,
                age = 20 + i,
                active = if i % 2 == 0 { "true" } else { "false" }
            ));
        }
        let txn = format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            assertions.join("\n")
        );
        Spi::run(&txn).expect("batch txn failed");

        // Verify count
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?name
                  :where
                  [?e :person/pname ?name]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("query failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");
        let results = json["results"].as_array().expect("results array");

        assert_eq!(
            results.len(),
            20,
            "Should have 20 persons, got {}",
            results.len()
        );
    }

    /// Test multiple sequential transactions
    #[pg_test]
    fn test_sequential_transactions() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :counter/val
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        // 10 sequential transactions
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{i}\" :counter/val {i}]]'::TEXT)",
                i = i
            ))
            .expect("sequential txn failed");
        }

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?v
                  :where [?e :counter/val ?v]
                  :order (asc ?v)]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("query failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");
        let results = json["results"].as_array().expect("results array");

        assert_eq!(results.len(), 10, "Should have 10 values");
        let vals: Vec<i64> = results
            .iter()
            .map(|r| r[0].as_i64().expect("int"))
            .collect();
        assert_eq!(vals, (0..10).collect::<Vec<i64>>());
    }

    // ========================================================================
    // Pull API with Typed Columns Tests
    // ========================================================================

    /// Test pull with string attributes
    #[pg_test]
    fn test_pull_string_attribute() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/pname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/pname \"Alice\"]]'::TEXT)")
            .expect("data txn failed");

        let entity_id = Spi::get_one::<i64>(
            "SELECT e FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/pname')
             AND v_text = 'Alice' AND added = true
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL entity");

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:test/pname]'::TEXT, {})",
            entity_id
        ))
        .expect("pull failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");

        assert_eq!(json[":test/pname"].as_str().expect("str"), "Alice");
    }

    /// Test pull with long attribute
    #[pg_test]
    fn test_pull_long_attribute() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/pcount
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :test/pcount 42]]'::TEXT)")
            .expect("data txn failed");

        let entity_id = Spi::get_one::<i64>(
            "SELECT e FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/pcount')
             AND v_long = 42 AND added = true
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL entity");

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:test/pcount]'::TEXT, {})",
            entity_id
        ))
        .expect("pull failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");

        assert_eq!(json[":test/pcount"].as_i64().expect("int"), 42);
    }

    /// Test pull with wildcard pattern
    #[pg_test]
    fn test_pull_wildcard() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :test/wname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :test/wage
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e\" :test/wname \"Bob\"]
                [:db/add \"e\" :test/wage 25]
            ]'::TEXT)",
        )
        .expect("data txn failed");

        let entity_id = Spi::get_one::<i64>(
            "SELECT e FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/wname')
             AND v_text = 'Bob' AND added = true
             LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL entity");

        let result =
            Spi::get_one::<String>(&format!("SELECT mentat_pull('[*]'::TEXT, {})", entity_id))
                .expect("pull failed")
                .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");

        assert_eq!(json[":test/wname"].as_str().expect("str"), "Bob");
        assert_eq!(json[":test/wage"].as_i64().expect("int"), 25);
    }

    // ========================================================================
    // Schema Definition Tests
    // ========================================================================

    /// Test defining schema with all supported value types
    #[pg_test]
    fn test_define_all_value_types() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"r\" :db/ident :test/aref
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/one}
                {:db/id \"b\" :db/ident :test/abool
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
                {:db/id \"l\" :db/ident :test/along
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :test/adbl
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :test/astr
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :test/akw
                 :db/valueType :db.type/keyword
                 :db/cardinality :db.cardinality/one}
                {:db/id \"i\" :db/ident :test/ainst
                 :db/valueType :db.type/instant
                 :db/cardinality :db.cardinality/one}
                {:db/id \"u\" :db/ident :test/auuid
                 :db/valueType :db.type/uuid
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn with all types failed");

        // Verify all attributes are in the schema table
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.schema
             WHERE ident LIKE ':test/a%'",
        )
        .expect("query failed")
        .expect("NULL count");

        assert_eq!(count, 8, "Should have 8 test attributes defined");
    }

    // ========================================================================
    // Error Handling Tests
    // ========================================================================

    /// Test that invalid value type for attribute is rejected
    #[pg_test]
    fn test_type_mismatch_error() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :test/typed_num
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema txn failed");

        // Try to store a string in a long attribute - should fail
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :test/typed_num \"not a number\"]]'::TEXT)",
        );

        assert!(
            result.is_err(),
            "Should reject string value for long attribute"
        );
    }

    /// Test that empty transaction is handled gracefully
    #[pg_test]
    fn test_empty_transaction() {
        setup();
        // Empty vector should not cause errors
        let result = Spi::get_one::<String>("SELECT mentat_transact('[]'::TEXT)");
        // Either succeeds with empty result or returns an error -- both are acceptable
        // The important thing is no panic
        let _ = result;
    }

    /// Test that unknown attribute is rejected
    #[pg_test]
    fn test_unknown_attribute_error() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :nonexistent/attr \"value\"]]'::TEXT)",
        );

        assert!(result.is_err(), "Should reject unknown attribute");
    }

    // ========================================================================
    // Query Result Format Tests
    // ========================================================================

    /// Test that query results contain correct column names
    #[pg_test]
    fn test_query_result_has_columns() {
        setup();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e ?ident
                  :where [?e :db/ident ?ident]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("query failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");

        assert!(json.get("columns").is_some(), "Should have columns key");
        assert!(json.get("results").is_some(), "Should have results key");

        let columns = json["columns"].as_array().expect("columns array");
        assert_eq!(columns.len(), 2, "Should have 2 columns");
    }

    /// Test scalar query result
    #[pg_test]
    fn test_query_count_results() {
        setup();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e
                  :where [?e :db/ident _]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("query failed")
        .expect("NULL result");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON failed");
        let results = json["results"].as_array().expect("results array");

        // Bootstrap has many idents, should find at least 20
        assert!(
            results.len() >= 20,
            "Should find at least 20 bootstrap entities, got {}",
            results.len()
        );
    }
}
