// Regression tests for the pg_trgm trigram-similarity binding:
//   [(similar-to $ :attr "needle" threshold) [[?e ?val ?score]]]
//
// pg_trgm is an OPTIONAL contrib extension. Tests that need it use
// `mentat.has_pg_trgm()` and skip when it isn't installed in the
// pgrx-managed cluster.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._trgm_capture(stmt TEXT) RETURNS TEXT
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN '';
             EXCEPTION WHEN OTHERS THEN
                 RETURN SQLERRM;
             END;
             $$",
        )
        .expect("error-capture helper");
    }

    fn capture_error(sql: &str) -> String {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<String>(&format!("SELECT mentat._trgm_capture('{}')", escaped))
            .expect("capture")
            .unwrap_or_default()
    }

    fn has_pg_trgm() -> bool {
        Spi::get_one::<bool>("SELECT mentat.has_pg_trgm()")
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    fn install_name_attr_with_data() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/n :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :p/n \"Alice\"}
                {:db/id \"b\" :p/n \"Alyce\"}
                {:db/id \"c\" :p/n \"Alicia\"}
                {:db/id \"d\" :p/n \"Robert\"}
            ]'::TEXT)",
        )
        .expect("data tx");
    }

    #[pg_test]
    fn pg_test_trgm_has_pg_trgm_returns_bool() {
        setup();
        // Either outcome valid; just verify the helper runs.
        let _ = Spi::get_one::<bool>("SELECT mentat.has_pg_trgm()")
            .expect("call")
            .unwrap_or(false);
    }

    /// At threshold 0.3 (pg_trgm default), "Alice"/"Alyce"/"Alicia" all
    /// match; "Robert" does not.
    #[pg_test]
    fn pg_test_trgm_similar_to_basic() {
        setup();
        if !has_pg_trgm() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS pg_trgm");
        }
        if !has_pg_trgm() {
            return;
        }
        install_name_attr_with_data();
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?s :where [(similar-to $ :p/n \"Alice\" 0.3) [[?e ?n ?s]]]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        let names: std::collections::HashSet<String> = results
            .iter()
            .map(|r| r[0].as_str().expect("name").to_string())
            .collect();
        assert!(names.contains("Alice"));
        assert!(names.contains("Alyce"));
        assert!(names.contains("Alicia"));
        assert!(!names.contains("Robert"), "Robert should not match Alice");

        // "Alice" itself scores 1.0.
        let alice_score = results
            .iter()
            .find(|r| r[0].as_str() == Some("Alice"))
            .and_then(|r| r[1].as_f64())
            .expect("Alice score");
        assert!((alice_score - 1.0).abs() < 1e-6);
    }

    /// A very strict threshold (0.95) drops everything except an exact match.
    #[pg_test]
    fn pg_test_trgm_strict_threshold() {
        setup();
        if !has_pg_trgm() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS pg_trgm");
        }
        if !has_pg_trgm() {
            return;
        }
        install_name_attr_with_data();
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n :where [(similar-to $ :p/n \"Alice\" 0.95) [[?e ?n ?s]]]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        let names: Vec<String> = results
            .iter()
            .map(|r| r[0].as_str().expect("name").to_string())
            .collect();
        assert_eq!(names, vec!["Alice".to_string()]);
    }

    /// `mentat.create_trgm_index(':p/n')` is idempotent and returns the
    /// deterministic index name.
    #[pg_test]
    fn pg_test_trgm_create_index_idempotent() {
        setup();
        if !has_pg_trgm() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS pg_trgm");
        }
        if !has_pg_trgm() {
            return;
        }
        install_name_attr_with_data();

        let n1 = Spi::get_one::<String>("SELECT mentat.create_trgm_index(':p/n')")
            .expect("first call")
            .expect("NULL");
        let n2 = Spi::get_one::<String>("SELECT mentat.create_trgm_index(':p/n')")
            .expect("second call")
            .expect("NULL");
        assert_eq!(n1, n2, "deterministic index name");
        assert!(n1.starts_with("current_text_trgm_"), "name: {}", n1);

        // Index actually exists.
        let exists = Spi::get_one::<bool>(&format!(
            "SELECT EXISTS (SELECT 1 FROM pg_indexes \
             WHERE schemaname='mentat' AND indexname='{}')",
            n1
        ))
        .expect("exists query")
        .expect("NULL");
        assert!(exists, "index {} should exist", n1);

        // Drop returns true the first time, false the second.
        let dropped1 = Spi::get_one::<bool>("SELECT mentat.drop_trgm_index(':p/n')")
            .expect("drop1")
            .expect("NULL");
        let dropped2 = Spi::get_one::<bool>("SELECT mentat.drop_trgm_index(':p/n')")
            .expect("drop2")
            .expect("NULL");
        assert!(dropped1, "first drop should report existed=true");
        assert!(!dropped2, "second drop should report existed=false");
    }

    /// Wrong arity for similar-to surfaces :db.error/fn-arity.
    #[pg_test]
    fn pg_test_trgm_arity_error() {
        setup();
        install_name_attr_with_data();
        let err = capture_error(
            "SELECT mentat_query('[:find ?n :where [(similar-to $ :p/n \"Alice\") [[?e ?n ?s]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arity") && err.contains("similar-to"),
            "expected fn-arity error, got: {}",
            err,
        );
    }

    /// Out-of-range threshold (1.5) raises :db.error/fn-arg.
    #[pg_test]
    fn pg_test_trgm_threshold_out_of_range() {
        setup();
        install_name_attr_with_data();
        let err = capture_error(
            "SELECT mentat_query('[:find ?n :where [(similar-to $ :p/n \"Alice\" 1.5) [[?e ?n ?s]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arg") && err.contains("threshold"),
            "expected fn-arg threshold error, got: {}",
            err,
        );
    }

    /// create_trgm_index with an unknown attribute raises a clear error.
    #[pg_test]
    fn pg_test_trgm_create_index_unknown_attr() {
        setup();
        if !has_pg_trgm() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS pg_trgm");
        }
        if !has_pg_trgm() {
            return;
        }
        let err = capture_error("SELECT mentat.create_trgm_index(':not/registered')");
        assert!(
            err.contains(":db.error/unknown-attribute"),
            "expected unknown-attribute error, got: {}",
            err,
        );
    }
}
