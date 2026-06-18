// Regression tests for the rum integration:
//   [(rum-fulltext $ :attr "term") [[?e ?val ?score]]]
//
// rum is an OPTIONAL extension (postgrespro/rum, PostgreSQL license).
// Tests skip when it isn't installed in the pgrx-managed cluster.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._rum_capture(stmt TEXT) RETURNS TEXT
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
        Spi::get_one::<String>(&format!("SELECT mentat._rum_capture('{}')", escaped))
            .expect("capture")
            .unwrap_or_default()
    }

    fn has_rum() -> bool {
        Spi::get_one::<bool>("SELECT mentat.has_rum()")
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    fn install_body_attr_with_data() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :issue/body :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one :db/fulltext true}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :issue/body \"the database crashed last night\"}
                {:db/id \"b\" :issue/body \"refactor the indexer for cache locality\"}
                {:db/id \"c\" :issue/body \"database queries slower after upgrade\"}
                {:db/id \"d\" :issue/body \"the cat sat on the mat\"}
            ]'::TEXT)",
        )
        .expect("data tx");
    }

    #[pg_test]
    fn pg_test_rum_has_rum_returns_bool() {
        setup();
        let _ = Spi::get_one::<bool>("SELECT mentat.has_rum()")
            .expect("call")
            .unwrap_or(false);
    }

    /// Without an index, rum-fulltext works against a sequential scan
    /// (rum is an index — its absence only affects performance, not
    /// correctness of the @@ filter).
    #[pg_test]
    fn pg_test_rum_fulltext_basic_no_index() {
        setup();
        install_body_attr_with_data();
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?b :where \
             [(rum-fulltext $ :issue/body \"database\") [[?e ?b ?s]]]]'::TEXT, \
             '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        let bodies: Vec<String> = results
            .iter()
            .map(|r| r[0].as_str().expect("body").to_string())
            .collect();
        assert_eq!(bodies.len(), 2, "two rows match \"database\"");
        assert!(bodies.iter().any(|b| b.contains("crashed")));
        assert!(bodies.iter().any(|b| b.contains("queries")));
        // "the cat sat on the mat" must NOT match.
        assert!(!bodies.iter().any(|b| b.contains("cat")));
    }

    /// With a rum index, the same query returns the same rows, plus a
    /// non-zero `rum_ts_score`.
    #[pg_test]
    fn pg_test_rum_fulltext_with_index_returns_score() {
        setup();
        if !has_rum() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS rum");
        }
        if !has_rum() {
            return;
        }
        install_body_attr_with_data();

        let idx_name =
            Spi::get_one::<String>("SELECT mentat.create_rum_fulltext_index(':issue/body')")
                .expect("create index")
                .expect("NULL");
        assert!(
            idx_name.starts_with("current_text_rum_"),
            "idx: {}",
            idx_name
        );

        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?b ?s :where \
             [(rum-fulltext $ :issue/body \"database\") [[?e ?b ?s]]] \
             :order (desc ?s)]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        assert_eq!(results.len(), 2, "two rows match");
        for row in results {
            let score = row[1].as_f64().expect("score");
            assert!(
                score > 0.0,
                "rum_ts_score should be positive, got {}",
                score
            );
        }
    }

    /// Idempotent index creation + drop.
    #[pg_test]
    fn pg_test_rum_index_idempotent() {
        setup();
        if !has_rum() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS rum");
        }
        if !has_rum() {
            return;
        }
        install_body_attr_with_data();

        let n1 = Spi::get_one::<String>("SELECT mentat.create_rum_fulltext_index(':issue/body')")
            .expect("create 1")
            .expect("NULL");
        let n2 = Spi::get_one::<String>("SELECT mentat.create_rum_fulltext_index(':issue/body')")
            .expect("create 2")
            .expect("NULL");
        assert_eq!(n1, n2);

        let dropped1 = Spi::get_one::<bool>("SELECT mentat.drop_rum_fulltext_index(':issue/body')")
            .expect("drop 1")
            .expect("NULL");
        let dropped2 = Spi::get_one::<bool>("SELECT mentat.drop_rum_fulltext_index(':issue/body')")
            .expect("drop 2")
            .expect("NULL");
        assert!(dropped1);
        assert!(!dropped2);
    }

    /// Wrong arity surfaces :db.error/fn-arity.
    #[pg_test]
    fn pg_test_rum_fulltext_arity_error() {
        setup();
        install_body_attr_with_data();
        let err = capture_error(
            "SELECT mentat_query('[:find ?b :where [(rum-fulltext $) [[?e ?b ?s]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arity") && err.contains("rum-fulltext"),
            "expected fn-arity error, got: {}",
            err,
        );
    }

    /// create_rum_fulltext_index for an unknown attribute fails clearly.
    #[pg_test]
    fn pg_test_rum_create_index_unknown_attr() {
        setup();
        if !has_rum() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS rum");
        }
        if !has_rum() {
            return;
        }
        let err = capture_error("SELECT mentat.create_rum_fulltext_index(':not/registered')");
        assert!(
            err.contains(":db.error/unknown-attribute"),
            "expected unknown-attribute, got: {}",
            err,
        );
    }
}
