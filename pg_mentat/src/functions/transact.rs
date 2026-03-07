use edn::entities::OpType;
use edn::parse;
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use std::collections::BTreeMap;

/// Entids for built-in schema attributes (from bootstrap data in 06_bootstrap_data.sql).
mod bootstrap_entids {
    pub const DB_IDENT: i64 = 10;
    pub const DB_VALUE_TYPE: i64 = 11;
    pub const DB_CARDINALITY: i64 = 12;
    pub const DB_UNIQUE: i64 = 13;
    pub const DB_INDEX: i64 = 14;
    pub const DB_FULLTEXT: i64 = 15;
    pub const DB_COMPONENT: i64 = 16;
    pub const DB_NO_HISTORY: i64 = 17;
    pub const DB_IS_COMPONENT: i64 = 18;
}

/// Schema attribute properties collected during the first pass.
#[derive(Default)]
struct SchemaBuilder {
    ident: Option<String>,       // :db/ident value (keyword as ":ns/name")
    value_type: Option<String>,  // value_type enum string (e.g. "string", "long")
    cardinality: Option<String>, // cardinality enum string ("one" or "many")
    unique: Option<String>,      // unique_type enum string ("value" or "identity")
    indexed: Option<bool>,       // :db/index
    fulltext: Option<bool>,      // :db/fulltext
    component: Option<bool>,     // :db/isComponent or :db/component
    no_history: Option<bool>,    // :db/noHistory
}

/// A single parsed assertion ready for insertion.
struct PendingDatom {
    e: i64,
    a: i64,
    v_bytes: Vec<u8>,
    v_type_tag: i16,
    added: bool,
}

/// Map a keyword ident like ":db.type/string" to the value_type enum label "string".
fn keyword_to_value_type(kw: &edn::symbols::Keyword) -> Option<&'static str> {
    let name = kw.name();
    match name {
        "ref" => Some("ref"),
        "boolean" => Some("boolean"),
        "instant" => Some("instant"),
        "long" => Some("long"),
        "double" => Some("double"),
        "string" => Some("string"),
        "keyword" => Some("keyword"),
        "uuid" => Some("uuid"),
        "bytes" => Some("bytes"),
        _ => None,
    }
}

/// Map a keyword ident like ":db.cardinality/one" to the cardinality_type enum label.
fn keyword_to_cardinality(kw: &edn::symbols::Keyword) -> Option<&'static str> {
    match kw.name() {
        "one" => Some("one"),
        "many" => Some("many"),
        _ => None,
    }
}

/// Map a keyword ident like ":db.unique/value" to the unique_type enum label.
fn keyword_to_unique(kw: &edn::symbols::Keyword) -> Option<&'static str> {
    match kw.name() {
        "value" => Some("value"),
        "identity" => Some("identity"),
        _ => None,
    }
}

