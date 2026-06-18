// Regression tests for the (fuzzy-match $ :attr "pattern" k) Datalog
// where-fn that compiles to a pg_tre %~~ tre_pattern(...) join.
//
// pg_tre is an OPTIONAL extension. These tests are gated on
// `mentat.has_pg_tre()` returning true; if pg_tre is not present in the
// pgrx test cluster, the body-of-test branch is skipped and the
// soft-dep error path is exercised instead.
//
// To run these tests, the pg_tre shared library must be installed AND
// added to shared_preload_libraries in the test cluster's postgresql.conf,
// then the cluster restarted. See docs/src/fuzzy-search.md.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;
    use std::collections::HashSet;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        // Helper: run a statement in a PL/pgSQL EXCEPTION block (which gives
        // a subtransaction) and return the error message. Returns an empty
        // string when the statement does NOT raise. This is the pattern
        // used elsewhere in the test suite (see commit 9080ca7) because
        // bare Spi::get_one() longjmps out of the test on SPI errors and
        // never returns Err.
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._fuzzy_test_capture_error(stmt TEXT) RETURNS TEXT
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

    /// Run `sql` in a subtransaction; return the SQLERRM (empty if no error).
    fn capture_error(sql: &str) -> String {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<String>(&format!(
            "SELECT mentat._fuzzy_test_capture_error('{}')",
            escaped
        ))
        .expect("capture_error call")
        .unwrap_or_default()
    }

    /// Returns true if pg_tre is installed in the test database.
    /// Skips the body of pg_tre-dependent tests when false.
    fn has_pg_tre() -> bool {
        Spi::get_one::<bool>("SELECT mentat.has_pg_tre()")
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    /// Define a :doc/body string attribute and return its entid.
    fn install_doc_attr() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :doc/body :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
    }

    /// Insert five docs with deliberate typo variants of \"database\".
    /// Returns the entids in insertion order.
    fn install_typo_dataset() -> [i64; 5] {
        install_doc_attr();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"d1\" :doc/body \"the database error happens at scale\"}
                {:db/id \"d2\" :doc/body \"the databse error happens at scale\"}
                {:db/id \"d3\" :doc/body \"an unrelated row about cats\"}
                {:db/id \"d4\" :doc/body \"the dattabaze error happens at scale\"}
                {:db/id \"d5\" :doc/body \"datbase error in production\"}
            ]'::TEXT)::TEXT",
        )
        .expect("data tx")
        .expect("NULL tx report");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse tx report");
        [
            j["tempids"]["d1"].as_i64().expect("d1"),
            j["tempids"]["d2"].as_i64().expect("d2"),
            j["tempids"]["d3"].as_i64().expect("d3"),
            j["tempids"]["d4"].as_i64().expect("d4"),
            j["tempids"]["d5"].as_i64().expect("d5"),
        ]
    }

    /// Run a `:find ?e` (relation form) fuzzy-match query and return the
    /// entid set in column 0.
    fn fuzzy_query_entids(pattern: &str, k: i64) -> HashSet<i64> {
        let q = format!(
            "[:find ?e ?val :where [(fuzzy-match $ :doc/body \"{}\" {}) [[?e ?val]]]]",
            pattern, k
        );
        let sql = format!(
            "SELECT mentat_query('{}'::TEXT, '{{}}'::jsonb)::TEXT",
            q.replace('\'', "''")
        );
        let raw = Spi::get_one::<String>(&sql).expect("query").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse query result");
        j["results"]
            .as_array()
            .expect("results array")
            .iter()
            .map(|row| {
                row.as_array().expect("row")[0]
                    .as_i64()
                    .expect("entid is i64")
            })
            .collect()
    }

    /// `mentat.create_tre_index(:doc/body)` raises a specific
    /// :db.error/missing-extension error when pg_tre is not installed.
    /// This test is the inverse of the others: it runs ONLY when pg_tre
    /// is absent.
    #[pg_test]
    fn pg_test_fuzzy_match_soft_dep_error() {
        setup();
        if has_pg_tre() {
            // pg_tre IS installed in this environment; the soft-dep error
            // path doesn't apply. The happy-path tests below cover the
            // working case.
            return;
        }
        install_doc_attr();
        let err = capture_error("SELECT mentat.create_tre_index(':doc/body')");
        assert!(
            err.contains(":db.error/missing-extension"),
            "error should be :db.error/missing-extension, got: {}",
            err,
        );
        assert!(
            err.contains("pg_tre"),
            "error should name pg_tre, got: {}",
            err,
        );
    }

    /// k=0 (exact match): only the canonical \"database\" row matches.
    #[pg_test]
    fn pg_test_fuzzy_match_exact() {
        setup();
        if !has_pg_tre() {
            return; // soft-dep absent; covered by pg_test_fuzzy_match_soft_dep_error
        }
        let [d1, _d2, _d3, _d4, _d5] = install_typo_dataset();
        Spi::run("SELECT mentat.create_tre_index(':doc/body')").expect("create_tre_index");
        let got = fuzzy_query_entids("database", 0);
        let expected: HashSet<i64> = [d1].into_iter().collect();
        assert_eq!(
            got, expected,
            "k=0 should match only the exact \"database\" row, got {:?}",
            got,
        );
    }

    /// k=1 (one edit): canonical + 1-edit variants match. Specifically:
    ///   d1 \"database\"  exact match
    ///   d2 \"databse\"   one transposition
    ///   d5 \"datbase\"   one transposition
    /// d3 (cats, irrelevant) and d4 (\"dattabaze\", >1 edit) excluded.
    #[pg_test]
    fn pg_test_fuzzy_match_k1() {
        setup();
        if !has_pg_tre() {
            return;
        }
        let [d1, d2, d3, d4, d5] = install_typo_dataset();
        Spi::run("SELECT mentat.create_tre_index(':doc/body')").expect("create_tre_index");
        let got = fuzzy_query_entids("database", 1);
        let expected: HashSet<i64> = [d1, d2, d5].into_iter().collect();
        assert_eq!(
            got, expected,
            "k=1 mismatch. got={:?} expected={:?} (d3={} cats excluded; d4={} >1 edit excluded)",
            got, expected, d3, d4,
        );
    }

    /// k=2 (two edits): all four typo variants match; the unrelated cats
    /// row stays excluded.
    #[pg_test]
    fn pg_test_fuzzy_match_k2() {
        setup();
        if !has_pg_tre() {
            return;
        }
        let [d1, d2, d3, d4, d5] = install_typo_dataset();
        Spi::run("SELECT mentat.create_tre_index(':doc/body')").expect("create_tre_index");
        let got = fuzzy_query_entids("database", 2);
        let expected: HashSet<i64> = [d1, d2, d4, d5].into_iter().collect();
        assert_eq!(
            got, expected,
            "k=2 mismatch. got={:?} expected={:?} (d3={} cats must remain excluded)",
            got, expected, d3,
        );
    }

    /// Wrong arity should produce a specific :db.error/fn-arity error.
    #[pg_test]
    fn pg_test_fuzzy_match_arity_error() {
        setup();
        install_doc_attr();
        let err = capture_error(
            "SELECT mentat_query('[:find ?e :where [(fuzzy-match $ :doc/body \"x\") [[?e ?v]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arity"),
            ":db.error/fn-arity expected, got: {}",
            err,
        );
    }

    /// k > 8 should be rejected to bound regex compilation cost.
    #[pg_test]
    fn pg_test_fuzzy_match_k_out_of_range() {
        setup();
        install_doc_attr();
        let err = capture_error(
            "SELECT mentat_query('[:find ?e :where [(fuzzy-match $ :doc/body \"x\" 99) [[?e ?v]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arg") && err.contains("k must be in"),
            "k-range error expected, got: {}",
            err,
        );
    }

    /// create_tre_index on a non-text attribute returns a typed error.
    #[pg_test]
    fn pg_test_create_tre_index_wrong_type_error() {
        setup();
        if !has_pg_tre() {
            return; // wrong-type check is gated behind has_pg_tre()
        }
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :doc/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        let err = capture_error("SELECT mentat.create_tre_index(':doc/age')");
        assert!(
            err.contains(":db.error/wrong-type-for-tre-index"),
            "wrong-type error expected, got: {}",
            err,
        );
    }
}
