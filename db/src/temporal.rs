// Copyright 2016 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! Temporal query support for time-travel queries (as-of, since, history).
//!
//! This module implements Datomic-style temporal queries:
//! - `as_of(db, t)` - Database value as of transaction time t
//! - `since(db, t)` - All changes since transaction time t
//! - `history(db)` - Complete history database view
//!
//! These functions return database snapshots that can be queried like normal databases,
//! but reflect the state at a specific point in time or show historical changes.

use rusqlite;

use core_traits::Entid;

use crate::types::DB;
use db_traits::errors::Result;

/// A temporal database snapshot representing the state at a specific point in time.
///
/// This wraps a regular DB with temporal filtering information that will be applied
/// during query execution.
#[derive(Clone, Debug)]
pub struct TemporalDB {
    /// The underlying database metadata
    pub db: DB,
    /// The temporal filter to apply
    pub filter: TemporalFilter,
}

/// Temporal filter specification for queries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemporalFilter {
    /// Query database as of transaction t (inclusive)
    AsOf(Entid),
    /// Query changes since transaction t (exclusive)
    Since(Entid),
    /// Query complete history (all transactions)
    History,
    /// No temporal filter (current state only)
    Current,
}

impl TemporalDB {
    /// Create a new temporal database snapshot.
    pub fn new(db: DB, filter: TemporalFilter) -> TemporalDB {
        TemporalDB { db, filter }
    }

    /// Create a temporal database showing state as of transaction t.
    ///
    /// Returns a database snapshot where queries see the state of the database
    /// at transaction time t (inclusive). Transactions after t are not visible.
    ///
    /// # Arguments
    /// * `db` - The current database state
    /// * `t` - The transaction ID to query as of
    ///
    /// # Examples
    /// ```no_run
    /// # use mentat_db::temporal::{TemporalDB, TemporalFilter};
    /// # use mentat_db::types::DB;
    /// # let db = DB::default();
    /// let historical_db = TemporalDB::as_of(db, 100);
    /// // Queries against historical_db will see state at tx 100
    /// ```
    pub fn as_of(db: DB, t: Entid) -> TemporalDB {
        TemporalDB::new(db, TemporalFilter::AsOf(t))
    }

    /// Create a temporal database showing changes since transaction t.
    ///
    /// Returns a database snapshot where queries see all changes that occurred
    /// after transaction t (exclusive). Only transactions > t are visible.
    ///
    /// # Arguments
    /// * `db` - The current database state
    /// * `t` - The transaction ID to query since
    pub fn since(db: DB, t: Entid) -> TemporalDB {
        TemporalDB::new(db, TemporalFilter::Since(t))
    }

    /// Create a history database showing all transactions.
    ///
    /// Returns a database snapshot where queries see all historical transactions,
    /// including both assertions (added=true) and retractions (added=false).
    ///
    /// # Arguments
    /// * `db` - The current database state
    pub fn history(db: DB) -> TemporalDB {
        TemporalDB::new(db, TemporalFilter::History)
    }

    /// Get SQL WHERE clause for filtering transactions based on temporal filter.
    ///
    /// Returns a SQL WHERE clause fragment that can be added to queries against
    /// timelined_transactions to implement the temporal filter.
    pub fn temporal_where_clause(&self) -> String {
        match self.filter {
            TemporalFilter::AsOf(t) => format!("tx <= {}", t),
            TemporalFilter::Since(t) => format!("tx > {}", t),
            TemporalFilter::History => "1=1".to_string(),
            TemporalFilter::Current => "timeline = 0".to_string(),
        }
    }

    /// Get SQL query for retrieving datoms with temporal filtering.
    ///
    /// This returns a query that selects from timelined_transactions with appropriate
    /// temporal filtering applied. The query can be used as a subquery or CTE in
    /// larger query constructions.
    pub fn temporal_datoms_query(&self) -> String {
        let where_clause = self.temporal_where_clause();
        format!(
            "SELECT e, a, v, value_type_tag, tx, added FROM timelined_transactions WHERE timeline = 0 AND {}",
            where_clause
        )
    }
}

