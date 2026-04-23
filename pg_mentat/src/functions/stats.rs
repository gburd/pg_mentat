use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::json;

/// Return query performance statistics from pg_stat_statements if available.
///
/// Returns aggregate statistics about mentat query and transaction function calls.
/// Requires the pg_stat_statements extension to be installed.
///
/// Returns JSON like:
/// ```json
/// {
///   "functions": [
///     {
///       "function": "mentat_query",
///       "calls": 150,
///       "avg_duration_ms": 12.5,
///       "min_duration_ms": 0.3,
///       "max_duration_ms": 250.0,
///       "total_duration_ms": 1875.0
///     },
///     ...
///   ],
///   "database_stats": {
///     "total_datoms": 5000,
///     "total_transactions": 42,
///     "schema_attributes": 15,
///     "partitions": {
///       "db.part/db": { "next_entid": 200, "used": 200 },
///       "db.part/user": { "next_entid": 10500, "used": 500 },
///       "db.part/tx": { "next_entid": 1000042, "used": 42 }
///     }
///   }
/// }
/// ```
#[pg_extern]
pub fn mentat_query_stats() -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let mut functions_arr = Vec::new();

    // Try pg_stat_user_functions for function call stats
    Spi::connect(|client| {
        // Query pg_stat_user_functions for mentat functions
        let func_query = r#"
            SELECT funcname::TEXT,
                   calls::BIGINT,
                   total_time::DOUBLE PRECISION,
                   self_time::DOUBLE PRECISION
            FROM pg_stat_user_functions
            WHERE funcname LIKE 'mentat_%'
            ORDER BY calls DESC
        "#;

        match client.select(func_query, None, &[]) {
            Ok(rows) => {
                for row in rows {
                    let funcname: String = match row.get(1) {
                        Ok(Some(v)) => v,
                        _ => continue,
                    };
                    let calls: i64 = match row.get(2) {
                        Ok(Some(v)) => v,
                        _ => 0,
                    };
                    let total_time: f64 = match row.get(3) {
                        Ok(Some(v)) => v,
                        _ => 0.0,
                    };
                    let self_time: f64 = match row.get(4) {
                        Ok(Some(v)) => v,
                        _ => 0.0,
                    };

                    let avg_ms = if calls > 0 {
                        total_time / calls as f64
                    } else {
                        0.0
                    };

                    functions_arr.push(json!({
                        "function": funcname,
                        "calls": calls,
                        "total_duration_ms": total_time,
                        "self_duration_ms": self_time,
                        "avg_duration_ms": avg_ms,
                    }));
                }
            }
            Err(_) => {
                // pg_stat_user_functions may not be available; skip gracefully
            }
        }

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })?;

    // Gather database statistics
    let db_stats = Spi::connect(|client| {
        let total_datoms: i64 = client
            .select(
                "SELECT COUNT(*)::BIGINT FROM mentat.datoms WHERE added = true",
                None,
                &[],
            )
            .ok()
            .and_then(|mut rows| rows.next().and_then(|r| r.get::<i64>(1).ok().flatten()))
            .unwrap_or(0);

        let total_transactions: i64 = client
            .select(
                "SELECT COUNT(*)::BIGINT FROM mentat.transactions",
                None,
                &[],
            )
            .ok()
            .and_then(|mut rows| rows.next().and_then(|r| r.get::<i64>(1).ok().flatten()))
            .unwrap_or(0);

        let schema_attributes: i64 = client
            .select("SELECT COUNT(*)::BIGINT FROM mentat.schema", None, &[])
            .ok()
            .and_then(|mut rows| rows.next().and_then(|r| r.get::<i64>(1).ok().flatten()))
            .unwrap_or(0);

        // Partition info
        let mut partitions = serde_json::Map::new();
        if let Ok(rows) = client.select(
            "SELECT name, start_entid, next_entid FROM mentat.partitions ORDER BY name",
            None,
            &[],
        ) {
            for row in rows {
                let name: String = match row.get(1) {
                    Ok(Some(v)) => v,
                    _ => continue,
                };
                let start_entid: i64 = match row.get(2) {
                    Ok(Some(v)) => v,
                    _ => 0,
                };
                let next_entid: i64 = match row.get(3) {
                    Ok(Some(v)) => v,
                    _ => 0,
                };
                partitions.insert(
                    name,
                    json!({
                        "next_entid": next_entid,
                        "used": next_entid - start_entid,
                    }),
                );
            }
        }

        json!({
            "total_datoms": total_datoms,
            "total_transactions": total_transactions,
            "schema_attributes": schema_attributes,
            "partitions": partitions,
        })
    });

    let result = json!({
        "functions": functions_arr,
        "database_stats": db_stats,
    });

    Ok(JsonB(result))
}

