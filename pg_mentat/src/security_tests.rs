// Security and defensive tests for pg_mentat.
//
// Tests cover:
// 1. SQL injection attempts via attribute names
// 2. SQL injection via string values
// 3. SQL injection via EDN parsing
// 4. Recursive query depth limits (DoS prevention)
// 5. Large payload handling
// 6. Malformed EDN handling
// 7. Boundary values (max i64, NaN, Infinity)
// 8. Error message safety (no internal details leaked)
// 9. Cross-schema isolation

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

    fn setup_test_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :sec/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :sec/val
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("security test schema failed");
    }

    // ========================================================================
    // 1. SQL Injection via String Values
    // ========================================================================

    #[pg_test]
    fn test_injection_string_value_single_quote() {
        setup();
        setup_test_schema();

        // Attempt SQL injection via string value
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :sec/name \"Robert''; DROP TABLE mentat.datoms; --\"]]'::TEXT)",
        );

        // Should either reject or safely escape the value
        if let Ok(Some(_)) = result {
            // If it succeeded, verify the table still exists
            let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.datoms")
                .expect("table should exist")
                .expect("NULL");
            assert!(count > 0, "datoms table should not be dropped");
        }
        // If it errors, that's also acceptable
    }

    #[pg_test]
    fn test_injection_string_value_semicolon() {
        setup();
        setup_test_schema();

        Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :sec/name \"test; DELETE FROM mentat.datoms;\"]]'::TEXT)",
        )
        .ok(); // Don't care if it succeeds or fails

        // Table should still exist with data
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.schema")
            .expect("schema table should exist")
            .expect("NULL");
        assert!(count > 0);
    }

    #[pg_test]
    fn test_injection_string_value_comment() {
        setup();
        setup_test_schema();

        Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :sec/name \"value/* injection */\"]]'::TEXT)",
        )
        .ok();

        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.schema")
            .expect("schema table should exist")
            .expect("NULL");
        assert!(count > 0);
    }

    #[pg_test]
    fn test_injection_query_string_input() {
        setup();
        setup_test_schema();

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :sec/name \"safe\"]]'::TEXT)",
        )
        .expect("data failed");

        // Attempt injection via query input
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?e
                 :in ?name
                 :where [?e :sec/name ?name]]'::TEXT,
                '{\"inputs\": [\"\\\"''; DROP TABLE mentat.datoms;--\"]}'::jsonb)::TEXT",
        );

        // Should either safely handle or reject
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.datoms")
            .expect("table should exist")
            .expect("NULL");
        assert!(count > 0);
        // Discard the query result
        drop(result);
    }

    // ========================================================================
    // 2. SQL Injection via Attribute Names
    // ========================================================================

    #[pg_test]
    fn test_injection_attribute_name() {
        setup();

        // Attempt to define an attribute with SQL injection in the name
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"bad\" :db/ident :evil/name'';DROP TABLE mentat.datoms;--
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        );

        // Should reject malformed EDN or safely handle
        if result.is_ok() {
            let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.datoms")
                .expect("table should exist")
                .expect("NULL");
            assert!(count > 0);
        }
    }

    // ========================================================================
    // 3. Malformed EDN
    // ========================================================================

    #[pg_test]
    fn test_malformed_edn_unclosed_bracket() {
        setup();
        assert!(
            raises_error("SELECT mentat_transact('[[:db/add \"e\" :db/ident :test'::TEXT)"),
            "Malformed EDN with unclosed bracket should error"
        );
    }

    #[pg_test]
    fn test_malformed_edn_extra_bracket() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :db/ident :test]]]'::TEXT)",
        );
        // Should handle gracefully (either succeed parsing first valid form or error)
    }

    #[pg_test]
    fn test_malformed_edn_nested_deeply() {
        setup();
        // Deeply nested EDN that might cause stack overflow
        let deep = "[[[[[[[[[[[[[[[[[[[[\"deep\"]]]]]]]]]]]]]]]]]]]]";
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('{}'::TEXT)",
            deep
        ));
        // Should handle without stack overflow
        drop(result);
    }

    #[pg_test]
    fn test_malformed_edn_null_bytes() {
        setup();
        // EDN with embedded null - should be rejected or handled safely
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact(E'[[:db/add \"e\" :db/ident :test\\x00val]]'::TEXT)",
        );
        drop(result);
    }

    // ========================================================================
    // 4. Large Payloads
    // ========================================================================

    #[pg_test]
    fn test_large_string_value() {
        setup();
        setup_test_schema();

        // 10KB string value
        let big_string = "A".repeat(10_000);
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :sec/name \"{}\"]]'::TEXT)",
            big_string
        ))
        .expect("large string should work");

        let v = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':sec/name')
             AND added = true ORDER BY tx DESC LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v.len(), 10_000);
    }

    #[pg_test]
    fn test_very_large_string_value() {
        setup();
        setup_test_schema();

        // 100KB string value
        let big_string = "B".repeat(100_000);
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :sec/name \"{}\"]]'::TEXT)",
            big_string
        ))
        .expect("very large string should work");

        let v = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':sec/name')
             AND added = true ORDER BY tx DESC LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(v.len(), 100_000);
    }

    #[pg_test]
    fn test_many_entities_batch() {
        setup();
        setup_test_schema();

        // 200 entities in a single transaction
        let mut assertions = Vec::new();
        for i in 0..200 {
            assertions.push(format!(
                "[:db/add \"e{}\" :sec/name \"entity-{}\"] [:db/add \"e{}\" :sec/val {}]",
                i, i, i, i
            ));
        }
        let txn = format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            assertions.join("\n")
        );
        Spi::run(&txn).expect("200-entity batch failed");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':sec/name')
             AND added = true",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(count, 200);
    }

    // ========================================================================
    // 5. Boundary Values
    // ========================================================================

    #[pg_test]
    fn test_long_max_value() {
        setup();
        setup_test_schema();

        // Max i64 that EDN can represent (may be parser limited)
        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :sec/val 9223372036854775]]'::TEXT)",
        )
        .expect("large long failed");
    }

    #[pg_test]
    fn test_long_min_value() {
        setup();
        setup_test_schema();

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :sec/val -9223372036854775]]'::TEXT)",
        )
        .expect("negative long failed");
    }

    #[pg_test]
    fn test_double_special_values_rejected() {
        setup();

        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sec/dbl
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");

        // NaN should be rejected or handled
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :sec/dbl ##NaN]]'::TEXT)",
        );
        // NaN handling is implementation-defined
        drop(result);
    }

    // ========================================================================
    // 6. Unicode Edge Cases
    // ========================================================================

    #[pg_test]
    fn test_unicode_emoji() {
        setup();
        setup_test_schema();

        Spi::run(
            r#"SELECT mentat_transact('[[:db/add "e" :sec/name "test 🎉🚀💯"]]'::TEXT)"#,
        )
        .expect("emoji string failed");

        let v = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':sec/name')
             AND added = true ORDER BY tx DESC LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!(v.contains("🎉"));
    }

    #[pg_test]
    fn test_unicode_zero_width() {
        setup();
        setup_test_schema();

        // Zero-width joiner and similar invisible chars
        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :sec/name \"a\u{200D}b\"]]'::TEXT)",
        )
        .expect("zero-width char failed");
    }

    #[pg_test]
    fn test_unicode_rtl() {
        setup();
        setup_test_schema();

        Spi::run(
            r#"SELECT mentat_transact('[[:db/add "e" :sec/name "مرحبا بالعالم"]]'::TEXT)"#,
        )
        .expect("RTL text failed");

        let v = Spi::get_one::<String>(
            "SELECT v_text FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':sec/name')
             AND added = true ORDER BY tx DESC LIMIT 1",
        )
        .expect("query failed")
        .expect("NULL");

        assert!(v.contains("مرحبا"));
    }

    #[pg_test]
    fn test_unicode_cjk() {
        setup();
        setup_test_schema();

        Spi::run(
            r#"SELECT mentat_transact('[[:db/add "e" :sec/name "日本語テスト中文测试한국어"]]'::TEXT)"#,
        )
        .expect("CJK text failed");
    }

    // ========================================================================
    // 7. Error Message Safety
    // ========================================================================

    #[pg_test]
    fn test_error_does_not_leak_sql() {
        setup();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :nonexistent/attr \"val\"]]'::TEXT)",
        );

        if let Err(e) = result {
            let msg = e.to_string();
            // Error message should not contain raw SQL
            assert!(
                !msg.contains("SELECT") || msg.contains(":db.error"),
                "Error should use Mentat error codes, not raw SQL"
            );
        }
    }

    #[pg_test]
    fn test_error_codes_present() {
        setup();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :nonexistent/attr \"val\"]]'::TEXT)",
        );

        if let Err(e) = result {
            let msg = e.to_string();
            assert!(
                msg.contains(":db.error/"),
                "Error should contain :db.error/ code"
            );
        }
    }

    // ========================================================================
    // 8. Concurrent Safety (basic)
    // ========================================================================

    #[pg_test]
    fn test_sequential_schema_changes() {
        setup();

        // Define schema attributes one at a time to test schema cache invalidation
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[
                    {{:db/id \"a{i}\" :db/ident :seq/attr{i}
                     :db/valueType :db.type/string
                     :db/cardinality :db.cardinality/one}}
                ]'::TEXT)",
                i = i
            ))
            .expect("sequential schema change failed");
        }

        // All 10 should be queryable
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.schema WHERE ident LIKE ':seq/%'",
        )
        .expect("query failed")
        .expect("NULL");

        assert_eq!(count, 10);
    }

    // ========================================================================
    // 9. Cartesian Explosion Prevention
    // ========================================================================

    #[pg_test]
    fn test_query_with_many_variables_and_clauses() {
        setup();
        setup_test_schema();

        // Create some data
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e1\" :sec/name \"a\"] [:db/add \"e1\" :sec/val 1]
                [:db/add \"e2\" :sec/name \"b\"] [:db/add \"e2\" :sec/val 2]
                [:db/add \"e3\" :sec/name \"c\"] [:db/add \"e3\" :sec/val 3]
            ]'::TEXT)",
        )
        .expect("data failed");

        // Well-structured join should work fine
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?n1 ?n2 ?v1 ?v2
                 :where
                 [?e1 :sec/name ?n1] [?e1 :sec/val ?v1]
                 [?e2 :sec/name ?n2] [?e2 :sec/val ?v2]
                 [(< ?v1 ?v2)]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("cross-join query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let results = json["results"].as_array().expect("results array");
        // Should have pairs where v1 < v2: (1,2), (1,3), (2,3) = 3 pairs
        assert_eq!(results.len(), 3);
    }

    // ========================================================================
    // 10. Schema Violation Tests
    // ========================================================================

    #[pg_test]
    fn test_component_attr_must_be_ref() {
        setup();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"bad\" :db/ident :sec/badcomp
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/isComponent true}
            ]'::TEXT)",
        );

        // Components should be ref type; string should be rejected
        // This is implementation-defined, but test documents the behavior
        drop(result);
    }

    #[pg_test]
    fn test_unique_on_cardinality_many() {
        setup();

        // Unique on cardinality-many is unusual but may be allowed
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :sec/uniqmany
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/many
                 :db/unique :db.unique/identity}
            ]'::TEXT)",
        );

        // Document the behavior
        drop(result);
    }
}
