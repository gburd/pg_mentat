/// Comprehensive EDN-based helper functions for pg_mentat
///
/// These functions provide EDN-native interfaces for batch operations,
/// import/export, and advanced query patterns.
use crate::error::MentatError;
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::spi::Spi;
use pgrx::JsonB;
use serde_json::{json, Value as JsonValue};

/// Execute multiple operations in a single EDN batch document
///
/// Supports: :query, :transact, :pull, :entity operations
///
/// Example:
/// ```sql
/// SELECT mentat.batch('[
///   [:query [:find ?e :where [?e :person/name]]]
///   [:transact [{:db/id "new" :person/name "Alice"}]]
///   [:pull [:person/name :person/email] 100]
///   [:entity 101]
/// ]');
/// ```
///
/// Returns JSONB array with results for each operation:
/// ```json
/// [
///   {"type": "query", "results": [[100], [101]]},
///   {"type": "transact", "tx-id": 1001, "tempids": {"new": 102}},
///   {"type": "pull", "result": {":person/name": "Alice", ...}},
///   {"type": "entity", "result": {":db/id": 101, ...}}
/// ]
/// ```
#[pg_extern(schema = "mentat")]
fn batch(edn_batch: &str) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    use edn::parse;

    // Parse the EDN batch document
    let value_and_span = parse::value(edn_batch)?;
    let value = value_and_span.without_spans();

    // Expect a vector of operations
    let ops = match value {
        edn::Value::Vector(v) => v,
        _ => return Err(MentatError::InvalidTransaction {
            message: "Batch document must be an EDN vector of operations. Example: [[:query ...] [:transact ...]]".to_string(),
        }.into()),
    };

    let mut results = Vec::new();

    // Execute each operation in sequence
    for op in ops {
        match op {
            edn::Value::Vector(ref op_vec) if !op_vec.is_empty() => {
                let op_type = &op_vec[0];

                let result = match op_type {
                    edn::Value::Keyword(kw) if kw.namespace().is_none() => match kw.name() {
                        "query" => execute_query_op(&op_vec)?,
                        "transact" => execute_transact_op(&op_vec)?,
                        "pull" => execute_pull_op(&op_vec)?,
                        "entity" => execute_entity_op(&op_vec)?,
                        "schema" => execute_schema_op()?,
                        other => return Err(MentatError::UnknownBatchOp {
                            op: other.to_string(),
                        }.into()),
                    },
                    _ => return Err(MentatError::BatchMissingArg {
                        op: "batch".to_string(),
                        message: "Each batch operation must start with a keyword (:query, :transact, :pull, :entity, :schema).".to_string(),
                    }.into()),
                };

                results.push(result);
            }
            _ => return Err(MentatError::BatchMissingArg {
                op: "batch".to_string(),
                message: "Each batch operation must be a vector starting with a keyword. Example: [:query [:find ?e :where [?e :attr ?v]]]".to_string(),
            }.into()),
        }
    }

    Ok(JsonB(json!(results)))
}

/// Helper: Execute a query operation
fn execute_query_op(
    op_vec: &[edn::Value],
) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    if op_vec.len() < 2 {
        return Err(MentatError::BatchMissingArg {
            op: "query".to_string(),
            message: "Requires a query pattern. Example: [:query [:find ?e :where [?e :person/name]]]".to_string(),
        }.into());
    }

    // Convert query EDN to string
    let query_str = edn::Value::Vector(op_vec[1..].to_vec()).to_string();

    // Get inputs if provided (third element)
    let inputs = if op_vec.len() > 2 {
        // Parse inputs from EDN to JSON
        edn_to_json(&op_vec[2])?
    } else {
        json!({})
    };

    // Execute query
    let result = crate::functions::query::mentat_query(&query_str, JsonB(inputs))?;

    Ok(json!({
        "type": "query",
        "results": result.0
    }))
}

