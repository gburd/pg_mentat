// Regression tests for the pgvector integration:
//   [(vector-near $ :attr "[v1,v2,...]" k [:cosine|:l2|:inner]) [[?e ?dist]]]
//
// pgvector is an OPTIONAL extension. Tests skip when it isn't installed.
// Vectors live in a per-attribute aux table populated through the
// `mentat.set_vector` SPI helper — pg_mentat does NOT yet support
// :db.type/vector in its schema (tracked in docs/INTEGRATIONS.md).

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._pgv_capture(stmt TEXT) RETURNS TEXT
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
        Spi::get_one::<String>(&format!("SELECT mentat._pgv_capture('{}')", escaped))
            .expect("capture")
            .unwrap_or_default()
    }

    fn has_pgvector() -> bool {
        Spi::get_one::<bool>("SELECT mentat.has_pgvector()")
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    /// Create three docs with deterministic eids so we can populate vectors
    /// without pinning to actual entids returned by transact (which can
    /// shift across tests).
    fn install_docs_with_vectors() -> (i64, i64, i64) {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :doc/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/ident :doc/embedding :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :doc/title \"Postgres\"  :doc/embedding \"x\"}
                {:db/id \"b\" :doc/title \"Datalog\"   :doc/embedding \"x\"}
                {:db/id \"c\" :doc/title \"Cookies\"   :doc/embedding \"x\"}
            ]'::TEXT)",
        )
        .expect("data tx");

        // Look up the three entids from the title datoms.
        let e_a = Spi::get_one::<i64>(
            "SELECT e FROM mentat.datoms_text_new \
             WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':doc/title') \
               AND v = 'Postgres'",
        )
        .expect("e_a")
        .expect("NULL");
        let e_b = Spi::get_one::<i64>(
            "SELECT e FROM mentat.datoms_text_new \
             WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':doc/title') \
               AND v = 'Datalog'",
        )
        .expect("e_b")
        .expect("NULL");
        let e_c = Spi::get_one::<i64>(
            "SELECT e FROM mentat.datoms_text_new \
             WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':doc/title') \
               AND v = 'Cookies'",
        )
        .expect("e_c")
        .expect("NULL");

        // Attach vector aux table + populate.
        Spi::run("SELECT mentat.attach_vector_attribute(':doc/embedding', 3)").expect("attach");
        Spi::run(&format!(
            "SELECT mentat.set_vector({}, ':doc/embedding', '[0.9, 0.1, 0.0]')",
            e_a
        ))
        .expect("set a");
        Spi::run(&format!(
            "SELECT mentat.set_vector({}, ':doc/embedding', '[0.0, 0.9, 0.1]')",
            e_b
        ))
        .expect("set b");
        Spi::run(&format!(
            "SELECT mentat.set_vector({}, ':doc/embedding', '[0.0, 0.0, 1.0]')",
            e_c
        ))
        .expect("set c");

        (e_a, e_b, e_c)
    }

    #[pg_test]
    fn pg_test_pgv_has_pgvector_returns_bool() {
        setup();
        let _ = Spi::get_one::<bool>("SELECT mentat.has_pgvector()")
            .expect("call")
            .unwrap_or(false);
    }

    /// `vector-near` returns top-K rows in ascending cosine-distance order
    /// and JOINs back to subsequent patterns by entid (no cartesian product).
    #[pg_test]
    fn pg_test_pgv_vector_near_top_k_with_join() {
        setup();
        if !has_pgvector() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS vector");
        }
        if !has_pgvector() {
            return;
        }
        let (_e_a, _e_b, _e_c) = install_docs_with_vectors();

        // Query [1,0,0]: closest is Postgres (0.9 in dim0). With k=2 we
        // get Postgres plus one other; the join to :doc/title must NOT
        // multiply rows.
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?title ?dist :where \
             [(vector-near $ :doc/embedding \"[1,0,0]\" 2) [[?e ?dist]]] \
             [?e :doc/title ?title] :order (asc ?dist)]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        assert_eq!(
            results.len(),
            2,
            "K=2 should produce exactly 2 rows after join, got: {:?}",
            results
        );

        let titles: Vec<String> = results
            .iter()
            .map(|r| r[0].as_str().expect("title").to_string())
            .collect();
        // First row by ascending distance must be Postgres (closest to [1,0,0]).
        assert_eq!(titles[0], "Postgres", "closest match should be Postgres");
    }

    /// L2 distance gives a different ordering than cosine in general.
    /// For these vectors all unit-ish, L2 still ranks Postgres first.
    #[pg_test]
    fn pg_test_pgv_vector_near_l2_distance() {
        setup();
        if !has_pgvector() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS vector");
        }
        if !has_pgvector() {
            return;
        }
        install_docs_with_vectors();

        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?title ?dist :where \
             [(vector-near $ :doc/embedding \"[1,0,0]\" 1 :l2) [[?e ?dist]]] \
             [?e :doc/title ?title]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        assert_eq!(results.len(), 1, "K=1 should produce exactly 1 row");
        assert_eq!(results[0][0].as_str(), Some("Postgres"));
    }

    /// Idempotent attach — calling twice returns the same table name and
    /// does not corrupt the existing rows.
    #[pg_test]
    fn pg_test_pgv_attach_idempotent() {
        setup();
        if !has_pgvector() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS vector");
        }
        if !has_pgvector() {
            return;
        }
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :doc/v :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        let n1 = Spi::get_one::<String>("SELECT mentat.attach_vector_attribute(':doc/v', 4)")
            .expect("attach 1")
            .expect("NULL");
        let n2 = Spi::get_one::<String>("SELECT mentat.attach_vector_attribute(':doc/v', 4)")
            .expect("attach 2")
            .expect("NULL");
        assert_eq!(n1, n2);
        assert!(n1.starts_with("mentat.attr_"), "name: {}", n1);
    }

    /// set_vector then del_vector cycle.
    #[pg_test]
    fn pg_test_pgv_set_then_del() {
        setup();
        if !has_pgvector() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS vector");
        }
        if !has_pgvector() {
            return;
        }
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :doc/v :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run("SELECT mentat.attach_vector_attribute(':doc/v', 2)").expect("attach");
        Spi::run("SELECT mentat.set_vector(99999, ':doc/v', '[1,2]')").expect("set");
        let dropped = Spi::get_one::<bool>("SELECT mentat.del_vector(99999, ':doc/v')")
            .expect("del 1")
            .expect("NULL");
        assert!(dropped, "first del should report true");
        let dropped2 = Spi::get_one::<bool>("SELECT mentat.del_vector(99999, ':doc/v')")
            .expect("del 2")
            .expect("NULL");
        assert!(!dropped2, "second del should report false");
    }

    /// Bad arity surfaces :db.error/fn-arity.
    #[pg_test]
    fn pg_test_pgv_arity_error() {
        setup();
        let err = capture_error(
            "SELECT mentat_query('[:find ?e :where [(vector-near $ :doc/v) [[?e ?d]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arity") && err.contains("vector-near"),
            "expected fn-arity, got: {}",
            err,
        );
    }

    /// Unknown distance op surfaces :db.error/fn-arg.
    #[pg_test]
    fn pg_test_pgv_unknown_distance_op() {
        setup();
        if !has_pgvector() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS vector");
        }
        if !has_pgvector() {
            return;
        }
        install_docs_with_vectors();
        let err = capture_error(
            "SELECT mentat_query('[:find ?e :where [(vector-near $ :doc/embedding \"[1,0,0]\" 1 :hamming) [[?e ?d]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arg") && err.contains("distance"),
            "expected fn-arg distance error, got: {}",
            err,
        );
    }

    /// Unknown attribute surfaces :db.error/unknown-attribute (compile-time).
    #[pg_test]
    fn pg_test_pgv_unknown_attr_compile() {
        setup();
        if !has_pgvector() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS vector");
        }
        if !has_pgvector() {
            return;
        }
        let err = capture_error(
            "SELECT mentat_query('[:find ?e :where [(vector-near $ :no/such \"[1,0,0]\" 1) [[?e ?d]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/unknown-attribute"),
            "expected unknown-attribute, got: {}",
            err,
        );
    }

    /// HNSW index helper succeeds and is idempotent.
    #[pg_test]
    fn pg_test_pgv_hnsw_index_idempotent() {
        setup();
        if !has_pgvector() {
            let _ = capture_error("CREATE EXTENSION IF NOT EXISTS vector");
        }
        if !has_pgvector() {
            return;
        }
        install_docs_with_vectors();
        let n1 = Spi::get_one::<String>(
            "SELECT mentat.create_hnsw_vector_index(':doc/embedding', 'cosine')",
        )
        .expect("create 1")
        .expect("NULL");
        let n2 = Spi::get_one::<String>(
            "SELECT mentat.create_hnsw_vector_index(':doc/embedding', 'cosine')",
        )
        .expect("create 2")
        .expect("NULL");
        assert_eq!(n1, n2);
        assert!(n1.starts_with("attr_") && n1.ends_with("_hnsw_cosine"));
    }
}