/// Process an EDN transaction and return a TxReport
///
/// Accepts an EDN transaction like:
/// ```edn
/// [[:db/add "tempid" :person/name "Alice"]
///  [:db/add "tempid" :person/age 30]]
/// ```
///
/// When transactions include schema-defining assertions (:db/ident, :db/valueType,
/// :db/cardinality, etc.), the mentat.schema and mentat.idents tables are updated
/// so that newly defined attributes become immediately resolvable.
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
    Spi::run_with_args(
        "INSERT INTO mentat.transactions (tx, tx_instant) VALUES ($1, CURRENT_TIMESTAMP)",
        &[DatumWithOid::from(tx_id)],
    )?;

    // ========================================================================
    // Two-pass transaction processing:
    //   Pass 1: Parse all assertions, allocate tempids, collect schema definitions
    //   Between: Install new schema attributes into mentat.schema and mentat.idents
    //   Pass 2: Insert all datoms into mentat.datoms
    // ========================================================================

    let mut tempid_map: BTreeMap<String, i64> = BTreeMap::new();
    let mut pending_datoms: Vec<PendingDatom> = Vec::new();
    let mut schema_builders: BTreeMap<i64, SchemaBuilder> = BTreeMap::new();

    // --- Pass 1: Parse assertions and collect schema metadata ---

    for entity_value in entities {
        match entity_value {
            edn::Value::Vector(ref entity_vec) if entity_vec.len() >= 4 => {
                let op = match &entity_vec[0] {
                    edn::Value::Keyword(kw) if kw.name() == "add" => OpType::Add,
                    edn::Value::Keyword(kw) if kw.name() == "retract" => OpType::Retract,
                    _ => continue,
                };

                let e = resolve_entity_place(&entity_vec[1], &mut tempid_map)?;
                let a = resolve_attribute(&entity_vec[2])?;
                let (v_bytes, v_type_tag) = encode_value(&entity_vec[3])?;
                let added = matches!(op, OpType::Add);

                // Detect schema-defining assertions and collect them
                if added {
                    collect_schema_assertion(e, a, &entity_vec[3], &mut schema_builders);
                }

                pending_datoms.push(PendingDatom {
                    e,
                    a,
                    v_bytes,
                    v_type_tag,
                    added,
                });
            }
            edn::Value::Map(ref map) => {
                let e = if let Some(id_val) =
                    map.get(&edn::Value::Keyword(edn::symbols::Keyword::plain("db/id")))
                {
                    resolve_entity_place(id_val, &mut tempid_map)?
                } else {
                    Spi::get_one::<i64>("SELECT mentat.allocate_entid('db.part/user')")
                        .ok()
                        .flatten()
                        .ok_or("Failed to allocate entity ID")?
                };

                for (attr_key, attr_value) in map {
                    if let edn::Value::Keyword(kw) = attr_key {
                        if kw.name() == "db/id" {
                            continue;
                        }
                    }

                    let a = resolve_attribute(attr_key)?;
                    let (v_bytes, v_type_tag) = encode_value(attr_value)?;

                    collect_schema_assertion(e, a, attr_value, &mut schema_builders);

                    pending_datoms.push(PendingDatom {
                        e,
                        a,
                        v_bytes,
                        v_type_tag,
                        added: true,
                    });
                }
            }
            _ => {}
        }
    }

    // --- Between passes: Install new schema attributes ---
    install_schema_attributes(&schema_builders)?;

    // --- Pass 2: Insert all datoms ---
    let datom_count = pending_datoms.len();
    for datom in &pending_datoms {
        Spi::run_with_args(
            "INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                DatumWithOid::from(datom.e),
                DatumWithOid::from(datom.a),
                DatumWithOid::from(datom.v_bytes.clone()),
                DatumWithOid::from(tx_id),
                DatumWithOid::from(datom.added),
                DatumWithOid::from(datom.v_type_tag),
            ],
        )?;
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

/// Collect schema-defining assertions for an entity.
///
/// When an assertion targets a built-in schema attribute (:db/ident, :db/valueType, etc.),
/// record the value in the SchemaBuilder for that entity so we can install the attribute
/// definition before inserting datoms.
fn collect_schema_assertion(
    entity_id: i64,
    attr_entid: i64,
    value: &edn::Value,
    builders: &mut BTreeMap<i64, SchemaBuilder>,
) {
    match attr_entid {
        bootstrap_entids::DB_IDENT => {
            if let edn::Value::Keyword(kw) = value {
                let ident_str = format!("{}", kw);
                builders.entry(entity_id).or_default().ident = Some(ident_str);
            }
        }
        bootstrap_entids::DB_VALUE_TYPE => {
            if let edn::Value::Keyword(kw) = value {
                if let Some(vt) = keyword_to_value_type(kw) {
                    builders.entry(entity_id).or_default().value_type = Some(vt.to_string());
                }
            }
        }
        bootstrap_entids::DB_CARDINALITY => {
            if let edn::Value::Keyword(kw) = value {
                if let Some(ct) = keyword_to_cardinality(kw) {
                    builders.entry(entity_id).or_default().cardinality = Some(ct.to_string());
                }
            }
        }
        bootstrap_entids::DB_UNIQUE => {
            if let edn::Value::Keyword(kw) = value {
                if let Some(ut) = keyword_to_unique(kw) {
                    builders.entry(entity_id).or_default().unique = Some(ut.to_string());
                }
            }
        }
        bootstrap_entids::DB_INDEX => {
            if let edn::Value::Boolean(b) = value {
                builders.entry(entity_id).or_default().indexed = Some(*b);
            }
        }
        bootstrap_entids::DB_FULLTEXT => {
            if let edn::Value::Boolean(b) = value {
                builders.entry(entity_id).or_default().fulltext = Some(*b);
            }
        }
        bootstrap_entids::DB_COMPONENT | bootstrap_entids::DB_IS_COMPONENT => {
            if let edn::Value::Boolean(b) = value {
                builders.entry(entity_id).or_default().component = Some(*b);
            }
        }
        bootstrap_entids::DB_NO_HISTORY => {
            if let edn::Value::Boolean(b) = value {
                builders.entry(entity_id).or_default().no_history = Some(*b);
            }
        }
        _ => {}
    }
}

