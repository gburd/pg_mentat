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
    Spi::connect(|client| {
        client
            .select(&query, Some(1), &[])
            .map(|t| t.first().get_one::<i64>().ok().flatten().is_some())
    })
    .unwrap_or(false)
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
        ("mentat.datoms_ref_new", 0, "v::text"),
        ("mentat.datoms_boolean_new", 1, "v::text"),
        ("mentat.datoms_long_new", 2, "v::text"),
        ("mentat.datoms_double_new", 3, "v::text"),
        ("mentat.datoms_instant_new", 4, "v::text"),
        ("mentat.datoms_text_new", 7, "v"),
        ("mentat.datoms_keyword_new", 8, "v"),
        ("mentat.datoms_uuid_new", 10, "v::text"),
        ("mentat.datoms_bytes_new", 11, "encode(v, 'hex')"),
    ];

    legs.iter()
        .map(|(table, tag, v_expr)| {
            format!(
                "SELECT e, a, {tag}::SMALLINT AS value_type_tag, {v_expr} AS v_text, tx \
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
        r"CREATE OR REPLACE VIEW {schema}.entities AS
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
ORDER BY d.e",
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
        r"CREATE OR REPLACE VIEW {schema}.attributes AS
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
ORDER BY s.entid",
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
        r"CREATE OR REPLACE VIEW {schema}.facts AS
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
ORDER BY d.e, d.a",
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
        (
            "boolean_values",
            "mentat.datoms_boolean_new",
            "d.v AS value",
        ),
        ("references", "mentat.datoms_ref_new", "d.v AS value"),
        (
            "instant_values",
            "mentat.datoms_instant_new",
            "d.v AS value",
        ),
        ("uuid_values", "mentat.datoms_uuid_new", "d.v AS value"),
        (
            "keyword_values",
            "mentat.datoms_keyword_new",
            "d.v AS value",
        ),
        ("bytes_values", "mentat.datoms_bytes_new", "d.v AS value"),
    ];

    let mut sql = String::new();
    for (view_name, table, value_expr) in &types {
        if !sql.is_empty() {
            sql.push_str(";\n");
        }
        sql.push_str(&format!(
            r"CREATE OR REPLACE VIEW {schema}.{view_name} AS
SELECT
    d.e AS entity_id,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
    {value_expr},
    d.tx
FROM {table} d
LEFT JOIN {schema}.schema s ON s.entid = d.a
WHERE d.store_id = {sid} AND d.added = true
ORDER BY d.e, d.a",
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
        r"CREATE OR REPLACE VIEW {schema}.searchable_text AS
SELECT
    d.e AS entity_id,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
    d.v AS value,
    to_tsvector('english', d.v) AS search_vector,
    d.tx
FROM mentat.datoms_text_new d
LEFT JOIN {schema}.schema s ON s.entid = d.a
WHERE d.store_id = {sid} AND d.added = true AND d.v IS NOT NULL
ORDER BY d.e, d.a",
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
        r"CREATE OR REPLACE FUNCTION {schema}.entities_with_attribute(attr_ident TEXT)
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
$$ LANGUAGE plpgsql STABLE",
        schema = schema,
        union = union,
    )
}

// ---------------------------------------------------------------------------
// Relationship navigation views
// ---------------------------------------------------------------------------

/// Generate SQL for the `entity_references` view.
///
/// Shows all reference relationships between entities with human-readable
/// attribute names. This is the core view for navigating entity graphs.
///
/// Columns: source_entity, attribute, target_entity, target_ident, tx
fn entity_references_view_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);
    format!(
        r"CREATE OR REPLACE VIEW {schema}.entity_references AS
SELECT
    d.e AS source_entity,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
    d.v AS target_entity,
    i.ident AS target_ident,
    d.tx
FROM mentat.datoms_ref_new d
LEFT JOIN {schema}.schema s ON s.entid = d.a
LEFT JOIN {schema}.idents i ON i.entid = d.v
WHERE d.store_id = {sid} AND d.added = true
ORDER BY d.e, d.a",
        schema = schema,
        sid = sid,
    )
}

/// Generate SQL for the `reverse_references` view.
///
/// Shows which entities reference a given entity (reverse navigation).
/// Useful for answering "who references entity X?" questions.
///
/// Columns: target_entity, attribute, source_entity, tx
fn reverse_references_view_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);
    format!(
        r"CREATE OR REPLACE VIEW {schema}.reverse_references AS
SELECT
    d.v AS target_entity,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
    d.e AS source_entity,
    d.tx
FROM mentat.datoms_ref_new d
LEFT JOIN {schema}.schema s ON s.entid = d.a
WHERE d.store_id = {sid} AND d.added = true
ORDER BY d.v, d.a",
        schema = schema,
        sid = sid,
    )
}

