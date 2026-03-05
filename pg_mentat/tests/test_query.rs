// Copyright 2026
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! Query tests ported from /tests/query.rs
//!
//! These tests validate core datalog query functionality against PostgreSQL.

use pgrx::prelude::*;

#[path = "test_common.rs"]
mod common;

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;
    use crate::common::{setup_test_db, bootstrap_schema, query};

    /// Test basic relational query (Rel result type).
    ///
    /// Equivalent to test_rel() in /tests/query.rs
    #[pg_test]
    fn test_pg_rel() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Query all idents from schema
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?x ?ident :where [?x :db/ident ?ident]]',
                '{}'::jsonb
            )"
        )
        .expect("Query failed");

        // Parse JSON result
        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        // Check structure
        assert!(json.get("columns").is_some(), "Missing columns");
        assert!(json.get("results").is_some(), "Missing results");

        let results = json["results"].as_array()
            .expect("results should be array");

        // Should have at least the 10 core schema attributes
        assert!(results.len() >= 10, "Expected at least 10 schema idents, got {}", results.len());

        // Each row should have 2 values (entity ID and ident)
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            assert_eq!(row_arr.len(), 2, "Expected 2 values per row");
        }
    }

    /// Test scalar query that returns no results.
    ///
    /// Equivalent to test_failing_scalar() in /tests/query.rs
    #[pg_test]
    fn test_pg_failing_scalar() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Query for fulltext attributes (none should exist yet)
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?x . :where [?x :db/fulltext true]]',
                '{}'::jsonb
            )"
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        // Scalar query with no results should return null
        assert!(json["result"].is_null(), "Expected null for failing scalar query");
    }

    /// Test scalar query that succeeds.
    ///
    /// Equivalent to test_scalar() in /tests/query.rs
    #[pg_test]
    fn test_pg_scalar() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Query for the ident of entity 1 (should be :db/ident)
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?ident . :where [1 :db/ident ?ident]]',
                '{}'::jsonb
            )"
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        // Should return a single keyword value
        let keyword = json["result"].as_str()
            .expect("Expected string result");

        assert_eq!(keyword, ":db/ident", "Expected :db/ident");
    }

    /// Test tuple query.
    ///
    /// Equivalent to test_tuple() in /tests/query.rs
    #[pg_test]
    fn test_pg_tuple() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Query for entity 1's ident and value type
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find [?ident ?type] :where [1 :db/ident ?ident] [1 :db/valueType ?type]]',
                '{}'::jsonb
            )"
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        // Tuple query returns an array
        let tuple = json["result"].as_array()
            .expect("Expected array result");

        assert_eq!(tuple.len(), 2, "Expected 2-tuple");
        assert_eq!(tuple[0].as_str().expect("First element should be string"), ":db/ident");
    }

    /// Test collection query.
    ///
    /// Equivalent to test_coll() in /tests/query.rs
    #[pg_test]
    fn test_pg_coll() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Query for all ident values
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find [?ident ...] :where [?e :db/ident ?ident]]',
                '{}'::jsonb
            )"
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        // Collection query returns an array of single values
        let coll = json["result"].as_array()
            .expect("Expected array result");

        assert!(coll.len() >= 10, "Expected at least 10 idents");

        // All elements should be strings (keywords)
        for elem in coll {
            assert!(elem.is_string(), "Collection element should be string");
        }
    }

    /// Test query with input parameters.
    ///
    /// Tests query inputs binding.
    #[pg_test]
    fn test_pg_query_with_inputs() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Add test data
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"person1\" :person/name \"Alice\"]
                 [:db/add \"person1\" :person/age 30]]
            ')"
        ).expect("Transaction failed");

        // Query with input parameter
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e :in $ ?name :where [?e :person/name ?name]]',
                '{\"inputs\": [\"Alice\"]}'::jsonb
            )"
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 1, "Expected 1 result");
    }

    /// Test query with multiple clauses.
    #[pg_test]
    fn test_pg_multi_clause() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Query with join
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e ?ident ?type
                  :where
                  [?e :db/ident ?ident]
                  [?e :db/valueType ?type]]',
                '{}'::jsonb
            )"
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should have results for attributes with both ident and valueType
        assert!(results.len() >= 5, "Expected at least 5 results");

        // Each row should have 3 values
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            assert_eq!(row_arr.len(), 3);
        }
    }

    /// Test query with negation.
    #[pg_test]
    fn test_pg_query_not() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Find all entities that don't have :db/fulltext set to true
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e
                  :where
                  [?e :db/ident]
                  (not [?e :db/fulltext true])]',
                '{}'::jsonb
            )"
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Most schema attributes shouldn't have fulltext enabled
        assert!(results.len() >= 8, "Expected at least 8 non-fulltext attributes");
    }

    /// Test query with OR clause.
    #[pg_test]
    fn test_pg_query_or() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Find entities with specific idents
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e
                  :where
                  (or [?e :db/ident :db/ident]
                      [?e :db/ident :db/valueType])]',
                '{}'::jsonb
            )"
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 2, "Expected exactly 2 results");
    }

    /// Test query ordering.
    #[pg_test]
    fn test_pg_query_order() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Query with explicit ordering
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e ?ident
                  :where [?e :db/ident ?ident]
                  :order (asc ?e)]',
                '{}'::jsonb
            )"
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Verify ordering: entity IDs should be ascending
        let mut prev_id: i64 = 0;
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let current_id = row_arr[0].as_i64().expect("First element should be int");
            assert!(current_id > prev_id, "Results should be ordered ascending");
            prev_id = current_id;
        }
    }

    /// Test query with limit.
    #[pg_test]
    fn test_pg_query_limit() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Query with limit
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query(
                '[:find ?e ?ident
                  :where [?e :db/ident ?ident]
                  :limit 5]',
                '{}'::jsonb
            )"
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 5, "Expected exactly 5 results due to limit");
    }
}
