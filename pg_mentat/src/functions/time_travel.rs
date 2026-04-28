use crate::functions::store_management::{get_schema_for_store, quote_ident, validate_store_name};
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::json;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve a store name to its schema prefix (e.g. "mentat." or "mentat_foo.").
fn resolve_schema_prefix(store_name: &str) -> String {
    let schema = get_schema_for_store(store_name);
    format!("{}.", quote_ident(&schema))
}

/// Look up the `store_id` for a given schema name.
///
/// The `mentat` schema maps to the `"default"` store; any `mentat_<name>`
/// schema maps to `<name>`.  The id is fetched from `mentat.stores`.
fn get_store_id_for_schema(schema: &str) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let store_name = if schema == "mentat" {
        "default"
    } else if let Some(name) = schema.strip_prefix("mentat_") {
        name
    } else {
        return Err(crate::error::MentatError::InvalidStoreName {
            store_name: schema.to_string(),
            reason: "Schema must be 'mentat' or 'mentat_*'".to_string(),
        }
        .into());
    };

    let store_id: Option<i32> = Spi::get_one_with_args(
        "SELECT store_id FROM mentat.stores WHERE store_name = $1",
        &[DatumWithOid::from(store_name)],
    )?;

    store_id.ok_or_else(|| {
        crate::error::MentatError::StoreNotFound {
            store_name: store_name.to_string(),
        }
        .into()
    })
}

/// Extract the results array from a query response envelope.
///
/// `mentat_query_internal` returns different shapes depending on FindSpec:
/// - FindRel:    `{"columns": [...], "results": [[...], ...]}`
/// - FindColl:   `{"result": [...]}`
/// - FindTuple:  `{"result": [...]}`
/// - FindScalar: `{"result": <value>}`
/// - Aggregate:  `{"result": <value>}`
///
/// For diff purposes we normalise all of these into a JSON array of rows.
fn extract_results_array(response: &serde_json::Value) -> serde_json::Value {
    // FindRel with "results" key
    if let Some(results) = response.get("results") {
        return results.clone();
    }
    // FindColl / FindTuple with "result" key that is an array
    if let Some(result) = response.get("result") {
        if result.is_array() {
            // Wrap each element as a single-element row for uniform diffing
            let rows: Vec<serde_json::Value> = result
                .as_array()
                .unwrap()
                .iter()
                .map(|v| json!([v]))
                .collect();
            return json!(rows);
        }
        // FindScalar: single value -- wrap as a one-row, one-column result
        if !result.is_null() {
            return json!([[result]]);
        }
    }
    // Fall through: treat as empty result set
    json!([])
}

/// Run a Datalog query at a specific transaction ID and return the raw JSON results.
///
/// This delegates to `mentat_query_internal` with the `asOf` key injected into
/// the inputs JSON object.
fn query_as_of(
    query: &str,
    inputs: &JsonB,
    as_of_tx: i64,
    schema_prefix: &str,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let mut inputs_obj = match &inputs.0 {
        serde_json::Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };
    inputs_obj.insert("asOf".to_string(), json!(as_of_tx));
    let merged = JsonB(serde_json::Value::Object(inputs_obj));

    crate::functions::query::mentat_query_internal(query, merged, schema_prefix)
}

/// Compute the set difference between two JSON result arrays.
///
/// Both `old_results` and `new_results` are expected to be JSON arrays of
/// arrays (the standard Mentat `:find` result shape).  Returns a JSON object:
///
/// ```json
/// {
///   "added":   [ ...rows in new but not old... ],
///   "removed": [ ...rows in old but not new... ],
///   "unchanged_count": <number>
/// }
/// ```
///
/// Row equality is determined by the canonical JSON serialisation of each row,
/// which is correct for all Mentat value types (strings, ints, bools, floats).
fn compute_result_diff(
    old_results: &serde_json::Value,
    new_results: &serde_json::Value,
) -> serde_json::Value {
    let old_rows = old_results.as_array().cloned().unwrap_or_default();
    let new_rows = new_results.as_array().cloned().unwrap_or_default();

    // Build sets of serialised rows for O(n) comparison.
    let old_set: HashSet<String> = old_rows.iter().map(|r| r.to_string()).collect();
    let new_set: HashSet<String> = new_rows.iter().map(|r| r.to_string()).collect();

    let added: Vec<&serde_json::Value> = new_rows
        .iter()
        .filter(|r| !old_set.contains(&r.to_string()))
        .collect();

    let removed: Vec<&serde_json::Value> = old_rows
        .iter()
        .filter(|r| !new_set.contains(&r.to_string()))
        .collect();

    let unchanged_count = old_rows.len() - removed.len();

    json!({
        "added": added,
        "removed": removed,
        "unchanged_count": unchanged_count,
    })
}