/// Generate SQL for the `graph_edges` view.
///
/// Treats all reference datoms as directed edges in a graph. Useful for
/// graph traversal queries using recursive CTEs.
///
/// Columns: source, edge_type, target, tx
fn graph_edges_view_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);
    format!(
        r"CREATE OR REPLACE VIEW {schema}.graph_edges AS
SELECT
    d.e AS source,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS edge_type,
    d.v AS target,
    d.tx
FROM mentat.datoms_ref_new d
LEFT JOIN {schema}.schema s ON s.entid = d.a
WHERE d.store_id = {sid} AND d.added = true",
        schema = schema,
        sid = sid,
    )
}

// ---------------------------------------------------------------------------
// Transaction history and temporal views
// ---------------------------------------------------------------------------

/// Generate SQL for the `tx_log` view.
///
/// Shows the transaction log with timestamps, datom counts per transaction,
/// and the types of changes made. Gives SQL users a chronological audit trail.
///
/// Columns: tx, tx_time, datom_count
fn tx_log_view_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);
    let union = all_datoms_union_sql(&sid, "");
    format!(
        r"CREATE OR REPLACE VIEW {schema}.tx_log AS
SELECT
    t.tx,
    t.tx_instant AS tx_time,
    COALESCE(d.datom_count, 0) AS datom_count
FROM {schema}.transactions t
LEFT JOIN (
    SELECT tx, COUNT(*) AS datom_count
    FROM (
{union}
    ) sub
    GROUP BY tx
) d ON d.tx = t.tx
ORDER BY t.tx DESC",
        schema = schema,
        union = union,
    )
}

/// Generate SQL for the `entity_history` view.
///
/// Shows how entity attributes changed over time by including both
/// assertions (added=true) and retractions (added=false) from all
/// type-specific tables.
///
/// Columns: entity_id, attribute, value, value_type, tx, tx_time, operation
fn entity_history_view_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);

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
        ("mentat.datoms_long_new", "long", "d.v::TEXT".to_string()),
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
        ("mentat.datoms_text_new", "string", "d.v".to_string()),
        (
            "mentat.datoms_keyword_new",
            "keyword",
            "':' || d.v".to_string(),
        ),
        ("mentat.datoms_uuid_new", "uuid", "d.v::TEXT".to_string()),
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
                "SELECT d.e, d.a, {v_expr} AS value, '{type_name}' AS value_type, d.tx, d.added \
                 FROM {table} d \
                 WHERE d.store_id = {sid}",
                v_expr = v_expr,
                type_name = type_name,
                table = table,
                sid = sid,
            )
        })
        .collect::<Vec<_>>()
        .join("\nUNION ALL\n");

    format!(
        r"CREATE OR REPLACE VIEW {schema}.entity_history AS
SELECT
    d.e AS entity_id,
    COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
    d.value,
    d.value_type,
    d.tx,
    t.tx_instant AS tx_time,
    CASE WHEN d.added THEN 'assert' ELSE 'retract' END AS operation
FROM (
{union}
) d
LEFT JOIN {schema}.schema s ON s.entid = d.a
LEFT JOIN {schema}.transactions t ON t.tx = d.tx
ORDER BY d.tx DESC, d.e, d.a",
        schema = schema,
        union = union,
    )
}