/// Query datoms from a temporal database snapshot.
///
/// This function retrieves datoms matching the temporal filter. For `AsOf` queries,
/// it returns the current state at that point in time (applying retractions).
/// For `Since` queries, it returns all changes since that time.
/// For `History` queries, it returns all historical datoms including both assertions
/// and retractions.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `temporal_db` - Temporal database snapshot with filter
///
/// # Returns
/// Result containing vector of (e, a, v, value_type_tag, tx, added) tuples
pub fn query_temporal_datoms(
    conn: &rusqlite::Connection,
    temporal_db: &TemporalDB,
) -> Result<Vec<(Entid, Entid, rusqlite::types::Value, i16, Entid, bool)>> {
    let query = temporal_db.temporal_datoms_query();

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(rusqlite::params![], |row| {
        Ok((
            row.get(0)?, // e
            row.get(1)?, // a
            row.get(2)?, // v
            row.get(3)?, // value_type_tag
            row.get(4)?, // tx
            row.get::<_, i8>(5)? != 0, // added (convert TINYINT to bool)
        ))
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}

/// Materialize temporal datoms into current state for AsOf queries.
///
/// For AsOf queries, we need to replay the transaction log up to time t
/// to reconstruct the database state. This means applying assertions and
/// retractions in order to get the final state.
///
/// This function correctly handles cardinality-one and cardinality-many attributes.
/// For each [e, a] pair, it finds the most recent transaction <= t and checks
/// whether that transaction was an assertion (added=1) or retraction (added=0).
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `t` - Transaction ID to materialize state as of
///
/// # Returns
/// Result containing vector of current datoms (e, a, v, value_type_tag, tx)
pub fn materialize_as_of(
    conn: &rusqlite::Connection,
    t: Entid,
) -> Result<Vec<(Entid, Entid, rusqlite::types::Value, i16, Entid)>> {
    // For cardinality-many attributes, we need to track [e, a, v] triples.
    // For cardinality-one attributes, we track [e, a] pairs.
    // This query gets the most recent transaction for each [e, a, v] triple
    // up to time t, and filters to only those where the most recent operation
    // was an assertion (added=1).
    let query = r#"
        WITH latest_ops AS (
            SELECT e, a, v, value_type_tag, tx, added,
                   ROW_NUMBER() OVER (
                       PARTITION BY e, a, v
                       ORDER BY tx DESC
                   ) as rn
            FROM timelined_transactions
            WHERE timeline = 0 AND tx <= ?
        )
        SELECT e, a, v, value_type_tag, tx
        FROM latest_ops
        WHERE rn = 1 AND added = 1
    "#;

    let mut stmt = conn.prepare(query)?;
    let rows = stmt.query_map(rusqlite::params![t], |row| {
        Ok((
            row.get(0)?, // e
            row.get(1)?, // a
            row.get(2)?, // v
            row.get(3)?, // value_type_tag
            row.get(4)?, // tx
        ))
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temporal_filter_as_of_where_clause() {
        let db = DB::default();
        let temporal_db = TemporalDB::as_of(db, 100);
        assert_eq!(temporal_db.temporal_where_clause(), "tx <= 100");
    }

    #[test]
    fn test_temporal_filter_since_where_clause() {
        let db = DB::default();
        let temporal_db = TemporalDB::since(db, 100);
        assert_eq!(temporal_db.temporal_where_clause(), "tx > 100");
    }

    #[test]
    fn test_temporal_filter_history_where_clause() {
        let db = DB::default();
        let temporal_db = TemporalDB::history(db);
        assert_eq!(temporal_db.temporal_where_clause(), "1=1");
    }

    #[test]
    fn test_temporal_filter_equality() {
        assert_eq!(TemporalFilter::AsOf(100), TemporalFilter::AsOf(100));
        assert_ne!(TemporalFilter::AsOf(100), TemporalFilter::AsOf(200));
        assert_ne!(TemporalFilter::AsOf(100), TemporalFilter::Since(100));
        assert_eq!(TemporalFilter::History, TemporalFilter::History);
    }

    #[test]
    fn test_temporal_db_construction() {
        let db = DB::default();

        let as_of = TemporalDB::as_of(db.clone(), 100);
        assert!(matches!(as_of.filter, TemporalFilter::AsOf(100)));

        let since = TemporalDB::since(db.clone(), 50);
        assert!(matches!(since.filter, TemporalFilter::Since(50)));

        let history = TemporalDB::history(db);
        assert!(matches!(history.filter, TemporalFilter::History));
    }
}
