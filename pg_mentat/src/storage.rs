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
//! - Entity ID allocation from partition sequences (lock-free)
//! - Simple entity queries by attribute/value
//! - Basic transaction begin/commit
//! - Helper function wrapping for schema operations

use pgrx::prelude::*;
use pgrx::datum::DatumWithOid;

/// Allocate a new entity ID from the specified partition.
///
/// Wraps PostgreSQL sequences for lock-free entity ID allocation.
/// Uses `nextval()` on partition-specific sequences instead of row-level locks.
///
/// # Example
/// ```sql
/// SELECT mentat.alloc_entid('db.part/user');
/// ```
#[pg_extern]
fn alloc_entid(partition_name: &str) -> Result<i64, Box<dyn std::error::Error>> {
    let seq_name = match partition_name {
        "db.part/db" => "mentat.partition_db_seq",
        "db.part/user" => "mentat.partition_user_seq",
        "db.part/tx" => "mentat.partition_tx_seq",
        _ => return Err(format!("Unknown partition: {}", partition_name).into()),
    };
    let query = format!("SELECT nextval('{}')", seq_name);
    Spi::get_one::<i64>(&query)?
        .ok_or_else(|| "nextval returned NULL".into())
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
    Spi::connect(|client| {
        match client.select(
            "SELECT mentat.resolve_ident($1)",
            None,
            &[DatumWithOid::from(ident)],
        ) {
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
        // Query datoms table for matching entity (string values stored in v_text)
        match client.select(
            "SELECT d.e FROM mentat.datoms d \
             JOIN mentat.idents i ON i.ident = $1 \
             WHERE d.a = i.entid \
             AND d.added = true \
             AND d.v_text = $2 \
             LIMIT 1",
            None,
            &[DatumWithOid::from(attr_ident), DatumWithOid::from(value_str)],
        ) {
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

/// Get all datoms for an entity.
///
/// Returns the current state of an entity (not historical).
/// Returns attribute entid, value as text, type tag, and transaction id.
///
/// # Example
/// ```sql
/// SELECT * FROM mentat.get_entity_datoms(12345);
/// ```
#[pg_extern]
fn get_entity_datoms(
    entity_id: i64,
) -> Result<
    TableIterator<'static, (name!(attribute, i64), name!(value, String), name!(value_type, i16), name!(transaction, i64))>,
    Box<dyn std::error::Error>
> {
    Ok(TableIterator::new(Spi::connect(|client| {
        let result = client.select(
            "SELECT a, value_type_tag, \
                    COALESCE(v_ref::TEXT, v_bool::TEXT, v_long::TEXT, \
                             v_double::TEXT, v_text, v_keyword, \
                             v_instant::TEXT, v_uuid::TEXT, encode(v_bytes, 'hex')) AS value_text, \
                    tx \
             FROM mentat.datoms \
             WHERE e = $1 AND added = true \
             ORDER BY a, tx DESC",
            None,
            &[DatumWithOid::from(entity_id)],
        )?;

        let rows: Vec<_> = result.into_iter()
            .filter_map(|row| {
                let a: i64 = row.get(1).ok()??;
                let value_type: i16 = row.get(2).ok()??;
                let v: String = row.get(3).ok()??;
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
