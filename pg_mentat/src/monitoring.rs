//! Production monitoring infrastructure for pg_mentat.
//!
//! Provides:
//! - Slow query detection and logging
//! - Query execution statistics (per-backend counters)
//! - Index health monitoring via SQL views
//! - GUC parameters for configuring thresholds
//!
//! All statistics are per-backend (thread-local) since PostgreSQL backends are
//! single-process. They reset when the backend disconnects.

use pgrx::prelude::*;
use pgrx::{GucContext, GucFlags, GucRegistry, GucSetting};
use std::cell::RefCell;
use std::time::Instant;

// ============================================================================
// GUC Configuration
// ============================================================================

/// Slow query threshold in milliseconds. Queries exceeding this duration
/// are logged at WARNING level with their execution time and generated SQL.
/// Default 100ms. Set to 0 to disable slow query logging.
pub static SLOW_QUERY_THRESHOLD_MS: GucSetting<i32> = GucSetting::<i32>::new(100);

/// Whether to log generated SQL for all queries (not just slow ones).
/// Useful for debugging but verbose. Default false.
pub static LOG_ALL_QUERIES: GucSetting<bool> = GucSetting::<bool>::new(false);

/// Register monitoring GUC parameters.
///
/// Called from `_PG_init` during extension load.
pub fn register_monitoring_gucs() {
    GucRegistry::define_int_guc(
        c"mentat.slow_query_threshold_ms",
        c"Slow query logging threshold in milliseconds.",
        c"Queries exceeding this duration are logged at WARNING level with execution time and generated SQL. Set to 0 to disable. Default 100ms.",
        &SLOW_QUERY_THRESHOLD_MS,
        0,
        i32::MAX,
        GucContext::Userset,
        GucFlags::default(),
    );

    GucRegistry::define_bool_guc(
        c"mentat.log_all_queries",
        c"Log generated SQL for all Mentat queries.",
        c"When enabled, logs the generated SQL for every query at NOTICE level. Useful for debugging but verbose in production.",
        &LOG_ALL_QUERIES,
        GucContext::Userset,
        GucFlags::default(),
    );
}

// ============================================================================
// Query Execution Timing
// ============================================================================

/// RAII guard that measures query execution time and logs slow queries.
///
/// Create at the start of query execution; on drop, it checks the elapsed time
/// against `mentat.slow_query_threshold_ms` and logs if exceeded.
pub struct QueryTimer {
    start: Instant,
    datalog_query: String,
    sql_query: Option<String>,
}

impl QueryTimer {
    /// Start timing a Datalog query execution.
    pub fn start(datalog_query: &str) -> Self {
        Self {
            start: Instant::now(),
            datalog_query: datalog_query.to_string(),
            sql_query: None,
        }
    }

    /// Record the generated SQL (called after SQL generation but before execution).
    pub fn set_sql(&mut self, sql: &str) {
        if LOG_ALL_QUERIES.get() {
            pgrx::notice!("mentat SQL: {}", sql);
        }
        self.sql_query = Some(sql.to_string());
    }

    /// Complete the timer, log if slow, and update statistics.
    /// Returns the elapsed duration in milliseconds.
    pub fn finish(self) -> f64 {
        let elapsed = self.start.elapsed();
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;

        // Update per-backend statistics
        QUERY_STATS.with(|stats| {
            let mut stats = stats.borrow_mut();
            stats.total_queries += 1;
            stats.total_execution_ms += elapsed_ms;
            if elapsed_ms > stats.max_execution_ms {
                stats.max_execution_ms = elapsed_ms;
            }
        });

        // Log slow queries
        let threshold = SLOW_QUERY_THRESHOLD_MS.get();
        if threshold > 0 && elapsed_ms > f64::from(threshold) {
            QUERY_STATS.with(|stats| {
                stats.borrow_mut().slow_queries += 1;
            });

            let sql_preview = self
                .sql_query
                .as_deref()
                .unwrap_or("<not captured>")
                .chars()
                .take(500)
                .collect::<String>();

            pgrx::warning!(
                "mentat slow query ({:.1}ms > {}ms threshold): datalog={}, sql={}",
                elapsed_ms,
                threshold,
                self.datalog_query.chars().take(200).collect::<String>(),
                sql_preview,
            );
        }

        elapsed_ms
    }
}

// ============================================================================
// Per-Backend Statistics
// ============================================================================

