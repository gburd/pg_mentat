/// SQL convenience helper functions for pg_mentat
///
/// These functions provide simplified access to common operations:
/// - Entity and attribute lookups
/// - Entity retraction
/// - Value listing
use crate::error::MentatError;
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::spi::Spi;
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
        "SELECT e FROM mentat.datoms WHERE a = $1 AND v_text = $2 AND value_type_tag = 7 AND added = true LIMIT 1",
        &[
            DatumWithOid::from(attr_id),
            DatumWithOid::from(value),
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
            SELECT value_type_tag, \
                   v_ref, v_bool, v_long, v_double, \
                   v_text, v_keyword, \
                   EXTRACT(EPOCH FROM v_instant)::BIGINT * 1000000 + \
                   EXTRACT(MICROSECOND FROM v_instant)::BIGINT % 1000000 AS v_instant_micros, \
                   v_uuid::TEXT, v_bytes \
            FROM mentat.datoms \
            WHERE a = $1 AND added = true \
            ORDER BY value_type_tag, v_ref, v_long, v_text, v_keyword";

        let mut values = Vec::new();

        for row in client.select(query, None, &[DatumWithOid::from(attr_id)])? {
            if let Ok(Some(type_tag)) = row.get::<i16>(1) {
                if let Ok(decoded) = decode_row_value(&row, type_tag, 2) {
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
        SELECT a, value_type_tag, \
               v_ref, v_bool, v_long, v_double, \
               v_text, v_keyword, \
               EXTRACT(EPOCH FROM v_instant)::BIGINT * 1000000 + \
               EXTRACT(MICROSECOND FROM v_instant)::BIGINT % 1000000 AS v_instant_micros, \
               v_uuid::TEXT, v_bytes \
        FROM mentat.datoms \
        WHERE e = $1 AND added = true";

    // Collect (attr_id, edn_value_repr) pairs
    let retractions: Vec<(i64, String)> = Spi::connect(|client| {
        let mut retract_list = Vec::new();

        for row in client.select(facts_query, None, &[DatumWithOid::from(entity_id)])? {
            if let (Ok(Some(attr_id)), Ok(Some(type_tag))) =
                (row.get::<i64>(1), row.get::<i16>(2))
            {
                if let Ok(decoded) = decode_row_value(&row, type_tag, 3) {
                    let edn_repr = format_edn_value(&decoded);
                    retract_list.push((attr_id, edn_repr));
                }
            }
        }

        Ok::<_, pgrx::spi::SpiError>(retract_list)
    })?;

    if retractions.is_empty() {
        return Err(MentatError::NothingToRetract {
            entity: entity_id,
        }.into());
    }

    let count = retractions.len() as i64;

    // Build EDN retraction transaction
    let mut tx_data = String::from("[");

    for (i, (attr_id, value_repr)) in retractions.iter().enumerate() {
        if i > 0 {
            tx_data.push_str("\n  ");
        }

        // Resolve attribute ident
        let attr_ident = crate::cache::get_cache()
            .get_ident(*attr_id)
            .ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
                MentatError::AttributeNotFound {
                    attr: format!("entid:{}", attr_id),
                    available: crate::error::get_available_attributes(),
                    suggestion: None,
                }.into()
            })?;

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
