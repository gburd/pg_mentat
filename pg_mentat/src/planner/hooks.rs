// Copyright 2026
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! PostgreSQL planner hooks for Mentat query optimization.
//!
//! Current features:
//! - SQL helper functions for index hints and query analysis
//! - Cost estimation utilities
//! - GUC configuration parameters for optimizer hints
//! - Automatic SET LOCAL hints applied during query execution
//!
//! GUC Parameters:
//! - `mentat.enable_optimizer_hints` (bool, default true): Enable/disable automatic
//!   optimizer hints (SET LOCAL enable_seqscan, work_mem) during query execution.
//! - `mentat.default_work_mem` (string, default "64MB"): The work_mem value to
//!   SET LOCAL for complex Mentat queries (those with joins, aggregates, or CTEs).

use std::ffi::CString;

use pgrx::prelude::*;
use pgrx::{GucContext, GucFlags, GucRegistry, GucSetting};

/// Detect the optimal index for a datom query pattern.
///
/// Returns the suggested index name based on the specified access pattern:
/// - 'e': Entity-first access -> idx_mentat_eavt
/// - 'a': Attribute-first access -> idx_mentat_aevt
/// - 'v': Value-first access -> idx_mentat_vaet
/// - 'av': Attribute-Value access -> idx_mentat_avet
///
/// # Example
/// ```sql
/// SELECT mentat.suggest_index('a');  -- Returns 'idx_mentat_aevt'
/// ```
#[pg_extern]
fn suggest_index(access_pattern: &str) -> Result<String, Box<dyn std::error::Error>> {
    let index = match access_pattern {
        "e" | "ea" | "eav" | "eavt" => "idx_mentat_eavt",
        "a" | "ae" | "aev" | "aevt" => "idx_mentat_aevt",
        "av" | "ave" | "avet" => "idx_mentat_avet",
        "v" | "va" | "vae" | "vaet" => "idx_mentat_vaet",
        _ => "idx_mentat_aevt", // Default to attribute-first
    };

    Ok(index.to_string())
}

/// Estimate the cost of a datom query operation.
///
/// Returns an estimated cost multiplier based on the access pattern and
/// expected cardinality. Lower numbers indicate more efficient queries.
///
/// # Example
/// ```sql
/// SELECT mentat.estimate_query_cost('a', 1000);  -- Attribute scan of 1000 rows
/// ```
#[pg_extern]
fn estimate_query_cost(
    access_pattern: &str,
    estimated_rows: i64,
) -> Result<f64, Box<dyn std::error::Error>> {
    // Cost multipliers based on index effectiveness
    let index_cost = match access_pattern {
        "e" | "ea" | "eav" | "eavt" => 1.0, // Entity lookup is cheapest
        "a" | "ae" | "aev" | "aevt" => 1.2, // Attribute scan
        "av" | "ave" | "avet" => 1.1,       // Attribute-value is efficient
        "v" | "va" | "vae" | "vaet" => 2.0, // Value-first scan is expensive
        _ => 1.5,
    };

    // Simple logarithmic cost model
    let row_cost = if estimated_rows > 0 {
        f64::from(estimated_rows as i32).log10()
    } else {
        1.0
    };

    Ok(index_cost * row_cost)
}

/// Analyze and provide optimization hints for a query.
///
/// This function analyzes a SQL query string and provides recommendations
/// for optimal index usage when querying the mentat_datoms table.
///
/// # Example
/// ```sql
/// SELECT mentat.analyze_query('SELECT * FROM mentat_datoms WHERE a = 123');
/// ```
#[pg_extern]
fn analyze_query(query_text: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Phase 1: Basic pattern detection (typed columns: v_ref, v_long, v_text, etc.)
    let has_value_filter = query_text.contains("v_ref")
        || query_text.contains("v_long")
        || query_text.contains("v_double")
        || query_text.contains("v_text")
        || query_text.contains("v_keyword")
        || query_text.contains("v_bool")
        || query_text.contains("v_instant")
        || query_text.contains("v_uuid")
        || query_text.contains("v_bytes");
    let pattern = if query_text.contains("WHERE e =") || query_text.contains("WHERE e=") {
        "Entity-first (use EAVT index)"
    } else if query_text.contains("WHERE a =") || query_text.contains("WHERE a=") {
        if has_value_filter {
            "Attribute-Value (use AVET index)"
        } else {
            "Attribute-first (use AEVT index)"
        }
    } else if has_value_filter {
        "Value-first (use VAET index)"
    } else {
        "No specific pattern detected (use AEVT index as default)"
    };

    Ok(format!("Pattern: {}", pattern))
}

/// Get available mentat indexes and their usage recommendations.
///
/// Returns information about all Mentat datom indexes and when to use each one.
///
/// # Example
/// ```sql
/// SELECT * FROM mentat.get_index_info();
/// ```
#[pg_extern]
fn get_index_info() -> Result<
    TableIterator<
        'static,
        (
            name!(index_name, String),
            name!(access_pattern, String),
            name!(use_when, String),
        ),
    >,
    Box<dyn std::error::Error>,
