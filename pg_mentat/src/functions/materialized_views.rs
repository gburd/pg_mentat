/// Datalog-powered materialized views with automatic refresh capabilities.
///
/// Materialized views cache the results of a Datalog query as a PostgreSQL
/// materialized view. They can be refreshed manually or automatically via
/// a trigger on the store's datoms table (on_write policy).
///
/// Metadata about each materialized view is stored in the
/// `mentat.materialized_views` table for tracking and management.
use crate::error::MentatError;
use crate::functions::store_management::{get_schema_for_store, quote_ident, validate_store_name};
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::json;

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Maximum length for a view name.
const MAX_VIEW_NAME_LEN: usize = 58;

/// Validate a materialized view name.
///
/// Rules:
/// - Must not be empty
/// - Must be <= 58 characters
/// - Must start with a letter or underscore
/// - Must contain only letters, digits, and underscores
fn validate_view_name(name: &str) -> Result<(), MentatError> {
    if name.is_empty() {
        return Err(MentatError::InvalidQuery {
            message: "View name cannot be empty.".to_string(),
            suggestion: None,
        });
    }

    if name.len() > MAX_VIEW_NAME_LEN {
        return Err(MentatError::InvalidQuery {
            message: format!(
                "View name exceeds maximum length of {} characters.",
                MAX_VIEW_NAME_LEN
            ),
            suggestion: None,
        });
    }

    let first = name
        .chars()
        .next()
        .ok_or_else(|| MentatError::InvalidQuery {
            message: "View name cannot be empty.".to_string(),
            suggestion: None,
        })?;

    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(MentatError::InvalidQuery {
            message: "View name must start with a letter or underscore.".to_string(),
            suggestion: None,
        });
    }

    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(MentatError::InvalidQuery {
            message: "View name must contain only letters, digits, and underscores.".to_string(),
            suggestion: None,
        });
    }

    Ok(())
}

/// Validate that the refresh_policy is one of the supported values.
fn validate_refresh_policy(policy: &str) -> Result<(), MentatError> {
    match policy {
        "manual" | "on_write" => Ok(()),
        _ => Err(MentatError::InvalidQuery {
            message: format!(
                "Invalid refresh policy '{}'. Supported policies: 'manual', 'on_write'.",
                policy
            ),
            suggestion: Some("Use 'manual' for explicit refresh or 'on_write' for automatic refresh on data changes.".to_string()),
        }),
    }
}

// ---------------------------------------------------------------------------
// Metadata helpers
// ---------------------------------------------------------------------------

/// Check if a materialized view with the given name exists for a store.
fn matview_exists(
    store_name: &str,
    view_name: &str,
) -> Result<bool, pgrx::spi::SpiError> {
    Spi::connect(|client| {
        let exists = client
            .select(
                "SELECT 1 FROM mentat.materialized_views WHERE store_name = $1 AND view_name = $2",
                None,
                &[
                    DatumWithOid::from(store_name),
                    DatumWithOid::from(view_name),
                ],
            )?
            .next()
            .is_some();
        Ok(exists)
    })
}

// ---------------------------------------------------------------------------
// Helpers: create refresh triggers
// ---------------------------------------------------------------------------

/// Generate the SQL for the trigger function that refreshes a materialized view
/// when datoms change.
fn refresh_trigger_function_sql(
    schema: &str,
    view_name: &str,
) -> String {
    let func_name = format!("{}.mentat_matview_refresh_{}", schema, view_name);
    let qualified_view = format!("{}.{}", schema, view_name);

    format!(
        r#"
CREATE OR REPLACE FUNCTION {func_name}() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    REFRESH MATERIALIZED VIEW {qualified_view};
    RETURN NULL;
END;
$$;
"#,
        func_name = func_name,
        qualified_view = qualified_view,
    )
}

/// Generate the SQL for the trigger that fires the refresh function.
fn refresh_trigger_sql(schema: &str, view_name: &str) -> String {
    let trigger_name = format!("mentat_matview_trg_{}", view_name);
    let func_name = format!("{}.mentat_matview_refresh_{}", schema, view_name);

    format!(
        r#"
CREATE OR REPLACE TRIGGER {trigger_name}
    AFTER INSERT OR UPDATE OR DELETE ON {schema}.datoms
    FOR EACH STATEMENT
    EXECUTE FUNCTION {func_name}();
"#,
        trigger_name = trigger_name,
        schema = schema,
        func_name = func_name,
    )
}

/// Create the refresh trigger for a materialized view (on_write policy).
fn create_refresh_trigger(
    schema: &str,
    view_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let func_sql = refresh_trigger_function_sql(schema, view_name);
    Spi::run(&func_sql)?;

    let trg_sql = refresh_trigger_sql(schema, view_name);
    Spi::run(&trg_sql)?;

    Ok(())
}

