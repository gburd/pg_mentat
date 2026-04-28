/// Virtual table views for pg_mentat stores.
///
/// Generates SQL views that provide human-readable access to the underlying
/// type-specific datoms tables, schema, and idents. These views are created
/// automatically when a new store is provisioned via `mentat_create_store()`
/// and can also be regenerated on demand.
///
/// As of Phase 3, views query the type-specific tables (`mentat.datoms_*_new`)
/// via UNION ALL instead of the legacy wide `{schema}.datoms` table.
use crate::functions::store_management::{get_schema_for_store, quote_ident, validate_store_name};
use pgrx::prelude::*;

// ---------------------------------------------------------------------------
// Extension detection
// ---------------------------------------------------------------------------

/// Check whether a PostgreSQL extension is installed in the current database.
fn extension_available(ext_name: &str) -> bool {
    let query = format!(
        "SELECT 1 FROM pg_extension WHERE extname = '{}'",
        ext_name.replace('\'', "''")
    );
    Spi::get_one::<i64>(&query)
        .ok()
        .flatten()
        .is_some()
}

// ---------------------------------------------------------------------------
// Helpers for type-specific table queries
// ---------------------------------------------------------------------------

/// Generate a subquery that resolves a schema name to a store_id.
///
/// Schema "mentat" maps to store_name "default"; "mentat_foo" maps to "foo".
fn store_id_subquery(schema: &str) -> String {
    let store_name = if schema == "mentat" {
        "default".to_string()
    } else if let Some(name) = schema.strip_prefix("mentat_") {
        name.to_string()
    } else {
        // Fallback: treat the whole schema name as the store name
        schema.to_string()
    };
    format!(
        "(SELECT store_id FROM mentat.stores WHERE store_name = '{}')",
        store_name.replace('\'', "''")
    )
}

/// Generate a UNION ALL query across all 9 type-specific tables that projects
/// columns in a unified format compatible with the old wide-row layout.
///
/// Projected columns: e, a, value_type_tag, v_text (value as text), tx
///
/// The `extra_where` parameter is appended to each leg's WHERE clause (must
/// start with " AND ..." if non-empty).
fn all_datoms_union_sql(store_id_expr: &str, extra_where: &str) -> String {
    let legs = [
        (
            "mentat.datoms_ref_new",
            0,
            "v::text",
        ),
        (
            "mentat.datoms_boolean_new",
            1,
            "v::text",
        ),
        (
            "mentat.datoms_long_new",
            2,
            "v::text",
        ),
        (
            "mentat.datoms_double_new",
            3,
            "v::text",
        ),
        (
            "mentat.datoms_instant_new",
            4,
            "v::text",
        ),
        (
            "mentat.datoms_text_new",
            7,
            "v",
        ),
        (
            "mentat.datoms_keyword_new",
            8,
            "v",
        ),
        (
            "mentat.datoms_uuid_new",
            10,
            "v::text",
        ),
        (
            "mentat.datoms_bytes_new",
            11,
            "encode(v, 'hex')",
        ),
    ];

    legs.iter()
        .map(|(table, tag, v_expr)| {
            format!(
                "SELECT e, a, {tag} AS value_type_tag, {v_expr} AS v_text, tx \
                 FROM {table} \
                 WHERE store_id = {sid} AND added = true{extra}",
                tag = tag,
                v_expr = v_expr,
                table = table,
                sid = store_id_expr,
                extra = extra_where,
            )
        })
        .collect::<Vec<_>>()
        .join("\nUNION ALL\n")
}

// ---------------------------------------------------------------------------
// View SQL generators
// ---------------------------------------------------------------------------

/// Generate SQL for the `entities` view.
///
/// Shows all distinct entities with their earliest and latest transaction timestamps.
///
/// Columns: entity_id, first_tx, last_tx, first_ts, last_ts, attribute_count
fn entities_view_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);
    let union = all_datoms_union_sql(&sid, "");
    format!(
        r#"CREATE OR REPLACE VIEW {schema}.entities AS
SELECT
    d.e AS entity_id,
    MIN(d.tx) AS first_tx,
    MAX(d.tx) AS last_tx,
    MIN(t.tx_instant) AS first_ts,
    MAX(t.tx_instant) AS last_ts,
    COUNT(DISTINCT d.a) AS attribute_count
FROM (
{union}
) d
JOIN {schema}.transactions t ON t.tx = d.tx
GROUP BY d.e
ORDER BY d.e"#,
        schema = schema,
        union = union,
    )
}

