// Copyright 2026
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! Time-travel query tests (as-of, since, history).
//!
//! Tests temporal query functionality against PostgreSQL.

use pgrx::prelude::*;

#[path = "test_common.rs"]
mod common;

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;
    use crate::common::{setup_test_db, bootstrap_schema, transact};

    /// Setup test data with multiple transactions.
    fn setup_temporal_data() -> (i64, i64, i64) {
        // Transaction 1: Initial data
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"name-attr\" :db/ident :person/name]
                 [:db/add \"name-attr\" :db/valueType :db.type/string]
                 [:db/add \"name-attr\" :db/cardinality :db.cardinality/one]

                 [:db/add \"age-attr\" :db/ident :person/age]
                 [:db/add \"age-attr\" :db/valueType :db.type/long]
                 [:db/add \"age-attr\" :db/cardinality :db.cardinality/one]

                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/age 25]]
            ')"
        ).expect("Transaction 1 failed");

        let tx1 = Spi::get_one::<i64>(
            "SELECT MAX(tx) FROM mentat.datoms"
        ).expect("Failed to get tx1").expect("tx1 is null");

        // Transaction 2: Update age
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"p1\" :person/age 26]]
            ')"
        ).expect("Transaction 2 failed");

        let tx2 = Spi::get_one::<i64>(
            "SELECT MAX(tx) FROM mentat.datoms WHERE tx > $1",
        ).expect("Failed to get tx2").expect("tx2 is null");

        // Transaction 3: Update age again and add another person
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"p1\" :person/age 27]
                 [:db/add \"p2\" :person/name \"Bob\"]
                 [:db/add \"p2\" :person/age 30]]
            ')"
        ).expect("Transaction 3 failed");

        let tx3 = Spi::get_one::<i64>(
            "SELECT MAX(tx) FROM mentat.datoms"
        ).expect("Failed to get tx3").expect("tx3 is null");

        (tx1, tx2, tx3)
    }

    /// Test as-of query to view database state at a specific transaction.
    #[pg_test]
    fn test_pg_as_of() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        let (tx1, tx2, _tx3) = setup_temporal_data();

        // Query as-of tx1 should show age 25
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_query('
                [:find ?age .
                 :where
                 [?p :person/name \"Alice\"]
                 [?p :person/age ?age]]
            ', '{{\"asOf\": {}}}'::jsonb)",
            tx1
        )).expect("as-of tx1 query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let age = json["result"].as_i64().expect("Age should be integer");
        assert_eq!(age, 25, "Age at tx1 should be 25");

        // Query as-of tx2 should show age 26
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_query('
                [:find ?age .
                 :where
                 [?p :person/name \"Alice\"]
                 [?p :person/age ?age]]
            ', '{{\"asOf\": {}}}'::jsonb)",
            tx2
        )).expect("as-of tx2 query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let age = json["result"].as_i64().expect("Age should be integer");
        assert_eq!(age, 26, "Age at tx2 should be 26");
    }

    /// Test since query to view changes after a specific transaction.
    #[pg_test]
    fn test_pg_since() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        let (tx1, _tx2, _tx3) = setup_temporal_data();

        // Query changes since tx1 should show new datoms
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_query('
                [:find ?e ?a ?v ?tx ?added
                 :where
                 [?e ?a ?v ?tx ?added]]
            ', '{{\"since\": {}}}'::jsonb)",
            tx1
        )).expect("since query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should have datoms from tx2 and tx3
        assert!(results.len() > 0, "Should have datoms since tx1");

        // All transactions should be > tx1
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            let tx = row_arr[3].as_i64().expect("TX should be integer");
            assert!(tx > tx1, "All transactions should be > tx1");
        }
    }

    /// Test history query to view all datoms including retractions.
    #[pg_test]
    fn test_pg_history() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        let (_tx1, _tx2, _tx3) = setup_temporal_data();

        // Query history for Alice's age should show all versions
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?age ?tx ?added
                 :where
                 [?p :person/name \"Alice\"]
                 [?p :person/age ?age ?tx ?added]]
            ', '{\"history\": true}'::jsonb)"
        ).expect("history query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should have multiple versions (3 assertions + 2 retractions = 5 total)
        assert!(results.len() >= 3, "Should have at least 3 age datoms (assertions)");

        // Verify we have different ages
        let ages: Vec<i64> = results.iter()
            .map(|row| {
                let row_arr = row.as_array().expect("Row should be array");
                row_arr[0].as_i64().expect("Age should be integer")
            })
            .collect();

        assert!(ages.contains(&25), "Should contain age 25");
        assert!(ages.contains(&26), "Should contain age 26");
        assert!(ages.contains(&27), "Should contain age 27");
    }

    /// Test as-of with entity that doesn't exist yet.
    #[pg_test]
    fn test_pg_as_of_future_entity() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        let (tx1, _tx2, _tx3) = setup_temporal_data();

        // Query for Bob as-of tx1 (before Bob existed)
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_query('
                [:find ?age .
                 :where
                 [?p :person/name \"Bob\"]
                 [?p :person/age ?age]]
            ', '{{\"asOf\": {}}}'::jsonb)",
            tx1
        )).expect("as-of query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        // Should return null (Bob didn't exist at tx1)
        assert!(json["result"].is_null(), "Bob should not exist at tx1");
    }

    /// Test history with retractions.
    #[pg_test]
    fn test_pg_history_retraction() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");

        // Add and retract data
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/add \"status-attr\" :db/ident :person/status]
                 [:db/add \"status-attr\" :db/valueType :db.type/string]
                 [:db/add \"status-attr\" :db/cardinality :db.cardinality/one]

                 [:db/add \"p1\" :person/name \"Alice\"]
                 [:db/add \"p1\" :person/status \"active\"]]
            ')"
        ).expect("Transaction 1 failed");

        // Retract status
        Spi::run(
            "SELECT mentat.mentat_transact('
                [[:db/retract \"p1\" :person/status \"active\"]]
            ')"
        ).expect("Retraction failed");

        // Query history
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?status ?tx ?added
                 :where
                 [?p :person/name \"Alice\"]
                 [?p :person/status ?status ?tx ?added]]
            ', '{\"history\": true}'::jsonb)"
        ).expect("history query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should have 2 datoms: one assertion (added=true) and one retraction (added=false)
        assert_eq!(results.len(), 2, "Should have assertion and retraction");

        // Check that we have both added=true and added=false
        let has_assertion = results.iter().any(|row| {
            let row_arr = row.as_array().unwrap();
            row_arr[2].as_bool().unwrap() == true
        });

        let has_retraction = results.iter().any(|row| {
            let row_arr = row.as_array().unwrap();
            row_arr[2].as_bool().unwrap() == false
        });

        assert!(has_assertion, "Should have assertion");
        assert!(has_retraction, "Should have retraction");
    }

    /// Test combining as-of with multiple clauses.
    #[pg_test]
    fn test_pg_as_of_complex() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        let (tx1, _tx2, tx3) = setup_temporal_data();

        // At tx1, only Alice should exist
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_query('
                [:find (count ?p)
                 :where
                 [?p :person/name ?name]]
            ', '{{\"asOf\": {}}}'::jsonb)",
            tx1
        )).expect("as-of count query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let count = json["result"].as_i64().expect("Count should be integer");
        assert_eq!(count, 1, "Only Alice should exist at tx1");

        // At tx3, both Alice and Bob should exist
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_query('
                [:find (count ?p)
                 :where
                 [?p :person/name ?name]]
            ', '{{\"asOf\": {}}}'::jsonb)",
            tx3
        )).expect("as-of count query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let count = json["result"].as_i64().expect("Count should be integer");
        assert_eq!(count, 2, "Both Alice and Bob should exist at tx3");
    }

    /// Test transaction metadata queries.
    #[pg_test]
    fn test_pg_tx_metadata() {
        setup_test_db().expect("Failed to setup test db");
        bootstrap_schema().expect("Failed to bootstrap schema");
        setup_temporal_data();

        // Query all transactions with their timestamps
        let result = Spi::get_one::<String>(
            "SELECT mentat.mentat_query('
                [:find ?tx ?instant
                 :where
                 [?tx :db/txInstant ?instant]]
            ', '{}'::jsonb)"
        ).expect("tx metadata query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array()
            .expect("Expected array");

        // Should have at least the transactions we created
        assert!(results.len() >= 3, "Should have at least 3 transactions");

        // All timestamps should be valid
        for row in results {
            let row_arr = row.as_array().expect("Row should be array");
            assert!(row_arr[1].is_string(), "Timestamp should be string");
        }
    }
}
