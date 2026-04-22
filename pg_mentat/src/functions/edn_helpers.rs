/// Comprehensive EDN-based helper functions for pg_mentat
///
/// These functions provide EDN-native interfaces for batch operations,
/// import/export, and advanced query patterns.
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
        _ => return Err("Batch document must be an EDN vector".into()),
    };

    let mut results = Vec::new();

    // Execute each operation in sequence
    for op in ops {
        match op {
            edn::Value::Vector(ref op_vec) if !op_vec.is_empty() => {
                let op_type = &op_vec[0];

                let result = match op_type {
                    edn::Value::Keyword(kw) if kw.namespace() == Some("") => {
                        match kw.name() {
                            "query" => execute_query_op(&op_vec)?,
                            "transact" => execute_transact_op(&op_vec)?,
                            "pull" => execute_pull_op(&op_vec)?,
                            "entity" => execute_entity_op(&op_vec)?,
                            "schema" => execute_schema_op()?,
                            other => return Err(format!("Unknown operation type: {}", other).into()),
                        }
                    }
                    _ => return Err("Operation must start with a keyword".into()),
                };

                results.push(result);
            }
            _ => return Err("Each operation must be a vector starting with a keyword".into()),
        }
    }

    Ok(JsonB(json!(results)))
}

/// Helper: Execute a query operation
fn execute_query_op(op_vec: &[edn::Value]) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    if op_vec.len() < 2 {
        return Err("Query operation requires query pattern".into());
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
fn execute_transact_op(op_vec: &[edn::Value]) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    if op_vec.len() < 2 {
        return Err("Transact operation requires transaction data".into());
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
fn execute_pull_op(op_vec: &[edn::Value]) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    if op_vec.len() < 3 {
        return Err("Pull operation requires pattern and entity ID".into());
    }

    // Get pattern string
    let pattern_str = op_vec[1].to_string();

    // Get entity ID
    let entity_id = match &op_vec[2] {
        edn::Value::Integer(n) => *n,
        _ => return Err("Pull entity ID must be an integer".into()),
    };

    // Execute pull
    let result = crate::functions::pull::mentat_pull(&pattern_str, entity_id)?;

    Ok(json!({
        "type": "pull",
        "result": result.0
    }))
}

/// Helper: Execute an entity operation
fn execute_entity_op(op_vec: &[edn::Value]) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    if op_vec.len() < 2 {
        return Err("Entity operation requires entity ID".into());
    }

    // Get entity ID
    let entity_id = match &op_vec[1] {
        edn::Value::Integer(n) => *n,
        _ => return Err("Entity ID must be an integer".into()),
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
                SELECT a, v, value_type_tag \
                FROM mentat.datoms \
                WHERE e = $1 AND added = true \
                ORDER BY a";

            let mut entity_facts = Vec::new();

            for row in client.select(query, None, &[DatumWithOid::from(entity_id)])? {
                if let (Ok(Some(attr_id)), Ok(Some(value_bytes)), Ok(Some(type_tag))) = (
                    row.get::<i64>(1),
                    row.get::<Vec<u8>>(2),
                    row.get::<i16>(3)
                ) {
                    entity_facts.push((attr_id, value_bytes, type_tag));
                }
            }

            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(entity_facts)
        })?;

        if facts.is_empty() {
            continue; // Skip entities with no facts
        }

        // Build EDN map for this entity
        let mut entity_edn = format!("{{:db/id {}", entity_id);

        for (attr_id, value_bytes, type_tag) in facts {
            // Resolve attribute ident
            let attr_ident = crate::cache::get_cache()
                .get_ident(attr_id)
                .ok_or_else(|| format!("Failed to resolve attribute {}", attr_id))?;

            // Decode value to EDN representation
            let value_edn = value_to_edn(&value_bytes, type_tag)?;

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

/// Helper: Convert BYTEA value to EDN string representation
fn value_to_edn(bytes: &[u8], type_tag: i16) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match type_tag {
        1 => {
            // boolean
            if bytes.is_empty() {
                return Err("Invalid boolean value: empty bytes".into());
            }
            Ok(if bytes[0] != 0 { "true" } else { "false" }.to_string())
        }
        2 | 5 => {
            // long or ref (both i64)
            if bytes.len() != 8 {
                return Err(format!("Invalid i64 value: expected 8 bytes, got {}", bytes.len()).into());
            }
            let val = i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Ok(val.to_string())
        }
        3 => {
            // double (f64)
            if bytes.len() != 8 {
                return Err(format!("Invalid double: expected 8 bytes, got {}", bytes.len()).into());
            }
            let val = f64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Ok(val.to_string())
        }
        4 => {
            // instant (microseconds since epoch)
            if bytes.len() != 8 {
                return Err(format!("Invalid instant: expected 8 bytes, got {}", bytes.len()).into());
            }
            let micros = i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            // Format as #inst "YYYY-MM-DD..."
            Ok(format!("#inst {}", micros)) // Simplified - full impl would format timestamp
        }
        7 => {
            // string
            let s = String::from_utf8(bytes.to_vec())?;
            Ok(format!("\"{}\"", s.replace('"', "\\\"")))
        }
        8 => {
            // keyword
            let s = String::from_utf8(bytes.to_vec())?;
            Ok(if s.starts_with(':') { s } else { format!(":{}", s) })
        }
        9 => {
            // uuid
            if bytes.len() != 16 {
                return Err(format!("Invalid UUID: expected 16 bytes, got {}", bytes.len()).into());
            }
            Ok(format!("#uuid \"{}\"", hex::encode(bytes)))
        }
        11 => {
            // bytes
            Ok(format!("#bytes \"{}\"", hex::encode(bytes)))
        }
        _ => Err(format!("Unsupported type tag: {}", type_tag).into()),
    }
}

/// Helper: Convert EDN value to JSON value
fn edn_to_json(edn_val: &edn::Value) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
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

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use super::*;

    #[pg_test]
    fn test_edn_helpers_compile() {
        // Compilation test
        assert!(true);
    }
}
