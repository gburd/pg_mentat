use crate::error::MentatError;
use crate::functions::store_management::get_schema_for_store;
use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::json;

/// Return complete schema information as JSON
///
/// Returns all attributes with their properties:
/// ```json
/// {
///   ":person/name": {
///     "entid": 65,
///     "valueType": "string",
///     "cardinality": "one",
///     "unique": null,
///     "indexed": true,
///     "fulltext": false,
///     "component": false,
///     "noHistory": false
///   },
///   ...
/// }
/// ```
#[pg_extern]
pub fn mentat_schema() -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    mentat_schema_in_store("default")
}

/// Return complete schema information as JSON from a named store
///
/// Returns all attributes with their properties from the specified store.
///
/// # Example
/// ```sql
/// SELECT mentat_schema_in_store('my_store');
/// ```
#[pg_extern]
pub fn mentat_schema_in_store(store: &str) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let schema_name = get_schema_for_store(store);
    let mut schema_map = serde_json::Map::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT entid, ident, value_type::TEXT, cardinality::TEXT, \
                   unique_constraint::TEXT, indexed, fulltext, component, no_history \
            FROM {schema}.schema \
            ORDER BY entid",
            schema = schema_name
        );

        for row in client.select(&query, None, &[])? {
            // Column indices are 1-based in pgrx
            let entid: i64 = row.get(1)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing entid in mentat.schema row".to_string(),
            })?;
            let ident: String = row.get(2)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing ident in mentat.schema row".to_string(),
            })?;
            let value_type: String = row.get(3)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing value_type in mentat.schema row".to_string(),
            })?;
            let cardinality: String = row.get(4)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing cardinality in mentat.schema row".to_string(),
            })?;
            let unique_constraint: Option<String> = row.get(5)?;
            let indexed: bool = row.get(6)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing indexed in mentat.schema row".to_string(),
            })?;
            let fulltext: bool = row.get(7)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing fulltext in mentat.schema row".to_string(),
            })?;
            let component: bool = row.get(8)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing component in mentat.schema row".to_string(),
            })?;
            let no_history: bool = row.get(9)?.ok_or_else(|| MentatError::DataIntegrity {
                message: "Missing no_history in mentat.schema row".to_string(),
            })?;

            let attr_info = json!({
                "entid": entid,
                "valueType": value_type,
                "cardinality": cardinality,
                "unique": unique_constraint,
                "indexed": indexed,
                "fulltext": fulltext,
                "component": component,
                "noHistory": no_history
            });

            schema_map.insert(ident, attr_info);
        }

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })?;

    Ok(JsonB(serde_json::Value::Object(schema_map)))
}
