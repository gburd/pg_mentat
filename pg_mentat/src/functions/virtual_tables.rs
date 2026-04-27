/// Virtual table views for pg_mentat stores.
///
/// Generates SQL views that provide human-readable access to the underlying
/// datoms/schema/idents tables. These views are created automatically when
/// a new store is provisioned via `mentat_create_store()` and can also be
/// regenerated on demand.
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
// View SQL generators
// ---------------------------------------------------------------------------

/// Generate SQL for the `entities` view.
///
/// Shows all distinct entities with their earliest and latest transaction timestamps.
///
/// Columns: entity_id, first_tx, last_tx, first_ts, last_ts, attribute_count
fn entities_view_sql(schema: &str) -> String {
    format!(
        r#"CREATE OR REPLACE VIEW {schema}.entities AS
SELECT
    d.e AS entity_id,
    MIN(d.tx) AS first_tx,
    MAX(d.tx) AS last_tx,
    MIN(t.tx_instant) AS first_ts,
    MAX(t.tx_instant) AS last_ts,
    COUNT(DISTINCT d.a) AS attribute_count
FROM {schema}.datoms d
JOIN {schema}.transactions t ON t.tx = d.tx
WHERE d.added = TRUE
GROUP BY d.e
ORDER BY d.e"#,
        schema = schema
    )
}

/// Generate SQL for the `attributes` view.
///
/// Shows schema information with human-readable attribute names.
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
/// and value type tags are resolved to readable forms.
///
/// Columns: entity_id, attribute, value, value_type, tx, tx_time
fn facts_view_sql(schema: &str) -> String {
    format!(
        r#"CREATE OR REPLACE VIEW {schema}.facts AS
SELECT
    d.e AS entity_id,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
    CASE d.value_type_tag
        WHEN 0  THEN COALESCE((SELECT ri.ident FROM {schema}.idents ri WHERE ri.entid = d.v_ref), d.v_ref::TEXT)
        WHEN 1  THEN d.v_bool::TEXT
        WHEN 2  THEN d.v_long::TEXT
        WHEN 3  THEN d.v_double::TEXT
        WHEN 4  THEN d.v_instant::TEXT
        WHEN 7  THEN d.v_text
        WHEN 8  THEN ':' || d.v_keyword
        WHEN 10 THEN d.v_uuid::TEXT
        WHEN 11 THEN encode(d.v_bytes, 'hex')
        ELSE '(unknown type ' || d.value_type_tag::TEXT || ')'
    END AS value,
    CASE d.value_type_tag
        WHEN 0  THEN 'ref'
        WHEN 1  THEN 'boolean'
        WHEN 2  THEN 'long'
        WHEN 3  THEN 'double'
        WHEN 4  THEN 'instant'
        WHEN 7  THEN 'string'
        WHEN 8  THEN 'keyword'
        WHEN 10 THEN 'uuid'
        WHEN 11 THEN 'bytes'
        ELSE 'unknown'
    END AS value_type,
    d.tx,
    t.tx_instant AS tx_time
FROM {schema}.datoms d
LEFT JOIN {schema}.schema s ON s.entid = d.a
LEFT JOIN {schema}.transactions t ON t.tx = d.tx
WHERE d.added = TRUE
ORDER BY d.e, d.a"#,
        schema = schema
    )
}

/// Generate SQL for type-specific value views.
///
/// Each view filters datoms to a single value type for convenient querying.
fn type_specific_views_sql(schema: &str) -> String {
    let types = [
        ("text_values", "7", "v_text", "d.v_text AS value"),
        ("numeric_values", "2", "v_long", "d.v_long AS value"),
        ("double_values", "3", "v_double", "d.v_double AS value"),
        ("boolean_values", "1", "v_bool", "d.v_bool AS value"),
        ("references", "0", "v_ref", "d.v_ref AS value"),
        ("instant_values", "4", "v_instant", "d.v_instant AS value"),
        ("uuid_values", "10", "v_uuid", "d.v_uuid AS value"),
        ("keyword_values", "8", "v_keyword", "d.v_keyword AS value"),
        ("bytes_values", "11", "v_bytes", "d.v_bytes AS value"),
    ];

    let mut sql = String::new();
    for (view_name, tag, _col, value_expr) in &types {
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
FROM {schema}.datoms d
LEFT JOIN {schema}.schema s ON s.entid = d.a
WHERE d.value_type_tag = {tag} AND d.added = TRUE
ORDER BY d.e, d.a"#,
            schema = schema,
            view_name = view_name,
            value_expr = value_expr,
            tag = tag,
        ));
    }
    sql
}