/// Helper: Execute a transact operation
fn execute_transact_op(
    op_vec: &[edn::Value],
) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    if op_vec.len() < 2 {
        return Err(MentatError::BatchMissingArg {
            op: "transact".to_string(),
            message: "Requires transaction data. Example: [:transact [[:db/add \"new\" :person/name \"Alice\"]]]".to_string(),
        }.into());
    }

    // Convert tx data to string
    let tx_data = edn::Value::Vector(op_vec[1..].to_vec()).to_string();

    // Execute transaction
    let result_str = crate::functions::transact::mentat_transact(&tx_data)?;
    let result: JsonValue = serde_json::from_str(&result_str)?;

    Ok(json!({
        "type": "transact",
        "result": result
    }))
}

/// Helper: Execute a pull operation
///
/// Supports both single entity and multi-entity pulls:
///   [:pull [:person/name] 100]
///   [:pull [:person/name] [100 101 102]]
fn execute_pull_op(
    op_vec: &[edn::Value],
) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    if op_vec.len() < 3 {
        return Err(MentatError::BatchMissingArg {
            op: "pull".to_string(),
            message: "Requires a pattern and entity ID(s). Example: [:pull [:person/name] 100] or [:pull [:person/name] [100 101]]".to_string(),
        }.into());
    }

    // Get pattern string
    let pattern_str = op_vec[1].to_string();

    match &op_vec[2] {
        edn::Value::Integer(n) => {
            // Single entity pull
            let result = crate::functions::pull::mentat_pull(&pattern_str, *n)?;
            Ok(json!({
                "type": "pull",
                "result": result.0
            }))
        }
        edn::Value::Vector(ids) => {
            // Multi-entity pull
            let mut entity_ids = Vec::with_capacity(ids.len());
            for id_val in ids {
                match id_val {
                    edn::Value::Integer(n) => entity_ids.push(*n),
                    _ => return Err(MentatError::BatchMissingArg {
                        op: "pull".to_string(),
                        message: "Entity IDs must be integers. Example: [:pull [:person/name] [100 101]]".to_string(),
                    }.into()),
                }
            }
            let result = crate::functions::pull::mentat_pull_many(&pattern_str, entity_ids)?;
            Ok(json!({
                "type": "pull-many",
                "results": result.0
            }))
        }
        _ => Err(MentatError::BatchMissingArg {
            op: "pull".to_string(),
            message: "Entity ID must be an integer or vector of integers. Example: [:pull [:person/name] 100] or [:pull [:person/name] [100 101]]".to_string(),
        }.into()),
    }
}

/// Helper: Execute an entity operation
fn execute_entity_op(
    op_vec: &[edn::Value],
) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    if op_vec.len() < 2 {
        return Err(MentatError::BatchMissingArg {
            op: "entity".to_string(),
            message: "Requires an entity ID. Example: [:entity 100]".to_string(),
        }.into());
    }

    // Get entity ID
    let entity_id = match &op_vec[1] {
        edn::Value::Integer(n) => *n,
        _ => return Err(MentatError::BatchMissingArg {
            op: "entity".to_string(),
            message: "Entity ID must be an integer. Example: [:entity 100]".to_string(),
        }.into()),
    };

    // Execute entity lookup
    let result = crate::functions::entity::mentat_entity(entity_id)?;

    Ok(json!({
        "type": "entity",
        "result": result.0
    }))
}

/// Helper: Execute a schema operation
fn execute_schema_op() -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    let result = crate::functions::schema::mentat_schema()?;

    Ok(json!({
        "type": "schema",
        "result": result.0
    }))
}

use crate::types::constants::type_tag;