/// Drop the refresh trigger and function for a materialized view.
fn drop_refresh_trigger(
    schema: &str,
    view_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let trigger_name = format!("mentat_matview_trg_{}", view_name);
    let func_name = format!("{}.mentat_matview_refresh_{}", schema, view_name);

    Spi::run(&format!(
        "DROP TRIGGER IF EXISTS {} ON {}.datoms",
        trigger_name, schema
    ))?;
    Spi::run(&format!("DROP FUNCTION IF EXISTS {}()", func_name))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API functions
// ---------------------------------------------------------------------------

/// Create a materialized view from a Datalog query.
///
/// The query results are cached in a PostgreSQL materialized view within the
/// store's schema. The view can be refreshed manually with `mentat_refresh()`
/// or automatically when the `on_write` refresh policy is used.
///
/// # Arguments
/// - `store_name`: Name of the store to query (use "default" for the default store)
/// - `view_name`: Name for the materialized view
/// - `datalog_query`: Datalog query string whose results populate the view
/// - `refresh_policy`: Refresh policy - "manual" (default) or "on_write"
///
/// # Example
/// ```sql
/// SELECT mentat_materialize('default', 'people_cache',
///     '[:find ?e ?name :where [?e :person/name ?name]]',
///     'manual');
/// SELECT * FROM mentat.people_cache;
/// ```
#[pg_extern]
pub fn materialize(
    store_name: &str,
    view_name: &str,
    datalog_query: &str,
    refresh_policy: default!(&str, "'manual'"),
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    validate_store_name(store_name)?;
    validate_view_name(view_name)?;
    validate_refresh_policy(refresh_policy)?;

    if datalog_query.trim().is_empty() {
        return Err(Box::new(MentatError::InvalidQuery {
            message: "Datalog query cannot be empty.".to_string(),
            suggestion: None,
        }));
    }

    // Check if view already exists
    if matview_exists(store_name, view_name)? {
        return Err(Box::new(MentatError::InvalidQuery {
            message: format!(
                "Materialized view '{}' already exists for store '{}'.",
                view_name, store_name
            ),
            suggestion: Some(format!(
                "Use mentat_drop_matview('{}', '{}') first to remove the existing view.",
                store_name, view_name
            )),
        }));
    }

    let schema = get_schema_for_store(store_name);
    let quoted_schema = quote_ident(&schema);

    // Infer columns from the Datalog query using mentat_query_sql
    let query_info = Spi::connect(|client| {
        let mut rows = client.select(
            "SELECT mentat_query_sql($1, '{}'::jsonb)",
            None,
            &[DatumWithOid::from(datalog_query)],
        )?;
        match rows.next() {
            Some(row) => {
                let result: Option<JsonB> = row.get::<JsonB>(1)?;
                Ok::<_, pgrx::spi::SpiError>(result)
            }
            None => Ok(None),
        }
    })?;

    let columns = match &query_info {
        Some(JsonB(info)) => {
            if let Some(cols) = info.get("columns").and_then(|c| c.as_array()) {
                cols.iter()
                    .filter_map(|c| c.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            } else {
                return Err(Box::new(MentatError::InvalidQuery {
                    message: "Could not extract column names from query.".to_string(),
                    suggestion: None,
                }));
            }
        }
        None => {
            return Err(Box::new(MentatError::InvalidQuery {
                message: "Query SQL generation returned no results.".to_string(),
                suggestion: None,
            }));
        }
    };

    let col_count = columns.len();
    if col_count == 0 {
        return Err(Box::new(MentatError::InvalidQuery {
            message: "Datalog query has no :find columns.".to_string(),
            suggestion: None,
        }));
    }

    if col_count > 8 {
        return Err(Box::new(MentatError::InvalidQuery {
            message: format!(
                "Materialized views support up to 8 columns, but this query has {}.",
                col_count
            ),
            suggestion: Some(
                "Use mentat_query() for queries with more than 8 columns.".to_string(),
            ),
        }));
    }

    // Build column aliases from mentat_query_view output (col1..colN -> named columns)
    let col_defs: String = columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            // Sanitize column name: replace ? and / with _
            let safe_name = name
                .trim_start_matches('?')
                .replace('/', "_");
            format!("col{} AS {}", i + 1, quote_ident(&safe_name))
        })
        .collect::<Vec<_>>()
        .join(", ");

    // Escape single quotes in query for embedding in SQL
    let escaped_query = datalog_query.replace('\'', "''");

    // Build the CREATE MATERIALIZED VIEW statement
    let create_sql = if store_name == "default" {
        format!(
            "CREATE MATERIALIZED VIEW {schema}.{view} AS \
             SELECT {cols} FROM mentat_query_view('{query}', '{{}}'::jsonb)",
            schema = quoted_schema,
            view = quote_ident(view_name),
            cols = col_defs,
            query = escaped_query,
        )
    } else {
        format!(
            "CREATE MATERIALIZED VIEW {schema}.{view} AS \
             SELECT {cols} FROM mentat_query_view_store('{store}', '{query}', '{{}}'::jsonb)",
            schema = quoted_schema,
            view = quote_ident(view_name),
            store = store_name.replace('\'', "''"),
            cols = col_defs,
            query = escaped_query,
        )
    };

    Spi::run(&create_sql)?;

    // Set up on_write trigger if requested
    if refresh_policy == "on_write" {
        create_refresh_trigger(&quoted_schema, view_name)?;
    }

    // Register in metadata table
    Spi::run_with_args(
        "INSERT INTO mentat.materialized_views (store_name, view_name, datalog_query, refresh_policy) \
         VALUES ($1, $2, $3, $4)",
        &[
            DatumWithOid::from(store_name),
            DatumWithOid::from(view_name),
            DatumWithOid::from(datalog_query),
            DatumWithOid::from(refresh_policy),
        ],
    )?;

    let column_list = columns
        .iter()
        .map(|c| c.trim_start_matches('?').replace('/', "_"))
        .collect::<Vec<_>>()
        .join(", ");

    Ok(format!(
        "Materialized view '{}.{}' created with {} columns ({}) and refresh policy '{}'.",
        quoted_schema, view_name, col_count, column_list, refresh_policy
    ))
}

