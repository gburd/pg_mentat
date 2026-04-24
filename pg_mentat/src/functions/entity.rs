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
    let mut entity_map = serde_json::Map::new();

    // Always include the entity ID
    entity_map.insert(":db/id".to_string(), json!(entity_id));

    Spi::connect(|client| {
        let query = "SELECT s.ident, d.value_type_tag, \
                            d.v_ref, d.v_bool, d.v_long, d.v_double, \
                            d.v_text, d.v_keyword, \
                            EXTRACT(EPOCH FROM d.v_instant)::BIGINT * 1000000 + \
                            EXTRACT(MICROSECOND FROM d.v_instant)::BIGINT % 1000000 AS v_instant_micros, \
                            d.v_uuid::TEXT, d.v_bytes \
                     FROM mentat.datoms d \
                     JOIN mentat.schema s ON d.a = s.entid \
                     WHERE d.e = $1 AND d.added = true";

        for row in client.select(query, None, &[DatumWithOid::from(entity_id)])? {
            let ident: String = row.get(1)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing ident column in schema join for entity query".to_string(),
            })?;
            let v_type_tag: i16 = row.get(2)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing value_type_tag column in datoms for entity query".to_string(),
            })?;

            // Decode value from typed columns
            let decoded_value = decode_row_value(&row, v_type_tag, 3)?;

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

/// Decode a typed value from an SPI row.
///
/// Columns at col_offset:
///   +0 = v_ref, +1 = v_bool, +2 = v_long, +3 = v_double,
///   +4 = v_text, +5 = v_keyword, +6 = v_instant_micros, +7 = v_uuid::TEXT, +8 = v_bytes
fn decode_row_value(
    row: &pgrx::spi::SpiHeapTupleData<'_>,
    type_tag: i16,
    col_offset: usize,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    match type_tag {
        type_tag::REF => {
            let ref_id: i64 = row.get(col_offset)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_ref for ref type".to_string(),
            })?;
            Ok(json!(ref_id))
        }
        type_tag::BOOLEAN => {
            let b: bool = row.get(col_offset + 1)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_bool for boolean type".to_string(),
            })?;
            Ok(json!(b))
        }
        type_tag::LONG => {
            let n: i64 = row.get(col_offset + 2)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_long for long type".to_string(),
            })?;
            Ok(json!(n))
        }
        type_tag::DOUBLE => {
            let f: f64 = row.get(col_offset + 3)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_double for double type".to_string(),
            })?;
            Ok(json!(f))
        }
        type_tag::STRING => {
            let s: String = row.get(col_offset + 4)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_text for string type".to_string(),
            })?;
            Ok(json!(s))
        }
        type_tag::KEYWORD => {
            let s: String = row.get(col_offset + 5)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_keyword for keyword type".to_string(),
            })?;
            Ok(json!(format!(":{s}")))
        }
        type_tag::INSTANT => {
            let micros: i64 = row.get(col_offset + 6)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_instant_micros for instant type".to_string(),
            })?;
            Ok(json!(micros))
        }
        type_tag::UUID => {
            let s: String = row.get(col_offset + 7)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_uuid for uuid type".to_string(),
            })?;
            Ok(json!(s))
        }
        type_tag::BYTES => {
            let b: Vec<u8> = row.get(col_offset + 8)?.ok_or_else(|| MentatError::DataCorruption {
                message: "Missing v_bytes for bytes type".to_string(),
            })?;
            Ok(json!(hex::encode(b)))
        }
        _ => Err(MentatError::UnsupportedType { type_tag }.into()),
    }
}
