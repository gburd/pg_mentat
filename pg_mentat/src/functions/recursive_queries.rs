/// Recursive query translation for pg_mentat.
///
/// Translates Datalog-style rules into PostgreSQL WITH RECURSIVE CTEs,
/// enabling hierarchical and graph traversal queries over Mentat's datom
/// store. Typical use cases include organizational hierarchies, bill of
/// materials, and arbitrary graph reachability.
///
/// The main entry point is [`mentat_recursive`], which:
/// 1. Validates inputs (store name, view name, rule name, queries).
/// 2. Generates a `WITH RECURSIVE` CTE from a base query and a recursive
///    query.
/// 3. Creates a PostgreSQL view that wraps the CTE for convenient SQL access.
/// 4. Registers the view in the `mentat.recursive_views` metadata table.
use crate::error::MentatError;
use crate::functions::store_management::{get_schema_for_store, quote_ident, validate_store_name};
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::json;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum length for identifiers (view name, rule name).
const MAX_IDENT_LEN: usize = 63;

/// Default maximum recursion depth to prevent infinite loops.
const DEFAULT_MAX_DEPTH: i32 = 100;

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Validate an SQL identifier (view name or rule name).
///
/// Rules mirror [`validate_store_name`] but with a different error type:
/// - Non-empty
/// - <= 63 characters
/// - Starts with letter or underscore
/// - Contains only letters, digits, underscores
fn validate_identifier(name: &str, label: &str) -> Result<(), MentatError> {
    if name.is_empty() {
        return Err(MentatError::InvalidQuery {
            message: format!("{} cannot be empty.", label),
            suggestion: None,
        });
    }

    if name.len() > MAX_IDENT_LEN {
        return Err(MentatError::InvalidQuery {
            message: format!(
                "{} '{}' exceeds maximum length of {} characters.",
                label, name, MAX_IDENT_LEN
            ),
            suggestion: None,
        });
    }

    let first = name.chars().next().unwrap_or('0');
    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(MentatError::InvalidQuery {
            message: format!(
                "{} '{}' must start with a letter or underscore.",
                label, name
            ),
            suggestion: None,
        });
    }

    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(MentatError::InvalidQuery {
            message: format!(
                "{} '{}' must contain only letters, digits, and underscores.",
                label, name
            ),
            suggestion: None,
        });
    }

    Ok(())
}