/// Manually refresh a materialized view.
///
/// Re-evaluates the underlying Datalog query and updates the cached results.
/// If `concurrently` is true, the refresh happens without locking the view
/// for reads (requires a unique index on the view).
///
/// # Arguments
/// - `store_name`: Name of the store the view belongs to
/// - `view_name`: Name of the materialized view to refresh
/// - `concurrently`: Whether to refresh concurrently (default: false)
///
/// # Example
/// ```sql
/// SELECT mentat_refresh('default', 'people_cache');
/// SELECT mentat_refresh('default', 'people_cache', true);
/// ```
#[pg_extern]
pub fn refresh(
    store_name: &str,
    view_name: &str,
    concurrently: default!(bool, "false"),
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    validate_store_name(store_name)?;
    validate_view_name(view_name)?;

    // Verify the view exists in metadata
    if !matview_exists(store_name, view_name)? {
        return Err(Box::new(MentatError::InvalidQuery {
            message: format!(
                "Materialized view '{}' not found for store '{}'.",
                view_name, store_name
            ),
            suggestion: Some(format!(
                "Use mentat_materialize('{}', '{}', '<query>') to create a view first.",
                store_name, view_name
            )),
        }));
    }

    let schema = get_schema_for_store(store_name);
    let quoted_schema = quote_ident(&schema);
    let qualified_view = format!("{}.{}", quoted_schema, quote_ident(view_name));

    let refresh_sql = if concurrently {
        format!("REFRESH MATERIALIZED VIEW CONCURRENTLY {}", qualified_view)
    } else {
        format!("REFRESH MATERIALIZED VIEW {}", qualified_view)
    };

    Spi::run(&refresh_sql)?;

    Ok(format!(
        "Materialized view '{}' refreshed{}.",
        qualified_view,
        if concurrently { " concurrently" } else { "" }
    ))
}

/// Drop a materialized view and clean up its metadata and triggers.
///
/// # Arguments
/// - `store_name`: Name of the store the view belongs to
/// - `view_name`: Name of the materialized view to drop
///
/// # Example
/// ```sql
/// SELECT mentat_drop_matview('default', 'people_cache');
/// ```
#[pg_extern]
pub fn drop_matview(
    store_name: &str,
    view_name: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    validate_store_name(store_name)?;
    validate_view_name(view_name)?;

    // Verify the view exists in metadata
    if !matview_exists(store_name, view_name)? {
        return Err(Box::new(MentatError::InvalidQuery {
            message: format!(
                "Materialized view '{}' not found for store '{}'.",
                view_name, store_name
            ),
            suggestion: None,
        }));
    }

    let schema = get_schema_for_store(store_name);
    let quoted_schema = quote_ident(&schema);

    // Drop any refresh trigger (safe even if none exists)
    drop_refresh_trigger(&quoted_schema, view_name)?;

    // Drop the materialized view
    let qualified_view = format!("{}.{}", quoted_schema, quote_ident(view_name));
    Spi::run(&format!(
        "DROP MATERIALIZED VIEW IF EXISTS {} CASCADE",
        qualified_view
    ))?;

    // Remove from metadata table
    Spi::run_with_args(
        "DELETE FROM mentat.materialized_views WHERE store_name = $1 AND view_name = $2",
        &[
            DatumWithOid::from(store_name),
            DatumWithOid::from(view_name),
        ],
    )?;

    Ok(format!(
        "Materialized view '{}' dropped from store '{}'.",
        view_name, store_name
    ))
}