/// Generate SQL for the `attributes` view.
///
/// Shows schema information with human-readable attribute names.
/// This view does not reference datoms tables so no changes are needed.
///
/// Columns: entid, ident, value_type, cardinality, unique_constraint, indexed,
///          fulltext, component, no_history
fn attributes_view_sql(schema: &str) -> String {
    format!(
        r#"CREATE OR REPLACE VIEW {schema}.attributes AS
SELECT
    s.entid,
    s.ident,
    s.value_type::TEXT AS value_type,
    s.cardinality::TEXT AS cardinality,
    s.unique_constraint::TEXT AS unique_constraint,
    s.indexed,
    s.fulltext,
    s.component,
    s.no_history
FROM {schema}.schema s
ORDER BY s.entid"#,
        schema = schema
    )
}

/// Generate SQL for the `facts` view.
///
/// Shows a human-readable EAVT representation where entity IDs, attribute IDs,
/// and value type tags are resolved to readable forms.  Each UNION leg directly
/// produces the display value and type name, avoiding the old CASE expression
/// over value_type_tag.
///
/// Columns: entity_id, attribute, value, value_type, tx, tx_time
fn facts_view_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);

    // Each leg produces: e, a, value (text), value_type (text), tx
    let legs = [
        (
            "mentat.datoms_ref_new",
            "ref",
            format!(
                "COALESCE((SELECT ri.ident FROM {schema}.idents ri WHERE ri.entid = d.v), d.v::TEXT)",
                schema = schema
            ),
        ),
        (
            "mentat.datoms_boolean_new",
            "boolean",
            "d.v::TEXT".to_string(),
        ),
        (
            "mentat.datoms_long_new",
            "long",
            "d.v::TEXT".to_string(),
        ),
        (
            "mentat.datoms_double_new",
            "double",
            "d.v::TEXT".to_string(),
        ),
        (
            "mentat.datoms_instant_new",
            "instant",
            "d.v::TEXT".to_string(),
        ),
        (
            "mentat.datoms_text_new",
            "string",
            "d.v".to_string(),
        ),
        (
            "mentat.datoms_keyword_new",
            "keyword",
            "':' || d.v".to_string(),
        ),
        (
            "mentat.datoms_uuid_new",
            "uuid",
            "d.v::TEXT".to_string(),
        ),
        (
            "mentat.datoms_bytes_new",
            "bytes",
            "encode(d.v, 'hex')".to_string(),
        ),
    ];

    let union = legs
        .iter()
        .map(|(table, type_name, v_expr)| {
            format!(
                "SELECT d.e, d.a, {v_expr} AS value, '{type_name}' AS value_type, d.tx \
                 FROM {table} d \
                 WHERE d.store_id = {sid} AND d.added = true",
                v_expr = v_expr,
                type_name = type_name,
                table = table,
                sid = sid,
            )
        })
        .collect::<Vec<_>>()
        .join("\nUNION ALL\n");

    format!(
        r#"CREATE OR REPLACE VIEW {schema}.facts AS
SELECT
    d.e AS entity_id,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
    d.value,
    d.value_type,
    d.tx,
    t.tx_instant AS tx_time
FROM (
{union}
) d
LEFT JOIN {schema}.schema s ON s.entid = d.a
LEFT JOIN {schema}.transactions t ON t.tx = d.tx
ORDER BY d.e, d.a"#,
        schema = schema,
        union = union,
    )
}

