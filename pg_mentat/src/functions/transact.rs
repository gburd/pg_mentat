use pgrx::prelude::*;
use edn::entities::{Entity, OpType};
use edn::parse;

/// Process an EDN transaction and return a TxReport
///
/// Accepts an EDN transaction like:
/// ```edn
/// [[:db/add "tempid" :person/name "Alice"]
///  [:db/add "tempid" :person/age 30]]
/// ```
#[pg_extern]
fn mentat_transact(edn_tx: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Parse EDN transaction
    let value_and_span = parse::value(edn_tx)?;
    let value = value_and_span.without_spans();

    // Validate it's a vector
    let entities = match value {
        edn::Value::Vector(ref vec) => vec,
        _ => return Err("Transaction must be a vector of entities".into()),
    };

    // Allocate transaction ID
    let tx_id = Spi::get_one::<i64>("SELECT mentat.allocate_entid('db.part/tx')")
        .ok()
        .flatten()
        .ok_or("Failed to allocate transaction ID")?;

    // Create transaction record
    Spi::run(&format!(
        "INSERT INTO mentat.transactions (tx_id, instant) VALUES ({}, CURRENT_TIMESTAMP)",
        tx_id
    ))?;

    // Process each entity in the transaction
    let mut tempid_map: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    let mut datom_count = 0;

    for entity_value in entities {
        // Parse entity structure
        match entity_value {
            edn::Value::Vector(ref entity_vec) if entity_vec.len() >= 4 => {
                // Parse [:db/add e a v] or [:db/retract e a v]
                let op = match &entity_vec[0] {
                    edn::Value::Keyword(kw) if kw.name() == "add" => OpType::Add,
                    edn::Value::Keyword(kw) if kw.name() == "retract" => OpType::Retract,
                    _ => continue,
                };

                // Resolve or allocate entity ID
                let e = resolve_entity_place(&entity_vec[1], &mut tempid_map)?;

                // Resolve attribute ID
                let a = resolve_attribute(&entity_vec[2])?;

                // Encode value as BYTEA
                let (v_bytes, v_type_tag) = encode_value(&entity_vec[3])?;

                // Insert datom
                let added = matches!(op, OpType::Add);
                Spi::run(&format!(
                    "INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag) VALUES ({}, {}, decode('{}', 'hex'), {}, {}, {})",
                    e, a, hex::encode(&v_bytes), tx_id, added, v_type_tag
                ))?;

                datom_count += 1;
            }
            edn::Value::Map(ref map) => {
                // Process map notation {:db/id "tempid" :attr1 val1 ...}
                let e = if let Some(id_val) = map.get(&edn::Value::Keyword(
                    edn::symbols::Keyword::plain("db/id")
                )) {
                    resolve_entity_place(id_val, &mut tempid_map)?
                } else {
                    // Allocate new entity ID
                    Spi::get_one::<i64>("SELECT mentat.allocate_entid('db.part/user')")
                        .ok()
                        .flatten()
                        .ok_or("Failed to allocate entity ID")?
                };

                // Insert each attribute-value pair
                for (attr_key, value) in map {
                    // Skip :db/id
                    if let edn::Value::Keyword(kw) = attr_key {
                        if kw.name() == "db/id" {
                            continue;
                        }
                    }

                    let a = resolve_attribute(attr_key)?;
                    let (v_bytes, v_type_tag) = encode_value(value)?;

                    Spi::run(&format!(
                        "INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag) VALUES ({}, {}, decode('{}', 'hex'), {}, true, {})",
                        e, a, hex::encode(&v_bytes), tx_id, v_type_tag
                    ))?;

                    datom_count += 1;
                }
            }
            _ => {}
        }
    }

    // Build TxReport response
    let tempids_json: Vec<String> = tempid_map
        .iter()
        .map(|(k, v)| format!("\"{}\":{}", k, v))
        .collect();

    Ok(format!(
        "{{\"tx-id\":{},\"tx-instant\":null,\"tempids\":{{{}}},\"datoms-inserted\":{}}}",
        tx_id,
        tempids_json.join(","),
        datom_count
    ))
}

/// Resolve entity place (entid, tempid, or ident)
fn resolve_entity_place(
    value: &edn::Value,
    tempid_map: &mut std::collections::BTreeMap<String, i64>,
) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    match value {
        edn::Value::Integer(i) => Ok(*i),
        edn::Value::Text(ref s) => {
            // Tempid: allocate or reuse
            if let Some(&existing) = tempid_map.get::<str>(s.as_ref()) {
                Ok(existing)
            } else {
                let entid = Spi::get_one::<i64>("SELECT mentat.allocate_entid('db.part/user')")
                    .ok()
                    .flatten()
                    .ok_or("Failed to allocate entity ID")?;
                tempid_map.insert(s.to_string(), entid);
                Ok(entid)
            }
        }
        edn::Value::Keyword(kw) => {
            // Resolve keyword ident
            let ident_str = format!("{}:{}", kw.namespace().unwrap_or(""), kw.name());
            let entid = Spi::get_one::<i64>(&format!(
                "SELECT mentat.resolve_ident('{}')",
                ident_str
            ))
            .ok()
            .flatten()
            .ok_or("Failed to resolve ident")?;
            Ok(entid)
        }
        _ => Err("Invalid entity place".into()),
    }
}

/// Resolve attribute (entid or ident)
fn resolve_attribute(value: &edn::Value) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    match value {
        edn::Value::Integer(i) => Ok(*i),
        edn::Value::Keyword(kw) => {
            let ident_str = format!("{}:{}", kw.namespace().unwrap_or(""), kw.name());
            let entid = Spi::get_one::<i64>(&format!(
                "SELECT mentat.resolve_ident('{}')",
                ident_str
            ))
            .ok()
            .flatten()
            .ok_or("Failed to resolve attribute")?;
            Ok(entid)
        }
        _ => Err("Invalid attribute".into()),
    }
}

/// Encode EDN value as BYTEA with type tag
/// Returns (bytes, type_tag) where type_tag corresponds to mentat.value_type
fn encode_value(
    value: &edn::Value,
) -> Result<(Vec<u8>, i16), Box<dyn std::error::Error + Send + Sync>> {
    match value {
        edn::Value::Boolean(b) => Ok((vec![if *b { 1 } else { 0 }], 1)), // boolean = 1
        edn::Value::Integer(i) => Ok((i.to_le_bytes().to_vec(), 2)),     // long = 2
        edn::Value::Text(ref s) => Ok((s.as_bytes().to_vec(), 7)),       // string = 7
        edn::Value::Keyword(kw) => {
            let s = format!("{}:{}", kw.namespace().unwrap_or(""), kw.name());
            Ok((s.as_bytes().to_vec(), 8)) // keyword = 8
        }
        _ => Err("Unsupported value type".into()),
    }
}