/// Export entities to EDN format
///
/// Takes a list of entity IDs and exports them as EDN transaction data
/// that can be imported into another database.
///
/// Example:
/// ```sql
/// SELECT mentat.export_edn(ARRAY[100, 101, 102]);
/// ```
///
/// Returns:
/// ```edn
/// [
///   {:db/id 100
///    :person/name "Alice"
///    :person/email "alice@example.com"}
///   {:db/id 101
///    :person/name "Bob"}
/// ]
/// ```
#[pg_extern(schema = "mentat")]
fn export_edn(entity_ids: Vec<i64>) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut edn_parts = Vec::new();

    for entity_id in entity_ids {
        // Get all facts for this entity
        let facts = Spi::connect(|client| {
            let query = "\
                SELECT a, value_type_tag, \
                       v_ref, v_bool, v_long, v_double, \
                       v_text, v_keyword, \
                       EXTRACT(EPOCH FROM v_instant)::BIGINT * 1000000 + \
                       EXTRACT(MICROSECOND FROM v_instant)::BIGINT % 1000000 AS v_instant_micros, \
                       v_uuid::TEXT, v_bytes \
                FROM mentat.datoms \
                WHERE e = $1 AND added = true \
                ORDER BY a";

            let mut entity_facts: Vec<(i64, String)> = Vec::new();

            for row in client.select(query, None, &[DatumWithOid::from(entity_id)])? {
                if let (Ok(Some(attr_id)), Ok(Some(tt))) =
                    (row.get::<i64>(1), row.get::<i16>(2))
                {
                    let edn_val = row_value_to_edn(&row, tt, 3)?;
                    entity_facts.push((attr_id, edn_val));
                }
            }

            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(entity_facts)
        })?;

        if facts.is_empty() {
            continue; // Skip entities with no facts
        }

        // Build EDN map for this entity
        let mut entity_edn = format!("{{:db/id {}", entity_id);

        for (attr_id, value_edn) in facts {
            // Resolve attribute ident
            let attr_ident = crate::cache::get_cache()
                .get_ident(attr_id)
                .ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
                    MentatError::AttributeNotFound {
                        attr: format!("entid:{}", attr_id),
                        available: crate::error::get_available_attributes(),
                        suggestion: None,
                    }.into()
                })?;

            entity_edn.push_str(&format!("\n   {} {}", attr_ident, value_edn));
        }

        entity_edn.push('}');
        edn_parts.push(entity_edn);
    }

    // Wrap in vector
    Ok(format!("[\n  {}\n]", edn_parts.join("\n  ")))
}

/// Import entities from EDN format
///
/// Takes EDN transaction data and imports it into the database.
/// Supports both tempid allocation and explicit entity IDs.
///
/// Example:
/// ```sql
/// SELECT mentat.import_edn('[
///   {:db/id "alice"
///    :person/name "Alice"
///    :person/email "alice@example.com"}
///   {:db/id "bob"
///    :person/name "Bob"}
/// ]');
/// ```
///
/// Returns transaction report:
/// ```json
/// {
///   "tx-id": 1001,
///   "tempids": {"alice": 100, "bob": 101},
///   "datoms-inserted": 4
/// }
/// ```
#[pg_extern(schema = "mentat")]
fn import_edn(edn_data: &str) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    // Import is just a transact operation
    let result_str = crate::functions::transact::mentat_transact(edn_data)?;
    let result: JsonValue = serde_json::from_str(&result_str)?;
    Ok(JsonB(result))
}

/// Query and export matching entities to EDN
///
/// Executes a query and exports all matching entities to EDN format.
///
/// Example:
/// ```sql
/// SELECT mentat.query_export_edn(
///   '[:find ?e :where [?e :person/name]]',
///   '{}'
/// );
/// ```
#[pg_extern(schema = "mentat")]
fn query_export_edn(
    query: &str,
    inputs: JsonB,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Execute query to get entity IDs
    let result = crate::functions::query::mentat_query(query, inputs)?;

    // Extract entity IDs from query results
    let mut entity_ids = Vec::new();

    if let JsonValue::Object(obj) = result.0 {
        if let Some(JsonValue::Array(results)) = obj.get("results") {
            for row in results {
                if let JsonValue::Array(cols) = row {
                    if let Some(JsonValue::Number(n)) = cols.get(0) {
                        if let Some(id) = n.as_i64() {
                            entity_ids.push(id);
                        }
                    }
                }
            }
        }
    }

    // Export entities
    export_edn(entity_ids)
}

/// Export entire database to EDN format
///
/// Exports all entities with their facts as EDN transaction data.
/// **Warning:** Can be very large for big databases.
///
/// Example:
/// ```sql
/// SELECT mentat.export_all_edn();
/// ```
#[pg_extern(schema = "mentat")]
fn export_all_edn() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Get all distinct entity IDs
    let entity_ids: Vec<i64> = Spi::connect(|client| {
        let query = "SELECT DISTINCT e FROM mentat.datoms WHERE added = true ORDER BY e";

        let mut ids = Vec::new();
        for row in client.select(query, None, &[])? {
            if let Ok(Some(id)) = row.get::<i64>(1) {
                ids.push(id);
            }
        }

        Ok::<_, pgrx::spi::SpiError>(ids)
    })?;

    export_edn(entity_ids)
}

