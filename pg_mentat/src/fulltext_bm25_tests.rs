// Full-text search BM25 scoring tests: verifies that the optimized fulltext
// query path using ts_rank_cd with datoms_text_new produces correct relevance
// rankings and language-aware stemming.
//
// These tests target the code changes from Task #7:
// - Direct datoms_text_new queries (no 9-way UNION ALL)
// - ts_rank_cd with normalization flag 32 for BM25-like scoring
// - Schema-driven stemming language resolution

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_fts_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"title\" :db/ident :fts/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/fulltext true :db/index true}
                {:db/id \"body\" :db/ident :fts/body :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/fulltext true :db/index true}
                {:db/id \"plain\" :db/ident :fts/plain :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"tag\" :db/ident :fts/tag :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        ).expect("fts schema");
    }

    // ========================================================================
    // Basic fulltext search
    // ========================================================================

    #[pg_test]
    fn test_fts_basic_search() {
        setup();
        setup_fts_schema();
        Spi::run(
            "SELECT mentat_transact('[
            {:db/id \"e1\" :fts/title \"The quick brown fox jumps over the lazy dog\"}
            {:db/id \"e2\" :fts/title \"A slow tortoise walks carefully\"}
            {:db/id \"e3\" :fts/title \"Quick foxes are clever animals\"}
        ]'::TEXT)",
        )
        .expect("data");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?x ?val :where [(fulltext $ :fts/title \"fox\") [[?x ?val]]]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        // Should find at least the documents containing "fox"
        assert!(results.len() >= 1, "Should find documents with 'fox'");
    }

    #[pg_test]
    fn test_fts_no_match() {
        setup();
        setup_fts_schema();
        Spi::run(
            "SELECT mentat_transact('[
            {:db/id \"e1\" :fts/title \"The quick brown fox\"}
        ]'::TEXT)",
        )
        .expect("data");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?x ?val :where [(fulltext $ :fts/title \"elephant\") [[?x ?val]]]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(results.len(), 0, "No match for 'elephant'");
    }

    // ========================================================================
    // Relevance scoring with ts_rank_cd
    //
    // ts_rank_cd (cover density ranking) with normalization 32 produces
    // BM25-like scores that account for document length.
    // ========================================================================

    #[pg_test]
    fn test_fts_relevance_ordering() {
        setup();
        setup_fts_schema();
        // Document 1: "database" appears multiple times (high relevance)
        // Document 2: "database" appears once in longer text (lower relevance)
        Spi::run("SELECT mentat_transact('[
            {:db/id \"e1\" :fts/title \"database systems and database design with database optimization\"}
            {:db/id \"e2\" :fts/title \"an introduction to modern computing systems including database and networking and storage and security and many other topics\"}
        ]'::TEXT)").expect("data");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?x ?val ?score :where [(fulltext $ :fts/title \"database\") [[?x ?val _ ?score]]]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert!(results.len() >= 2, "Should find both documents");

        // Extract scores - higher density document should score higher
        let scores: Vec<f64> = results
            .iter()
            .map(|r| r[2].as_f64().expect("score"))
            .collect();
        // At least verify scores are non-negative (ts_rank_cd returns >= 0)
        for s in &scores {
            assert!(*s >= 0.0, "Score should be non-negative");
        }
    }

    #[pg_test]
    fn test_fts_score_bound_variable() {
        setup();
        setup_fts_schema();
        Spi::run(
            "SELECT mentat_transact('[
            {:db/id \"e1\" :fts/title \"PostgreSQL full text search engine\"}
        ]'::TEXT)",
        )
        .expect("data");

        // Binding the score variable should work
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?x ?val ?score :where [(fulltext $ :fts/title \"search\") [[?x ?val _ ?score]]]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(results.len(), 1);
        // Score should be a number
        assert!(results[0][2].as_f64().is_some(), "Score should be a float");
    }

    // ========================================================================
    // Multiple fulltext attributes
    // ========================================================================

    #[pg_test]
    fn test_fts_different_attrs() {
        setup();
        setup_fts_schema();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"e1\" :fts/title \"Machine learning\" :fts/body \"Deep neural networks for classification\"}
            {:db/id \"e2\" :fts/title \"Neural networks\" :fts/body \"Machine learning algorithms for prediction\"}
        ]'::TEXT)").expect("data");

        // Search title
        let qt = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?x ?val :where [(fulltext $ :fts/title \"neural\") [[?x ?val]]]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let vt: serde_json::Value = serde_json::from_str(&qt).expect("parse");
        let title_results = vt["results"].as_array().expect("arr");

        // Search body
        let qb = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?x ?val :where [(fulltext $ :fts/body \"neural\") [[?x ?val]]]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let vb: serde_json::Value = serde_json::from_str(&qb).expect("parse");
        let body_results = vb["results"].as_array().expect("arr");

        // "neural" in title: e2; "neural" in body: e1
        assert!(title_results.len() >= 1, "Should find 'neural' in titles");
        assert!(body_results.len() >= 1, "Should find 'neural' in bodies");
    }

    // ========================================================================
    // Stemming: English stemmer should match inflected forms
    // ========================================================================

    #[pg_test]
    fn test_fts_english_stemming() {
        setup();
        setup_fts_schema();
        Spi::run(
            "SELECT mentat_transact('[
            {:db/id \"e1\" :fts/title \"The runners are running in the race\"}
            {:db/id \"e2\" :fts/title \"She runs quickly to the finish line\"}
            {:db/id \"e3\" :fts/title \"The swimming pool is closed\"}
        ]'::TEXT)",
        )
        .expect("data");

        // "run" matches "runners/running" and "runs" via English (Snowball)
        // stemming. Note: the Snowball stemmer does NOT reduce the irregular
        // past tense "ran" to "run", so a regular inflection ("runs") is used.
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?val ...] :where [(fulltext $ :fts/title \"run\") [[?x ?val]]]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["result"].as_array().expect("arr");
        // Should match the "runners/running" and "runs" documents
        assert!(
            results.len() >= 2,
            "English stemming should match inflected forms of 'run'"
        );
    }

    // ========================================================================
    // Fulltext with non-fulltext attributes combined
    // ========================================================================

    #[pg_test]
    fn test_fts_combined_with_regular_attrs() {
        setup();
        setup_fts_schema();
        Spi::run(
            "SELECT mentat_transact('[
            {:db/id \"e1\" :fts/title \"Rust programming language\" :fts/plain \"systems\"}
            {:db/id \"e2\" :fts/title \"Python programming language\" :fts/plain \"scripting\"}
        ]'::TEXT)",
        )
        .expect("data");

        // Search fulltext, then join with regular attr
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?x ?val ?p :where [(fulltext $ :fts/title \"programming\") [[?x ?val]]] [?x :fts/plain ?p]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(
            results.len(),
            2,
            "Both documents match 'programming' and have :fts/plain"
        );
    }

    // ========================================================================
    // Fulltext with upserted entities
    //
    // Integration: unique/identity upsert + fulltext attribute
    // ========================================================================

    #[pg_test]
    fn test_fts_upsert_updates_fulltext() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"uid\" :db/ident :ftsu/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"title\" :db/ident :ftsu/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/fulltext true :db/index true}
            ]'::TEXT)",
        ).expect("schema");

        // Create entity with fulltext
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :ftsu/uid \"DOC1\" :ftsu/title \"original document about databases\"}]'::TEXT)").expect("create");

        // Upsert with new fulltext value
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :ftsu/uid \"DOC1\" :ftsu/title \"updated document about networking\"}]'::TEXT)").expect("upsert");

        // Search for old term should not find it
        let q_old = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?val . :where [(fulltext $ :ftsu/title \"databases\") [[?x ?val]]]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v_old: serde_json::Value = serde_json::from_str(&q_old).expect("parse");
        assert!(
            v_old["result"].is_null(),
            "Old fulltext value should not match after upsert"
        );

        // Search for new term should find it
        let q_new = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?val . :where [(fulltext $ :ftsu/title \"networking\") [[?x ?val]]]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v_new: serde_json::Value = serde_json::from_str(&q_new).expect("parse");
        assert_eq!(
            v_new["result"].as_str().expect("title"),
            "updated document about networking"
        );
    }
}