/// Generate SQL for the `recent_changes` view.
///
/// Shows the most recent assertions, limited to the last 100 transactions.
/// Useful for monitoring and audit queries without scanning all history.
///
/// Columns: entity_id, attribute, value, value_type, tx, tx_time
fn recent_changes_view_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);

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
        ("mentat.datoms_long_new", "long", "d.v::TEXT".to_string()),
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
        ("mentat.datoms_text_new", "string", "d.v".to_string()),
        (
            "mentat.datoms_keyword_new",
            "keyword",
            "':' || d.v".to_string(),
        ),
        ("mentat.datoms_uuid_new", "uuid", "d.v::TEXT".to_string()),
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
                 WHERE d.store_id = {sid} AND d.added = true \
                 AND d.tx >= (SELECT COALESCE(MAX(tx) - 100, 0) FROM {schema}.transactions)",
                v_expr = v_expr,
                type_name = type_name,
                table = table,
                sid = sid,
                schema = schema,
            )
        })
        .collect::<Vec<_>>()
        .join("\nUNION ALL\n");

    format!(
        r"CREATE OR REPLACE VIEW {schema}.recent_changes AS
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
ORDER BY d.tx DESC, d.e, d.a",
        schema = schema,
        union = union,
    )
}

// ---------------------------------------------------------------------------
// Schema summary and statistics views
// ---------------------------------------------------------------------------

/// Generate SQL for the `schema_summary` view.
///
/// Shows each user-defined attribute with usage statistics: how many entities
/// use each attribute and the total number of asserted datoms for it.
/// Excludes system attributes (entid < 100) by default.
///
/// Columns: ident, value_type, cardinality, unique_constraint, indexed,
///          entity_count, datom_count
fn schema_summary_view_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);
    let union = all_datoms_union_sql(&sid, "");
    format!(
        r"CREATE OR REPLACE VIEW {schema}.schema_summary AS
SELECT
    s.ident,
    s.value_type::TEXT AS value_type,
    s.cardinality::TEXT AS cardinality,
    s.unique_constraint::TEXT AS unique_constraint,
    s.indexed,
    s.fulltext,
    COALESCE(u.entity_count, 0) AS entity_count,
    COALESCE(u.datom_count, 0) AS datom_count
FROM {schema}.schema s
LEFT JOIN (
    SELECT a, COUNT(DISTINCT e) AS entity_count, COUNT(*) AS datom_count
    FROM (
{union}
    ) sub
    GROUP BY a
) u ON u.a = s.entid
WHERE s.entid >= 100
ORDER BY s.ident",
        schema = schema,
        union = union,
    )
}

// ---------------------------------------------------------------------------
// Convenience SQL functions
// ---------------------------------------------------------------------------

