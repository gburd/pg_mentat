use pgrx::prelude::*;
use pgrx::JsonB;

/// Pull entity data using a pull pattern
///
/// Accepts a pull pattern like:
/// ```edn
/// [:person/name :person/age]
/// ```
/// and an entity ID
#[pg_extern]
fn mentat_pull(pattern: &str, entity_id: i64) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    // Parse the pull pattern (placeholder)
    // TODO: Use mentat_query_pull to execute pull query

    // Query the datoms for this entity
    let result = Spi::connect(|client| {
        let query = format!(
            "SELECT a, v, value_type_tag FROM mentat.datoms WHERE e = {} AND added = true",
            entity_id
        );

        let mut attrs = Vec::new();

        for row in client.select(&query, None, &[])? {
            let a: i64 = row.get(1)?.ok_or("Missing attribute")?;
            let _v_bytes: Vec<u8> = row.get(2)?.ok_or("Missing value")?;
            let _v_type: i16 = row.get(3)?.ok_or("Missing type")?;

            attrs.push(a);
        }

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(attrs)
    })?;

    let response = format!(
        "{{\"pattern\":\"{}\",\"entity\":{},\"attributes\":{}}}",
        pattern,
        entity_id,
        result.len()
    );

    Ok(JsonB(serde_json::from_str(&response)?))
}