// ---------------------------------------------------------------------------
// mentat_diff -- compare query results across two transaction points
// ---------------------------------------------------------------------------

/// Compare query results between two transactions.
///
/// Runs the given Datalog query at both `from_tx` and `to_tx` (using temporal
/// `asOf` filtering) and returns the set difference.
///
/// # Result shape
/// ```json
/// {
///   "from_tx": <from_tx>,
///   "to_tx":   <to_tx>,
///   "added":   [ ...rows present at to_tx but not at from_tx... ],
///   "removed": [ ...rows present at from_tx but not at to_tx... ],
///   "unchanged_count": <number>
/// }
/// ```
///
/// # Example
/// ```sql
/// SELECT mentat_diff(
///     'default',
///     1000001, 1000005,
///     '[:find ?e ?name :where [?e :person/name ?name]]',
///     '{}'::jsonb
/// );
/// ```
#[pg_extern]
pub fn diff(
    store_name: &str,
    from_tx: i64,
    to_tx: i64,
    query: &str,
    inputs: JsonB,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    if store_name != "default" {
        validate_store_name(store_name)?;
    }

    if from_tx >= to_tx {
        return Err(Box::new(crate::error::MentatError::InvalidQuery {
            message: format!(
                "from_tx ({}) must be less than to_tx ({})",
                from_tx, to_tx
            ),
            suggestion: Some(
                "Swap the transaction IDs so from_tx < to_tx.".to_string(),
            ),
        }));
    }

    let schema_prefix = resolve_schema_prefix(store_name);

    let old_result = query_as_of(query, &inputs, from_tx, &schema_prefix)?;
    let new_result = query_as_of(query, &inputs, to_tx, &schema_prefix)?;

    // Extract the "results" array from the query response envelope.
    // For FindRel queries the shape is {"columns": [...], "results": [...]}.
    // For FindColl/FindScalar the shape is {"result": ...}.
    let old_data = extract_results_array(&old_result.0);
    let new_data = extract_results_array(&new_result.0);

    let diff = compute_result_diff(&old_data, &new_data);

    // Merge tx metadata into the diff object.
    let mut result = match diff {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    result.insert("from_tx".to_string(), json!(from_tx));
    result.insert("to_tx".to_string(), json!(to_tx));

    Ok(JsonB(serde_json::Value::Object(result)))
}

/// Compare query results between two transactions on the default store.
///
/// Convenience wrapper around `mentat_diff` for the default store.
///
/// # Example
/// ```sql
/// SELECT mentat_diff_default(
///     1000001, 1000005,
///     '[:find ?e ?name :where [?e :person/name ?name]]',
///     '{}'::jsonb
/// );
/// ```
#[pg_extern]
pub fn diff_default(
    from_tx: i64,
    to_tx: i64,
    query: &str,
    inputs: JsonB,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    diff("default", from_tx, to_tx, query, inputs)
}

// ---------------------------------------------------------------------------
// mentat_log -- audit log of raw datom changes between two transactions
// ---------------------------------------------------------------------------

/// Return an audit log of datom-level changes between two transactions.
///
/// Queries the datoms table directly (including retractions) for all datoms
/// whose `tx` falls in the range `(start_tx, end_tx]`.  This gives a
/// low-level view of every assertion and retraction that occurred.
///
/// # Result shape
/// ```json
/// [
///   {
///     "tx": 1000002,
///     "tx_instant": "2025-01-15 10:30:00+00",
///     "e": 10001,
///     "a": 65,
///     "v": "Alice",
///     "added": true
///   },
///   ...
/// ]
/// ```
///
/// # Example
/// ```sql
/// SELECT mentat_log('default', 1000000, 1000010);
/// ```
#[pg_extern]
pub fn log(
    store_name: &str,
    start_tx: i64,
    end_tx: i64,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    if store_name != "default" {
        validate_store_name(store_name)?;
    }

    if start_tx > end_tx {
        return Err(Box::new(crate::error::MentatError::InvalidQuery {
            message: format!(
                "start_tx ({}) must be less than or equal to end_tx ({})",
                start_tx, end_tx
            ),
            suggestion: Some(
                "Swap the transaction IDs so start_tx <= end_tx.".to_string(),
            ),
        }));
    }

    let schema = get_schema_for_store(store_name);
    let quoted_schema = quote_ident(&schema);

    // Resolve the store_id for filtering the type-specific tables.
    let store_id = get_store_id_for_schema(&schema)?;

    // Query all type-specific tables with UNION ALL, joined with
    // transactions for the tx_instant timestamp.  Each sub-query selects
    // a `type_tag` literal and casts `v` to text so that the UNION is
    // type-compatible.  Both assertions and retractions are included.
    let sql = format!(
        r#"
        SELECT d.tx,
               t.tx_instant::TEXT AS tx_instant,
               d.e,
               d.a,
               d.type_tag,
               d.v_text,
               d.added
        FROM (
            SELECT tx, e, a, 0::smallint AS type_tag, v::text AS v_text, added
              FROM mentat.datoms_ref_new
             WHERE store_id = $1 AND tx > $2 AND tx <= $3
            UNION ALL
            SELECT tx, e, a, 1::smallint, v::text, added
              FROM mentat.datoms_boolean_new
             WHERE store_id = $1 AND tx > $2 AND tx <= $3
            UNION ALL
            SELECT tx, e, a, 2::smallint, v::text, added
              FROM mentat.datoms_long_new
             WHERE store_id = $1 AND tx > $2 AND tx <= $3
            UNION ALL
            SELECT tx, e, a, 3::smallint, v::text, added
              FROM mentat.datoms_double_new
             WHERE store_id = $1 AND tx > $2 AND tx <= $3
            UNION ALL
            SELECT tx, e, a, 4::smallint, v::text, added
              FROM mentat.datoms_instant_new
             WHERE store_id = $1 AND tx > $2 AND tx <= $3
            UNION ALL
            SELECT tx, e, a, 7::smallint, v, added
              FROM mentat.datoms_text_new
             WHERE store_id = $1 AND tx > $2 AND tx <= $3
            UNION ALL
            SELECT tx, e, a, 8::smallint, v, added
              FROM mentat.datoms_keyword_new
             WHERE store_id = $1 AND tx > $2 AND tx <= $3
            UNION ALL
            SELECT tx, e, a, 10::smallint, v::text, added
              FROM mentat.datoms_uuid_new
             WHERE store_id = $1 AND tx > $2 AND tx <= $3
            UNION ALL
            SELECT tx, e, a, 11::smallint, encode(v, 'hex'), added
              FROM mentat.datoms_bytes_new
             WHERE store_id = $1 AND tx > $2 AND tx <= $3
        ) d
        JOIN {schema}.transactions t ON t.tx = d.tx
        ORDER BY d.tx ASC, d.e ASC, d.a ASC
        "#,
        schema = quoted_schema,
    );

    let entries = Spi::connect(|client| {
        let mut result = Vec::new();

        let rows = client.select(
            &sql,
            None,
            &[
                DatumWithOid::from(store_id),
                DatumWithOid::from(start_tx),
                DatumWithOid::from(end_tx),
            ],
        )?;

        for row in rows {
            // Column layout from the UNION ALL query:
            //   1 = tx, 2 = tx_instant, 3 = e, 4 = a,
            //   5 = type_tag, 6 = v_text, 7 = added
            let tx: i64 = row.get::<i64>(1)?.unwrap_or(0);
            let tx_instant: String = row.get::<String>(2)?.unwrap_or_default();
            let e: i64 = row.get::<i64>(3)?.unwrap_or(0);
            let a: i64 = row.get::<i64>(4)?.unwrap_or(0);
            let type_tag: i16 = row.get::<i16>(5)?.unwrap_or(0);
            let v_text: String = row.get::<String>(6)?.unwrap_or_default();
            let added: bool = row.get::<bool>(7)?.unwrap_or(true);

            // Convert the text representation back to the appropriate JSON type.
            let v = decode_text_value(type_tag, &v_text);

            // Resolve attribute entid to ident if possible.
            let a_display: serde_json::Value =
                if let Some(ident) = crate::cache::get_cache().get_ident(a) {
                    json!(ident)
                } else {
                    json!(a)
                };

            result.push(json!({
                "tx": tx,
                "tx_instant": tx_instant,
                "e": e,
                "a": a_display,
                "v": v,
                "added": added,
            }));
        }

        Ok::<_, pgrx::spi::SpiError>(result)
    })?;

    Ok(JsonB(json!(entries)))
}

/// Return an audit log of datom-level changes on the default store.
///
/// Convenience wrapper around `mentat_log` for the default store.
///
/// # Example
/// ```sql
/// SELECT mentat_log_default(1000000, 1000010);
/// ```
#[pg_extern]
pub fn log_default(
    start_tx: i64,
    end_tx: i64,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    log("default", start_tx, end_tx)
}

// ---------------------------------------------------------------------------
// Value decoding helper
// ---------------------------------------------------------------------------

/// Convert a text-encoded value back to its natural JSON representation.
///
/// The UNION ALL query casts every value to `text`.  This function parses
/// that text back into the JSON type that makes sense for the given
/// `type_tag` so that the audit log is human-readable.
fn decode_text_value(type_tag: i16, v_text: &str) -> serde_json::Value {
    match type_tag {
        0 => {
            // ref -- integer entity id
            v_text.parse::<i64>().map(|n| json!(n)).unwrap_or(json!(v_text))
        }
        1 => {
            // boolean
            match v_text {
                "t" | "true" => json!(true),
                "f" | "false" => json!(false),
                _ => json!(v_text),
            }
        }
        2 => {
            // long
            v_text.parse::<i64>().map(|n| json!(n)).unwrap_or(json!(v_text))
        }
        3 => {
            // double
            v_text.parse::<f64>().map(|n| json!(n)).unwrap_or(json!(v_text))
        }
        4 => {
            // instant (timestamp cast to text)
            json!(v_text)
        }
        7 => {
            // string (already text)
            json!(v_text)
        }
        8 => {
            // keyword -- prepend colon for display
            json!(format!(":{}", v_text))
        }
        10 => {
            // uuid (cast to text)
            json!(v_text)
        }
        11 => {
            // bytes (hex-encoded)
            json!(format!("\\x{}", v_text))
        }
        _ => {
            json!(format!("<type_tag:{}>", type_tag))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_result_diff_empty() {
        let old = json!([]);
        let new = json!([]);
        let diff = compute_result_diff(&old, &new);

        assert_eq!(diff["added"], json!([]));
        assert_eq!(diff["removed"], json!([]));
        assert_eq!(diff["unchanged_count"], json!(0));
    }

    #[test]
    fn test_compute_result_diff_additions() {
        let old = json!([[1, "Alice"]]);
        let new = json!([[1, "Alice"], [2, "Bob"]]);
        let diff = compute_result_diff(&old, &new);

        assert_eq!(diff["added"], json!([[2, "Bob"]]));
        assert_eq!(diff["removed"], json!([]));
        assert_eq!(diff["unchanged_count"], json!(1));
    }

    #[test]
    fn test_compute_result_diff_removals() {
        let old = json!([[1, "Alice"], [2, "Bob"]]);
        let new = json!([[1, "Alice"]]);
        let diff = compute_result_diff(&old, &new);

        assert_eq!(diff["added"], json!([]));
        assert_eq!(diff["removed"], json!([[2, "Bob"]]));
        assert_eq!(diff["unchanged_count"], json!(1));
    }

    #[test]
    fn test_compute_result_diff_mixed() {
        let old = json!([[1, "Alice"], [2, "Bob"]]);
        let new = json!([[1, "Alice"], [3, "Charlie"]]);
        let diff = compute_result_diff(&old, &new);

        assert_eq!(diff["added"], json!([[3, "Charlie"]]));
        assert_eq!(diff["removed"], json!([[2, "Bob"]]));
        assert_eq!(diff["unchanged_count"], json!(1));
    }

    #[test]
    fn test_compute_result_diff_complete_replacement() {
        let old = json!([[1, "Alice"]]);
        let new = json!([[2, "Bob"]]);
        let diff = compute_result_diff(&old, &new);

        assert_eq!(diff["added"], json!([[2, "Bob"]]));
        assert_eq!(diff["removed"], json!([[1, "Alice"]]));
        assert_eq!(diff["unchanged_count"], json!(0));
    }

    #[test]
    fn test_compute_result_diff_non_array() {
        // If the inputs aren't arrays, treat them as empty.
        let old = json!(42);
        let new = json!("hello");
        let diff = compute_result_diff(&old, &new);

        assert_eq!(diff["added"], json!([]));
        assert_eq!(diff["removed"], json!([]));
        assert_eq!(diff["unchanged_count"], json!(0));
    }

    #[test]
    fn test_extract_results_array_find_rel() {
        let response = json!({"columns": ["?e", "?name"], "results": [[1, "Alice"], [2, "Bob"]]});
        let extracted = extract_results_array(&response);
        assert_eq!(extracted, json!([[1, "Alice"], [2, "Bob"]]));
    }

    #[test]
    fn test_extract_results_array_find_coll() {
        let response = json!({"result": ["Alice", "Bob"]});
        let extracted = extract_results_array(&response);
        assert_eq!(extracted, json!([["Alice"], ["Bob"]]));
    }

    #[test]
    fn test_extract_results_array_find_scalar() {
        let response = json!({"result": 42});
        let extracted = extract_results_array(&response);
        assert_eq!(extracted, json!([[42]]));
    }

    #[test]
    fn test_extract_results_array_null_result() {
        let response = json!({"result": null});
        let extracted = extract_results_array(&response);
        assert_eq!(extracted, json!([]));
    }

    #[test]
    fn test_extract_results_array_empty() {
        let response = json!({});
        let extracted = extract_results_array(&response);
        assert_eq!(extracted, json!([]));
    }

    #[test]
    fn test_decode_text_value_ref() {
        assert_eq!(decode_text_value(0, "12345"), json!(12345));
    }

    #[test]
    fn test_decode_text_value_boolean() {
        assert_eq!(decode_text_value(1, "t"), json!(true));
        assert_eq!(decode_text_value(1, "true"), json!(true));
        assert_eq!(decode_text_value(1, "f"), json!(false));
        assert_eq!(decode_text_value(1, "false"), json!(false));
    }

    #[test]
    fn test_decode_text_value_long() {
        assert_eq!(decode_text_value(2, "42"), json!(42));
        assert_eq!(decode_text_value(2, "-100"), json!(-100));
    }

    #[test]
    fn test_decode_text_value_double() {
        assert_eq!(decode_text_value(3, "3.14"), json!(3.14));
    }

    #[test]
    fn test_decode_text_value_instant() {
        assert_eq!(
            decode_text_value(4, "2025-01-15 10:30:00+00"),
            json!("2025-01-15 10:30:00+00")
        );
    }

    #[test]
    fn test_decode_text_value_string() {
        assert_eq!(decode_text_value(7, "hello world"), json!("hello world"));
    }

    #[test]
    fn test_decode_text_value_keyword() {
        assert_eq!(decode_text_value(8, "person/name"), json!(":person/name"));
    }

    #[test]
    fn test_decode_text_value_uuid() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(decode_text_value(10, uuid), json!(uuid));
    }

    #[test]
    fn test_decode_text_value_bytes() {
        assert_eq!(decode_text_value(11, "deadbeef"), json!("\\xdeadbeef"));
    }

    #[test]
    fn test_decode_text_value_unknown() {
        assert_eq!(decode_text_value(99, "foo"), json!("<type_tag:99>"));
    }
}