/// Find slow queries by analyzing recent transaction history.
///
/// Returns the N most recent transactions with their size (datom count)
/// and timing information from the mentat.transactions table.
///
/// Arguments:
///   - limit_count: Maximum number of transactions to return (default: 20)
///
/// Returns JSON like:
/// ```json
/// [
///   {
///     "tx": 1000042,
///     "tx_instant": "2025-01-15T10:30:00Z",
///     "datom_count": 150,
///     "assertions": 140,
///     "retractions": 10
///   },
///   ...
/// ]
/// ```
#[pg_extern]
pub fn mentat_slow_queries(
    limit_count: default!(i32, 20),
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let limit_val = if limit_count <= 0 { 20 } else { limit_count };

    let results = Spi::connect(|client| {
        let query = format!(
            r#"
            SELECT t.tx,
                   t.tx_instant::TEXT,
                   (SELECT COUNT(*) FROM mentat.datoms d WHERE d.tx = t.tx)::BIGINT AS datom_count,
                   (SELECT COUNT(*) FROM mentat.datoms d WHERE d.tx = t.tx AND d.added = true)::BIGINT AS assertions,
                   (SELECT COUNT(*) FROM mentat.datoms d WHERE d.tx = t.tx AND d.added = false)::BIGINT AS retractions
            FROM mentat.transactions t
            ORDER BY t.tx DESC
            LIMIT {}
            "#,
            limit_val
        );

        let mut result_arr = Vec::new();
        for row in client.select(&query, None, &[])? {
            let tx: i64 = match row.get(1) {
                Ok(Some(v)) => v,
                _ => continue,
            };
            let tx_instant: String = match row.get(2) {
                Ok(Some(v)) => v,
                _ => "unknown".to_string(),
            };
            let datom_count: i64 = match row.get(3) {
                Ok(Some(v)) => v,
                _ => 0,
            };
            let assertions: i64 = match row.get(4) {
                Ok(Some(v)) => v,
                _ => 0,
            };
            let retractions: i64 = match row.get(5) {
                Ok(Some(v)) => v,
                _ => 0,
            };

            result_arr.push(json!({
                "tx": tx,
                "tx_instant": tx_instant,
                "datom_count": datom_count,
                "assertions": assertions,
                "retractions": retractions,
            }));
        }

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(result_arr)
    })?;

    Ok(JsonB(json!(results)))
}

/// Return database size and index statistics.
///
/// Provides storage-level information about mentat tables and indexes.
///
/// Returns JSON like:
/// ```json
/// {
///   "tables": {
///     "mentat.datoms": { "size": "8192 bytes", "row_estimate": 5000 },
///     "mentat.schema": { "size": "8192 bytes", "row_estimate": 15 },
///     ...
///   },
///   "indexes": [
///     { "name": "idx_datoms_eavt", "size": "16384 bytes" },
///     ...
///   ]
/// }
/// ```
#[pg_extern]
pub fn mentat_storage_stats() -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let result = Spi::connect(|client| {
        let mut tables = serde_json::Map::new();

        // Table sizes
        let table_query = r#"
            SELECT schemaname || '.' || relname AS table_name,
                   pg_size_pretty(pg_total_relation_size(relid)) AS total_size,
                   n_live_tup::BIGINT AS row_estimate
            FROM pg_stat_user_tables
            WHERE schemaname = 'mentat'
            ORDER BY pg_total_relation_size(relid) DESC
        "#;

        if let Ok(rows) = client.select(table_query, None, &[]) {
            for row in rows {
                let table_name: String = match row.get(1) {
                    Ok(Some(v)) => v,
                    _ => continue,
                };
                let total_size: String = match row.get(2) {
                    Ok(Some(v)) => v,
                    _ => "unknown".to_string(),
                };
                let row_estimate: i64 = match row.get(3) {
                    Ok(Some(v)) => v,
                    _ => 0,
                };
                tables.insert(
                    table_name,
                    json!({
                        "size": total_size,
                        "row_estimate": row_estimate,
                    }),
                );
            }
        }

        // Index sizes
        let mut indexes_arr = Vec::new();
        let idx_query = r#"
            SELECT indexrelname AS index_name,
                   pg_size_pretty(pg_relation_size(indexrelid)) AS index_size
            FROM pg_stat_user_indexes
            WHERE schemaname = 'mentat'
            ORDER BY pg_relation_size(indexrelid) DESC
        "#;

        if let Ok(rows) = client.select(idx_query, None, &[]) {
            for row in rows {
                let index_name: String = match row.get(1) {
                    Ok(Some(v)) => v,
                    _ => continue,
                };
                let index_size: String = match row.get(2) {
                    Ok(Some(v)) => v,
                    _ => "unknown".to_string(),
                };
                indexes_arr.push(json!({
                    "name": index_name,
                    "size": index_size,
                }));
            }
        }

        json!({
            "tables": tables,
            "indexes": indexes_arr,
        })
    });

    Ok(JsonB(result))
}
