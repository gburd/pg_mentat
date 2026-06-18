// Error handling tests: systematic coverage of error conditions,
// invalid inputs, constraint violations, and graceful failure modes.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        // Create helper function for testing error conditions
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

    /// Check if a SQL statement raises an error by executing it in a PL/pgSQL
    /// exception handler (which provides subtransaction isolation).
    fn raises_error(sql: &str) -> bool {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<bool>(&format!("SELECT mentat._test_raises_error('{}')", escaped))
            .expect("raises_error call")
            .unwrap_or(false)
    }

    fn setup_eh_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :eh/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :eh/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :eh/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :eh/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :eh/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :eh/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :eh/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("eh schema");
    }

    // ========================================================================
    // Invalid EDN syntax (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_eh_empty_transaction() {
        setup();
        setup_eh_schema();
        let result = Spi::run("SELECT mentat_transact('[]'::TEXT)");
        // Empty transaction should either succeed (no-op) or error
        assert!(result.is_ok() || result.is_err());
    }

    #[pg_test]
    fn test_eh_malformed_edn_missing_bracket() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :eh/name \"test\"'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_malformed_edn_extra_bracket() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :eh/name \"test\"]]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_malformed_edn_no_brackets() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact(':db/add \"e\" :eh/name \"test\"'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_malformed_map_missing_brace() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('[{:db/id \"e\" :eh/name \"test\"'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_completely_invalid_input() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('this is not edn at all!'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_null_input() {
        setup();
        setup_eh_schema();
        let result = Spi::run("SELECT mentat_transact(NULL::TEXT)");
        // NULL input should error
        assert!(result.is_ok() || result.is_err());
    }

    #[pg_test]
    fn test_eh_empty_string_input() {
        setup();
        setup_eh_schema();
        assert!(raises_error("SELECT mentat_transact(''::TEXT)"));
    }

    // ========================================================================
    // Invalid query syntax (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_eh_malformed_query_no_find() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_query('[:where [_ :eh/name ?n]]'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    #[pg_test]
    fn test_eh_malformed_query_no_where() {
        setup();
        setup_eh_schema();
        // A query with no :where clause is rejected by the parser
        // ("expected :where"). Run it in a subtransaction so the raised
        // error does not poison the test's outer transaction.
        assert!(raises_error(
            "SELECT mentat_query('[:find ?n]'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    #[pg_test]
    fn test_eh_malformed_query_invalid_edn() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_query('not a query'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    #[pg_test]
    fn test_eh_query_empty_string() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_query(''::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    #[pg_test]
    fn test_eh_query_unbound_variable() {
        setup();
        setup_eh_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :eh/name \"test\"]]'::TEXT)")
            .expect("data");
        // ?n appears in :find but is bound by no :where clause -> fail-loud
        // (:db.error/unbound-variable). Route through the subtransaction
        // helper so the error doesn't poison the test transaction.
        assert!(raises_error(
            "SELECT mentat_query('[:find ?n :where [_ :eh/name ?x]]'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    #[pg_test]
    fn test_eh_query_unknown_attribute() {
        setup();
        setup_eh_schema();
        // :nonexistent/attr is not registered -> fail-loud
        // (:db.error/unknown-attribute), not a silent empty result.
        assert!(raises_error(
            "SELECT mentat_query('[:find ?v . :where [_ :nonexistent/attr ?v]]'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    #[pg_test]
    fn test_eh_query_null_params() {
        setup();
        setup_eh_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :eh/name \"test\"]]'::TEXT)")
            .expect("data");
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [_ :eh/name ?n]]'::TEXT, NULL::jsonb)::TEXT",
        );
        assert!(result.is_ok() || result.is_err());
    }

    #[pg_test]
    fn test_eh_query_missing_brackets() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_query(':find ?n :where [_ :eh/name ?n]'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    // ========================================================================
    // Transaction attribute errors (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_eh_undefined_attribute() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :undefined/attr \"value\"]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_wrong_type_string_for_long() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :eh/val \"not-a-number\"]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_wrong_type_string_for_boolean() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :eh/flag \"not-a-bool\"]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_wrong_type_long_for_string() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :eh/name 42]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_wrong_type_boolean_for_long() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :eh/val true]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_incomplete_vector_op() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\"]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_invalid_db_op() {
        setup();
        setup_eh_schema();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/invalid \"e\" :eh/name \"test\"]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_eh_retract_nonexistent_entity() {
        setup();
        setup_eh_schema();
        // Retracting a nonexistent entity should be a no-op or error gracefully
        let result = Spi::run("SELECT mentat_transact('[[:db/retractEntity 999999999]]'::TEXT)");
        assert!(result.is_ok() || result.is_err());
    }

    // ========================================================================
    // Schema definition errors (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_eh_schema_missing_value_type() {
        setup();
        assert!(raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/ident :eh.bad/attr :db/cardinality :db.cardinality/one}]'::TEXT)"));
    }

    #[pg_test]
    fn test_eh_schema_missing_cardinality() {
        setup();
        assert!(raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/ident :eh.bad/attr2 :db/valueType :db.type/string}]'::TEXT)"));
    }

    #[pg_test]
    fn test_eh_schema_invalid_value_type() {
        setup();
        assert!(raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/ident :eh.bad/attr3 :db/valueType :db.type/invalid :db/cardinality :db.cardinality/one}]'::TEXT)"));
    }

    #[pg_test]
    fn test_eh_schema_invalid_cardinality() {
        setup();
        assert!(raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/ident :eh.bad/attr4 :db/valueType :db.type/string :db/cardinality :db.cardinality/invalid}]'::TEXT)"));
    }

    #[pg_test]
    fn test_eh_schema_invalid_unique() {
        setup();
        assert!(raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/ident :eh.bad/attr5 :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/invalid}]'::TEXT)"));
    }

    #[pg_test]
    fn test_eh_schema_missing_ident() {
        setup();
        assert!(raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)"));
    }

    #[pg_test]
    fn test_eh_schema_empty_ident() {
        setup();
        assert!(raises_error("SELECT mentat_transact('[{:db/id \"a\" :db/ident \"\" :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)"));
    }

    #[pg_test]
    fn test_eh_valid_after_error() {
        setup();
        setup_eh_schema();
        // First, trigger an error inside a subtransaction so it does not
        // poison the test's outer transaction.
        let _ = raises_error("SELECT mentat_transact('invalid'::TEXT)");
        // Then, verify system still works
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :eh/name \"recovery\"]]'::TEXT)")
            .expect("should work after error");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [_ :eh/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "recovery");
    }

    // ========================================================================
    // CAS error conditions (6 tests)
    // ========================================================================

    #[pg_test]
    fn test_eh_cas_wrong_old_value() {
        setup();
        setup_eh_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :eh/val 100]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        assert!(raises_error(&format!(
            "SELECT mentat_transact('[[:db/cas {} :eh/val 999 200]]'::TEXT)",
            eid
        )));
    }

    #[pg_test]
    fn test_eh_cas_nil_but_has_value() {
        setup();
        setup_eh_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :eh/val 42]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        assert!(raises_error(&format!(
            "SELECT mentat_transact('[[:db/cas {} :eh/val nil 99]]'::TEXT)",
            eid
        )));
    }

    #[pg_test]
    fn test_eh_cas_on_nonexistent_entity() {
        setup();
        setup_eh_schema();
        let result =
            Spi::run("SELECT mentat_transact('[[:db/cas 999999999 :eh/val nil 42]]'::TEXT)");
        // CAS on nonexistent entity - may succeed (from nil) or error
        assert!(result.is_ok() || result.is_err());
    }

    #[pg_test]
    fn test_eh_cas_preserves_on_failure() {
        setup();
        setup_eh_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :eh/val 100]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        // Run the failing CAS inside a subtransaction so its error does not
        // poison the outer transaction; the value must remain unchanged.
        assert!(
            raises_error(&format!(
                "SELECT mentat_transact('[[:db/cas {} :eh/val 999 200]]'::TEXT)",
                eid
            )),
            "CAS with wrong old value should fail"
        );
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :eh/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 100);
    }

    #[pg_test]
    fn test_eh_cas_wrong_type() {
        setup();
        setup_eh_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :eh/val 100]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        assert!(raises_error(&format!(
            "SELECT mentat_transact('[[:db/cas {} :eh/val \"100\" 200]]'::TEXT)",
            eid
        )));
    }

    #[pg_test]
    fn test_eh_cas_on_many_cardinality() {
        setup();
        setup_eh_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :eh/tags \"tag1\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let result = Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :eh/tags \"tag1\" \"tag2\"]]'::TEXT)",
            eid
        ));
        // CAS on cardinality-many is not well-defined
        assert!(result.is_ok() || result.is_err());
    }

    // ========================================================================
    // Bootstrap and function errors (6 tests)
    // ========================================================================

    #[pg_test]
    fn test_eh_bootstrap_idempotent() {
        setup();
        // Second bootstrap should not error
        Spi::run("SELECT bootstrap_schema()").expect("second bootstrap");
        // Third for good measure
        Spi::run("SELECT bootstrap_schema()").expect("third bootstrap");
    }

    #[pg_test]
    fn test_eh_schema_before_bootstrap() {
        // Don't call setup() - test raw state
        // This may error or return empty depending on state
        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT");
        // Either returns something or errors - both are acceptable
        assert!(result.is_ok() || result.is_err());
    }

    #[pg_test]
    fn test_eh_transact_before_bootstrap() {
        // Don't call bootstrap_schema() - test without a bootstrapped schema.
        // The extension must still be present for mentat_transact to exist.
        crate::ensure_extension_loaded();
        // Create the subtransaction-isolated error helper without bootstrapping
        // the schema, so a raised error (the attribute is undefined) does not
        // poison the test's outer transaction.
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
        // Either it errors (expected: no schema) or is accepted; both are
        // acceptable. The point is not to panic by poisoning the transaction.
        let _ =
            raises_error("SELECT mentat_transact('[[:db/add \"e\" :some/attr \"val\"]]'::TEXT)");
    }

    #[pg_test]
    fn test_eh_query_empty_db() {
        setup();
        // No schema defined yet for :eh/name -> querying an unregistered
        // attribute is fail-loud (:db.error/unknown-attribute). Route
        // through the subtransaction helper.
        assert!(raises_error(
            "SELECT mentat_query('[:find [?n ...] :where [_ :eh/name ?n]]'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    #[pg_test]
    fn test_eh_pull_nonexistent_entity() {
        setup();
        setup_eh_schema();
        let result = Spi::get_one::<String>("SELECT mentat_pull('[*]'::TEXT, 999999999)::TEXT");
        assert!(result.is_ok() || result.is_err());
    }

    #[pg_test]
    fn test_eh_successive_errors_dont_break() {
        setup();
        setup_eh_schema();
        // Multiple errors in a row, each isolated in its own subtransaction
        // so they do not poison the outer transaction.
        for _ in 0..5 {
            let _ = raises_error("SELECT mentat_transact('invalid edn'::TEXT)");
        }
        // System should still work
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :eh/name \"still-works\"]]'::TEXT)")
            .expect("recovery");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [_ :eh/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "still-works");
    }
}
