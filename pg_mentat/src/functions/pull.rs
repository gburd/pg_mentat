use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::spi::SpiClient;
use pgrx::JsonB;
use serde_json::json;

/// Pull entity data using a pull pattern.
///
/// Accepts a pull pattern (EDN vector of keywords) like:
/// ```edn
/// [:person/name :person/age]
/// ```
/// and an entity ID. Returns a JSON map of the requested attributes:
/// ```json
/// {":person/name": "Alice", ":person/age": 30}
/// ```
///
/// The special pattern `[*]` pulls all attributes for the entity.
#[pg_extern]
fn mentat_pull(
    pattern: &str,
    entity_id: i64,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    // Parse the pull pattern as EDN
    let parsed =
        edn::parse::value(pattern).map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("Invalid pull pattern: {e}").into()
        })?;
    let pattern_value = parsed.without_spans();

    let keywords = match &pattern_value {
        edn::Value::Vector(items) => extract_pull_keywords(items)?,
        _ => return Err("Pull pattern must be a vector (e.g. [:person/name :person/age])".into()),
    };

    // Build the result map
    let mut result_map = serde_json::Map::new();
    result_map.insert(":db/id".to_string(), json!(entity_id));

    Spi::connect(|client| {
        if keywords.is_empty() {
            // Wildcard pull: fetch all attributes for this entity
            pull_all_attributes(&client, entity_id, &mut result_map)
        } else {
            // Selective pull: fetch only requested attributes
            pull_specific_attributes(&client, entity_id, &keywords, &mut result_map)
        }
    })?;

    Ok(JsonB(serde_json::Value::Object(result_map)))
}

/// Extract keyword strings from the pull pattern vector.
/// Returns an empty vec if the pattern is `[*]` (wildcard pull).
fn extract_pull_keywords(
    items: &[edn::Value],
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    // Check for wildcard pattern [*]
    if items.len() == 1 {
        if let edn::Value::PlainSymbol(ref sym) = items[0] {
            if sym.name() == "*" {
                return Ok(Vec::new());
            }
        }
    }

    let mut keywords = Vec::new();
    for item in items {
        match item {
            edn::Value::Keyword(kw) => {
                // Reconstruct the keyword string with colon prefix, matching schema ident format
                let kw_str = if let Some(ns) = kw.namespace() {
                    format!(":{ns}/{}", kw.name())
                } else {
                    format!(":{}", kw.name())
                };
                keywords.push(kw_str);
            }
            edn::Value::PlainSymbol(ref sym) if sym.name() == "*" => {
                // Wildcard mixed with specific attrs: treat as wildcard
                return Ok(Vec::new());
            }
            _ => {
                return Err(format!("Pull pattern elements must be keywords, got: {item}").into());
            }
        }
    }
    Ok(keywords)
}

/// Pull all attributes for an entity (wildcard `[*]` pattern).
fn pull_all_attributes(
    client: &SpiClient<'_>,
    entity_id: i64,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let query = "SELECT s.ident, s.cardinality::TEXT, d.v, d.value_type_tag \
                 FROM mentat.datoms d \
                 JOIN mentat.schema s ON d.a = s.entid \
                 WHERE d.e = $1 AND d.added = true \
                 ORDER BY s.ident";

    for row in client.select(query, None, &[DatumWithOid::from(entity_id)])? {
        let ident: String = row.get(1)?.ok_or("Missing ident")?;
        let cardinality: String = row.get(2)?.ok_or("Missing cardinality")?;
        let v_bytes: Vec<u8> = row.get(3)?.ok_or("Missing value")?;
        let v_type_tag: i16 = row.get(4)?.ok_or("Missing type tag")?;

        let decoded = decode_typed_value(&v_bytes, v_type_tag)?;
        insert_value(result_map, &ident, decoded, &cardinality);
    }

    Ok(())
}