/// Per-backend query execution statistics.
///
/// These accumulate for the lifetime of the PostgreSQL backend process and
/// reset when the backend disconnects.
#[derive(Default)]
struct QueryStats {
    /// Total number of queries executed.
    total_queries: u64,
    /// Total execution time across all queries (ms).
    total_execution_ms: f64,
    /// Maximum single query execution time (ms).
    max_execution_ms: f64,
    /// Number of queries that exceeded the slow query threshold.
    slow_queries: u64,
    /// Number of queries that used schema-aware single-table optimization.
    schema_aware_hits: u64,
    /// Number of queries that fell back to UNION ALL.
    union_all_fallbacks: u64,
    /// Number of cache hits in the prepared statement cache.
    stmt_cache_hits: u64,
    /// Number of cache misses in the prepared statement cache.
    stmt_cache_misses: u64,
}

thread_local! {
    static QUERY_STATS: RefCell<QueryStats> = RefCell::new(QueryStats::default());
}

/// Record a schema-aware optimization hit (single-table query).
pub fn record_schema_aware_hit() {
    QUERY_STATS.with(|stats| {
        stats.borrow_mut().schema_aware_hits += 1;
    });
}

/// Record a UNION ALL fallback (type unknown at compile time).
pub fn record_union_all_fallback() {
    QUERY_STATS.with(|stats| {
        stats.borrow_mut().union_all_fallbacks += 1;
    });
}

/// Record a prepared statement cache hit.
pub fn record_stmt_cache_hit() {
    QUERY_STATS.with(|stats| {
        stats.borrow_mut().stmt_cache_hits += 1;
    });
}

/// Record a prepared statement cache miss.
pub fn record_stmt_cache_miss() {
    QUERY_STATS.with(|stats| {
        stats.borrow_mut().stmt_cache_misses += 1;
    });
}

// ============================================================================
// SQL-Exposed Statistics Functions
// ============================================================================

/// Return per-backend query execution statistics as a JSON object.
///
/// Statistics accumulate for the lifetime of the backend and reset on disconnect.
/// This tracks in-process timing data complementing `mentat_query_stats()` from
/// `pg_stat_user_functions`.
///
/// # Example
/// ```sql
/// SELECT mentat_backend_stats();
/// -- Returns: {"total_queries": 42, "avg_ms": 12.3, "max_ms": 156.7, ...}
/// ```
#[pg_extern]
pub fn mentat_backend_stats() -> pgrx::JsonB {
    let stats = QUERY_STATS.with(|s| {
        let s = s.borrow();
        serde_json::json!({
            "total_queries": s.total_queries,
            "total_execution_ms": (s.total_execution_ms * 100.0).round() / 100.0,
            "avg_execution_ms": if s.total_queries > 0 {
                ((s.total_execution_ms / s.total_queries as f64) * 100.0).round() / 100.0
            } else {
                0.0
            },
            "max_execution_ms": (s.max_execution_ms * 100.0).round() / 100.0,
            "slow_queries": s.slow_queries,
            "schema_aware_hits": s.schema_aware_hits,
            "union_all_fallbacks": s.union_all_fallbacks,
            "stmt_cache_hits": s.stmt_cache_hits,
            "stmt_cache_misses": s.stmt_cache_misses,
            "stmt_cache_hit_rate": if s.stmt_cache_hits + s.stmt_cache_misses > 0 {
                ((s.stmt_cache_hits as f64 / (s.stmt_cache_hits + s.stmt_cache_misses) as f64) * 10000.0).round() / 100.0
            } else {
                0.0
            },
            "schema_aware_rate": if s.schema_aware_hits + s.union_all_fallbacks > 0 {
                ((s.schema_aware_hits as f64 / (s.schema_aware_hits + s.union_all_fallbacks) as f64) * 10000.0).round() / 100.0
            } else {
                0.0
            },
        })
    });

    pgrx::JsonB(stats)
}

/// Reset per-backend query execution statistics.
///
/// # Example
/// ```sql
/// SELECT mentat_reset_stats();
/// ```
#[pg_extern]
pub fn mentat_reset_stats() -> &'static str {
    QUERY_STATS.with(|stats| {
        *stats.borrow_mut() = QueryStats::default();
    });
    "stats reset"
}

// ============================================================================
// Index Health Monitoring
// ============================================================================