/// Generate SQL for the `searchable_text` view.
///
/// Provides a view with `to_tsvector` applied to text values for full-text search.
///
/// Columns: entity_id, attribute, value, search_vector, tx
fn searchable_text_view_sql(schema: &str) -> String {
    format!(
        r#"CREATE OR REPLACE VIEW {schema}.searchable_text AS
SELECT
    d.e AS entity_id,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
    d.v_text AS value,
    to_tsvector('english', d.v_text) AS search_vector,
    d.tx
FROM {schema}.datoms d
LEFT JOIN {schema}.schema s ON s.entid = d.a
WHERE d.value_type_tag = 7 AND d.added = TRUE AND d.v_text IS NOT NULL
ORDER BY d.e, d.a"#,
        schema = schema
    )
}

/// Generate SQL for the `entities_with_attribute()` helper function.
///
/// Returns all entities that have a given attribute ident asserted.
fn entities_with_attribute_fn_sql(schema: &str) -> String {
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
    SELECT DISTINCT d.e, d.tx
    FROM {schema}.datoms d
    WHERE d.a = attr_entid AND d.added = TRUE
    ORDER BY d.e;
END;
$$ LANGUAGE plpgsql STABLE"#,
        schema = schema
    )
}

/// Generate SQL for optional trigram indexes (requires pg_trgm extension).
///
/// Creates GIN trigram indexes on v_text and v_keyword columns for fast
/// LIKE/ILIKE and similarity searches.
fn trigram_indexes_sql(schema: &str, store_name: &str) -> String {
    format!(
        r#"CREATE INDEX IF NOT EXISTS idx_{name}_trgm_text
ON {schema}.datoms USING GIN (v_text gin_trgm_ops)
WHERE value_type_tag = 7 AND added = TRUE;

CREATE INDEX IF NOT EXISTS idx_{name}_trgm_keyword
ON {schema}.datoms USING GIN (v_keyword gin_trgm_ops)
WHERE value_type_tag = 8 AND added = TRUE"#,
        schema = schema,
        name = store_name
    )
}

/// Generate SQL for optional full-text search indexes (requires pg_textsearch
/// or built-in tsvector support).
///
/// Creates a GIN index on a generated tsvector column for full-text search.
fn fulltext_index_sql(schema: &str, store_name: &str) -> String {
    format!(
        r#"CREATE INDEX IF NOT EXISTS idx_{name}_fts_text
ON {schema}.datoms USING GIN (to_tsvector('english', COALESCE(v_text, '')))
WHERE value_type_tag = 7 AND added = TRUE"#,
        schema = schema,
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
    fn test_entities_view_sql_contains_group_by() {
        let sql = entities_view_sql("mentat");
        assert!(sql.contains("GROUP BY d.e"));
        assert!(sql.contains("mentat.datoms"));
        assert!(sql.contains("mentat.transactions"));
    }

    #[test]
    fn test_attributes_view_sql_references_schema_table() {
        let sql = attributes_view_sql("mentat");
        assert!(sql.contains("mentat.schema"));
        assert!(sql.contains("value_type"));
        assert!(sql.contains("cardinality"));
    }

    #[test]
    fn test_facts_view_sql_decodes_all_types() {
        let sql = facts_view_sql("mentat");
        assert!(sql.contains("WHEN 0"));  // ref
        assert!(sql.contains("WHEN 1"));  // boolean
        assert!(sql.contains("WHEN 2"));  // long
        assert!(sql.contains("WHEN 3"));  // double
        assert!(sql.contains("WHEN 4"));  // instant
        assert!(sql.contains("WHEN 7"));  // string
        assert!(sql.contains("WHEN 8"));  // keyword
        assert!(sql.contains("WHEN 10")); // uuid
        assert!(sql.contains("WHEN 11")); // bytes
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
    }

    #[test]
    fn test_searchable_text_view_sql_uses_tsvector() {
        let sql = searchable_text_view_sql("mentat");
        assert!(sql.contains("to_tsvector"));
        assert!(sql.contains("value_type_tag = 7"));
    }

    #[test]
    fn test_entities_with_attribute_fn_sql() {
        let sql = entities_with_attribute_fn_sql("mentat");
        assert!(sql.contains("FUNCTION mentat.entities_with_attribute"));
        assert!(sql.contains("attr_ident TEXT"));
        assert!(sql.contains("RETURN QUERY"));
    }

    #[test]
    fn test_trigram_indexes_sql() {
        let sql = trigram_indexes_sql("mentat", "default");
        assert!(sql.contains("gin_trgm_ops"));
        assert!(sql.contains("idx_default_trgm_text"));
        assert!(sql.contains("idx_default_trgm_keyword"));
    }

    #[test]
    fn test_fulltext_index_sql() {
        let sql = fulltext_index_sql("mentat", "default");
        assert!(sql.contains("to_tsvector"));
        assert!(sql.contains("idx_default_fts_text"));
    }

    #[test]
    fn test_custom_schema_name() {
        let sql = entities_view_sql("mentat_my_store");
        assert!(sql.contains("mentat_my_store.datoms"));
        assert!(sql.contains("mentat_my_store.entities"));
    }
}