/// Generate SQL for type-specific value views.
///
/// Each view now queries its corresponding type-specific table directly,
/// which is simpler and more efficient than filtering the wide datoms table
/// by value_type_tag.
fn type_specific_views_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);

    // (view_name, table, value_expr)
    let types = [
        ("text_values", "mentat.datoms_text_new", "d.v AS value"),
        ("numeric_values", "mentat.datoms_long_new", "d.v AS value"),
        ("double_values", "mentat.datoms_double_new", "d.v AS value"),
        ("boolean_values", "mentat.datoms_boolean_new", "d.v AS value"),
        ("references", "mentat.datoms_ref_new", "d.v AS value"),
        ("instant_values", "mentat.datoms_instant_new", "d.v AS value"),
        ("uuid_values", "mentat.datoms_uuid_new", "d.v AS value"),
        ("keyword_values", "mentat.datoms_keyword_new", "d.v AS value"),
        ("bytes_values", "mentat.datoms_bytes_new", "d.v AS value"),
    ];

    let mut sql = String::new();
    for (view_name, table, value_expr) in &types {
        if !sql.is_empty() {
            sql.push_str(";\n");
        }
        sql.push_str(&format!(
            r#"CREATE OR REPLACE VIEW {schema}.{view_name} AS
SELECT
    d.e AS entity_id,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
    {value_expr},
    d.tx
FROM {table} d
LEFT JOIN {schema}.schema s ON s.entid = d.a
WHERE d.store_id = {sid} AND d.added = true
ORDER BY d.e, d.a"#,
            schema = schema,
            view_name = view_name,
            value_expr = value_expr,
            table = table,
            sid = sid,
        ));
    }
    sql
}

/// Generate SQL for the `searchable_text` view.
///
/// Provides a view with `to_tsvector` applied to text values for full-text search.
/// Now queries `mentat.datoms_text_new` directly.
///
/// Columns: entity_id, attribute, value, search_vector, tx
fn searchable_text_view_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);
    format!(
        r#"CREATE OR REPLACE VIEW {schema}.searchable_text AS
SELECT
    d.e AS entity_id,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
    d.v AS value,
    to_tsvector('english', d.v) AS search_vector,
    d.tx
FROM mentat.datoms_text_new d
LEFT JOIN {schema}.schema s ON s.entid = d.a
WHERE d.store_id = {sid} AND d.added = true AND d.v IS NOT NULL
ORDER BY d.e, d.a"#,
        schema = schema,
        sid = sid,
    )
}

/// Generate SQL for the `entities_with_attribute()` helper function.
///
/// Returns all entities that have a given attribute ident asserted.
/// Now queries across all type-specific tables with UNION ALL.
fn entities_with_attribute_fn_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);

    let tables = [
        "mentat.datoms_ref_new",
        "mentat.datoms_boolean_new",
        "mentat.datoms_long_new",
        "mentat.datoms_double_new",
        "mentat.datoms_instant_new",
        "mentat.datoms_text_new",
        "mentat.datoms_keyword_new",
        "mentat.datoms_uuid_new",
        "mentat.datoms_bytes_new",
    ];

    let union = tables
        .iter()
        .map(|table| {
            format!(
                "SELECT DISTINCT d.e, d.tx FROM {table} d \
                 WHERE d.store_id = {sid} AND d.a = attr_entid AND d.added = true",
                table = table,
                sid = sid,
            )
        })
        .collect::<Vec<_>>()
        .join("\n        UNION ALL\n        ");

    format!(
        r#"CREATE OR REPLACE FUNCTION {schema}.entities_with_attribute(attr_ident TEXT)
RETURNS TABLE(entity_id BIGINT, tx BIGINT)
AS $$
DECLARE
    attr_entid BIGINT;
BEGIN
    SELECT entid INTO attr_entid FROM {schema}.schema WHERE ident = attr_ident;
    IF attr_entid IS NULL THEN
        RAISE EXCEPTION 'Unknown attribute ident: %', attr_ident;
    END IF;

    RETURN QUERY
    SELECT DISTINCT sub.e, sub.tx FROM (
        {union}
    ) sub
    ORDER BY sub.e;
END;
$$ LANGUAGE plpgsql STABLE"#,
        schema = schema,
        union = union,
    )
}

/// Generate SQL for optional trigram indexes (requires pg_trgm extension).
///
/// Creates GIN trigram indexes on the text and keyword type-specific tables
/// for fast LIKE/ILIKE and similarity searches.
fn trigram_indexes_sql(_schema: &str, store_name: &str) -> String {
    format!(
        r#"CREATE INDEX IF NOT EXISTS idx_{name}_trgm_text
ON mentat.datoms_text_new USING GIN (v gin_trgm_ops)
WHERE added = true;

CREATE INDEX IF NOT EXISTS idx_{name}_trgm_keyword
ON mentat.datoms_keyword_new USING GIN (v gin_trgm_ops)
WHERE added = true"#,
        name = store_name
    )
}

