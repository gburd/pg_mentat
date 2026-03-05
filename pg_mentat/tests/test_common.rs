// Copyright 2026
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! Common test infrastructure for pg_mentat tests.
//!
//! This module provides utilities for setting up PostgreSQL test databases
//! and porting existing SQLite-based Mentat tests to PostgreSQL.

use pgrx::prelude::*;

/// Initialize a test database with the pg_mentat schema.
///
/// This sets up:
/// - EDN type
/// - Datoms tables
/// - Schema tables
/// - Indexes
///
/// Uses pgrx's SPI to execute initialization within a test transaction.
pub fn setup_test_db() -> Result<(), Box<dyn std::error::Error>> {
    Spi::run(
        r#"
        -- Create schema if not exists
        CREATE SCHEMA IF NOT EXISTS mentat;

        -- Datoms table (core storage)
        CREATE TABLE IF NOT EXISTS mentat.datoms (
            e BIGINT NOT NULL,
            a BIGINT NOT NULL,
            v mentat.EdnValue NOT NULL,
            tx BIGINT NOT NULL,
            added BOOLEAN NOT NULL DEFAULT TRUE
        );

        -- Schema table (attribute definitions)
        CREATE TABLE IF NOT EXISTS mentat.schema (
            entid BIGINT PRIMARY KEY,
            ident TEXT UNIQUE NOT NULL,
            value_type INTEGER NOT NULL,
            cardinality INTEGER NOT NULL,
            unique_value BOOLEAN DEFAULT FALSE,
            index_value BOOLEAN DEFAULT FALSE,
            fulltext BOOLEAN DEFAULT FALSE,
            component BOOLEAN DEFAULT FALSE,
            no_history BOOLEAN DEFAULT FALSE
        );

        -- Idents table (keyword to entity ID mapping)
        CREATE TABLE IF NOT EXISTS mentat.idents (
            ident TEXT PRIMARY KEY,
            entid BIGINT UNIQUE NOT NULL
        );

        -- Partitions table
        CREATE TABLE IF NOT EXISTS mentat.partitions (
            part TEXT PRIMARY KEY,
            start_id BIGINT NOT NULL,
            end_id BIGINT NOT NULL
        );

        -- Transactions table
        CREATE TABLE IF NOT EXISTS mentat.transactions (
            tx BIGINT PRIMARY KEY,
            tx_instant TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );

        -- Indexes
        CREATE INDEX IF NOT EXISTS idx_datoms_eavt ON mentat.datoms (e, a, v, tx);
        CREATE INDEX IF NOT EXISTS idx_datoms_aevt ON mentat.datoms (a, e, v, tx);
        CREATE INDEX IF NOT EXISTS idx_datoms_avet ON mentat.datoms (a, v, e, tx);
        CREATE INDEX IF NOT EXISTS idx_datoms_vaet ON mentat.datoms (v, a, e, tx);
        CREATE INDEX IF NOT EXISTS idx_datoms_tx ON mentat.datoms (tx);

        -- Bootstrap core schema
        INSERT INTO mentat.partitions (part, start_id, end_id) VALUES
            ('db.part/db', 0, 10000),
            ('db.part/user', 10000, 1000000),
            ('db.part/tx', 1000000, 2000000)
        ON CONFLICT (part) DO NOTHING;

        -- Initialize bootstrap transaction
        INSERT INTO mentat.transactions (tx, tx_instant)
        VALUES (1000000, '2025-01-01T00:00:00Z')
        ON CONFLICT (tx) DO NOTHING;
        "#,
    )?;

    Ok(())
}

