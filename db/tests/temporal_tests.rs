// Copyright 2016 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! Integration tests for temporal queries (as-of, since, history).

#[macro_use]
extern crate mentat_db;

#[macro_use]
extern crate log;

use std::borrow::Borrow;

use mentat_db::debug::TestConn;
use mentat_db::temporal::{TemporalDB, TemporalFilter, materialize_as_of, query_temporal_datoms};
use mentat_db::types::DB;

#[test]
fn test_as_of_single_transaction() {
    let mut conn = TestConn::default();
    conn.sanitized_partition_map();

    // Transact some initial data
    let t = r#"
        [{:db/id :db/doc :db/doc "initial doc"}]
    "#;

    let report1 = assert_transact!(conn, t);
    let tx1 = report1.tx_id;

    // Verify current state
    assert_matches!(
        conn.datoms(),
        r#"[[37 :db/doc "initial doc"]]"#
    );

    // Now update the value
    let t2 = r#"
        [{:db/id :db/doc :db/doc "updated doc"}]
    "#;

    let report2 = assert_transact!(conn, t2);
    let tx2 = report2.tx_id;

    // Verify updated state
    assert_matches!(
        conn.datoms(),
        r#"[[37 :db/doc "updated doc"]]"#
    );

    // Query as-of tx1 should show initial value
    let db = DB::new(conn.partition_map.clone(), conn.schema.clone());
    let temporal_db = TemporalDB::as_of(db, tx1);
    let datoms = materialize_as_of(&conn.sqlite, tx1).expect("as-of query failed");

    // Should have the initial value
    assert!(datoms.iter().any(|(e, a, v, _, tx)| {
        *e == 37 && *tx <= tx1
    }));

    // Query as-of tx2 should show updated value
    let datoms_tx2 = materialize_as_of(&conn.sqlite, tx2).expect("as-of query failed");
    assert!(datoms_tx2.iter().any(|(e, a, v, _, tx)| {
        *e == 37 && *tx <= tx2
    }));
}

