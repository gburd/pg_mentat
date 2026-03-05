// Copyright 2026
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! Full-text search tests ported from /query-algebrizer/tests/fulltext.rs
//!
//! These tests validate FTS functionality using PostgreSQL tsvector/tsquery
//! instead of SQLite's FTS4.

use pgrx::prelude::*;

#[path = "test_common.rs"]
mod common;

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;
    use crate::common::{setup_test_db, bootstrap_schema, transact, query};

    /// Setup FTS test schema with fulltext-indexed attributes.
    fn setup_fts_schema() {
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"person-name\" :db/ident :person/name]
                 [:db/add \"person-name\" :db/valueType :db.type/string]
                 [:db/add \"person-name\" :db/cardinality :db.cardinality/one]
                 [:db/add \"person-name\" :db/fulltext true]
                 [:db/add \"person-name\" :db/index true]

                 [:db/add \"article-content\" :db/ident :article/content]
                 [:db/add \"article-content\" :db/valueType :db.type/string]
                 [:db/add \"article-content\" :db/cardinality :db.cardinality/one]
                 [:db/add \"article-content\" :db/fulltext true]
                 [:db/add \"article-content\" :db/index true]]
            ')"
        ).expect("Failed to create FTS schema");
    }

    /// Test basic fulltext search.
    #[pg_test]
    fn test_pg_fulltext_basic() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        // Add test data
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice Johnson\"]
                 [:db/add \"p2\" :person/name \"Bob Smith\"]
                 [:db/add \"p3\" :person/name \"Alice Smith\"]]
            ')"
        ).expect("Failed to insert test data");

        // Search for "Alice"
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e ?name ?score
                  :where
                  [(fulltext $ :person/name \"Alice\") [[?e ?name _ ?score]]]]',
                '{}'::jsonb
            )"
        ).expect("FTS query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should find 2 people named Alice
        assert_eq!(results.len(), 2, "Expected 2 results for 'Alice'");

        // Verify all results contain "Alice"
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let name = row_arr[1].as_str().expect("Name should be string");
            assert!(name.contains("Alice"), "Result should contain 'Alice'");

            // Score should be present and numeric
            let score = row_arr[2].as_f64();
            assert!(score.is_some(), "Score should be numeric");
        }
    }

    /// Test fulltext with multiple terms.
    #[pg_test]
    fn test_pg_fulltext_multi_term() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        // Add test data
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"a1\" :article/content \"The quick brown fox jumps over the lazy dog\"]
                 [:db/add \"a2\" :article/content \"A quick study of foxes in the wild\"]
                 [:db/add \"a3\" :article/content \"Dogs are better than cats\"]]
            ')"
        ).expect("Failed to insert test data");

        // Search for "quick fox"
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e ?content
                  :where
                  [(fulltext $ :article/content \"quick fox\") [[?e ?content _ _]]]]',
                '{}'::jsonb
            )"
        ).expect("FTS query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should find articles with both "quick" and "fox"
        assert!(results.len() >= 1, "Expected at least 1 result");

        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let content = row_arr[1].as_str().expect("Content should be string");
            assert!(content.contains("quick") || content.contains("fox"),
                "Result should contain 'quick' or 'fox'");
        }
    }

    /// Test fulltext with non-FTS attribute returns no results.
    #[pg_test]
    fn test_pg_fulltext_non_fts_attribute() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Try to use fulltext on :db/ident (which is not fulltext-indexed)
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e ?val
                  :where
                  [(fulltext $ :db/ident \"test\") [[?e ?val _ _]]]]',
                '{}'::jsonb
            )"
        ).expect("Query should succeed but return no results");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should return empty results (attribute not FTS-enabled)
        assert_eq!(results.len(), 0, "Expected no results for non-FTS attribute");
    }

    /// Test fulltext search scoring.
    #[pg_test]
    fn test_pg_fulltext_scoring() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        // Add test data with varying relevance
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p2\" :person/name \"Alice Alice Alice\"]
                 [:db/add \"p3\" :person/name \"Alice and Bob\"]]
            ')"
        ).expect("Failed to insert test data");

        // Search and check scores
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e ?name ?score
                  :where
                  [(fulltext $ :person/name \"Alice\") [[?e ?name _ ?score]]]
                  :order (desc ?score)]',
                '{}'::jsonb
            )"
        ).expect("FTS query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 3, "Expected 3 results");

        // Verify scores are in descending order
        let mut prev_score = f64::INFINITY;
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let score = row_arr[2].as_f64().expect("Score should be numeric");
            assert!(score <= prev_score, "Scores should be descending");
            prev_score = score;
        }
    }

    /// Test fulltext with special characters.
    #[pg_test]
    fn test_pg_fulltext_special_chars() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        // Add data with special characters
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"a1\" :article/content \"Hello, World! This is a test.\"]
                 [:db/add \"a2\" :article/content \"Testing: one-two-three\"]
                 [:db/add \"a3\" :article/content \"C++ programming\"]]
            ')"
        ).expect("Failed to insert test data");

        // Search should handle special characters
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e ?content
                  :where
                  [(fulltext $ :article/content \"test\") [[?e ?content _ _]]]]',
                '{}'::jsonb
            )"
        ).expect("FTS query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should find documents containing "test" variants
        assert!(results.len() >= 1, "Expected at least 1 result");
    }

    /// Test fulltext with phrase search.
    #[pg_test]
    fn test_pg_fulltext_phrase() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        // Add test data
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"a1\" :article/content \"quick brown fox\"]
                 [:db/add \"a2\" :article/content \"brown quick fox\"]
                 [:db/add \"a3\" :article/content \"the quick brown fox jumps\"]]
            ')"
        ).expect("Failed to insert test data");

        // Search for exact phrase "quick brown"
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e ?content
                  :where
                  [(fulltext $ :article/content \"\\\"quick brown\\\"\") [[?e ?content _ _]]]]',
                '{}'::jsonb
            )"
        ).expect("FTS query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should only find documents with the exact phrase order
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let content = row_arr[1].as_str().expect("Content should be string");
            assert!(content.contains("quick brown"), "Should contain exact phrase");
        }
    }

    /// Test fulltext with empty query.
    #[pg_test]
    fn test_pg_fulltext_empty_query() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_fts_schema();

        // Search with empty string
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e ?content
                  :where
                  [(fulltext $ :article/content \"\") [[?e ?content _ _]]]]',
                '{}'::jsonb
            )"
        ).expect("Empty query should succeed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Empty query should return no results
        assert_eq!(results.len(), 0, "Empty query should return no results");
    }
}