/// Convert a typed value from an SPI row to EDN string representation.
///
/// Columns at col_offset:
///   +0 = v_ref, +1 = v_bool, +2 = v_long, +3 = v_double,
///   +4 = v_text, +5 = v_keyword, +6 = v_instant_micros, +7 = v_uuid::TEXT, +8 = v_bytes
fn row_value_to_edn(
    row: &pgrx::spi::SpiHeapTupleData<'_>,
    type_tag: i16,
    col_offset: usize,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match type_tag {
        type_tag::REF | type_tag::LONG => {
            // ref or long - both stored as BIGINT
            let col = if type_tag == type_tag::REF { col_offset } else { col_offset + 2 };
            let val: i64 = row.get(col)?.ok_or_else(|| MentatError::DataCorruption {
                message: format!("Missing value for type_tag {}", type_tag),
            })?;
            Ok(val.to_string())
        }
        type_tag::BOOLEAN => {
            let b: bool = row.get(col_offset + 1)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_bool".to_string(),
            })?;
            Ok(if b { "true" } else { "false" }.to_string())
        }
        type_tag::DOUBLE => {
            let f: f64 = row.get(col_offset + 3)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_double".to_string(),
            })?;
            Ok(f.to_string())
        }
        type_tag::INSTANT => {
            let micros: i64 = row.get(col_offset + 6)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_instant_micros".to_string(),
            })?;
            Ok(format!("#inst {}", micros))
        }
        type_tag::STRING => {
            let s: String = row.get(col_offset + 4)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_text".to_string(),
            })?;
            Ok(format!("\"{}\"", s.replace('"', "\\\"")))
        }
        type_tag::KEYWORD => {
            let s: String = row.get(col_offset + 5)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_keyword".to_string(),
            })?;
            Ok(if s.starts_with(':') {
                s
            } else {
                format!(":{}", s)
            })
        }
        type_tag::UUID => {
            let s: String = row.get(col_offset + 7)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_uuid".to_string(),
            })?;
            Ok(format!("#uuid \"{}\"", s))
        }
        type_tag::BYTES => {
            let b: Vec<u8> = row.get(col_offset + 8)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_bytes".to_string(),
            })?;
            Ok(format!("#bytes \"{}\"", hex::encode(b)))
        }
        _ => Err(MentatError::UnsupportedType { type_tag }.into()),
    }
}

/// Helper: Convert EDN value to JSON value
fn edn_to_json(
    edn_val: &edn::Value,
) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    match edn_val {
        edn::Value::Nil => Ok(JsonValue::Null),
        edn::Value::Boolean(b) => Ok(json!(*b)),
        edn::Value::Integer(n) => Ok(json!(*n)),
        edn::Value::Float(f) => Ok(json!(f.into_inner())),
        edn::Value::Text(s) => Ok(json!(s)),
        edn::Value::Keyword(kw) => Ok(json!(kw.to_string())),
        edn::Value::Vector(v) => {
            let mut arr = Vec::new();
            for item in v {
                arr.push(edn_to_json(item)?);
            }
            Ok(JsonValue::Array(arr))
        }
        edn::Value::Map(m) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in m {
                let key = match k {
                    edn::Value::Keyword(kw) => kw.to_string(),
                    edn::Value::Text(s) => s.clone(),
                    _ => k.to_string(),
                };
                obj.insert(key, edn_to_json(v)?);
            }
            Ok(JsonValue::Object(obj))
        }
        _ => Ok(json!(edn_val.to_string())),
    }
}

