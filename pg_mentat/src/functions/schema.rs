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
fn mentat_schema() -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let mut schema_map = serde_json::Map::new();

    Spi::connect(|client| {
        let query = "SELECT entid, ident, value_type::TEXT, cardinality::TEXT, \
                           unique_constraint::TEXT, indexed, fulltext, component, no_history \
                    FROM mentat.schema \
                    ORDER BY entid";

        for row in client.select(query, None, &[])? {
            // Column indices are 1-based in pgrx
            let entid: i64 = row.get(1)?.ok_or("Missing entid")?;
            let ident: String = row.get(2)?.ok_or("Missing ident")?;
            let value_type: String = row.get(3)?.ok_or("Missing value_type")?;
            let cardinality: String = row.get(4)?.ok_or("Missing cardinality")?;
            let unique_constraint: Option<String> = row.get(5)?;
            let indexed: bool = row.get(6)?.ok_or("Missing indexed")?;
            let fulltext: bool = row.get(7)?.ok_or("Missing fulltext")?;
            let component: bool = row.get(8)?.ok_or("Missing component")?;
            let no_history: bool = row.get(9)?.ok_or("Missing no_history")?;

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
