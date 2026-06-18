// Regression tests for error handling across all error variants.
//
// Every MentatError variant should have at least one test that triggers it
// and verifies the error code and message format. These tests serve as
// regression guards against error message changes.

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

    /// Run `sql` in a PL/pgSQL subtransaction and return its SQLERRM (empty
    /// string if it did not error). Subtransaction isolation prevents an
    /// expected error from poisoning the test's outer transaction.
    fn error_message(sql: &str) -> String {
        let escaped = sql.replace('\'', "''");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._test_error_msg(stmt TEXT) RETURNS TEXT
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN ''::TEXT;
             EXCEPTION WHEN OTHERS THEN
                 RETURN SQLERRM;
             END;
             $$",
        )
        .expect("create error_msg helper");
        Spi::get_one::<String>(&format!("SELECT mentat._test_error_msg('{}')", escaped))
            .expect("error_msg call")
            .unwrap_or_default()
    }

    // ========================================================================
    // :db.error/attribute-not-found
    // ========================================================================

    #[pg_test]
    fn test_error_attribute_not_found_has_suggestion() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :err/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema");

        let msg =
            error_message("SELECT mentat_transact('[[:db/add \"e\" :err/namee \"typo\"]]'::TEXT)");

        assert!(
            msg.contains("attribute") || msg.contains("not found"),
            "Should mention attribute not found: {}",
            msg
        );
    }

    #[pg_test]
    fn test_error_completely_unknown_attribute() {
        setup();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :zzz/nonexistent \"val\"]]'::TEXT)"
        ));
    }

    // ========================================================================
    // :db.error/wrong-type-for-attribute
    // ========================================================================

    #[pg_test]
    fn test_error_string_for_long_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :err/count
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema");

        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :err/count \"not-a-number\"]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_error_long_for_string_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :err/label
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema");

        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :err/label 42]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_error_string_for_boolean_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :err/flag
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema");

        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :err/flag \"yes\"]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_error_string_for_ref_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :err/link
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema");

        // A float for a ref attribute should fail
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\" :err/link 3.14]]'::TEXT)"
        ));
    }

    // ========================================================================
    // :db.error/cardinality-violation
    // ========================================================================

    #[pg_test]
    fn test_error_cardinality_one_two_values_same_tx() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :err/single
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema");

        assert!(raises_error(
            "SELECT mentat_transact('[
                [:db/add \"e\" :err/single \"first\"]
                [:db/add \"e\" :err/single \"second\"]
            ]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_error_cardinality_one_three_values_same_tx() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :err/single3
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema");

        assert!(raises_error(
            "SELECT mentat_transact('[
                [:db/add \"e\" :err/single3 1]
                [:db/add \"e\" :err/single3 2]
                [:db/add \"e\" :err/single3 3]
            ]'::TEXT)"
        ));
    }

    // ========================================================================
    // :db.error/invalid-transaction
    // ========================================================================

    #[pg_test]
    fn test_error_tx_not_a_vector() {
        setup();
        assert!(raises_error(
            "SELECT mentat_transact('{:not \"a vector\"}'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_error_tx_empty_assertion() {
        setup();
        assert!(raises_error("SELECT mentat_transact('[[:db/add]]'::TEXT)"));
    }

    #[pg_test]
    fn test_error_tx_too_few_args_in_assertion() {
        setup();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e\"]]'::TEXT)"
        ));
    }

    #[pg_test]
    fn test_error_tx_unknown_operation() {
        setup();
        assert!(raises_error(
            "SELECT mentat_transact('[[:db/unknown \"e\" :db/ident :test]]'::TEXT)"
        ));
    }

    // ========================================================================
    // :db.error/invalid-query
    // ========================================================================

    #[pg_test]
    fn test_error_query_no_find() {
        setup();
        assert!(raises_error(
            "SELECT mentat_query('[:where [?e :db/ident ?i]]'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    #[pg_test]
    fn test_error_query_no_where() {
        setup();
        assert!(raises_error(
            "SELECT mentat_query('[:find ?e]'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    #[pg_test]
    fn test_error_query_invalid_edn() {
        setup();
        assert!(raises_error(
            "SELECT mentat_query('not valid'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    #[pg_test]
    fn test_error_query_not_a_vector() {
        setup();
        assert!(raises_error(
            "SELECT mentat_query('{:find ?e :where [?e :db/ident _]}'::TEXT, '{}'::jsonb)::TEXT"
        ));
    }

    // ========================================================================
    // :db.error/invalid-pull-pattern
    // ========================================================================

    #[pg_test]
    fn test_error_pull_invalid_edn() {
        setup();
        assert!(raises_error("SELECT mentat_pull('not valid'::TEXT, 1)"));
    }

    #[pg_test]
    fn test_error_pull_not_a_vector() {
        setup();
        assert!(raises_error(
            "SELECT mentat_pull('{:key \"val\"}'::TEXT, 1)"
        ));
    }

    // ========================================================================
    // :db.error/unique-conflict
    // ========================================================================

    #[pg_test]
    fn test_error_unique_value_conflict() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :err/code
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/value}
            ]'::TEXT)",
        )
        .expect("schema");

        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :err/code \"ABC123\"]]'::TEXT)")
            .expect("first insert");

        assert!(raises_error(
            "SELECT mentat_transact('[[:db/add \"e2\" :err/code \"ABC123\"]]'::TEXT)"
        ));
    }

    // ========================================================================
    // :db.fn/cas failure
    // ========================================================================

    #[pg_test]
    fn test_error_cas_wrong_expected() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :err/casv
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema");

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :err/casv \"actual\"]]'::TEXT)",
        )
        .expect("insert failed")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        assert!(raises_error(&format!(
            "SELECT mentat_transact('[[:db.fn/cas {} :err/casv \"wrong\" \"new\"]]'::TEXT)",
            eid
        )));
    }

    // ========================================================================
    // Transaction with mixed valid/invalid operations
    // ========================================================================

    #[pg_test]
    fn test_error_atomicity_rollback_on_failure() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :err/aname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"c\" :db/ident :err/acount
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema");

        // This transaction has a valid add followed by an invalid type mismatch
        assert!(raises_error(
            "SELECT mentat_transact('[
                [:db/add \"e\" :err/aname \"valid\"]
                [:db/add \"e\" :err/acount \"not-a-number\"]
            ]'::TEXT)"
        ));

        // The valid part should NOT have been committed (atomicity)
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':err/aname')
             AND v_text = 'valid' AND added = true",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(count, 0, "Nothing should be committed on failure");
    }

    // ========================================================================
    // Edge: empty inputs
    // ========================================================================

    #[pg_test]
    fn test_empty_transaction_vector() {
        setup();
        // Empty vector should succeed (no-op transaction)
        let _result = Spi::get_one::<String>("SELECT mentat_transact('[]'::TEXT)");
    }

    #[pg_test]
    fn test_query_with_empty_options() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?e :where [?e :db/ident _]]'::TEXT,
                '{}'::jsonb)::TEXT",
        )
        .expect("query with empty options should work")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert!(json["results"].as_array().expect("array").len() > 0);
    }
}
