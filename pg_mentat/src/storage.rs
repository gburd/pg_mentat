// Copyright 2026
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! PostgreSQL storage backend for Mentat using pgrx SPI.
//!
//! This module provides storage operations for the Mentat datalog database running
//! inside PostgreSQL as an extension. It exposes functionality via SQL functions
//! that can be called from Datalog queries or directly from SQL.
//!
//! ## Phase 1 - Core Operations
//!
//! This initial implementation provides:
//! - Entity ID allocation from partitions
//! - Simple entity queries by attribute/value
//! - Basic transaction begin/commit
//! - Helper function wrapping for schema operations

use pgrx::prelude::*;

/// Allocate a new entity ID from the specified partition.
///
/// Wraps the `mentat.allocate_entid(partition_name TEXT)` PostgreSQL function.
///
/// # Example
/// ```sql
/// SELECT mentat.alloc_entid('db.part/user');
/// ```
#[pg_extern]
fn alloc_entid(partition_name: &str) -> Result<i64, Box<dyn std::error::Error>> {
    let query = format!("SELECT mentat.allocate_entid('{}')", partition_name);

    Spi::connect(|client| {
        let result = client.select(&query, None, None)?;

        if let Some(row) = result.first() {
            let entid: i64 = row.get(1)?
                .ok_or_else(|| "allocate_entid returned NULL")?;
            Ok(entid)
        } else {
            Err("allocate_entid returned no rows".into())
        }
    })
}

/// Resolve a keyword ident to its entity ID.
///
/// Wraps the `mentat.resolve_ident(keyword TEXT)` PostgreSQL function.
///
/// # Example
/// ```sql
/// SELECT mentat.resolve_ident_to_entid(':db/ident');
/// ```
#[pg_extern]
fn resolve_ident_to_entid(ident: &str) -> Result<Option<i64>, Box<dyn std::error::Error>> {
    let query = format!("SELECT mentat.resolve_ident('{}')", ident);

    Spi::connect(|client| {
        match client.select(&query, None, None) {
            Ok(result) => {
                if let Some(row) = result.first() {
                    let entid: Option<i64> = row.get(1)?;
                    Ok(entid)
                } else {
                    Ok(None)
                }
            }
            Err(_) => Ok(None),
        }
    })
}

/// Look up an entity by a unique attribute value.
///
/// For Phase 1, this provides basic lookup functionality.
/// Full implementation with proper value encoding in Phase 2.
///
/// # Example
/// ```sql
/// SELECT mentat.lookup_entity_by_attr(':user/email', 'alice@example.com');
/// ```
#[pg_extern]
fn lookup_entity_by_attr(
    attr_ident: &str,
    value_str: &str,
) -> Result<Option<i64>, Box<dyn std::error::Error>> {
    // Phase 1: Simple string-based lookup
    // Phase 2: Support all TypedValue types with proper encoding

    Spi::connect(|client| {
        // Query datoms table for matching entity
        let query = format!(
            "SELECT d.e FROM mentat.datoms d \
             JOIN mentat.idents i ON i.ident = '{}' \
             WHERE d.a = i.entid \
             AND d.added = true \
             AND encode(d.v, 'escape') = '{}' \
             LIMIT 1",
            attr_ident, value_str
        );

        match client.select(&query, None, None) {
            Ok(result) => {
                if let Some(row) = result.first() {
                    let entid: Option<i64> = row.get(1)?;
                    Ok(entid)
                } else {
                    Ok(None)
                }
            }
            Err(_) => Ok(None),
        }
    })
}

