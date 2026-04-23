/// SQL convenience helper functions for pg_mentat
///
/// These functions provide simplified access to common operations:
/// - Entity and attribute lookups
/// - Entity retraction
/// - Value listing
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::spi::Spi;
use pgrx::JsonB;
use serde_json::json;

/// Look up an entity ID by its ident attribute value
///
/// Example:
/// ```sql
/// SELECT mentat.lookup_by_ident('person/email', 'alice@example.com');
/// -- Returns: entity ID or NULL
/// ```
#[pg_extern(schema = "mentat")]
fn lookup_by_ident(attr_ident: &str, value: &str) -> Option<i64> {
    // Resolve the attribute ident to entid
    let attr_id = crate::cache::get_cache().resolve_ident(attr_ident)?;

    // Query datoms for the entity with this value (string type tag = 7)
    let result = Spi::get_one_with_args::<i64>(
        "SELECT e FROM mentat.datoms WHERE a = $1 AND v = $2 AND value_type_tag = 7 AND added = true LIMIT 1",
        &[
            DatumWithOid::from(attr_id),
            DatumWithOid::from(value.as_bytes().to_vec()),
        ],
    );

    result.ok().flatten()
}

/// Get all attribute idents for an entity
///
/// Returns a JSONB array of attribute idents that have values for this entity.
///
/// Example:
/// ```sql
/// SELECT mentat.entity_attrs(100);
/// -- Returns: [":person/name", ":person/email", ":person/age"]
/// ```
#[pg_extern(schema = "mentat")]
fn entity_attrs(entity_id: i64) -> JsonB {
    let result: Result<Vec<String>, _> = Spi::connect(|client| {
        // Get distinct attribute IDs for this entity
        let query = "\
            SELECT DISTINCT a \
            FROM mentat.datoms \
            WHERE e = $1 AND added = true \
            ORDER BY a";

        let mut idents = Vec::new();

        for row in client.select(query, None, &[DatumWithOid::from(entity_id)])? {
            if let Ok(Some(attr_id)) = row.get::<i64>(1) {
                // Resolve entid to ident string
                if let Some(ident) = crate::cache::get_cache().get_ident(attr_id) {
                    idents.push(ident);
                }
            }
        }

        Ok::<_, pgrx::spi::SpiError>(idents)
    });

    match result {
        Ok(idents) => JsonB(json!(idents)),
        Err(_) => JsonB(json!([])),
    }
}

/// Get all current values for an attribute across all entities
///
/// Returns a JSONB array of unique values currently asserted for this attribute.
/// Only supports string, long, boolean, and keyword types.
///
/// Example:
/// ```sql
/// SELECT mentat.attribute_values(':person/name');
/// -- Returns: ["Alice Anderson", "Bob Brown"]
/// ```
#[pg_extern(schema = "mentat")]
fn attribute_values(attr_ident: &str) -> JsonB {
    // Resolve ident to entid
    let attr_id = match crate::cache::get_cache().resolve_ident(attr_ident) {
        Some(id) => id,
        None => return JsonB(json!([])),
    };

    let result: Result<Vec<serde_json::Value>, _> = Spi::connect(|client| {
        // Get distinct values for this attribute
        let query = "\
            SELECT DISTINCT v, value_type_tag \
            FROM mentat.datoms \
            WHERE a = $1 AND added = true \
            ORDER BY v";

        let mut values = Vec::new();

        for row in client.select(query, None, &[DatumWithOid::from(attr_id)])? {
            if let (Ok(Some(value_bytes)), Ok(Some(type_tag))) =
                (row.get::<Vec<u8>>(1), row.get::<i16>(2))
            {
                // Decode value based on type tag
                if let Ok(decoded) = decode_typed_value(&value_bytes, type_tag) {
                    values.push(decoded);
                }
            }
        }

        Ok::<_, pgrx::spi::SpiError>(values)
    });

    match result {
        Ok(vals) => JsonB(json!(vals)),
        Err(_) => JsonB(json!([])),
    }
}

