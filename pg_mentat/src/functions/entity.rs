use crate::error::MentatError;
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::json;

/// Type tags matching encode_value in transact.rs.
mod type_tag {
    pub const REF: i16 = 0;
    pub const BOOLEAN: i16 = 1;
    pub const LONG: i16 = 2;
    pub const DOUBLE: i16 = 3;
    pub const INSTANT: i16 = 4;
    pub const STRING: i16 = 7;
    pub const KEYWORD: i16 = 8;
    pub const UUID: i16 = 10;
    pub const BYTES: i16 = 11;
}

/// Fetch all datoms for a specific entity and return as JSON
///
/// Returns entity data as a JSON map:
/// ```json
/// {
///   ":person/name": "Alice",
///   ":person/age": 30,
///   ":db/id": 123
/// }
/// ```
#[pg_extern]
pub fn mentat_entity(entity_id: i64) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    entity("default", entity_id)
}

/// Fetch all datoms for a specific entity from a named store and return as JSON
///
/// Returns entity data as a JSON map from the specified store.
/// Queries type-specific tables using UNION ALL with store_id filtering.
///
/// # Example
/// ```sql
/// SELECT mentat.entity('my_store', 123);
/// ```
#[pg_extern]
pub fn entity(store: &str, entity_id: i64) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let mut entity_map = serde_json::Map::new();

    // Always include the entity ID
    entity_map.insert(":db/id".to_string(), json!(entity_id));

    // Look up store_id from the store name
    let store_id: i32 = Spi::get_one_with_args(
        "SELECT store_id FROM mentat.stores WHERE store_name = $1",
        &[DatumWithOid::from(store)],
    )?
    .ok_or_else(|| MentatError::StoreNotFound {
        store_name: store.to_string(),
    })?;

    Spi::connect(|client| {
        // Query all type-specific tables with UNION ALL, joined to schema for ident
        let query = "
            SELECT s.ident, 0::SMALLINT AS type_tag, d.v::TEXT AS value
            FROM mentat.datoms_ref_new d
            JOIN mentat.schema s ON d.a = s.entid
            WHERE d.store_id = $1 AND d.e = $2 AND d.added = true
            UNION ALL
            SELECT s.ident, 1::SMALLINT, d.v::TEXT
            FROM mentat.datoms_boolean_new d
            JOIN mentat.schema s ON d.a = s.entid
            WHERE d.store_id = $1 AND d.e = $2 AND d.added = true
            UNION ALL
            SELECT s.ident, 2::SMALLINT, d.v::TEXT
            FROM mentat.datoms_long_new d
            JOIN mentat.schema s ON d.a = s.entid
            WHERE d.store_id = $1 AND d.e = $2 AND d.added = true
            UNION ALL
            SELECT s.ident, 3::SMALLINT, d.v::TEXT
            FROM mentat.datoms_double_new d
            JOIN mentat.schema s ON d.a = s.entid
            WHERE d.store_id = $1 AND d.e = $2 AND d.added = true
            UNION ALL
            SELECT s.ident, 4::SMALLINT,
                   (EXTRACT(EPOCH FROM d.v)::BIGINT * 1000000 +
                    EXTRACT(MICROSECOND FROM d.v)::BIGINT % 1000000)::TEXT
            FROM mentat.datoms_instant_new d
            JOIN mentat.schema s ON d.a = s.entid
            WHERE d.store_id = $1 AND d.e = $2 AND d.added = true
            UNION ALL
            SELECT s.ident, 7::SMALLINT, d.v
            FROM mentat.datoms_text_new d
            JOIN mentat.schema s ON d.a = s.entid
            WHERE d.store_id = $1 AND d.e = $2 AND d.added = true
            UNION ALL
            SELECT s.ident, 8::SMALLINT, d.v
            FROM mentat.datoms_keyword_new d
            JOIN mentat.schema s ON d.a = s.entid
            WHERE d.store_id = $1 AND d.e = $2 AND d.added = true
            UNION ALL
            SELECT s.ident, 10::SMALLINT, d.v::TEXT
            FROM mentat.datoms_uuid_new d
            JOIN mentat.schema s ON d.a = s.entid
            WHERE d.store_id = $1 AND d.e = $2 AND d.added = true
            UNION ALL
            SELECT s.ident, 11::SMALLINT, encode(d.v, 'hex')
            FROM mentat.datoms_bytes_new d
            JOIN mentat.schema s ON d.a = s.entid
            WHERE d.store_id = $1 AND d.e = $2 AND d.added = true
        ";

        for row in client.select(
            query,
            None,
            &[DatumWithOid::from(store_id), DatumWithOid::from(entity_id)],
        )? {
            let ident: String = row.get(1)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing ident column in schema join for entity query".to_string(),
            })?;
            let v_type_tag: i16 = row.get(2)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing type_tag column in entity query".to_string(),
            })?;
            let value_str: String = row.get(3)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing value column in entity query".to_string(),
            })?;

            // Decode value from the text representation based on type_tag
            let decoded_value = decode_text_value(v_type_tag, &value_str)?;

            // For cardinality-many attributes, we need to accumulate values
            if let Some(existing) = entity_map.get(&ident) {
                // Convert to array if not already
                let array = if existing.is_array() {
                    let mut arr = existing.as_array().unwrap().clone();
                    arr.push(decoded_value);
                    arr
                } else {
                    vec![existing.clone(), decoded_value]
                };
                entity_map.insert(ident, json!(array));
            } else {
                entity_map.insert(ident, decoded_value);
            }
        }

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })?;

    Ok(JsonB(serde_json::Value::Object(entity_map)))
}

/// Decode a typed value from its text representation and type_tag.
fn decode_text_value(
    type_tag: i16,
    value_str: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    match type_tag {
        type_tag::REF => {
            let ref_id: i64 = value_str.parse().map_err(|_| MentatError::DataCorruption {
                message: format!("Invalid ref value: {}", value_str),
            })?;
            Ok(json!(ref_id))
        }
        type_tag::BOOLEAN => {
            // PostgreSQL text representation of boolean: 't'/'f' or 'true'/'false'
            let b = match value_str {
                "t" | "true" => true,
                "f" | "false" => false,
                _ => {
                    return Err(MentatError::DataCorruption {
                        message: format!("Invalid boolean value: {}", value_str),
                    }
                    .into())
                }
            };
            Ok(json!(b))
        }
        type_tag::LONG => {
            let n: i64 = value_str.parse().map_err(|_| MentatError::DataCorruption {
                message: format!("Invalid long value: {}", value_str),
            })?;
            Ok(json!(n))
        }
        type_tag::DOUBLE => {
            let f: f64 = value_str.parse().map_err(|_| MentatError::DataCorruption {
                message: format!("Invalid double value: {}", value_str),
            })?;
            Ok(json!(f))
        }
        type_tag::STRING => Ok(json!(value_str)),
        type_tag::KEYWORD => Ok(json!(format!(":{}", value_str))),
        type_tag::INSTANT => {
            let micros: i64 = value_str.parse().map_err(|_| MentatError::DataCorruption {
                message: format!("Invalid instant value: {}", value_str),
            })?;
            Ok(json!(micros))
        }
        type_tag::UUID => Ok(json!(value_str)),
        type_tag::BYTES => {
            // Already hex-encoded by the SQL query
            Ok(json!(value_str))
        }
        _ => Err(MentatError::UnsupportedType { type_tag }.into()),
    }
}