/// Return index health information for all mentat tables.
///
/// Shows index size, usage statistics, and bloat estimates. This queries
/// PostgreSQL's internal statistics views to help identify unused indexes
/// and tables needing maintenance.
///
/// # Example
/// ```sql
/// SELECT * FROM mentat_index_health();
/// ```
#[pg_extern]
pub fn mentat_index_health(
) -> Result<
    TableIterator<
        'static,
        (
            name!(table_name, String),
            name!(index_name, String),
            name!(index_size, String),
            name!(table_size, String),
            name!(index_scans, Option<i64>),
            name!(rows_fetched, Option<i64>),
            name!(dead_tuples, Option<i64>),
            name!(bloat_estimate_pct, Option<f64>),
        ),
    >,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let rows = Spi::connect(|client| {
        let query = r"
            SELECT
                i.schemaname || '.' || i.relname AS table_name,
                i.indexrelname AS index_name,
                pg_size_pretty(pg_relation_size(i.indexrelid)) AS index_size,
                pg_size_pretty(pg_relation_size(i.relid)) AS table_size,
                i.idx_scan AS index_scans,
                i.idx_tup_fetch AS rows_fetched,
                t.n_dead_tup AS dead_tuples,
                CASE
                    WHEN pg_relation_size(i.relid) > 8192 AND t.n_live_tup > 0 THEN
                        ROUND(
                            (t.n_dead_tup::FLOAT / GREATEST(t.n_live_tup + t.n_dead_tup, 1)) * 100,
                            1
                        )
                    ELSE 0
                END AS bloat_estimate_pct
            FROM pg_stat_user_indexes i
            JOIN pg_stat_user_tables t ON t.relid = i.relid
            WHERE i.schemaname = 'mentat'
            ORDER BY pg_relation_size(i.indexrelid) DESC
        ";

        let result = client.select(query, None, &[])?;
        let mut rows = Vec::new();
        for row in result {
            let table_name: String = row.get(1)?.unwrap_or_default();
            let index_name: String = row.get(2)?.unwrap_or_default();
            let index_size: String = row.get(3)?.unwrap_or_default();
            let table_size: String = row.get(4)?.unwrap_or_default();
            let index_scans: Option<i64> = row.get(5)?;
            let rows_fetched: Option<i64> = row.get(6)?;
            let dead_tuples: Option<i64> = row.get(7)?;
            let bloat_estimate: Option<f64> = row.get(8)?;
            rows.push((
                table_name,
                index_name,
                index_size,
                table_size,
                index_scans,
                rows_fetched,
                dead_tuples,
                bloat_estimate,
            ));
        }
        Ok::<_, pgrx::spi::SpiError>(rows)
    })?;

    Ok(TableIterator::new(rows))
}