/// Generate SQL for the `lookup_entity()` function.
///
/// Finds entities by attribute value. This is the SQL equivalent of Datomic's
/// entity lookup: given an attribute ident and a text representation of a
/// value, returns all matching entity IDs.
///
/// Usage: SELECT * FROM {schema}.lookup_entity(':person/name', 'Alice');
fn lookup_entity_fn_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);

    // Each type-specific table requires matching the text value against
    // the native column with appropriate casting.
    format!(
        r"CREATE OR REPLACE FUNCTION {schema}.lookup_entity(attr_ident TEXT, search_value TEXT)
RETURNS TABLE(entity_id BIGINT, tx BIGINT)
AS $$
DECLARE
    attr_entid BIGINT;
    attr_type TEXT;
BEGIN
    SELECT entid, value_type::TEXT INTO attr_entid, attr_type
    FROM {schema}.schema WHERE ident = attr_ident;
    IF attr_entid IS NULL THEN
        RAISE EXCEPTION 'Unknown attribute ident: %', attr_ident;
    END IF;

    RETURN QUERY
    CASE attr_type
        WHEN 'string' THEN
            (SELECT d.e, d.tx FROM mentat.datoms_text_new d
             WHERE d.store_id = {sid} AND d.a = attr_entid AND d.added = true
             AND d.v = search_value)
        WHEN 'keyword' THEN
            (SELECT d.e, d.tx FROM mentat.datoms_keyword_new d
             WHERE d.store_id = {sid} AND d.a = attr_entid AND d.added = true
             AND d.v = search_value)
        WHEN 'long' THEN
            (SELECT d.e, d.tx FROM mentat.datoms_long_new d
             WHERE d.store_id = {sid} AND d.a = attr_entid AND d.added = true
             AND d.v = search_value::BIGINT)
        WHEN 'ref' THEN
            (SELECT d.e, d.tx FROM mentat.datoms_ref_new d
             WHERE d.store_id = {sid} AND d.a = attr_entid AND d.added = true
             AND d.v = search_value::BIGINT)
        WHEN 'boolean' THEN
            (SELECT d.e, d.tx FROM mentat.datoms_boolean_new d
             WHERE d.store_id = {sid} AND d.a = attr_entid AND d.added = true
             AND d.v = search_value::BOOLEAN)
        WHEN 'double' THEN
            (SELECT d.e, d.tx FROM mentat.datoms_double_new d
             WHERE d.store_id = {sid} AND d.a = attr_entid AND d.added = true
             AND d.v = search_value::DOUBLE PRECISION)
        WHEN 'instant' THEN
            (SELECT d.e, d.tx FROM mentat.datoms_instant_new d
             WHERE d.store_id = {sid} AND d.a = attr_entid AND d.added = true
             AND d.v = search_value::TIMESTAMPTZ)
        WHEN 'uuid' THEN
            (SELECT d.e, d.tx FROM mentat.datoms_uuid_new d
             WHERE d.store_id = {sid} AND d.a = attr_entid AND d.added = true
             AND d.v = search_value::UUID)
        ELSE
            (SELECT NULL::BIGINT, NULL::BIGINT WHERE false)
    END;
END;
$$ LANGUAGE plpgsql STABLE",
        schema = schema,
        sid = sid,
    )
}

/// Generate SQL for the `entity_value()` function.
///
/// Gets the current value of a single attribute on an entity, returned as text.
/// This is the simplest way for SQL users to look up a specific fact.
///
/// Usage: SELECT {schema}.entity_value(123, ':person/name');
fn entity_value_fn_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);
    format!(
        r"CREATE OR REPLACE FUNCTION {schema}.entity_value(eid BIGINT, attr_ident TEXT)
RETURNS TEXT
AS $$
DECLARE
    attr_entid BIGINT;
    attr_type TEXT;
    result TEXT;
BEGIN
    SELECT entid, value_type::TEXT INTO attr_entid, attr_type
    FROM {schema}.schema WHERE ident = attr_ident;
    IF attr_entid IS NULL THEN
        RAISE EXCEPTION 'Unknown attribute ident: %', attr_ident;
    END IF;

    CASE attr_type
        WHEN 'string' THEN
            SELECT d.v INTO result FROM mentat.datoms_text_new d
            WHERE d.store_id = {sid} AND d.e = eid AND d.a = attr_entid AND d.added = true
            ORDER BY d.tx DESC LIMIT 1;
        WHEN 'keyword' THEN
            SELECT ':' || d.v INTO result FROM mentat.datoms_keyword_new d
            WHERE d.store_id = {sid} AND d.e = eid AND d.a = attr_entid AND d.added = true
            ORDER BY d.tx DESC LIMIT 1;
        WHEN 'long' THEN
            SELECT d.v::TEXT INTO result FROM mentat.datoms_long_new d
            WHERE d.store_id = {sid} AND d.e = eid AND d.a = attr_entid AND d.added = true
            ORDER BY d.tx DESC LIMIT 1;
        WHEN 'ref' THEN
            SELECT COALESCE(
                (SELECT ri.ident FROM {schema}.idents ri WHERE ri.entid = d.v),
                d.v::TEXT
            ) INTO result FROM mentat.datoms_ref_new d
            WHERE d.store_id = {sid} AND d.e = eid AND d.a = attr_entid AND d.added = true
            ORDER BY d.tx DESC LIMIT 1;
        WHEN 'boolean' THEN
            SELECT d.v::TEXT INTO result FROM mentat.datoms_boolean_new d
            WHERE d.store_id = {sid} AND d.e = eid AND d.a = attr_entid AND d.added = true
            ORDER BY d.tx DESC LIMIT 1;
        WHEN 'double' THEN
            SELECT d.v::TEXT INTO result FROM mentat.datoms_double_new d
            WHERE d.store_id = {sid} AND d.e = eid AND d.a = attr_entid AND d.added = true
            ORDER BY d.tx DESC LIMIT 1;
        WHEN 'instant' THEN
            SELECT d.v::TEXT INTO result FROM mentat.datoms_instant_new d
            WHERE d.store_id = {sid} AND d.e = eid AND d.a = attr_entid AND d.added = true
            ORDER BY d.tx DESC LIMIT 1;
        WHEN 'uuid' THEN
            SELECT d.v::TEXT INTO result FROM mentat.datoms_uuid_new d
            WHERE d.store_id = {sid} AND d.e = eid AND d.a = attr_entid AND d.added = true
            ORDER BY d.tx DESC LIMIT 1;
        WHEN 'bytes' THEN
            SELECT encode(d.v, 'hex') INTO result FROM mentat.datoms_bytes_new d
            WHERE d.store_id = {sid} AND d.e = eid AND d.a = attr_entid AND d.added = true
            ORDER BY d.tx DESC LIMIT 1;
        ELSE
            result := NULL;
    END CASE;

    RETURN result;
END;
$$ LANGUAGE plpgsql STABLE",
        schema = schema,
        sid = sid,
    )
}