/// Bootstrap the core Mentat schema.
///
/// Adds the fundamental schema attributes:
/// - :db/ident
/// - :db/valueType
/// - :db/cardinality
/// - :db/unique
/// - :db/doc
/// - etc.
pub fn bootstrap_schema() -> Result<(), Box<dyn std::error::Error>> {
    Spi::run(
        r#"
        -- Core schema attributes
        INSERT INTO mentat.schema (entid, ident, value_type, cardinality, unique_value, index_value) VALUES
            (1, ':db/ident', 20, 1, true, true),  -- Keyword type
            (2, ':db/valueType', 21, 1, false, false),  -- Ref type
            (3, ':db/cardinality', 21, 1, false, false),
            (4, ':db/unique', 21, 1, false, false),
            (5, ':db/doc', 10, 1, false, false),  -- String type
            (6, ':db/isComponent', 1, 1, false, false),  -- Boolean type
            (7, ':db/fulltext', 1, 1, false, false),
            (8, ':db/index', 1, 1, false, false),
            (9, ':db/noHistory', 1, 1, false, false),
            (10, ':db/txInstant', 22, 1, false, true)  -- Instant type
        ON CONFLICT (entid) DO NOTHING;

        -- Map idents
        INSERT INTO mentat.idents (ident, entid) VALUES
            (':db/ident', 1),
            (':db/valueType', 2),
            (':db/cardinality', 3),
            (':db/unique', 4),
            (':db/doc', 5),
            (':db/isComponent', 6),
            (':db/fulltext', 7),
            (':db/index', 8),
            (':db/noHistory', 9),
            (':db/txInstant', 10)
        ON CONFLICT (ident) DO NOTHING;
        "#,
    )?;

    Ok(())
}

/// Clean up test data.
///
/// Truncates all mentat tables for a fresh test state.
pub fn cleanup_test_db() -> Result<(), Box<dyn std::error::Error>> {
    Spi::run(
        r#"
        TRUNCATE TABLE mentat.datoms CASCADE;
        TRUNCATE TABLE mentat.schema CASCADE;
        TRUNCATE TABLE mentat.idents CASCADE;
        TRUNCATE TABLE mentat.transactions CASCADE;
        DELETE FROM mentat.partitions WHERE part NOT IN ('db.part/db', 'db.part/user', 'db.part/tx');
        "#,
    )?;

    Ok(())
}

/// Execute a datalog query and return results as JSON.
///
/// This is a test helper that wraps the mentat_query function.
pub fn query(query_edn: &str, inputs_json: &str) -> Result<String, Box<dyn std::error::Error>> {
    let sql = format!(
        "SELECT mentat.mentat_query('{}', '{}'::jsonb)",
        query_edn.replace('\'', "''"),
        inputs_json.replace('\'', "''")
    );

    Spi::connect(|client| {
        let result = client.select(&sql, None, None)?;
        if let Some(row) = result.first() {
            let json: String = row
                .get(1)?
                .ok_or_else(|| "Query returned NULL")?;
            Ok(json)
        } else {
            Err("Query returned no rows".into())
        }
    })
}

/// Execute a transaction and return the transaction report.
pub fn transact(tx_data: &str) -> Result<String, Box<dyn std::error::Error>> {
    let sql = format!(
        "SELECT mentat.mentat_transact('{}')",
        tx_data.replace('\'', "''")
    );

    Spi::connect(|client| {
        let result = client.select(&sql, None, None)?;
        if let Some(row) = result.first() {
            let json: String = row
                .get(1)?
                .ok_or_else(|| "Transaction returned NULL")?;
            Ok(json)
        } else {
            Err("Transaction returned no rows".into())
        }
    })
}

/// Get entity data as JSON.
pub fn entity(entid: i64) -> Result<String, Box<dyn std::error::Error>> {
    let sql = format!("SELECT mentat.mentat_entity({})", entid);

    Spi::connect(|client| {
        let result = client.select(&sql, None, None)?;
        if let Some(row) = result.first() {
            let json: String = row
                .get(1)?
                .ok_or_else(|| "Entity lookup returned NULL")?;
            Ok(json)
        } else {
            Err("Entity lookup returned no rows".into())
        }
    })
}

/// Get the current schema as JSON.
pub fn schema() -> Result<String, Box<dyn std::error::Error>> {
    Spi::connect(|client| {
        let result = client.select("SELECT mentat.mentat_schema()", None, None)?;
        if let Some(row) = result.first() {
            let json: String = row
                .get(1)?
                .ok_or_else(|| "Schema returned NULL")?;
            Ok(json)
        } else {
            Err("Schema returned no rows".into())
        }
    })
}