/// Install new schema attributes into mentat.schema and mentat.idents.
///
/// For each entity that has at least :db/ident and :db/valueType, insert a row
/// into mentat.schema and mentat.idents. This must happen before datoms are
/// inserted so that foreign key constraints on datoms.a are satisfied.
fn install_schema_attributes(
    builders: &BTreeMap<i64, SchemaBuilder>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for (&entid, builder) in builders {
        let ident = match &builder.ident {
            Some(i) => i.clone(),
            None => continue, // No ident => not a complete attribute definition
        };

        let value_type = match &builder.value_type {
            Some(vt) => vt.clone(),
            None => continue, // No value type => not a complete attribute definition
        };

        let cardinality = builder.cardinality.as_deref().unwrap_or("one").to_string();
        let indexed = builder.indexed.unwrap_or(false);
        let fulltext = builder.fulltext.unwrap_or(false);
        let component = builder.component.unwrap_or(false);
        let no_history = builder.no_history.unwrap_or(false);

        // Insert into mentat.idents (keyword -> entid mapping)
        Spi::run_with_args(
            "INSERT INTO mentat.idents (ident, entid) VALUES ($1, $2) \
             ON CONFLICT (ident) DO UPDATE SET entid = EXCLUDED.entid",
            &[
                DatumWithOid::from(ident.as_str()),
                DatumWithOid::from(entid),
            ],
        )?;

        // Build the unique_constraint DatumWithOid: either a text value or NULL.
        let unique_datum = match &builder.unique {
            Some(u) => DatumWithOid::from(u.as_str()),
            None => DatumWithOid::null::<String>(),
        };

        // Insert into mentat.schema with all attribute properties.
        // Cast text parameters to the correct PostgreSQL enum types.
        Spi::run_with_args(
            "INSERT INTO mentat.schema \
                (entid, ident, value_type, cardinality, unique_constraint, \
                 indexed, fulltext, component, no_history) \
             VALUES ($1, $2, $3::mentat.value_type, $4::mentat.cardinality_type, \
                     $5::mentat.unique_type, $6, $7, $8, $9) \
             ON CONFLICT (entid) DO UPDATE SET \
                ident = EXCLUDED.ident, \
                value_type = EXCLUDED.value_type, \
                cardinality = EXCLUDED.cardinality, \
                unique_constraint = EXCLUDED.unique_constraint, \
                indexed = EXCLUDED.indexed, \
                fulltext = EXCLUDED.fulltext, \
                component = EXCLUDED.component, \
                no_history = EXCLUDED.no_history",
            &[
                DatumWithOid::from(entid),
                DatumWithOid::from(ident.as_str()),
                DatumWithOid::from(value_type.as_str()),
                DatumWithOid::from(cardinality.as_str()),
                unique_datum,
                DatumWithOid::from(indexed),
                DatumWithOid::from(fulltext),
                DatumWithOid::from(component),
                DatumWithOid::from(no_history),
            ],
        )?;
    }

    Ok(())
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
            // Resolve keyword ident using Display format (:namespace/name)
            let ident_str = format!("{}", kw);
            let entid = Spi::get_one_with_args::<i64>(
                "SELECT mentat.resolve_ident($1)",
                &[DatumWithOid::from(ident_str.as_str())],
            )
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
            // Use Display format (:namespace/name) to match schema ident storage
            let ident_str = format!("{}", kw);
            let entid = Spi::get_one_with_args::<i64>(
                "SELECT mentat.resolve_ident($1)",
                &[DatumWithOid::from(ident_str.as_str())],
            )
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
            // Store keyword without leading colon, using slash separator
            // e.g., :person/name -> "person/name"
            let display = format!("{}", kw); // produces ":person/name"
            let s = if display.starts_with(':') {
                &display[1..]
            } else {
                &display
            };
            Ok((s.as_bytes().to_vec(), 8)) // keyword = 8
        }
        _ => Err("Unsupported value type".into()),
    }
}