/// Generate SQL for the `count_by_attribute()` function.
///
/// Returns per-attribute counts: how many distinct entities have each attribute
/// asserted, and the total datom count. Useful for analytics and understanding
/// data distribution.
///
/// Usage: SELECT * FROM {schema}.count_by_attribute();
fn count_by_attribute_fn_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);
    let union = all_datoms_union_sql(&sid, "");
    format!(
        r"CREATE OR REPLACE FUNCTION {schema}.count_by_attribute()
RETURNS TABLE(attribute TEXT, entity_count BIGINT, datom_count BIGINT)
AS $$
BEGIN
    RETURN QUERY
    SELECT
        COALESCE(s.ident, 'entid:' || d.a::TEXT) AS attribute,
        COUNT(DISTINCT d.e) AS entity_count,
        COUNT(*) AS datom_count
    FROM (
{union}
    ) d
    LEFT JOIN {schema}.schema s ON s.entid = d.a
    GROUP BY d.a, s.ident
    ORDER BY datom_count DESC;
END;
$$ LANGUAGE plpgsql STABLE",
        schema = schema,
        union = union,
    )
}

/// Generate SQL for the `find_text()` function.
///
/// Full-text search wrapper that returns entities matching a text search query.
/// Uses PostgreSQL's built-in tsquery parsing so SQL users can use natural
/// language or boolean search operators.
///
/// Usage: SELECT * FROM {schema}.find_text('alice & engineer');
fn find_text_fn_sql(schema: &str) -> String {
    let sid = store_id_subquery(schema);
    format!(
        r"CREATE OR REPLACE FUNCTION {schema}.find_text(search_query TEXT)
RETURNS TABLE(entity_id BIGINT, attribute TEXT, value TEXT, rank REAL)
AS $$
BEGIN
    RETURN QUERY
    SELECT
        d.e,
        COALESCE(s.ident, 'entid:' || d.a::TEXT),
        d.v,
        ts_rank_cd(to_tsvector('english', d.v), plainto_tsquery('english', search_query))
    FROM mentat.datoms_text_new d
    LEFT JOIN {schema}.schema s ON s.entid = d.a
    WHERE d.store_id = {sid}
      AND d.added = true
      AND to_tsvector('english', d.v) @@ plainto_tsquery('english', search_query)
    ORDER BY ts_rank_cd(to_tsvector('english', d.v), plainto_tsquery('english', search_query)) DESC;
END;
$$ LANGUAGE plpgsql STABLE",
        schema = schema,
        sid = sid,
    )
}