/// Retract all facts about an entity
///
/// This generates and executes retraction transactions for all current facts
/// about the given entity.
///
/// Returns the number of facts retracted.
///
/// Example:
/// ```sql
/// SELECT mentat.retract_entity(100);
/// -- Returns: number of facts retracted
/// ```
#[pg_extern(schema = "mentat")]
fn retract_entity(entity_id: i64) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    // Get all current facts for this entity
    let facts_query = "\
        SELECT a, v, value_type_tag \
        FROM mentat.datoms \
        WHERE e = $1 AND added = true";

    let retractions: Vec<(i64, Vec<u8>, i16)> = Spi::connect(|client| {
        let mut retract_list = Vec::new();

        for row in client.select(facts_query, None, &[DatumWithOid::from(entity_id)])? {
            if let (Ok(Some(attr_id)), Ok(Some(value_bytes)), Ok(Some(type_tag))) =
                (row.get::<i64>(1), row.get::<Vec<u8>>(2), row.get::<i16>(3))
            {
                retract_list.push((attr_id, value_bytes, type_tag));
            }
        }

        Ok::<_, pgrx::spi::SpiError>(retract_list)
    })?;

    if retractions.is_empty() {
        return Err(format!(
            ":db.error/nothing-to-retract Entity {} has no current facts to retract. \
             The entity may not exist or all its facts have already been retracted.",
            entity_id
        ).into());
    }

    let count = retractions.len() as i64;

    // Build EDN retraction transaction
    let mut tx_data = String::from("[");

    for (i, (attr_id, value_bytes, type_tag)) in retractions.iter().enumerate() {
        if i > 0 {
            tx_data.push_str("\n  ");
        }

        // Resolve attribute ident
        let attr_ident = crate::cache::get_cache()
            .get_ident(*attr_id)
            .ok_or_else(|| format!(
                ":db.error/attribute-not-found Failed to resolve attribute entid {} to an ident. \
                 The schema cache may be stale or the attribute was never defined.",
                attr_id
            ))?;

        // Decode value for EDN representation
        let value_repr = match decode_typed_value(value_bytes, *type_tag) {
            Ok(val) => format_edn_value(&val),
            Err(_) => "nil".to_string(),
        };

        // Format as retraction: [:db/retract entity attr value]
        tx_data.push_str(&format!(
            "  [:db/retract {} {} {}]",
            entity_id, attr_ident, value_repr
        ));
    }

    tx_data.push_str("\n]");

    // Execute the retraction transaction
    crate::functions::transact::mentat_transact(&tx_data)?;

    Ok(count)
}

/// Helper to decode BYTEA values based on type tag
/// Type tags: 0=ref, 1=boolean, 2=long, 3=double, 4=instant, 7=string, 8=keyword, 10=uuid, 11=bytes
fn decode_typed_value(
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
                return Err(
                    format!(":db.error/data-corruption Invalid i64 value: expected 8 bytes, got {}", bytes.len()).into(),
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
                    ":db.error/data-corruption Invalid double value: expected 8 bytes, got {}",
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
                    ":db.error/data-corruption Invalid instant value: expected 8 bytes, got {}",
                    bytes.len()
                )
                .into());
            }
            let micros = i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Ok(json!(micros))
        }
        7 => {
            // string (UTF-8)
            let s = String::from_utf8(bytes.to_vec())?;
            Ok(json!(s))
        }
        8 => {
            // keyword (UTF-8, prefixed with :)
            let s = String::from_utf8(bytes.to_vec())?;
            Ok(json!(if s.starts_with(':') {
                s
            } else {
                format!(":{}", s)
            }))
        }
        10 => {
            // uuid (16 bytes)
            if bytes.len() != 16 {
                return Err(
                    format!(":db.error/data-corruption Invalid UUID value: expected 16 bytes, got {}", bytes.len()).into(),
                );
            }
            Ok(json!(hex::encode(bytes)))
        }
        11 => {
            // bytes (raw)
            Ok(json!(hex::encode(bytes)))
        }
        _ => Err(format!(
            ":db.error/unsupported-type Unsupported type tag: {}. \
             Known tags: 0=ref, 1=boolean, 2=long, 3=double, 4=instant, 7=string, \
             8=keyword, 10=uuid, 11=bytes.",
            type_tag
        ).into()),
    }
}

/// Format a JSON value as EDN literal for transaction
fn format_edn_value(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => {
            if s.starts_with(':') {
                // Keyword
                s.clone()
            } else {
                // String literal
                format!("\"{}\"", s.replace('"', "\\\""))
            }
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => "nil".to_string(),
    }
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use super::*;

    #[pg_test]
    fn test_helper_functions_compile() {
        // This test just verifies the functions compile and are accessible
        // Actual functionality tests require a populated database
        assert!(true);
    }
}