> {
    let indexes = vec![
        (
            "idx_mentat_eavt".to_string(),
            "Entity-first".to_string(),
            "Lookups by entity ID (e.g., get all attributes of an entity)".to_string(),
        ),
        (
            "idx_mentat_aevt".to_string(),
            "Attribute-first".to_string(),
            "Lookups by attribute (e.g., find all entities with :user/name)".to_string(),
        ),
        (
            "idx_mentat_avet".to_string(),
            "Attribute-Value".to_string(),
            "Lookups by attribute and value (e.g., find entity where :user/email = 'alice@example.com')".to_string(),
        ),
        (
            "idx_mentat_vaet".to_string(),
            "Value-first".to_string(),
            "Reverse lookups (e.g., find all entities referring to a specific entity)".to_string(),
        ),
    ];

    Ok(TableIterator::new(indexes))
}

// ============================================================================
// GUC Configuration Parameters
// ============================================================================

/// Whether to apply optimizer hints (SET LOCAL enable_seqscan, work_mem)
/// during Mentat query execution.
pub static ENABLE_OPTIMIZER_HINTS: GucSetting<bool> = GucSetting::<bool>::new(true);

/// The work_mem value to SET LOCAL for complex Mentat queries.
/// Only applied when `mentat.enable_optimizer_hints` is true and the query
/// involves multiple joins, aggregates, or CTEs.
pub static DEFAULT_WORK_MEM: GucSetting<Option<CString>> =
    GucSetting::<Option<CString>>::new(Some(c"64MB"));

/// Query timeout in milliseconds. Prevents runaway queries from blocking backends.
/// 0 means no timeout (default). Set to positive value (e.g., 30000 for 30 seconds)
/// to enforce per-query timeout via statement_timeout.
pub static QUERY_TIMEOUT_MS: GucSetting<i32> = GucSetting::<i32>::new(0);

/// Read the current value of `mentat.enable_optimizer_hints`.
pub fn optimizer_hints_enabled() -> bool {
    ENABLE_OPTIMIZER_HINTS.get()
}

/// Read the current value of `mentat.default_work_mem`.
pub fn default_work_mem() -> String {
    DEFAULT_WORK_MEM
        .get()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "64MB".to_string())
}

/// Read the current value of `mentat.query_timeout_ms`.
pub fn query_timeout_ms() -> i32 {
    QUERY_TIMEOUT_MS.get()
}

/// Initialize planner hooks and register GUC settings.
///
/// This function is called from the extension's `_PG_init` function.
/// Registers GUC parameters so users can configure optimizer behavior via:
///   SET mentat.enable_optimizer_hints = off;
///   SET mentat.default_work_mem = '128MB';
#[allow(dead_code)]
pub unsafe fn init_planner_hooks() {
    GucRegistry::define_bool_guc(
        c"mentat.enable_optimizer_hints",
        c"Enable automatic optimizer hints for Mentat queries.",
        c"When enabled, Mentat applies SET LOCAL enable_seqscan = off and SET LOCAL work_mem before executing generated SQL to encourage index usage on the datoms table.",
        &ENABLE_OPTIMIZER_HINTS,
        GucContext::Userset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"mentat.default_work_mem",
        c"Work memory for complex Mentat queries.",
        c"The value passed to SET LOCAL work_mem before executing Mentat queries that involve multiple pattern joins, aggregates, or CTEs. Only effective when mentat.enable_optimizer_hints is on.",
        &DEFAULT_WORK_MEM,
        GucContext::Userset,
        GucFlags::default(),
    );

    GucRegistry::define_int_guc(
        c"mentat.query_timeout_ms",
        c"Query timeout in milliseconds.",
        c"Maximum execution time for Mentat queries. Set to 0 (default) for no timeout. Set to positive value (e.g., 30000 for 30 seconds) to prevent runaway queries. Enforced via statement_timeout.",
        &QUERY_TIMEOUT_MS,
        0,
        i32::MAX,
        GucContext::Userset,
        GucFlags::default(),
    );

    tracing::info!("Mentat planner optimization hooks initialized with GUC parameters");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suggest_index_entity() {
        let result = suggest_index("e");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("eavt"));
    }

    #[test]
    fn test_suggest_index_attribute() {
        let result = suggest_index("a");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("aevt"));
    }

    #[test]
    fn test_suggest_index_value() {
        let result = suggest_index("v");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("vaet"));
    }

    #[test]
    fn test_suggest_index_attribute_value() {
        let result = suggest_index("av");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("avet"));
    }

    #[test]
    fn test_estimate_query_cost() {
        let result = estimate_query_cost("e", 100);
        assert!(result.is_ok());
        let cost = result.unwrap();
        assert!(cost > 0.0);
    }
}