/// Generate SQL for optional full-text search indexes (requires pg_textsearch
/// or built-in tsvector support).
///
/// Creates a GIN index on a generated tsvector column for full-text search.
fn fulltext_index_sql(_schema: &str, store_name: &str) -> String {
    format!(
        r#"CREATE INDEX IF NOT EXISTS idx_{name}_fts_text
ON mentat.datoms_text_new USING GIN (to_tsvector('english', COALESCE(v, '')))
WHERE added = true"#,
        name = store_name
    )
}

// ---------------------------------------------------------------------------
// Public API: create all views for a store
// ---------------------------------------------------------------------------

/// Create all virtual table views for a given store schema.
///
/// This is called internally by `mentat_create_store()` and can also be
/// invoked directly to regenerate views for an existing store.
pub fn create_virtual_tables_for_schema(
    schema: &str,
    store_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Core views
    Spi::run(&entities_view_sql(schema))?;
    Spi::run(&attributes_view_sql(schema))?;
    Spi::run(&facts_view_sql(schema))?;

    // Type-specific views
    Spi::run(&type_specific_views_sql(schema))?;

    // Full-text search view
    Spi::run(&searchable_text_view_sql(schema))?;

    // Helper function
    Spi::run(&entities_with_attribute_fn_sql(schema))?;

    // Optional extension-dependent indexes
    if extension_available("pg_trgm") {
        Spi::run(&trigram_indexes_sql(schema, store_name))?;
    }

    // Full-text search indexes use built-in tsvector; always available in PG 12+.
    // The pg_textsearch extension enhances this with additional dictionaries.
    Spi::run(&fulltext_index_sql(schema, store_name))?;

    // NOTE: pg_vector support is planned for future implementation.
    // When available, this will create HNSW or IVFFlat indexes on embedding
    // columns for semantic similarity search over entity attributes.

    Ok(())
}

// ---------------------------------------------------------------------------
// SQL-callable function
// ---------------------------------------------------------------------------