// ---------------------------------------------------------------------------
// Optional extension-dependent indexes
// ---------------------------------------------------------------------------

/// Generate SQL for optional trigram indexes (requires pg_trgm extension).
///
/// Creates GIN trigram indexes on the text and keyword type-specific tables
/// for fast LIKE/ILIKE and similarity searches.
fn trigram_indexes_sql(_schema: &str, store_name: &str) -> String {
    format!(
        r"CREATE INDEX IF NOT EXISTS idx_{name}_trgm_text
ON mentat.datoms_text_new USING GIN (v gin_trgm_ops)
WHERE added = true;

CREATE INDEX IF NOT EXISTS idx_{name}_trgm_keyword
ON mentat.datoms_keyword_new USING GIN (v gin_trgm_ops)
WHERE added = true",
        name = store_name
    )
}

/// Generate SQL for optional full-text search indexes (requires pg_textsearch
/// or built-in tsvector support).
///
/// Creates a GIN index on a generated tsvector column for full-text search.
fn fulltext_index_sql(_schema: &str, store_name: &str) -> String {
    format!(
        r"CREATE INDEX IF NOT EXISTS idx_{name}_fts_text
ON mentat.datoms_text_new USING GIN (to_tsvector('english', COALESCE(v, '')))
WHERE added = true",
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

    // Relationship navigation views
    Spi::run(&entity_references_view_sql(schema))?;
    Spi::run(&reverse_references_view_sql(schema))?;
    Spi::run(&graph_edges_view_sql(schema))?;

    // Transaction history and temporal views
    Spi::run(&tx_log_view_sql(schema))?;
    Spi::run(&entity_history_view_sql(schema))?;
    Spi::run(&recent_changes_view_sql(schema))?;

    // Schema summary view
    Spi::run(&schema_summary_view_sql(schema))?;

    // Helper functions
    Spi::run(&entities_with_attribute_fn_sql(schema))?;
    Spi::run(&lookup_entity_fn_sql(schema))?;
    Spi::run(&entity_value_fn_sql(schema))?;
    Spi::run(&count_by_attribute_fn_sql(schema))?;
    Spi::run(&find_text_fn_sql(schema))?;

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

    // --- Relationship navigation views ---

    #[test]
    fn test_entity_references_view_sql() {
        let sql = entity_references_view_sql("mentat");
        assert!(sql.contains("mentat.entity_references"));
        assert!(sql.contains("mentat.datoms_ref_new"));
        assert!(sql.contains("source_entity"));
        assert!(sql.contains("target_entity"));
        assert!(sql.contains("target_ident"));
        assert!(sql.contains("mentat.idents"));
    }

    #[test]
    fn test_reverse_references_view_sql() {
        let sql = reverse_references_view_sql("mentat");
        assert!(sql.contains("mentat.reverse_references"));
        assert!(sql.contains("mentat.datoms_ref_new"));
        assert!(sql.contains("d.v AS target_entity"));
        assert!(sql.contains("d.e AS source_entity"));
    }

    #[test]
    fn test_graph_edges_view_sql() {
        let sql = graph_edges_view_sql("mentat");
        assert!(sql.contains("mentat.graph_edges"));
        assert!(sql.contains("d.e AS source"));
        assert!(sql.contains("d.v AS target"));
        assert!(sql.contains("edge_type"));
    }

    #[test]
    fn test_entity_references_custom_schema() {
        let sql = entity_references_view_sql("mentat_test");
        assert!(sql.contains("mentat_test.entity_references"));
        assert!(sql.contains("store_name = 'test'"));
    }

    // --- Transaction history views ---

    #[test]
    fn test_tx_log_view_sql() {
        let sql = tx_log_view_sql("mentat");
        assert!(sql.contains("mentat.tx_log"));
        assert!(sql.contains("mentat.transactions"));
        assert!(sql.contains("datom_count"));
        assert!(sql.contains("tx_time"));
        assert!(sql.contains("ORDER BY t.tx DESC"));
    }

    #[test]
    fn test_entity_history_view_sql() {
        let sql = entity_history_view_sql("mentat");
        assert!(sql.contains("mentat.entity_history"));
        assert!(sql.contains("UNION ALL"));
        assert!(sql.contains("mentat.datoms_ref_new"));
        assert!(sql.contains("mentat.datoms_text_new"));
        assert!(sql.contains("'assert'"));
        assert!(sql.contains("'retract'"));
        assert!(sql.contains("operation"));
        // Should include retractions (no added=true filter)
        assert!(!sql.contains("AND d.added = true"));
    }

    #[test]
    fn test_recent_changes_view_sql() {
        let sql = recent_changes_view_sql("mentat");
        assert!(sql.contains("mentat.recent_changes"));
        assert!(sql.contains("UNION ALL"));
        assert!(sql.contains("MAX(tx) - 100"));
        assert!(sql.contains("ORDER BY d.tx DESC"));
    }

    // --- Schema summary view ---

    #[test]
    fn test_schema_summary_view_sql() {
        let sql = schema_summary_view_sql("mentat");
        assert!(sql.contains("mentat.schema_summary"));
        assert!(sql.contains("entity_count"));
        assert!(sql.contains("datom_count"));
        assert!(sql.contains("s.entid >= 100"));
        assert!(sql.contains("UNION ALL"));
    }

    // --- Convenience functions ---

    #[test]
    fn test_lookup_entity_fn_sql() {
        let sql = lookup_entity_fn_sql("mentat");
        assert!(sql.contains("FUNCTION mentat.lookup_entity"));
        assert!(sql.contains("attr_ident TEXT"));
        assert!(sql.contains("search_value TEXT"));
        assert!(sql.contains("RETURN QUERY"));
        assert!(sql.contains("mentat.datoms_text_new"));
        assert!(sql.contains("mentat.datoms_long_new"));
        assert!(sql.contains("mentat.datoms_ref_new"));
        assert!(sql.contains("mentat.datoms_boolean_new"));
        assert!(sql.contains("STABLE"));
    }

    #[test]
    fn test_entity_value_fn_sql() {
        let sql = entity_value_fn_sql("mentat");
        assert!(sql.contains("FUNCTION mentat.entity_value"));
        assert!(sql.contains("eid BIGINT"));
        assert!(sql.contains("attr_ident TEXT"));
        assert!(sql.contains("RETURNS TEXT"));
        assert!(sql.contains("mentat.datoms_text_new"));
        assert!(sql.contains("mentat.datoms_long_new"));
        assert!(sql.contains("mentat.datoms_ref_new"));
        assert!(sql.contains("ORDER BY d.tx DESC LIMIT 1"));
    }

    #[test]
    fn test_count_by_attribute_fn_sql() {
        let sql = count_by_attribute_fn_sql("mentat");
        assert!(sql.contains("FUNCTION mentat.count_by_attribute"));
        assert!(sql.contains("entity_count"));
        assert!(sql.contains("datom_count"));
        assert!(sql.contains("UNION ALL"));
        assert!(sql.contains("GROUP BY"));
    }

    #[test]
    fn test_find_text_fn_sql() {
        let sql = find_text_fn_sql("mentat");
        assert!(sql.contains("FUNCTION mentat.find_text"));
        assert!(sql.contains("search_query TEXT"));
        assert!(sql.contains("ts_rank"));
        assert!(sql.contains("plainto_tsquery"));
        assert!(sql.contains("mentat.datoms_text_new"));
        assert!(sql.contains("rank REAL"));
    }

    #[test]
    fn test_lookup_entity_custom_schema() {
        let sql = lookup_entity_fn_sql("mentat_test");
        assert!(sql.contains("FUNCTION mentat_test.lookup_entity"));
        assert!(sql.contains("store_name = 'test'"));
    }

    #[test]
    fn test_entity_value_custom_schema() {
        let sql = entity_value_fn_sql("mentat_test");
        assert!(sql.contains("FUNCTION mentat_test.entity_value"));
        assert!(sql.contains("mentat_test.schema"));
        assert!(sql.contains("mentat_test.idents"));
    }
}