/// List all materialized views, optionally filtered by store.
///
/// Returns a JSON array of view objects with store name, view name, query,
/// refresh policy, and creation time.
///
/// # Example
/// ```sql
/// SELECT mentat_list_matviews();
/// SELECT mentat_list_matviews('default');
/// ```
#[pg_extern]
pub fn list_matviews(
    store_name: default!(Option<&str>, "NULL"),
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let views = Spi::connect(|client| {
        let mut result = Vec::new();

        let rows = match store_name {
            Some(name) => client.select(
                "SELECT store_name, view_name, datalog_query, refresh_policy, created_at::TEXT \
                 FROM mentat.materialized_views WHERE store_name = $1 ORDER BY created_at",
                None,
                &[DatumWithOid::from(name)],
            )?,
            None => client.select(
                "SELECT store_name, view_name, datalog_query, refresh_policy, created_at::TEXT \
                 FROM mentat.materialized_views ORDER BY created_at",
                None,
                &[],
            )?,
        };

        for row in rows {
            let sname: String = row.get::<String>(1)?.unwrap_or_default();
            let vname: String = row.get::<String>(2)?.unwrap_or_default();
            let query_str: String = row.get::<String>(3)?.unwrap_or_default();
            let policy: String = row.get::<String>(4)?.unwrap_or_default();
            let created: String = row.get::<String>(5)?.unwrap_or_default();

            let schema = get_schema_for_store(&sname);

            result.push(json!({
                "store_name": sname,
                "view_name": vname,
                "qualified_name": format!("{}.{}", schema, vname),
                "datalog_query": query_str,
                "refresh_policy": policy,
                "created_at": created,
            }));
        }

        Ok::<_, pgrx::spi::SpiError>(result)
    })?;

    Ok(JsonB(json!(views)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_view_name_valid() {
        assert!(validate_view_name("my_view").is_ok());
        assert!(validate_view_name("view1").is_ok());
        assert!(validate_view_name("_private").is_ok());
        assert!(validate_view_name("a").is_ok());
    }

    #[test]
    fn test_validate_view_name_empty() {
        assert!(validate_view_name("").is_err());
    }

    #[test]
    fn test_validate_view_name_too_long() {
        let long_name = "a".repeat(59);
        assert!(validate_view_name(&long_name).is_err());
    }

    #[test]
    fn test_validate_view_name_starts_with_digit() {
        assert!(validate_view_name("1view").is_err());
    }

    #[test]
    fn test_validate_view_name_invalid_chars() {
        assert!(validate_view_name("my-view").is_err());
        assert!(validate_view_name("my view").is_err());
        assert!(validate_view_name("view!").is_err());
    }

    #[test]
    fn test_validate_refresh_policy_valid() {
        assert!(validate_refresh_policy("manual").is_ok());
        assert!(validate_refresh_policy("on_write").is_ok());
    }

    #[test]
    fn test_validate_refresh_policy_invalid() {
        assert!(validate_refresh_policy("").is_err());
        assert!(validate_refresh_policy("auto").is_err());
        assert!(validate_refresh_policy("on_read").is_err());
    }

    #[test]
    fn test_refresh_trigger_function_sql_contains_key_elements() {
        let sql = refresh_trigger_function_sql("mentat", "people_cache");
        assert!(sql.contains("mentat.mentat_matview_refresh_people_cache"));
        assert!(sql.contains("REFRESH MATERIALIZED VIEW mentat.people_cache"));
        assert!(sql.contains("RETURNS trigger"));
    }

    #[test]
    fn test_refresh_trigger_sql_format() {
        let sql = refresh_trigger_sql("mentat", "people_cache");
        assert!(sql.contains("mentat_matview_trg_people_cache"));
        assert!(sql.contains("AFTER INSERT OR UPDATE OR DELETE"));
        assert!(sql.contains("FOR EACH STATEMENT"));
        assert!(sql.contains("mentat.datoms"));
    }

    #[test]
    fn test_refresh_trigger_custom_schema() {
        let sql = refresh_trigger_function_sql("mentat_my_store", "my_view");
        assert!(sql.contains("mentat_my_store.mentat_matview_refresh_my_view"));
        assert!(sql.contains("REFRESH MATERIALIZED VIEW mentat_my_store.my_view"));
    }
}