/// Pretty-print an EDN string with proper indentation
///
/// Parses the input EDN and formats it with smart line-breaking:
/// compact for simple values, expanded for complex/nested structures.
///
/// The optional `width` parameter controls the target line width (default: 80).
/// Shorter widths produce more vertical output; longer widths keep more on one line.
///
/// Example:
/// ```sql
/// SELECT edn_pretty('{:person/name "Alice" :person/age 30}');
/// -- Returns:
/// -- {:person/age 30
/// --  :person/name "Alice"}
///
/// SELECT edn_pretty('[:find ?e :where [?e :person/name]]', 40);
/// -- Returns:
/// -- [:find
/// --  ?e
/// --  :where
/// --  [?e :person/name]]
/// ```
#[pg_extern(immutable, parallel_safe)]
fn edn_pretty(
    edn_input: &str,
    width: default!(Option<i32>, "NULL"),
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use edn::parse;

    let width = match width {
        Some(w) if w > 0 => w as usize,
        Some(_) => {
            return Err(MentatError::InvalidQuery {
                message: "width must be a positive integer".to_string(),
                suggestion: Some("Use a positive integer like 40, 80, or 120".to_string()),
            }
            .into());
        }
        None => 80,
    };

    let value = parse::value(edn_input)
        .map_err(|e| MentatError::InvalidQuery {
            message: format!("Failed to parse EDN: {}", e),
            suggestion: Some(
                "Ensure the input is valid EDN. Example: {:key \"value\"}".to_string(),
            ),
        })?
        .without_spans();

    value
        .to_pretty(width)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            MentatError::InvalidQuery {
                message: format!("Failed to format EDN: {}", e),
                suggestion: None,
            }
            .into()
        })
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use super::*;

    #[pg_test]
    fn test_edn_helpers_compile() {
        crate::ensure_extension_loaded();
        // Compilation test
        assert!(true);
    }

    #[pg_test]
    fn test_edn_pretty_simple_map() {
        crate::ensure_extension_loaded();
        let result = edn_pretty("{:a 1 :b 2}", None).unwrap();
        assert_eq!(result, "{:a 1 :b 2}");
    }

    #[pg_test]
    fn test_edn_pretty_vector() {
        crate::ensure_extension_loaded();
        let result = edn_pretty("[1 2 3 4 5]", None).unwrap();
        assert_eq!(result, "[1 2 3 4 5]");
    }

    #[pg_test]
    fn test_edn_pretty_narrow_width() {
        crate::ensure_extension_loaded();
        let result = edn_pretty("[1 2 3 4 5 6]", Some(10)).unwrap();
        assert!(result.contains('\n'), "narrow width should produce multi-line output");
    }

    #[pg_test]
    fn test_edn_pretty_nested() {
        crate::ensure_extension_loaded();
        let result = edn_pretty("{:a [1 2 3] :b {:c 4}}", None).unwrap();
        // Should parse and format without error
        assert!(result.contains(":a"));
        assert!(result.contains(":b"));
    }

    #[pg_test]
    fn test_edn_pretty_invalid_input() {
        crate::ensure_extension_loaded();
        let result = edn_pretty("{invalid", None);
        assert!(result.is_err());
    }

    #[pg_test]
    fn test_edn_pretty_invalid_width() {
        crate::ensure_extension_loaded();
        let result = edn_pretty("{:a 1}", Some(-1));
        assert!(result.is_err());
    }

    #[pg_test]
    fn test_edn_pretty_nil() {
        crate::ensure_extension_loaded();
        let result = edn_pretty("nil", None).unwrap();
        assert_eq!(result, "nil");
    }

    #[pg_test]
    fn test_edn_pretty_set() {
        crate::ensure_extension_loaded();
        let result = edn_pretty("#{1 2 3}", None).unwrap();
        assert!(result.starts_with("#{"));
        assert!(result.ends_with('}'));
    }

    #[pg_test]
    fn test_edn_pretty_list() {
        crate::ensure_extension_loaded();
        let result = edn_pretty("(1 2 3)", None).unwrap();
        assert!(result.starts_with('('));
        assert!(result.ends_with(')'));
    }

    #[pg_test]
    fn test_edn_pretty_keyword() {
        crate::ensure_extension_loaded();
        let result = edn_pretty(":person/name", None).unwrap();
        assert_eq!(result, ":person/name");
    }
}