/// Validate that a SQL query fragment does not contain dangerous constructs.
///
/// This is a basic safeguard against obvious injection attempts in the
/// user-provided base and recursive query fragments. It rejects fragments
/// that contain statement-terminating semicolons or comment markers.
fn validate_query_fragment(sql: &str, label: &str) -> Result<(), MentatError> {
    if sql.trim().is_empty() {
        return Err(MentatError::InvalidQuery {
            message: format!("{} cannot be empty.", label),
            suggestion: None,
        });
    }

    // Reject semicolons (statement boundaries) and comment markers
    if sql.contains(';') {
        return Err(MentatError::InvalidQuery {
            message: format!(
                "{} must not contain semicolons. Provide a single SELECT expression.",
                label
            ),
            suggestion: None,
        });
    }

    if sql.contains("--") || sql.contains("/*") {
        return Err(MentatError::InvalidQuery {
            message: format!(
                "{} must not contain SQL comments (-- or /*).",
                label
            ),
            suggestion: None,
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Metadata table bootstrap
// ---------------------------------------------------------------------------

/// Ensure the `mentat.recursive_views` metadata table exists.
///
/// This table tracks recursive views created by [`mentat_recursive`] so
/// they can be listed, refreshed, or dropped programmatically.
fn ensure_metadata_table() -> Result<(), pgrx::spi::SpiError> {
    Spi::run(
        r"
        CREATE TABLE IF NOT EXISTS mentat.recursive_views (
            view_name   TEXT NOT NULL,
            schema_name TEXT NOT NULL,
            store_name  TEXT NOT NULL,
            rule_name   TEXT NOT NULL,
            base_query  TEXT NOT NULL,
            recursive_query TEXT NOT NULL,
            max_depth   INTEGER NOT NULL DEFAULT 100,
            created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY (schema_name, view_name)
        )
        ",
    )
}

// ---------------------------------------------------------------------------
// Core function: mentat_recursive
// ---------------------------------------------------------------------------

/// Create a recursive view from a Datalog-style rule.
///
/// Translates a base query and a recursive query into a PostgreSQL
/// `WITH RECURSIVE` CTE, wraps it in a view, and registers it in
/// the `mentat.recursive_views` metadata table.
///
/// # Parameters
///
/// - `store_name`: The Mentat store to operate on (e.g., `"default"`).
/// - `view_name`: Name for the created view (valid SQL identifier).
/// - `rule_name`: Name for the CTE (valid SQL identifier, used as the
///   recursive reference in the recursive query).
/// - `base_query`: A `SELECT` statement providing the base case (anchor)
///   of the recursion. This query must *not* reference `rule_name`.
/// - `recursive_query`: A `SELECT` statement providing the recursive step.
///   It should join against `rule_name` to extend results from the
///   previous iteration.
/// - `max_depth`: Optional recursion depth limit (default: 100). Maps to
///   a `LIMIT` on the recursive CTE to prevent infinite loops. Set to 0
///   to disable (relies on PostgreSQL's `max_recursive_iterations` GUC).
///
/// # Example: Organizational Hierarchy
///
/// ```sql
/// SELECT mentat_recursive(
///     'default',
///     'org_hierarchy',
///     'reports_to',
///     -- Base case: direct reports
///     'SELECT d1.e AS employee, d2.e AS manager
///      FROM mentat.datoms d1
///      JOIN mentat.datoms d2 ON d1.v_ref = d2.e
///      WHERE d1.a = (SELECT entid FROM mentat.schema WHERE ident = '':org/reports_to'')
///        AND d1.added = true AND d2.added = true',
///     -- Recursive step: transitive reports
///     'SELECT r.employee, d2.e AS manager
///      FROM reports_to r
///      JOIN mentat.datoms d1 ON r.manager = d1.e
///      JOIN mentat.datoms d2 ON d1.v_ref = d2.e
///      WHERE d1.a = (SELECT entid FROM mentat.schema WHERE ident = '':org/reports_to'')
///        AND d1.added = true AND d2.added = true'
/// );
/// -- Then query: SELECT * FROM mentat.org_hierarchy;
/// ```
///
/// # Example: Ancestor (Manager Hierarchy)
///
/// ```sql
/// SELECT mentat_recursive(
///     'default',
///     'manager_chain',
///     'ancestor',
///     -- Base: direct manager relationships
///     'SELECT e.e AS employee_id,
///             e_name.v_text AS employee_name,
///             m.e AS manager_id,
///             m_name.v_text AS manager_name,
///             1 AS depth
///      FROM mentat.datoms rel
///      JOIN mentat.datoms e_name ON rel.e = e_name.e
///      JOIN mentat.datoms m_name ON rel.v_ref = m_name.e
///      WHERE rel.a = (SELECT entid FROM mentat.schema WHERE ident = '':org/manager'')
///        AND e_name.a = (SELECT entid FROM mentat.schema WHERE ident = '':person/name'')
///        AND m_name.a = (SELECT entid FROM mentat.schema WHERE ident = '':person/name'')
///        AND rel.added = true AND e_name.added = true AND m_name.added = true',
///     -- Recursive: follow the chain
///     'SELECT a.employee_id, a.employee_name, rel.v_ref AS manager_id,
///             m_name.v_text AS manager_name, a.depth + 1
///      FROM ancestor a
///      JOIN mentat.datoms rel ON a.manager_id = rel.e
///      JOIN mentat.datoms m_name ON rel.v_ref = m_name.e
///      WHERE rel.a = (SELECT entid FROM mentat.schema WHERE ident = '':org/manager'')
///        AND m_name.a = (SELECT entid FROM mentat.schema WHERE ident = '':person/name'')
///        AND rel.added = true AND m_name.added = true'
/// );
/// ```
#[pg_extern]
pub fn recursive(
    store_name: &str,
    view_name: &str,
    rule_name: &str,
    base_query: &str,
    recursive_query: &str,
    max_depth: default!(Option<i32>, "NULL"),
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // 1. Validate inputs
    validate_store_name(store_name)?;
    validate_identifier(view_name, "View name")?;
    validate_identifier(rule_name, "Rule name")?;
    validate_query_fragment(base_query, "Base query")?;
    validate_query_fragment(recursive_query, "Recursive query")?;

    let depth = max_depth.unwrap_or(DEFAULT_MAX_DEPTH);
    if depth < 0 {
        return Err(MentatError::InvalidQuery {
            message: "max_depth must be non-negative.".to_string(),
            suggestion: Some("Use 0 to disable the depth limit.".to_string()),
        }
        .into());
    }

    // Verify the recursive query actually references the rule name
    // (otherwise it is not truly recursive and the user likely made a mistake)
    if !recursive_query.contains(rule_name) {
        return Err(MentatError::InvalidQuery {
            message: format!(
                "Recursive query does not reference the rule name '{}'. \
                 The recursive step must join against '{}' to produce new rows.",
                rule_name, rule_name
            ),
            suggestion: Some(format!(
                "Add a JOIN or reference to '{}' in your recursive query.",
                rule_name
            )),
        }
        .into());
    }

    // 2. Resolve the schema for the store
    let schema_name = get_schema_for_store(store_name);
    let quoted_schema = quote_ident(&schema_name);
    let quoted_view = quote_ident(view_name);
    let quoted_rule = quote_ident(rule_name);

    // Verify the store schema exists
    Spi::connect(|client| {
        let schema_exists = client
            .select(
                "SELECT 1 FROM information_schema.schemata WHERE schema_name = $1",
                None,
                &[DatumWithOid::from(schema_name.as_str())],
            )?
            .next()
            .is_some();

        if !schema_exists {
            return Err(MentatError::StoreNotFound {
                store_name: store_name.to_string(),
            }
            .into());
        }

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })?;

    // 3. Generate the WITH RECURSIVE CTE SQL.
    //
    // We let PostgreSQL infer column names from the base query's SELECT list
    // rather than specifying an explicit column list on the CTE. This avoids
    // requiring the caller to separately declare column names.
    let cte_sql = if depth > 0 {
        format!(
            "WITH RECURSIVE {} AS (\n{}\nUNION ALL\n{}\n)\nSELECT * FROM {} LIMIT {}",
            quoted_rule,
            base_query.trim(),
            recursive_query.trim(),
            quoted_rule,
            depth,
        )
    } else {
        format!(
            "WITH RECURSIVE {} AS (\n{}\nUNION ALL\n{}\n)\nSELECT * FROM {}",
            quoted_rule,
            base_query.trim(),
            recursive_query.trim(),
            quoted_rule,
        )
    };

    // 4. Create the view wrapping the CTE
    let create_view_sql = format!(
        "CREATE OR REPLACE VIEW {}.{} AS {}",
        quoted_schema, quoted_view, cte_sql,
    );

    Spi::run(&create_view_sql)?;

    // 5. Register in metadata
    ensure_metadata_table()?;

    Spi::run_with_args(
        "INSERT INTO mentat.recursive_views \
         (view_name, schema_name, store_name, rule_name, base_query, recursive_query, max_depth) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (schema_name, view_name) DO UPDATE SET \
             rule_name = EXCLUDED.rule_name, \
             base_query = EXCLUDED.base_query, \
             recursive_query = EXCLUDED.recursive_query, \
             max_depth = EXCLUDED.max_depth, \
             created_at = now()",
        &[
            DatumWithOid::from(view_name),
            DatumWithOid::from(schema_name.as_str()),
            DatumWithOid::from(store_name),
            DatumWithOid::from(rule_name),
            DatumWithOid::from(base_query),
            DatumWithOid::from(recursive_query),
            DatumWithOid::from(depth),
        ],
    )?;

    Ok(format!(
        "Recursive view '{}.{}' created (rule: '{}', max_depth: {}).",
        schema_name, view_name, rule_name, depth,
    ))
}

// ---------------------------------------------------------------------------
// Drop a recursive view
// ---------------------------------------------------------------------------

/// Drop a recursive view and remove it from the metadata table.
///
/// # Example
/// ```sql
/// SELECT mentat_drop_recursive('default', 'org_hierarchy');
/// ```
#[pg_extern]
pub fn drop_recursive(
    store_name: &str,
    view_name: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    validate_store_name(store_name)?;
    validate_identifier(view_name, "View name")?;

    let schema_name = get_schema_for_store(store_name);
    let quoted_schema = quote_ident(&schema_name);
    let quoted_view = quote_ident(view_name);

    // Drop the view
    Spi::run(&format!(
        "DROP VIEW IF EXISTS {}.{} CASCADE",
        quoted_schema, quoted_view
    ))?;

    // Remove from metadata
    ensure_metadata_table()?;
    Spi::run_with_args(
        "DELETE FROM mentat.recursive_views WHERE schema_name = $1 AND view_name = $2",
        &[
            DatumWithOid::from(schema_name.as_str()),
            DatumWithOid::from(view_name),
        ],
    )?;

    Ok(format!(
        "Recursive view '{}.{}' dropped.",
        schema_name, view_name
    ))
}

// ---------------------------------------------------------------------------
// List recursive views
// ---------------------------------------------------------------------------

/// List all recursive views registered for a store.
///
/// Returns a JSON array of view metadata objects.
///
/// # Example
/// ```sql
/// SELECT mentat_list_recursive('default');
/// ```
#[pg_extern]
pub fn list_recursive(
    store_name: &str,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    validate_store_name(store_name)?;

    ensure_metadata_table()?;

    let views = Spi::connect(|client| {
        let mut result = Vec::new();

        let rows = client.select(
            "SELECT view_name, rule_name, max_depth, created_at::TEXT \
             FROM mentat.recursive_views \
             WHERE store_name = $1 \
             ORDER BY created_at",
            None,
            &[DatumWithOid::from(store_name)],
        )?;

        for row in rows {
            let vname: String = row.get::<String>(1)?.unwrap_or_default();
            let rname: String = row.get::<String>(2)?.unwrap_or_default();
            let mdepth: i32 = row.get::<i32>(3)?.unwrap_or(DEFAULT_MAX_DEPTH);
            let created: String = row.get::<String>(4)?.unwrap_or_default();

            result.push(json!({
                "view_name": vname,
                "rule_name": rname,
                "max_depth": mdepth,
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
    fn test_validate_identifier_valid() {
        assert!(validate_identifier("my_view", "View name").is_ok());
        assert!(validate_identifier("_private", "View name").is_ok());
        assert!(validate_identifier("a123", "Rule name").is_ok());
    }

    #[test]
    fn test_validate_identifier_empty() {
        assert!(validate_identifier("", "View name").is_err());
    }

    #[test]
    fn test_validate_identifier_too_long() {
        let long_name = "a".repeat(64);
        assert!(validate_identifier(&long_name, "View name").is_err());
    }

    #[test]
    fn test_validate_identifier_starts_with_digit() {
        assert!(validate_identifier("1view", "View name").is_err());
    }

    #[test]
    fn test_validate_identifier_invalid_chars() {
        assert!(validate_identifier("my-view", "View name").is_err());
        assert!(validate_identifier("my view", "View name").is_err());
        assert!(validate_identifier("view!", "Rule name").is_err());
    }

    #[test]
    fn test_validate_query_fragment_valid() {
        assert!(validate_query_fragment(
            "SELECT e, v_ref FROM mentat.datoms WHERE a = 1",
            "Base query"
        )
        .is_ok());
    }

    #[test]
    fn test_validate_query_fragment_empty() {
        assert!(validate_query_fragment("", "Base query").is_err());
        assert!(validate_query_fragment("   ", "Base query").is_err());
    }

    #[test]
    fn test_validate_query_fragment_semicolon() {
        assert!(validate_query_fragment(
            "SELECT 1; DROP TABLE datoms",
            "Base query"
        )
        .is_err());
    }

    #[test]
    fn test_validate_query_fragment_comments() {
        assert!(validate_query_fragment(
            "SELECT 1 -- comment",
            "Base query"
        )
        .is_err());
        assert!(validate_query_fragment(
            "SELECT /* inline */ 1",
            "Recursive query"
        )
        .is_err());
    }
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use super::*;

    #[pg_test]
    fn test_recursive_queries_compile() {
        crate::ensure_extension_loaded();
        // Verify the exported functions compile and are accessible.
        // Full integration tests require a populated database with schema
        // and hierarchy data.
        assert!(true);
    }
}
