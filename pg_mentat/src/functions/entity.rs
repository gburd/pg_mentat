use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::json;

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
        let query = "SELECT s.ident, d.v, d.value_type_tag \
             FROM mentat.datoms d \
             JOIN mentat.schema s ON d.a = s.entid \
             WHERE d.e = $1 AND d.added = true";

        for row in client.select(query, None, &[DatumWithOid::from(entity_id)])? {
            let ident: String = row.get(1)?.ok_or(
                ":db.error/data-integrity Missing ident column in schema join for entity query")?;
            let v_bytes: Vec<u8> = row.get(2)?.ok_or(
                ":db.error/data-integrity Missing value (v) column in datoms for entity query")?;
            let v_type_tag: i16 = row.get(3)?.ok_or(
                ":db.error/data-integrity Missing value_type_tag column in datoms for entity query")?;

            // Decode value based on type tag
            let decoded_value = decode_value(&v_bytes, v_type_tag)?;

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

/// Decode BYTEA value based on type tag
/// Type tags match the encoding in transact.rs:
/// 0=ref, 1=boolean, 2=long, 3=double, 4=instant, 7=string, 8=keyword, 10=uuid, 11=bytes
fn decode_value(
    bytes: &[u8],
    type_tag: i16,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    match type_tag {
        1 => {
            // boolean
            if bytes.is_empty() {
                return Err(":db.error/data-corruption Invalid boolean value: empty bytes. \
                            The datoms table may contain corrupted data.".into());
            }
            Ok(json!(bytes[0] != 0))
        }
        0 | 2 => {
            // ref or long (both i64 little-endian)
            if bytes.len() != 8 {
                return Err(format!(
                    ":db.error/data-corruption Invalid i64 value: expected 8 bytes, got {}. \
                     The datoms table may contain corrupted data.",
                    bytes.len()
                ).into());
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
                    ":db.error/data-corruption Invalid double value: expected 8 bytes, got {}.",
                    bytes.len()
                ).into());
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
                    ":db.error/data-corruption Invalid instant value: expected 8 bytes, got {}.",
                    bytes.len()
                ).into());
            }
            let micros = i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Ok(json!(micros))
        }
        7 => {
            // string
            let s = String::from_utf8(bytes.to_vec())?;
            Ok(json!(s))
        }
        8 => {
            // keyword - stored without leading colon
            let s = String::from_utf8(bytes.to_vec())?;
            Ok(json!(format!(":{}", s)))
        }
        10 => {
            // uuid (16 bytes)
            if bytes.len() != 16 {
                return Err(format!(
                    ":db.error/data-corruption Invalid UUID value: expected 16 bytes, got {}.",
                    bytes.len()
                ).into());
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
        _ => Err(format!(
            ":db.error/unsupported-type Unsupported value type tag: {}. \
             Known tags: 0=ref, 1=boolean, 2=long, 3=double, 4=instant, 7=string, \
             8=keyword, 10=uuid, 11=bytes.",
            type_tag
        )
        .into()),
    }
}