/// Return a health check summary for the mentat extension.
///
/// Verifies that core tables exist, schema is populated, and the
/// extension is functional. Returns JSON with status and details.
///
/// Status values:
/// - "healthy": Schema attributes exist and at least one store is configured
/// - "degraded": Stores exist but no user-defined schema attributes
/// - "unhealthy": No stores found (extension may not be properly initialized)
///
/// # Example
/// ```sql
/// SELECT mentat_health_check();
/// ```
#[pg_extern]
pub fn mentat_health_check() -> Result<pgrx::JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let result = Spi::connect(|client| {
        // Check core tables
        let schema_count: i64 = client
            .select("SELECT COUNT(*)::BIGINT FROM mentat.schema", None, &[])
            .ok()
            .and_then(|mut rows| rows.next().and_then(|r| r.get::<i64>(1).ok().flatten()))
            .unwrap_or(0);

        let store_count: i64 = client
            .select("SELECT COUNT(*)::BIGINT FROM mentat.stores", None, &[])
            .ok()
            .and_then(|mut rows| rows.next().and_then(|r| r.get::<i64>(1).ok().flatten()))
            .unwrap_or(0);

        let tx_count: i64 = client
            .select(
                "SELECT COUNT(*)::BIGINT FROM mentat.transactions",
                None,
                &[],
            )
            .ok()
            .and_then(|mut rows| rows.next().and_then(|r| r.get::<i64>(1).ok().flatten()))
            .unwrap_or(0);

        // Check type-specific tables have data
        let type_tables = [
            "mentat.datoms_ref_new",
            "mentat.datoms_long_new",
            "mentat.datoms_text_new",
            "mentat.datoms_boolean_new",
            "mentat.datoms_double_new",
            "mentat.datoms_keyword_new",
        ];

        let mut tables_with_data = 0i64;
        for table in &type_tables {
            let count: i64 = client
                .select(
                    &format!("SELECT COUNT(*)::BIGINT FROM {} LIMIT 1", table),
                    None,
                    &[],
                )
                .ok()
                .and_then(|mut rows| rows.next().and_then(|r| r.get::<i64>(1).ok().flatten()))
                .unwrap_or(0);
            if count > 0 {
                tables_with_data += 1;
            }
        }

        let status = if schema_count > 0 && store_count > 0 {
            "healthy"
        } else if store_count > 0 {
            "degraded"
        } else {
            "unhealthy"
        };

        // Per-backend stats
        let backend_stats = QUERY_STATS.with(|s| {
            let s = s.borrow();
            serde_json::json!({
                "total_queries": s.total_queries,
                "slow_queries": s.slow_queries,
                "max_execution_ms": (s.max_execution_ms * 100.0).round() / 100.0,
            })
        });

        let json = serde_json::json!({
            "status": status,
            "schema_attributes": schema_count,
            "stores": store_count,
            "transactions": tx_count,
            "type_tables_with_data": tables_with_data,
            "slow_query_threshold_ms": SLOW_QUERY_THRESHOLD_MS.get(),
            "backend_stats": backend_stats,
        });

        Ok::<_, pgrx::spi::SpiError>(json)
    })?;

    Ok(pgrx::JsonB(result))
}

// ============================================================================
// Index Health SQL View Creation
// ============================================================================

/// Generate the SQL to create the `mentat.index_health` view.
///
/// This view provides a persistent, queryable interface to index bloat
/// and usage statistics. It's created during `CREATE EXTENSION` and
/// can be refreshed at any time.
pub fn index_health_view_sql() -> &'static str {
    r"
    CREATE OR REPLACE VIEW mentat.index_health AS
    SELECT
        i.schemaname || '.' || i.relname AS table_name,
        i.indexrelname AS index_name,
        pg_size_pretty(pg_relation_size(i.indexrelid)) AS index_size,
        pg_size_pretty(pg_relation_size(i.relid)) AS table_size,
        i.idx_scan AS index_scans,
        i.idx_tup_fetch AS rows_fetched,
        t.n_dead_tup AS dead_tuples,
        t.n_live_tup AS live_tuples,
        CASE
            WHEN t.n_live_tup + t.n_dead_tup > 0 THEN
                ROUND(
                    (t.n_dead_tup::FLOAT / (t.n_live_tup + t.n_dead_tup)) * 100,
                    1
                )
            ELSE 0
        END AS bloat_estimate_pct,
        t.last_vacuum,
        t.last_autovacuum,
        t.last_analyze,
        t.last_autoanalyze
    FROM pg_stat_user_indexes i
    JOIN pg_stat_user_tables t ON t.relid = i.relid
    WHERE i.schemaname = 'mentat'
    ORDER BY pg_relation_size(i.indexrelid) DESC
    "
}

/// Generate the SQL to create the `mentat.table_health` view.
///
/// Shows table-level statistics including row counts, dead tuples,
/// sizes, and maintenance timestamps.
pub fn table_health_view_sql() -> &'static str {
    r"
    CREATE OR REPLACE VIEW mentat.table_health AS
    SELECT
        schemaname || '.' || relname AS table_name,
        n_live_tup AS live_tuples,
        n_dead_tup AS dead_tuples,
        CASE
            WHEN n_live_tup + n_dead_tup > 0 THEN
                ROUND(
                    (n_dead_tup::FLOAT / (n_live_tup + n_dead_tup)) * 100,
                    1
                )
            ELSE 0
        END AS dead_pct,
        pg_size_pretty(pg_relation_size(relid)) AS table_size,
        pg_size_pretty(pg_total_relation_size(relid)) AS total_size,
        last_vacuum,
        last_autovacuum,
        last_analyze,
        last_autoanalyze,
        vacuum_count,
        autovacuum_count,
        analyze_count,
        autoanalyze_count
    FROM pg_stat_user_tables
    WHERE schemaname = 'mentat'
    ORDER BY pg_total_relation_size(relid) DESC
    "
}
