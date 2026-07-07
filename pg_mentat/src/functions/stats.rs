use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::json;

/// Return query performance statistics from pg_stat_statements if available.
///
/// Returns aggregate statistics about mentat query and transaction function calls,
/// including per-function call counts, durations, and cache hit information from
/// the schema cache.
///
/// Returns JSON like:
/// ```json
/// {
///   "functions": [
///     {
///       "function": "mentat_query",
///       "calls": 150,
///       "avg_duration_ms": 12.5,
///       "total_duration_ms": 1875.0,
///       "self_duration_ms": 1200.0
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
///   },
///   "cache": {
///     "schema_cache_warmed": true
///   }
/// }
/// ```
#[pg_extern]
pub fn mentat_query_stats() -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let mut functions_arr = Vec::new();

    // Try pg_stat_user_functions for function call stats
    Spi::connect(|client| {
        // Query pg_stat_user_functions for mentat functions
        let func_query = r"
            SELECT funcname::TEXT,
                   calls::BIGINT,
                   total_time::DOUBLE PRECISION,
                   self_time::DOUBLE PRECISION
            FROM pg_stat_user_functions
            WHERE funcname LIKE 'mentat_%'
            ORDER BY calls DESC
        ";

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

        // Partition info - read from sequences for current allocation state
        let mut partitions = serde_json::Map::new();
        let seq_info = [
            ("db.part/db", "mentat.partition_db_seq", 0i64),
            ("db.part/user", "mentat.partition_user_seq", 1000001i64),
            ("db.part/tx", "mentat.partition_tx_seq", 1000000000000i64),
        ];
        for (name, seq_name, start_entid) in &seq_info {
            // Use last_value from pg_sequences to get current position
            // without advancing the sequence
            let next_entid: i64 = client
                .select(
                    &format!(
                        "SELECT last_value FROM pg_sequences WHERE schemaname = 'mentat' AND sequencename = '{}'",
                        seq_name.trim_start_matches("mentat.")
                    ),
                    None,
                    &[],
                )
                .ok()
                .and_then(|mut rows| rows.next().and_then(|r| r.get::<i64>(1).ok().flatten()))
                .unwrap_or(*start_entid);
            partitions.insert(
                name.to_string(),
                json!({
                    "next_entid": next_entid,
                    "used": next_entid - start_entid,
                }),
            );
        }

        json!({
            "total_datoms": total_datoms,
            "total_transactions": total_transactions,
            "schema_attributes": schema_attributes,
            "partitions": partitions,
        })
    });

    // Cache status from the schema cache
    let cache = crate::cache::get_cache();
    let cache_stats = json!({
        "schema_cache_warmed": cache.is_warmed(),
    });

    let result = json!({
        "functions": functions_arr,
        "database_stats": db_stats,
        "cache": cache_stats,
    });

    Ok(JsonB(result))
}

/// Find slow queries by filtering pg_stat_user_functions for mentat functions
/// whose average execution time exceeds the given threshold.
///
/// Falls back to showing the heaviest recent transactions (by datom count)
/// when pg_stat_user_functions is unavailable.
///
/// Arguments:
///   - threshold_ms: Minimum average duration in milliseconds to be considered slow (default: 100)
///
/// Returns JSON like:
/// ```json
/// {
///   "slow_functions": [
///     {
///       "function": "mentat_query",
///       "calls": 150,
///       "avg_duration_ms": 125.3,
///       "total_duration_ms": 18795.0
///     }
///   ],
///   "heavy_transactions": [
///     {
///       "tx": 1000042,
///       "tx_instant": "2025-01-15T10:30:00Z",
///       "datom_count": 150,
///       "assertions": 140,
///       "retractions": 10
///     }
///   ]
/// }
/// ```
#[pg_extern]
pub fn mentat_slow_queries(
    threshold_ms: default!(f64, 100.0),
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let threshold = if threshold_ms <= 0.0 {
        100.0
    } else {
        threshold_ms
    };

    // Find mentat functions whose average duration exceeds the threshold
    let slow_functions = Spi::connect(|client| {
        let query = format!(
            r"
            SELECT funcname::TEXT,
                   calls::BIGINT,
                   total_time::DOUBLE PRECISION,
                   self_time::DOUBLE PRECISION
            FROM pg_stat_user_functions
            WHERE funcname LIKE 'mentat_%'
              AND calls > 0
              AND (total_time / calls) > {threshold}
            ORDER BY (total_time / calls) DESC
            ",
        );

        let mut result_arr = Vec::new();
        match client.select(&query, None, &[]) {
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

                    let avg_ms = total_time / calls as f64;

                    result_arr.push(json!({
                        "function": funcname,
                        "calls": calls,
                        "avg_duration_ms": avg_ms,
                        "total_duration_ms": total_time,
                        "self_duration_ms": self_time,
                    }));
                }
            }
            Err(_) => {
                // pg_stat_user_functions not available
            }
        }

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(result_arr)
    })?;

    // Also show the 10 heaviest recent transactions by datom count
    let heavy_txns = Spi::connect(|client| {
        let query = r"
            SELECT t.tx,
                   t.tx_instant::TEXT,
                   (SELECT COUNT(*) FROM mentat.datoms d WHERE d.tx = t.tx)::BIGINT AS datom_count,
                   (SELECT COUNT(*) FROM mentat.datoms d WHERE d.tx = t.tx AND d.added = true)::BIGINT AS assertions,
                   (SELECT COUNT(*) FROM mentat.datoms d WHERE d.tx = t.tx AND d.added = false)::BIGINT AS retractions
            FROM mentat.transactions t
            ORDER BY t.tx DESC
            LIMIT 10
        ";

        let mut result_arr = Vec::new();
        for row in client.select(query, None, &[])? {
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

    let result = json!({
        "slow_functions": slow_functions,
        "heavy_transactions": heavy_txns,
    });

    Ok(JsonB(result))
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
        let table_query = r"
            SELECT schemaname || '.' || relname AS table_name,
                   pg_size_pretty(pg_total_relation_size(relid)) AS total_size,
                   n_live_tup::BIGINT AS row_estimate
            FROM pg_stat_user_tables
            WHERE schemaname = 'mentat'
            ORDER BY pg_total_relation_size(relid) DESC
        ";

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
        let idx_query = r"
            SELECT indexrelname AS index_name,
                   pg_size_pretty(pg_relation_size(indexrelid)) AS index_size
            FROM pg_stat_user_indexes
            WHERE schemaname = 'mentat'
            ORDER BY pg_relation_size(indexrelid) DESC
        ";

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
