// Regression tests for the fuzzystrmatch contrib bindings:
//   [(levenshtein ?a ?b) ?d]
//   [(soundex ?s) ?code]
//   [(metaphone ?s ?max) ?code]
//   [(daitch-mokotoff ?s) ?codes]
//
// fuzzystrmatch is an OPTIONAL extension. Tests gate the happy path on
// `mentat.has_fuzzystrmatch()` so the suite passes whether or not the
// pgrx-managed test cluster has it preloaded.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._fz_capture(stmt TEXT) RETURNS TEXT
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN '';
             EXCEPTION WHEN OTHERS THEN
                 RETURN SQLERRM;
             END;
             $$",
        )
        .expect("create error-capture helper");
    }

    fn capture_error(sql: &str) -> String {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<String>(&format!("SELECT mentat._fz_capture('{}')", escaped))
            .expect("capture")
            .unwrap_or_default()
    }

    fn has_fuzzystrmatch() -> bool {
        Spi::get_one::<bool>("SELECT mentat.has_fuzzystrmatch()")
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    fn install_name_attr() {
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
                {:db/id \"c\" :p/n \"Robert\"}
            ]'::TEXT)",
        )
        .expect("data tx");
    }

    /// `mentat.has_fuzzystrmatch()` returns boolean; both true and false
    /// outcomes are valid depending on the test cluster.
    #[pg_test]
    fn pg_test_fz_has_fuzzystrmatch_returns_bool() {
        setup();
        let v = Spi::get_one::<bool>("SELECT mentat.has_fuzzystrmatch()")
            .expect("call")
            .unwrap_or(false);
        // Just assert the call didn't panic; the value depends on
        // whether the cluster has the extension installed.
        let _ = v;
    }

    /// (levenshtein ?a ?b) binds the integer edit distance.
    #[pg_test]
    fn pg_test_fz_levenshtein_basic() {
        setup();
        if !has_fuzzystrmatch() {
            // Try to install it; contrib lives in $libdir, almost always
            // present alongside core PG.
            let _ = Spi::run("CREATE EXTENSION IF NOT EXISTS fuzzystrmatch");
        }
        if !has_fuzzystrmatch() {
            return; // contrib not available in this build — skip
        }
        install_name_attr();
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?d :where [?e :p/n ?n] [(levenshtein ?n \"Alice\") ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        // Parse: results should include each name with its distance to "Alice".
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        let mut found = std::collections::HashMap::new();
        for row in results {
            let name = row[0].as_str().expect("name str").to_string();
            let dist = row[1].as_i64().expect("dist int");
            found.insert(name, dist);
        }
        assert_eq!(found.get("Alice"), Some(&0), "Alice distance from Alice");
        assert_eq!(found.get("Alyce"), Some(&1), "Alyce distance from Alice");
        assert_eq!(found.get("Robert"), Some(&6), "Robert distance from Alice");
    }

    /// (soundex ?s) binds the 4-char Soundex code; "Alice" and "Alyce"
    /// share the same Soundex value (A420), proving the binding works.
    #[pg_test]
    fn pg_test_fz_soundex_groups_homophones() {
        setup();
        if !has_fuzzystrmatch() {
            let _ = Spi::run("CREATE EXTENSION IF NOT EXISTS fuzzystrmatch");
        }
        if !has_fuzzystrmatch() {
            return;
        }
        install_name_attr();
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?h :where [?e :p/n ?n] [(soundex ?n) ?h]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        let mut by_name = std::collections::HashMap::new();
        for row in results {
            by_name.insert(
                row[0].as_str().expect("name").to_string(),
                row[1].as_str().expect("hash").to_string(),
            );
        }
        assert_eq!(
            by_name.get("Alice"),
            by_name.get("Alyce"),
            "Alice and Alyce should soundex-match"
        );
        assert_ne!(
            by_name.get("Alice"),
            by_name.get("Robert"),
            "Alice and Robert should not soundex-match"
        );
    }

    /// (metaphone ?s ?max) takes a max-length argument; output is the
    /// phonetic encoding truncated to that length.
    #[pg_test]
    fn pg_test_fz_metaphone_basic() {
        setup();
        if !has_fuzzystrmatch() {
            let _ = Spi::run("CREATE EXTENSION IF NOT EXISTS fuzzystrmatch");
        }
        if !has_fuzzystrmatch() {
            return;
        }
        install_name_attr();
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?h :where [?e :p/n ?n] [(metaphone ?n 5) ?h]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        // We don't assert exact codes (Metaphone output varies by impl);
        // only that every row produces a non-empty code <= 5 chars.
        assert!(!results.is_empty(), "at least one row");
        for row in results {
            let code = row[1].as_str().expect("metaphone string");
            assert!(!code.is_empty(), "non-empty metaphone for {:?}", row[0]);
            assert!(
                code.len() <= 5,
                "<= 5 chars for {:?}, got {:?}",
                row[0],
                code
            );
        }
    }

    /// Wrong arity for levenshtein returns :db.error/fn-arity.
    #[pg_test]
    fn pg_test_fz_levenshtein_arity_error() {
        setup();
        install_name_attr();
        let err = capture_error(
            "SELECT mentat_query('[:find ?d :where [(levenshtein \"x\") ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arity") && err.contains("levenshtein"),
            "expected fn-arity error, got: {}",
            err,
        );
    }

    /// Soundex with non-text arg (an integer literal) returns :db.error/fn-arg.
    #[pg_test]
    fn pg_test_fz_soundex_wrong_arg_type() {
        setup();
        install_name_attr();
        let err = capture_error(
            "SELECT mentat_query('[:find ?h :where [(soundex 42) ?h]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arg") && err.contains("soundex"),
            "expected fn-arg error, got: {}",
            err,
        );
    }

    /// Chaining: levenshtein -> arithmetic. ?d from levenshtein flows into
    /// (* ?d 2) ?da. Demonstrates extra_var_bindings propagation between
    /// where-fns.
    #[pg_test]
    fn pg_test_fz_levenshtein_chained_with_arithmetic() {
        setup();
        if !has_fuzzystrmatch() {
            let _ = Spi::run("CREATE EXTENSION IF NOT EXISTS fuzzystrmatch");
        }
        if !has_fuzzystrmatch() {
            return;
        }
        install_name_attr();
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?da :where [?e :p/n ?n] [(levenshtein ?n \"Alice\") ?d] [(* ?d 2) ?da]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        let mut found = std::collections::HashMap::new();
        for row in results {
            found.insert(
                row[0].as_str().expect("name").to_string(),
                row[1].as_i64().expect("doubled-distance"),
            );
        }
        assert_eq!(found.get("Alice"), Some(&0));
        assert_eq!(found.get("Alyce"), Some(&2));
        assert_eq!(found.get("Robert"), Some(&12));
    }
}
