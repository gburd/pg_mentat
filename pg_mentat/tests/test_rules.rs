// Copyright 2026
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! Rules and recursive query tests ported from /query-algebrizer/tests/rules.rs
//!
//! Tests validation of rule-based queries against PostgreSQL.

use pgrx::prelude::*;

#[path = "test_common.rs"]
mod common;

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;
    use crate::common::{setup_test_db, bootstrap_schema, transact, query};

    /// Setup test schema for family relationships.
    fn setup_family_schema() {
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"parent\" :db/ident :family/parent]
                 [:db/add \"parent\" :db/valueType :db.type/ref]
                 [:db/add \"parent\" :db/cardinality :db.cardinality/many]

                 [:db/add \"child\" :db/ident :family/child]
                 [:db/add \"child\" :db/valueType :db.type/ref]
                 [:db/add \"child\" :db/cardinality :db.cardinality/many]

                 [:db/add \"name\" :db/ident :person/name]
                 [:db/add \"name\" :db/valueType :db.type/string]
                 [:db/add \"name\" :db/cardinality :db.cardinality/one]]
            ')"
        ).expect("Failed to create family schema");
    }

    /// Setup sample family data.
    fn setup_family_data() {
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"grandma\" :person/name \"Grandma\"]
                 [:db/add \"mom\" :person/name \"Mom\"]
                 [:db/add \"dad\" :person/name \"Dad\"]
                 [:db/add \"child1\" :person/name \"Alice\"]
                 [:db/add \"child2\" :person/name \"Bob\"]

                 [:db/add \"grandma\" :family/child \"mom\"]
                 [:db/add \"mom\" :family/child \"child1\"]
                 [:db/add \"mom\" :family/child \"child2\"]
                 [:db/add \"dad\" :family/child \"child1\"]
                 [:db/add \"dad\" :family/child \"child2\"]]
            ')"
        ).expect("Failed to insert family data");
    }

    /// Test basic rule definition and invocation.
    #[pg_test]
    fn test_pg_simple_rule() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_family_schema();
        setup_family_data();

        // Define a simple rule: parent(P, C) if P has child C
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?parent-name ?child-name
                 :in $
                 :where
                 [?p :family/child ?c]
                 [?p :person/name ?parent-name]
                 [?c :person/name ?child-name]]
            ', '{}'::jsonb)"
        ).expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should find parent-child relationships
        assert!(results.len() >= 3, "Expected at least 3 parent-child pairs");
    }

    /// Test recursive rule for ancestors.
    #[pg_test]
    fn test_pg_recursive_rule() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_family_schema();
        setup_family_data();

        // Use recursive CTE to find all ancestors
        // This tests that PostgreSQL recursive queries work correctly
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?ancestor-name ?descendant-name
                 :with
                 [[(ancestor ?a ?d)
                   [?a :family/child ?d]]
                  [(ancestor ?a ?d)
                   [?a :family/child ?x]
                   (ancestor ?x ?d)]]
                 :where
                 (ancestor ?anc ?desc)
                 [?anc :person/name ?ancestor-name]
                 [?desc :person/name ?descendant-name]]
            ', '{}'::jsonb)"
        ).expect("Recursive query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should find all ancestor-descendant relationships including transitive
        assert!(results.len() >= 2, "Expected at least 2 ancestor relationships");

        // Verify grandma -> grandchild relationship exists
        let has_grandma_to_alice = results.iter().any(|row| {
            let row_arr = row.as_array().unwrap();
            row_arr[0].as_str() == Some("Grandma") &&
            row_arr[1].as_str() == Some("Alice")
        });

        assert!(has_grandma_to_alice, "Should find Grandma -> Alice relationship");
    }

    /// Test rule with multiple clauses.
    #[pg_test]
    fn test_pg_rule_multi_clause() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_family_schema();
        setup_family_data();

        // Find siblings: share at least one parent
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?sib1-name ?sib2-name
                 :where
                 [?p :family/child ?s1]
                 [?p :family/child ?s2]
                 [(< ?s1 ?s2)]
                 [?s1 :person/name ?sib1-name]
                 [?s2 :person/name ?sib2-name]]
            ', '{}'::jsonb)"
        ).expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Alice and Bob are siblings
        assert!(results.len() >= 1, "Expected at least 1 sibling pair");
    }

    /// Test rule with built-in predicates.
    #[pg_test]
    fn test_pg_rule_with_predicates() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Add age data
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"age-attr\" :db/ident :person/age]
                 [:db/add \"age-attr\" :db/valueType :db.type/long]
                 [:db/add \"age-attr\" :db/cardinality :db.cardinality/one]

                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 25]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 30]
                 [:db/add \"p3\" :person/name \"Charlie\"]
                 [:db/add \"p3\" :person/age 35]]
            ')"
        ).expect("Failed to insert age data");

        // Find people over 28
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?name ?age
                 :where
                 [?p :person/name ?name]
                 [?p :person/age ?age]
                 [(> ?age 28)]]
            ', '{}'::jsonb)"
        ).expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 2, "Expected 2 people over 28");

        // Verify all ages are > 28
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let age = row_arr[1].as_i64().expect("Age should be integer");
            assert!(age > 28, "Age should be > 28");
        }
    }

    /// Test rule with negation.
    #[pg_test]
    fn test_pg_rule_negation() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_family_schema();
        setup_family_data();

        // Find people who are not parents
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?name
                 :where
                 [?p :person/name ?name]
                 (not [?p :family/child _])]
            ', '{}'::jsonb)"
        ).expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Alice and Bob are not parents
        assert_eq!(results.len(), 2, "Expected 2 non-parents");
    }

    /// Test rule with aggregation.
    #[pg_test]
    fn test_pg_rule_aggregation() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_family_schema();
        setup_family_data();

        // Count children per parent
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?parent-name (count ?child)
                 :where
                 [?p :family/child ?child]
                 [?p :person/name ?parent-name]]
            ', '{}'::jsonb)"
        ).expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should have counts for each parent
        assert!(results.len() >= 2, "Expected at least 2 parents with child counts");

        // Verify count values are numeric
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let count = row_arr[1].as_i64();
            assert!(count.is_some(), "Count should be numeric");
            assert!(count.unwrap() > 0, "Count should be positive");
        }
    }

    /// Test rule with OR clause.
    #[pg_test]
    fn test_pg_rule_or() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Add test data
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"role-attr\" :db/ident :person/role]
                 [:db/add \"role-attr\" :db/valueType :db.type/string]
                 [:db/add \"role-attr\" :db/cardinality :db.cardinality/one]

                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/role \"admin\"]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/role \"user\"]
                 [:db/add \"p3\" :person/name \"Charlie\"]
                 [:db/add \"p3\" :person/role \"moderator\"]]
            ')"
        ).expect("Failed to insert test data");

        // Find admins or moderators
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?name ?role
                 :where
                 [?p :person/name ?name]
                 [?p :person/role ?role]
                 (or [[?p :person/role \"admin\"]]
                     [[?p :person/role \"moderator\"]])]
            ', '{}'::jsonb)"
        ).expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 2, "Expected 2 results (admin and moderator)");
    }

    /// Test rule with bind clause.
    #[pg_test]
    fn test_pg_rule_bind() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Add test data
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 25]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 30]]
            ')"
        ).expect("Failed to insert test data");

        // Bind a computed value
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?name ?double-age
                 :where
                 [?p :person/name ?name]
                 [?p :person/age ?age]
                 [(* ?age 2) ?double-age]]
            ', '{}'::jsonb)"
        ).expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        assert_eq!(results.len(), 2, "Expected 2 results");

        // Verify doubled ages
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let double_age = row_arr[1].as_i64().expect("Double age should be integer");
            assert!(double_age >= 50, "Doubled age should be at least 50");
        }
    }
}