/// Regenerate all virtual table views for a named store.
///
/// This recreates the standard views (entities, attributes, facts, type-specific
/// views, searchable_text) and the `entities_with_attribute()` helper function
/// in the store's schema. It also creates extension-dependent indexes if the
/// relevant extensions (pg_trgm, etc.) are installed.
///
/// # Example
/// ```sql
/// SELECT mentat_create_virtual_tables('my_store');
/// ```
#[pg_extern]
pub fn create_virtual_tables(
    store_name: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    validate_store_name(store_name)?;

    let schema_name = get_schema_for_store(store_name);
    let quoted_schema = quote_ident(&schema_name);

    create_virtual_tables_for_schema(&quoted_schema, store_name)?;

    Ok(format!(
        "Virtual tables created for store '{}'.",
        store_name
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_id_subquery_default() {
        let sq = store_id_subquery("mentat");
        assert!(sq.contains("store_name = 'default'"));
    }

    #[test]
    fn test_store_id_subquery_custom() {
        let sq = store_id_subquery("mentat_my_store");
        assert!(sq.contains("store_name = 'my_store'"));
    }

    #[test]
    fn test_all_datoms_union_sql_covers_all_tables() {
        let sql = all_datoms_union_sql("42", "");
        assert!(sql.contains("mentat.datoms_ref_new"));
        assert!(sql.contains("mentat.datoms_boolean_new"));
        assert!(sql.contains("mentat.datoms_long_new"));
        assert!(sql.contains("mentat.datoms_double_new"));
        assert!(sql.contains("mentat.datoms_instant_new"));
        assert!(sql.contains("mentat.datoms_text_new"));
        assert!(sql.contains("mentat.datoms_keyword_new"));
        assert!(sql.contains("mentat.datoms_uuid_new"));
        assert!(sql.contains("mentat.datoms_bytes_new"));
        assert!(sql.contains("UNION ALL"));
        assert!(sql.contains("store_id = 42"));
    }

    #[test]
    fn test_entities_view_sql_uses_union() {
        let sql = entities_view_sql("mentat");
        assert!(sql.contains("UNION ALL"));
        assert!(sql.contains("mentat.datoms_ref_new"));
        assert!(sql.contains("GROUP BY d.e"));
        assert!(sql.contains("mentat.transactions"));
        assert!(!sql.contains("mentat.datoms d"));
    }

    #[test]
    fn test_attributes_view_sql_references_schema_table() {
        let sql = attributes_view_sql("mentat");
        assert!(sql.contains("mentat.schema"));
        assert!(sql.contains("value_type"));
        assert!(sql.contains("cardinality"));
    }

    #[test]
    fn test_facts_view_sql_uses_type_specific_tables() {
        let sql = facts_view_sql("mentat");
        assert!(sql.contains("mentat.datoms_ref_new"));
        assert!(sql.contains("mentat.datoms_boolean_new"));
        assert!(sql.contains("mentat.datoms_long_new"));
        assert!(sql.contains("mentat.datoms_double_new"));
        assert!(sql.contains("mentat.datoms_instant_new"));
        assert!(sql.contains("mentat.datoms_text_new"));
        assert!(sql.contains("mentat.datoms_keyword_new"));
        assert!(sql.contains("mentat.datoms_uuid_new"));
        assert!(sql.contains("mentat.datoms_bytes_new"));
        assert!(sql.contains("'ref'"));
        assert!(sql.contains("'boolean'"));
        assert!(sql.contains("'long'"));
        assert!(sql.contains("'double'"));
        assert!(sql.contains("'instant'"));
        assert!(sql.contains("'string'"));
        assert!(sql.contains("'keyword'"));
        assert!(sql.contains("'uuid'"));
        assert!(sql.contains("'bytes'"));
        assert!(sql.contains("UNION ALL"));
    }

    #[test]
    fn test_type_specific_views_sql_creates_all_views() {
        let sql = type_specific_views_sql("mentat_test");
        assert!(sql.contains("mentat_test.text_values"));
        assert!(sql.contains("mentat_test.numeric_values"));
        assert!(sql.contains("mentat_test.double_values"));
        assert!(sql.contains("mentat_test.boolean_values"));
        assert!(sql.contains("mentat_test.references"));
        assert!(sql.contains("mentat_test.instant_values"));
        assert!(sql.contains("mentat_test.uuid_values"));
        assert!(sql.contains("mentat_test.keyword_values"));
        assert!(sql.contains("mentat_test.bytes_values"));
        // Should query type-specific tables, not the wide datoms table
        assert!(sql.contains("mentat.datoms_text_new"));
        assert!(sql.contains("mentat.datoms_long_new"));
        assert!(sql.contains("mentat.datoms_ref_new"));
        assert!(!sql.contains("value_type_tag"));
    }

    #[test]
    fn test_searchable_text_view_sql_uses_text_table() {
        let sql = searchable_text_view_sql("mentat");
        assert!(sql.contains("to_tsvector"));
        assert!(sql.contains("mentat.datoms_text_new"));
        assert!(!sql.contains("value_type_tag"));
    }

    #[test]
    fn test_entities_with_attribute_fn_sql() {
        let sql = entities_with_attribute_fn_sql("mentat");
        assert!(sql.contains("FUNCTION mentat.entities_with_attribute"));
        assert!(sql.contains("attr_ident TEXT"));
        assert!(sql.contains("RETURN QUERY"));
        assert!(sql.contains("UNION ALL"));
        assert!(sql.contains("mentat.datoms_ref_new"));
        assert!(sql.contains("mentat.datoms_text_new"));
    }

    #[test]
    fn test_trigram_indexes_sql_uses_type_specific_tables() {
        let sql = trigram_indexes_sql("mentat", "default");
        assert!(sql.contains("gin_trgm_ops"));
        assert!(sql.contains("idx_default_trgm_text"));
        assert!(sql.contains("idx_default_trgm_keyword"));
        assert!(sql.contains("mentat.datoms_text_new"));
        assert!(sql.contains("mentat.datoms_keyword_new"));
    }

    #[test]
    fn test_fulltext_index_sql_uses_text_table() {
        let sql = fulltext_index_sql("mentat", "default");
        assert!(sql.contains("to_tsvector"));
        assert!(sql.contains("idx_default_fts_text"));
        assert!(sql.contains("mentat.datoms_text_new"));
    }

    #[test]
    fn test_custom_schema_name() {
        let sql = entities_view_sql("mentat_my_store");
        assert!(sql.contains("UNION ALL"));
        assert!(sql.contains("mentat_my_store.entities"));
        assert!(sql.contains("store_name = 'my_store'"));
    }
}