/// Pull specific attributes for an entity.
fn pull_specific_attributes(
    client: &SpiClient<'_>,
    entity_id: i64,
    keywords: &[String],
    result_map: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // For each requested attribute, resolve its entid and query the datoms
    for keyword in keywords {
        let query = "SELECT s.cardinality::TEXT, d.v, d.value_type_tag \
                     FROM mentat.datoms d \
                     JOIN mentat.schema s ON d.a = s.entid \
                     WHERE d.e = $1 AND s.ident = $2 AND d.added = true";

        for row in client.select(
            query,
            None,
            &[
                DatumWithOid::from(entity_id),
                DatumWithOid::from(keyword.as_str()),
            ],
        )? {
            let cardinality: String = row.get(1)?.ok_or("Missing cardinality")?;
            let v_bytes: Vec<u8> = row.get(2)?.ok_or("Missing value")?;
            let v_type_tag: i16 = row.get(3)?.ok_or("Missing type tag")?;

            let decoded = decode_typed_value(&v_bytes, v_type_tag)?;
            insert_value(result_map, keyword, decoded, &cardinality);
        }
    }

    Ok(())
}

/// Insert a decoded value into the result map, handling cardinality.
/// For cardinality "many", values are accumulated into a JSON array.
/// For cardinality "one", the last value wins (though there should only be one).
fn insert_value(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: serde_json::Value,
    cardinality: &str,
) {
    if cardinality == "many" {
        // Cardinality-many: always store as array
        if let Some(existing) = map.get_mut(key) {
            if let Some(arr) = existing.as_array_mut() {
                arr.push(value);
            } else {
                // Shouldn't happen since we always start with an array for "many",
                // but handle gracefully
                let prev = existing.clone();
                *existing = json!([prev, value]);
            }
        } else {
            map.insert(key.to_string(), json!([value]));
        }
    } else {
        // Cardinality-one: single value (last write wins if duplicates exist)
        map.insert(key.to_string(), value);
    }
}

/// Decode a BYTEA value based on value_type_tag.
///
/// Type tags (matching encode_value in transact.rs):
///   1 = boolean
///   2 = long (i64 little-endian)
///   3 = double (f64 little-endian)
///   4 = instant (i64 microseconds since epoch, little-endian)
///   5 = ref (i64 entity ID, little-endian)
///   7 = string (UTF-8 bytes)
///   8 = keyword (UTF-8 bytes, stored without leading colon)
///  10 = uuid (16 bytes)
///  11 = bytes (raw)
fn decode_typed_value(
    bytes: &[u8],
    type_tag: i16,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    match type_tag {
        1 => {
            // boolean
            if bytes.is_empty() {
                return Err("Invalid boolean value: empty bytes".into());
            }
            Ok(json!(bytes[0] != 0))
        }
        2 | 5 => {
            // long or ref (both i64 little-endian)
            if bytes.len() != 8 {
                return Err(
                    format!("Invalid i64 value: expected 8 bytes, got {}", bytes.len()).into(),
                );
            }
            let val = i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Ok(json!(val))
        }
        3 => {
            // double (f64 little-endian)
            if bytes.len() != 8 {
                return Err(format!(
                    "Invalid double value: expected 8 bytes, got {}",
                    bytes.len()
                )
                .into());
            }
            let val = f64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Ok(json!(val))
        }
        4 => {
            // instant (i64 microseconds since epoch, little-endian)
            if bytes.len() != 8 {
                return Err(format!(
                    "Invalid instant value: expected 8 bytes, got {}",
                    bytes.len()
                )
                .into());
            }
            let micros = i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            // Return as integer (microseconds since epoch)
            Ok(json!(micros))
        }
        7 => {
            // string (UTF-8)
            let s = String::from_utf8(bytes.to_vec())?;
            Ok(json!(s))
        }
        8 => {
            // keyword - stored without leading colon
            let s = String::from_utf8(bytes.to_vec())?;
            Ok(json!(format!(":{s}")))
        }
        10 => {
            // uuid (16 bytes)
            if bytes.len() != 16 {
                return Err(
                    format!("Invalid UUID value: expected 16 bytes, got {}", bytes.len()).into(),
                );
            }
            let uuid_str = format!(
                "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                bytes[0], bytes[1], bytes[2], bytes[3],
                bytes[4], bytes[5],
                bytes[6], bytes[7],
                bytes[8], bytes[9],
                bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
            );
            Ok(json!(uuid_str))
        }
        11 => {
            // raw bytes - return as hex string
            Ok(json!(hex::encode(bytes)))
        }
        _ => Err(format!("Unsupported value type tag: {type_tag}").into()),
    }
}