#[test]
fn test_since_shows_only_recent_changes() {
    let mut conn = TestConn::default();
    conn.sanitized_partition_map();

    // Initial schema transaction
    assert_transact!(
        conn,
        r#"[{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]"#
    );
    let schema_tx = conn.last_tx_id();

    // Add first entity
    assert_transact!(conn, r#"[{:person/name "Alice"}]"#);
    let tx1 = conn.last_tx_id();

    // Add second entity
    assert_transact!(conn, r#"[{:person/name "Bob"}]"#);
    let tx2 = conn.last_tx_id();

    // Query since tx1 should only show Bob, not Alice
    let db = DB::new(conn.partition_map.clone(), conn.schema.clone());
    let temporal_db = TemporalDB::since(db, tx1);
    let datoms = query_temporal_datoms(&conn.sqlite, &temporal_db).expect("since query failed");

    // Should only have changes after tx1 (i.e., Bob)
    let has_bob = datoms.iter().any(|(_, _, _, _, tx, _)| *tx == tx2);
    let has_alice = datoms.iter().any(|(_, _, _, _, tx, _)| *tx == tx1);

    assert!(has_bob, "Should include Bob from tx2");
    assert!(!has_alice, "Should NOT include Alice from tx1 (since is exclusive)");
}

#[test]
fn test_history_shows_all_transactions() {
    let mut conn = TestConn::default();
    conn.sanitized_partition_map();

    // Schema
    assert_transact!(
        conn,
        r#"[{:db/ident :item/count :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]"#
    );

    // Initial value
    assert_transact!(conn, r#"[[:db/add "e" :item/count 1]]"#);
    let tx1 = conn.last_tx_id();

    // Update value
    assert_transact!(conn, r#"[[:db/add 65536 :item/count 2]]"#);
    let tx2 = conn.last_tx_id();

    // Update again
    assert_transact!(conn, r#"[[:db/add 65536 :item/count 3]]"#);
    let tx3 = conn.last_tx_id();

    // Query history - should show all three values plus retractions
    let db = DB::new(conn.partition_map.clone(), conn.schema.clone());
    let temporal_db = TemporalDB::history(db);
    let datoms = query_temporal_datoms(&conn.sqlite, &temporal_db).expect("history query failed");

    // History should include all transactions
    let tx_ids: Vec<i64> = datoms.iter().map(|(_, _, _, _, tx, _)| *tx).collect();

    assert!(tx_ids.contains(&tx1), "History should include tx1");
    assert!(tx_ids.contains(&tx2), "History should include tx2");
    assert!(tx_ids.contains(&tx3), "History should include tx3");

    // History should include both assertions (added=true) and retractions (added=false)
    let has_assertions = datoms.iter().any(|(_, _, _, _, _, added)| *added);
    let has_retractions = datoms.iter().any(|(_, _, _, _, _, added)| !*added);

    assert!(has_assertions, "History should include assertions");
    assert!(has_retractions, "History should include retractions");
}

#[test]
fn test_as_of_with_retractions() {
    let mut conn = TestConn::default();
    conn.sanitized_partition_map();

    // Schema
    assert_transact!(
        conn,
        r#"[{:db/ident :item/tag :db/valueType :db.type/string :db/cardinality :db.cardinality/many}]"#
    );

    // Add multiple tags
    assert_transact!(
        conn,
        r#"[[:db/add "e" :item/tag "rust"]
            [:db/add "e" :item/tag "database"]]"#
    );
    let tx1 = conn.last_tx_id();

    // Count current tags (should be 2)
    let current_datoms = conn.datoms();
    let tag_count_initial = current_datoms.0.len() - 3; // Subtract schema attributes
    assert!(tag_count_initial >= 2, "Should have at least 2 tags initially");

    // Retract one tag using entity ID 65537 (the first user entity)
    assert_transact!(conn, r#"[[:db/retract 65537 :item/tag "rust"]]"#);
    let tx2 = conn.last_tx_id();

    // Current state should have fewer tags
    let current_datoms_after = conn.datoms();
    let tag_count_after = current_datoms_after.0.len() - 3; // Subtract schema attributes
    assert_eq!(tag_count_after, tag_count_initial - 1, "Should have 1 less tag after retraction");

    // Query as-of tx1 - should have both tag values
    let datoms_tx1 = materialize_as_of(&conn.sqlite, tx1).expect("as-of query failed");
    let tags_tx1: Vec<_> = datoms_tx1.iter()
        .filter(|(e, _, _, _, _)| *e == 65537)
        .collect();

    // Query as-of tx2 - should have only one tag value
    let datoms_tx2 = materialize_as_of(&conn.sqlite, tx2).expect("as-of query failed");
    let tags_tx2: Vec<_> = datoms_tx2.iter()
        .filter(|(e, _, _, _, _)| *e == 65537)
        .collect();

    // At tx1 we should have 2 tags (rust and database)
    assert_eq!(tags_tx1.len(), 2, "Should have 2 tags at tx1");

    // At tx2 we should have 1 tag (database only, rust retracted)
    assert_eq!(tags_tx2.len(), 1, "Should have 1 tag at tx2");

    // Verify the retracted tag is not present at tx2
    let has_rust_tx1 = tags_tx1.iter().any(|(_, _, v, _, _)| {
        matches!(v, rusqlite::types::Value::Text(ref s) if s == "rust")
    });
    let has_rust_tx2 = tags_tx2.iter().any(|(_, _, v, _, _)| {
        matches!(v, rusqlite::types::Value::Text(ref s) if s == "rust")
    });

    assert!(has_rust_tx1, "Should have 'rust' tag at tx1");
    assert!(!has_rust_tx2, "Should NOT have 'rust' tag at tx2");
}

#[test]
fn test_as_of_before_first_transaction() {
    let mut conn = TestConn::default();
    conn.sanitized_partition_map();

    // Add data
    assert_transact!(
        conn,
        r#"[{:db/id :db/doc :db/doc "some doc"}]"#
    );
    let tx1 = conn.last_tx_id();

    // Query as-of before any transactions should return empty
    let datoms = materialize_as_of(&conn.sqlite, tx1 - 1000).expect("as-of query failed");

    // Should not include our entity (only bootstrap schema entities)
    let has_our_entity = datoms.iter().any(|(e, _, _, _, _)| *e == 37);
    assert!(!has_our_entity, "Should not have entity 37 before it was transacted");
}

#[test]
fn test_as_of_future_transaction() {
    let mut conn = TestConn::default();
    conn.sanitized_partition_map();

    // Add data
    assert_transact!(
        conn,
        r#"[{:db/id :db/doc :db/doc "some doc"}]"#
    );
    let tx1 = conn.last_tx_id();

    // Query as-of future transaction should show current state
    let datoms = materialize_as_of(&conn.sqlite, tx1 + 1000).expect("as-of query failed");

    // Should include our entity
    let has_our_entity = datoms.iter().any(|(e, _, _, _, _)| *e == 37);
    assert!(has_our_entity, "Should have entity 37 when querying future time");
}

#[test]
fn test_temporal_filter_equality() {
    assert_eq!(TemporalFilter::AsOf(100), TemporalFilter::AsOf(100));
    assert_ne!(TemporalFilter::AsOf(100), TemporalFilter::AsOf(200));
    assert_ne!(TemporalFilter::AsOf(100), TemporalFilter::Since(100));
    assert_eq!(TemporalFilter::History, TemporalFilter::History);
    assert_eq!(TemporalFilter::Current, TemporalFilter::Current);
}

#[test]
fn test_temporal_where_clause_generation() {
    use mentat_db::types::DB;

    let db = DB::default();

    let as_of = TemporalDB::as_of(db.clone(), 100);
    assert_eq!(as_of.temporal_where_clause(), "tx <= 100");

    let since = TemporalDB::since(db.clone(), 50);
    assert_eq!(since.temporal_where_clause(), "tx > 50");

    let history = TemporalDB::history(db.clone());
    assert_eq!(history.temporal_where_clause(), "1=1");

    let current = TemporalDB::new(db, TemporalFilter::Current);
    assert_eq!(current.temporal_where_clause(), "timeline = 0");
}
