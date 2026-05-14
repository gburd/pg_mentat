// Regression tests for the pg_infer integration:
//   [(infer-similar a b [model]) ?score]
//   [(infer-implies a b [model]) ?bool]
//   [(infer-near $ :attr "text" k [model]) [[?e ?dist]]]
//   mentat.has_pg_infer / create_infer_index / drop_infer_index
//
// pg_infer is an OPTIONAL extension that requires PG18+. Tests skip
// the happy path when pg_infer isn't installed in the cluster
// (very likely, since pg_infer is experimental). We DO run negative-
// path tests on every cluster: arity, arg-type, missing-extension,
// unknown-attribute. Those should pass regardless.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._pginfer_capture(stmt TEXT) RETURNS TEXT
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
        Spi::get_one::<String>(&format!("SELECT mentat._pginfer_capture('{}')", escaped))
            .expect("capture")
            .unwrap_or_default()
    }

    fn has_pg_infer() -> bool {
        Spi::get_one::<bool>("SELECT mentat.has_pg_infer()")
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    /// Detection helper returns boolean.
    #[pg_test]
    fn pg_test_pginfer_has_pg_infer_returns_bool() {
        setup();
        let _ = Spi::get_one::<bool>("SELECT mentat.has_pg_infer()")
            .expect("call")
            .unwrap_or(false);
    }

    /// Wrong arity for infer-similar surfaces :db.error/fn-arity.
    #[pg_test]
    fn pg_test_pginfer_similar_arity_error() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/n :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :p/n \"X\"}]'::TEXT)")
            .expect("data tx");

        // 1-arg form is wrong (needs 2 or 3).
        let err = capture_error(
            "SELECT mentat_query('[:find ?s :where [?e :p/n ?n] [(infer-similar ?n) ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arity") && err.contains("infer-similar"),
            "expected fn-arity error, got: {}",
            err,
        );
    }

    /// Wrong arg type for infer-similar surfaces :db.error/fn-arg.
    #[pg_test]
    fn pg_test_pginfer_similar_wrong_arg_type() {
        setup();
        let err = capture_error(
            "SELECT mentat_query('[:find ?s :where [(infer-similar 42 \"x\") ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arg") && err.contains("infer-similar"),
            "expected fn-arg error, got: {}",
            err,
        );
    }

    /// infer-near arity: requires 4 or 5 args.
    #[pg_test]
    fn pg_test_pginfer_near_arity_error() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/n :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");

        let err = capture_error(
            "SELECT mentat_query('[:find ?e :where [(infer-near $ :p/n) [[?e ?d]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arity") && err.contains("infer-near"),
            "expected fn-arity error, got: {}",
            err,
        );
    }

    /// infer-near with k <= 0 is rejected at compile time.
    #[pg_test]
    fn pg_test_pginfer_near_k_must_be_positive() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/n :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        let err = capture_error(
            "SELECT mentat_query('[:find ?e :where [(infer-near $ :p/n \"x\" 0) [[?e ?d]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arg") && err.contains("k must be > 0"),
            "expected k>0 fn-arg error, got: {}",
            err,
        );
    }

    /// create_infer_index without pg_infer surfaces :db.error/missing-extension.
    #[pg_test]
    fn pg_test_pginfer_create_index_without_pg_infer() {
        setup();
        if has_pg_infer() {
            return;
        }
        let err =
            capture_error("SELECT mentat.create_infer_index(':not/registered', 'mymodel')");
        assert!(
            err.contains(":db.error/missing-extension") && err.contains("pg_infer"),
            "expected missing-extension error, got: {}",
            err,
        );
    }

    /// create_infer_index with unknown attribute (when pg_infer is present)
    /// surfaces :db.error/unknown-attribute. When pg_infer is absent, the
    /// missing-extension check fires first; we skip the unknown-attribute
    /// assertion in that case.
    #[pg_test]
    fn pg_test_pginfer_create_index_unknown_attr() {
        setup();
        if !has_pg_infer() {
            return;
        }
        let err =
            capture_error("SELECT mentat.create_infer_index(':not/registered', 'mymodel')");
        assert!(
            err.contains(":db.error/unknown-attribute"),
            "expected unknown-attribute error, got: {}",
            err,
        );
    }

    /// Compile-time: an infer-similar query against a string attribute
    /// must compile cleanly to SQL even when pg_infer is not installed.
    /// At execution, postgres will raise "function does not exist" \u2014 we
    /// assert the error mentions infer_similarity, proving the SQL we
    /// emitted is well-formed.
    #[pg_test]
    fn pg_test_pginfer_similar_compile_clean_without_pg_infer() {
        setup();
        if has_pg_infer() {
            return;
        }
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/n :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :p/n \"X\"}]'::TEXT)")
            .expect("data tx");

        let err = capture_error(
            "SELECT mentat_query('[:find ?n ?s :where [?e :p/n ?n] [(infer-similar ?n \"target\") ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        // The error must come from postgres, not from pg_mentat's parser.
        assert!(
            err.contains("infer_similarity") && err.contains("does not exist"),
            "expected infer_similarity-not-found from postgres, got: {}",
            err,
        );
    }

    /// Same compile-clean check for infer-near.
    #[pg_test]
    fn pg_test_pginfer_near_compile_clean_without_pg_infer() {
        setup();
        if has_pg_infer() {
            return;
        }
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/n :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");

        let err = capture_error(
            "SELECT mentat_query('[:find ?e :where [(infer-near $ :p/n \"x\" 5) [[?e ?d]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        // Must mention the <~> operator (or its equivalent function) from postgres.
        assert!(
            err.contains("<~>") || err.contains("operator")
                || err.contains("does not exist"),
            "expected pg-side <~> operator-not-found, got: {}",
            err,
        );
    }

    /// End-to-end happy path: requires pg_infer + a registered model.
    /// Skips otherwise. We don't have a vindex on disk in CI, so this
    /// test very nearly always skips \u2014 documented for completeness.
    #[pg_test]
    fn pg_test_pginfer_similar_with_pg_infer_e2e() {
        setup();
        if !has_pg_infer() {
            return;
        }
        // Probe whether ANY model is registered. If not, skip.
        let n_models = Spi::get_one::<i64>("SELECT count(*)::BIGINT FROM infer_models()")
            .ok()
            .flatten()
            .unwrap_or(0);
        if n_models == 0 {
            return;
        }
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/n :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :p/n \"France\"}]'::TEXT)")
            .expect("data tx");

        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?s :where [?e :p/n ?n] [(infer-similar ?n \"Paris\") ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        assert_eq!(results.len(), 1, "one row");
        let score = results[0][1].as_f64().expect("score");
        // We don't pin the exact score (model-dependent), only that it
        // is finite and nonzero.
        assert!(score.is_finite() && score != 0.0, "score: {}", score);
    }
}