/// Begin a Mentat transaction.
///
/// This creates temporary tables for staging transaction datoms.
/// Phase 1: Basic table creation.
///
/// # Example
/// ```sql
/// SELECT mentat.begin_transaction();
/// ```
#[pg_extern]
fn begin_transaction() -> Result<(), Box<dyn std::error::Error>> {
    Spi::connect(|client| {
        // Create temporary tables for transaction staging
        let statements = vec![
            "DROP TABLE IF EXISTS temp_exact_searches",
            "CREATE TEMPORARY TABLE temp_exact_searches (
                e0 BIGINT NOT NULL,
                a0 BIGINT NOT NULL,
                v0 BYTEA NOT NULL,
                value_type_tag0 SMALLINT NOT NULL,
                added0 BOOLEAN NOT NULL,
                flags0 SMALLINT NOT NULL
            ) ON COMMIT DROP",

            "DROP TABLE IF EXISTS temp_inexact_searches",
            "CREATE TEMPORARY TABLE temp_inexact_searches (
                e0 BIGINT NOT NULL,
                a0 BIGINT NOT NULL,
                v0 BYTEA NOT NULL,
                value_type_tag0 SMALLINT NOT NULL,
                added0 BOOLEAN NOT NULL,
                flags0 SMALLINT NOT NULL
            ) ON COMMIT DROP",

            "DROP TABLE IF EXISTS temp_search_results",
            "CREATE TEMPORARY TABLE temp_search_results (
                e0 BIGINT NOT NULL,
                a0 BIGINT NOT NULL,
                v0 BYTEA NOT NULL,
                value_type_tag0 SMALLINT NOT NULL,
                added0 BOOLEAN NOT NULL,
                flags0 SMALLINT NOT NULL,
                search_type TEXT NOT NULL,
                rid BIGINT,
                v BYTEA
            ) ON COMMIT DROP",
        ];

        for stmt in statements {
            client.update(stmt, None, None)?;
        }

        Ok(())
    })
}

/// Commit a Mentat transaction.
///
/// This finalizes the transaction by:
/// 1. Applying staged datoms to the datoms table
/// 2. Recording the transaction in the transactions table
///
/// Phase 1: Simplified commit without full conflict resolution.
///
/// # Example
/// ```sql
/// SELECT mentat.commit_transaction(12345678);
/// ```
#[pg_extern]
fn commit_transaction(tx_id: i64) -> Result<(), Box<dyn std::error::Error>> {
    Spi::connect(|client| {
        // Phase 1: Simplified commit
        // Phase 2: Add full conflict detection and resolution

        // Insert new datoms from staged searches
        let insert_query = format!(
            "INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag) \
             SELECT e0, a0, v0, {}, added0, value_type_tag0 \
             FROM temp_exact_searches \
             WHERE added0 = true",
            tx_id
        );

        client.update(&insert_query, None, None)?;

        // Record transaction
        let tx_query = format!(
            "INSERT INTO mentat.transactions (tx_id, instant) \
             VALUES ({}, CURRENT_TIMESTAMP)",
            tx_id
        );

        client.update(&tx_query, None, None)?;

        Ok(())
    })
}

/// Get all datoms for an entity.
///
/// Returns the current state of an entity (not historical).
/// Phase 1: Basic entity retrieval.
///
/// # Example
/// ```sql
/// SELECT * FROM mentat.get_entity_datoms(12345);
/// ```
#[pg_extern]
fn get_entity_datoms(
    entity_id: i64,
) -> Result<
    TableIterator<'static, (name!(attribute, i64), name!(value, Vec<u8>), name!(value_type, i16), name!(transaction, i64))>,
    Box<dyn std::error::Error>
> {
    let query = format!(
        "SELECT a, v, value_type_tag, tx \
         FROM mentat.datoms \
         WHERE e = {} AND added = true \
         ORDER BY a, tx DESC",
        entity_id
    );

    Ok(TableIterator::new(Spi::connect(|client| {
        let result = client.select(&query, None, None)?;

        let rows: Vec<_> = result.into_iter()
            .filter_map(|row| {
                let a: i64 = row.get(1).ok()??;
                let v: Vec<u8> = row.get(2).ok()??;
                let value_type: i16 = row.get(3).ok()??;
                let tx: i64 = row.get(4).ok()??;
                Some((a, v, value_type, tx))
            })
            .collect();

        Ok(rows)
    })?))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require PostgreSQL with the mentat schema installed
    // They should be run via `cargo pgrx test`

    #[pg_test]
    fn test_alloc_entid_basic() {
        // Test entity ID allocation
        // This will fail until schema is installed
        let result = alloc_entid("db.part/user");
        assert!(result.is_ok());
    }
}
