use edn::entities::OpType;
use edn::parse;
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use std::collections::BTreeMap;

/// Entids for built-in schema attributes (from bootstrap data in lib.rs bootstrap_schema()).
mod bootstrap_entids {
    pub const DB_IDENT: i64 = 1;
    pub const DB_VALUE_TYPE: i64 = 2;
    pub const DB_CARDINALITY: i64 = 3;
    pub const DB_UNIQUE: i64 = 4;
    #[allow(dead_code)]
    pub const DB_DOC: i64 = 5;
    pub const DB_IS_COMPONENT: i64 = 6;
    pub const DB_FULLTEXT: i64 = 7;
    pub const DB_INDEX: i64 = 8;
    pub const DB_NO_HISTORY: i64 = 9;
    pub const DB_TX_INSTANT: i64 = 10;
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

/// Helper to describe an EDN value type for error messages.
fn value_type_name(value: &edn::Value) -> &'static str {
    match value {
        edn::Value::Nil => "nil",
        edn::Value::Boolean(_) => "boolean",
        edn::Value::Integer(_) => "integer",
        edn::Value::Float(_) => "float",
        edn::Value::Text(_) => "text/string",
        edn::Value::Keyword(_) => "keyword",
        edn::Value::PlainSymbol(_) => "symbol",
        edn::Value::NamespacedSymbol(_) => "namespaced symbol",
        edn::Value::Vector(_) => "vector",
        edn::Value::List(_) => "list",
        edn::Value::Set(_) => "set",
        edn::Value::Map(_) => "map",
        _ => "unknown",
    }
}

/// Retrieve available attribute idents from the cache for use in error messages.
fn get_available_attributes_hint() -> String {
    let cache = crate::cache::get_cache();
    // We can't directly iterate the cache, so do a quick DB query
    let result: Result<Vec<String>, _> = pgrx::spi::Spi::connect(|client| {
        let mut idents = Vec::new();
        let rows = client.select(
            "SELECT ident FROM mentat.schema ORDER BY ident LIMIT 20",
            None,
            &[],
        )?;
        for row in rows {
            if let Ok(Some(ident)) = row.get::<String>(1) {
                idents.push(ident);
            }
        }
        Ok::<_, pgrx::spi::SpiError>(idents)
    });
    match result {
        Ok(idents) if !idents.is_empty() => {
            let shown: Vec<&str> = idents.iter().map(|s| s.as_str()).collect();
            if idents.len() >= 20 {
                format!("Available attributes (first 20): {}", shown.join(", "))
            } else {
                format!("Available attributes: {}", shown.join(", "))
            }
        }
        _ => "No schema attributes found. Did you forget to define schema with mentat_transact?".to_string(),
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
///
/// Uses a three-pass approach to handle transactions that both define schema
/// attributes and reference them in the same transaction:
///   Pass 1: Scan for schema definitions, allocate tempids, build pending ident map
///   Install: Write new schema to mentat.schema and mentat.idents
///   Pass 2: Parse all assertions using the now-resolvable idents, insert datoms
///
/// Transaction isolation: The entire transaction body executes inside a single
/// SPI connection block. SPI manages the subtransaction boundary so that all
/// reads and writes see a consistent snapshot. If any error occurs, SPI
/// automatically rolls back the subtransaction, preventing partial schema or
/// datom writes from persisting.
#[pg_extern]
pub fn mentat_transact(edn_tx: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    execute_transaction_body(edn_tx)
}

/// Internal function containing the actual transaction logic.
///
/// Runs within the caller's PostgreSQL transaction. Uses savepoints to ensure
/// that schema installation and datom insertion are atomic: if Pass 2 (datom
/// insertion) fails after schema was written in Pass 1, the savepoint rollback
/// undoes the schema changes too.
fn execute_transaction_body(
    edn_tx: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Parse EDN transaction
    let value_and_span = parse::value(edn_tx)?;
    let value = value_and_span.without_spans();

    // Validate it's a vector
    let entities = match value {
        edn::Value::Vector(ref vec) => vec,
        _ => return Err(format!(
            ":db.error/invalid-transaction Transaction must be a vector of entities, \
             got {}. Expected EDN like: [[:db/add \"tempid\" :attr \"value\"]]",
            value_type_name(&value)
        ).into()),
    };

    // Allocate transaction ID
    let tx_id = Spi::get_one::<i64>("SELECT mentat.allocate_entid('db.part/tx')")
        .ok()
        .flatten()
        .ok_or(":db.error/allocation-failed Failed to allocate transaction ID. \
                Check that the 'db.part/tx' partition exists and has available IDs.")?;

    // Create transaction record and get the timestamp as microseconds since epoch
    let tx_instant_micros = Spi::get_one_with_args::<i64>(
        "INSERT INTO mentat.transactions (tx, tx_instant) VALUES ($1, CURRENT_TIMESTAMP) \
         RETURNING (EXTRACT(EPOCH FROM tx_instant) * 1000000)::BIGINT",
        &[DatumWithOid::from(tx_id)],
    )
    .ok()
    .flatten()
    .ok_or(":db.error/tx-creation-failed Failed to create transaction record. \
             The mentat.transactions table may be missing or the insert failed.")?;

    // Insert :db/txInstant datom for this transaction
    let instant_bytes = tx_instant_micros.to_le_bytes().to_vec();
    Spi::run_with_args(
        "INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag) \
         VALUES ($1, $2, $3, $4, $5, $6)",
        &[
            DatumWithOid::from(tx_id),
            DatumWithOid::from(bootstrap_entids::DB_TX_INSTANT),
            DatumWithOid::from(instant_bytes),
            DatumWithOid::from(tx_id),
            DatumWithOid::from(true),
            DatumWithOid::from(4_i16), // type_tag::INSTANT = 4
        ],
    )?;

    // ========================================================================
    // Three-pass transaction processing:
    //   Pass 1: Scan for schema definitions, allocate tempids for schema entities
    //   Install: Write new attributes to mentat.schema and mentat.idents
    //   Pass 2: Parse ALL assertions (idents now resolvable), insert datoms
    // ========================================================================

    let mut tempid_map: BTreeMap<String, i64> = BTreeMap::new();
    let mut schema_builders: BTreeMap<i64, SchemaBuilder> = BTreeMap::new();

    // --- Pass 1: Scan for schema definitions ---
    // Only process :db/ident, :db/valueType, :db/cardinality, etc. assertions.
    // Allocate tempids encountered so they're stable across passes.
    for entity_value in entities {
        match entity_value {
            edn::Value::Vector(ref entity_vec) if entity_vec.len() >= 4 => {
                // Only process :db/add
                match &entity_vec[0] {
                    edn::Value::Keyword(kw) if kw.name() == "add" => {}
                    _ => continue,
                };

                // Allocate/resolve the entity tempid so it's stable
                let e = resolve_entity_place(&entity_vec[1], &mut tempid_map)?;

                // Try to resolve the attribute -- but only if it's a known
                // bootstrap schema attribute. We use try_resolve here because
                // user-defined attributes won't be in the DB yet.
                let a = match try_resolve_attribute(&entity_vec[2]) {
                    Some(a) => a,
                    None => continue, // Not a bootstrap attr, skip in schema scan
                };

                collect_schema_assertion(e, a, &entity_vec[3], &mut schema_builders);
            }
            edn::Value::Map(ref map) => {
                // Resolve entity for stable tempid allocation
                let e = if let Some(id_val) =
                    map.get(&edn::Value::Keyword(edn::symbols::Keyword::plain("db/id")))
                {
                    resolve_entity_place(id_val, &mut tempid_map)?
                } else {
                    Spi::get_one::<i64>("SELECT mentat.allocate_entid('db.part/user')")
                        .ok()
                        .flatten()
                        .ok_or(":db.error/allocation-failed Failed to allocate entity ID. \
                         Check that the 'db.part/user' partition exists and has available IDs.")?
                };

                for (attr_key, attr_value) in map {
                    if let edn::Value::Keyword(kw) = attr_key {
                        if kw.name() == "db/id" {
                            continue;
                        }
                    }
                    let a = match try_resolve_attribute(attr_key) {
                        Some(a) => a,
                        None => continue,
                    };
                    collect_schema_assertion(e, a, attr_value, &mut schema_builders);
                }
            }
            _ => {}
        }
    }

    // --- Install new schema attributes ---
    // This writes to mentat.idents and mentat.schema so that resolve_attribute()
    // will succeed for newly-defined attributes in Pass 2.
    //
    // Atomic schema installation: wrap schema install + datom insertion in a
    // savepoint. If datom insertion (Pass 2) fails, the savepoint rollback
    // undoes the schema writes too, preventing stale schema from persisting
    // without corresponding data.
    let has_schema_changes = !schema_builders.is_empty();
    if has_schema_changes {
        Spi::run("SAVEPOINT schema_install")?;
    }

    install_schema_attributes(&schema_builders)?;

    // --- Pass 2: Parse ALL assertions and insert datoms ---
    // Now all idents (both bootstrap and newly-defined) are resolvable.
    let mut pending_datoms: Vec<PendingDatom> = Vec::new();

    for entity_value in entities {
        match entity_value {
            // Handle :db/retractEntity - format: [:db/retractEntity entity-id]
            edn::Value::Vector(ref entity_vec)
                if entity_vec.len() == 2
                    && matches!(&entity_vec[0], edn::Value::Keyword(kw) if kw.name() == "retractEntity") =>
            {
                let e = resolve_entity_place(&entity_vec[1], &mut tempid_map)?;

                // Query all current datoms for this entity
                Spi::connect(|client| {
                    let rows = client.select(
                        "SELECT a, v, value_type_tag FROM mentat.datoms \
                         WHERE e = $1 AND added = true",
                        None,
                        &[DatumWithOid::from(e)],
                    )?;

                    for row in rows {
                        let a: i64 = row.get(1)?.ok_or("Missing attribute")?;
                        let v_bytes: Vec<u8> = row.get(2)?.ok_or("Missing value")?;
                        let v_type_tag: i16 = row.get(3)?.ok_or("Missing type tag")?;

                        pending_datoms.push(PendingDatom {
                            e,
                            a,
                            v_bytes,
                            v_type_tag,
                            added: false,
                        });
                    }

                    Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                })?;
            }
            // Handle :db.fn/cas - format: [:db.fn/cas e a old-value new-value]
            edn::Value::Vector(ref entity_vec)
                if entity_vec.len() == 5
                    && matches!(&entity_vec[0], edn::Value::Keyword(kw) if kw.name() == "cas" && kw.namespace() == Some("db.fn")) =>
            {
                let e = resolve_entity_place(&entity_vec[1], &mut tempid_map)?;
                let a = resolve_attribute(&entity_vec[2])?;
                let old_edn = &entity_vec[3];
                let new_edn = &entity_vec[4];

                let is_ref = lookup_value_type(a).as_deref() == Some("ref");

                // Get current value(s) for this (e, a) pair
                let current_values: Vec<(Vec<u8>, i16)> = Spi::connect(|client| {
                    let rows = client.select(
                        "SELECT v, value_type_tag FROM mentat.datoms \
                         WHERE e = $1 AND a = $2 AND added = true",
                        None,
                        &[DatumWithOid::from(e), DatumWithOid::from(a)],
                    )?;

                    let mut vals = Vec::new();
                    for row in rows {
                        let v_bytes: Vec<u8> = row.get(1)?.ok_or("Missing value")?;
                        let v_type_tag: i16 = row.get(2)?.ok_or("Missing type tag")?;
                        vals.push((v_bytes, v_type_tag));
                    }
                    Ok::<_, Box<dyn std::error::Error + Send + Sync>>(vals)
                })?;

                // Check cardinality -- CAS on cardinality-many with multiple values is an error
                if let Some(attr_info) = lookup_attribute_info(a) {
                    if attr_info.cardinality == "many" && current_values.len() > 1 {
                        return Err(format!(
                            ":db.fn/cas failed on entity {} attribute {}: \
                             cardinality-many attribute has {} values; \
                             CAS requires at most one existing value",
                            e, a, current_values.len()
                        ).into());
                    }
                }

                let old_is_nil = matches!(old_edn, edn::Value::Nil);

                // Encode old value for comparison (unless nil)
                let old_encoded: Option<(Vec<u8>, i16)> = if old_is_nil {
                    None
                } else if is_ref {
                    Some(encode_ref_value(old_edn, &mut tempid_map)?)
                } else {
                    Some(encode_value(old_edn)?)
                };

                // Compare current database state with expected old value
                let cas_matches = if old_is_nil {
                    // old-value is nil: expect no current value
                    current_values.is_empty()
                } else if let Some((ref old_bytes, old_tag)) = old_encoded {
                    // old-value is not nil: expect exactly one matching value
                    current_values.len() == 1
                        && current_values[0].0 == *old_bytes
                        && current_values[0].1 == old_tag
                } else {
                    false
                };

                if !cas_matches {
                    // Build a human-readable description of the current value
                    let current_desc = if current_values.is_empty() {
                        "nil".to_string()
                    } else {
                        current_values
                            .iter()
                            .map(|(v, tag)| format_stored_value(v, *tag))
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    return Err(format!(
                        ":db.fn/cas failed on entity {} attribute {}: expected {:?}, found {}",
                        e, a, old_edn, current_desc
                    ).into());
                }

                // CAS matched -- retract old value (if not nil) and assert new value
                if !old_is_nil {
                    if let Some((old_bytes, old_tag)) = old_encoded {
                        pending_datoms.push(PendingDatom {
                            e,
                            a,
                            v_bytes: old_bytes,
                            v_type_tag: old_tag,
                            added: false,
                        });
                    }
                }

                let (new_bytes, new_tag) = if is_ref {
                    encode_ref_value(new_edn, &mut tempid_map)?
                } else {
                    encode_value(new_edn)?
                };
                pending_datoms.push(PendingDatom {
                    e,
                    a,
                    v_bytes: new_bytes,
                    v_type_tag: new_tag,
                    added: true,
                });
            }
            // Handle :db/add and :db/retract - format: [:db/add e a v] or [:db/retract e a v]
            edn::Value::Vector(ref entity_vec) if entity_vec.len() >= 4 => {
                let op = match &entity_vec[0] {
                    edn::Value::Keyword(kw) if kw.name() == "add" => OpType::Add,
                    edn::Value::Keyword(kw) if kw.name() == "retract" => OpType::Retract,
                    _ => continue,
                };

                let e = resolve_entity_place(&entity_vec[1], &mut tempid_map)?;
                let a = resolve_attribute(&entity_vec[2])?;
                // Check if attribute is ref-type; if so, resolve value as entity reference
                let (v_bytes, v_type_tag) = if lookup_value_type(a).as_deref() == Some("ref") {
                    encode_ref_value(&entity_vec[3], &mut tempid_map)?
                } else {
                    encode_value(&entity_vec[3])?
                };
                let added = matches!(op, OpType::Add);

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
                        .ok_or(":db.error/allocation-failed Failed to allocate entity ID. \
                         Check that the 'db.part/user' partition exists and has available IDs.")?
                };

                for (attr_key, attr_value) in map {
                    if let edn::Value::Keyword(kw) = attr_key {
                        if kw.name() == "db/id" {
                            continue;
                        }
                    }

                    let a = resolve_attribute(attr_key)?;
                    // Check if attribute is ref-type; if so, resolve value as entity reference
                    let (v_bytes, v_type_tag) = if lookup_value_type(a).as_deref() == Some("ref") {
                        encode_ref_value(attr_value, &mut tempid_map)?
                    } else {
                        encode_value(attr_value)?
                    };

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

    // Validate and insert all datoms.
    // If schema was installed (savepoint active), catch errors to rollback
    // the savepoint before propagating.
    let datom_result = insert_datoms(&pending_datoms, tx_id);

    match datom_result {
        Ok(datom_count) => {
            // All datoms inserted successfully -- release the savepoint to
            // commit schema + datoms atomically.
            if has_schema_changes {
                Spi::run("RELEASE SAVEPOINT schema_install")?;
            }

            // Build TxReport response
            let tempids_json: Vec<String> = tempid_map
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", k, v))
                .collect();

            Ok(format!(
                "{{\"tx-id\":{},\"tx-instant\":{},\"tempids\":{{{}}},\"datoms-inserted\":{}}}",
                tx_id,
                tx_instant_micros,
                tempids_json.join(","),
                datom_count
            ))
        }
        Err(e) => {
            // Datom insertion failed -- rollback the savepoint so schema
            // changes are undone too, then invalidate the cache since we
            // may have populated it during install_schema_attributes.
            if has_schema_changes {
                let _ = Spi::run("ROLLBACK TO SAVEPOINT schema_install");
                crate::cache::get_cache().invalidate();
                crate::functions::query::clear_stmt_cache();
            }
            Err(e)
        }
    }
}

/// Insert all pending datoms, validating constraints and handling cardinality
/// semantics. Returns the number of datoms processed.
fn insert_datoms(
    pending_datoms: &[PendingDatom],
    tx_id: i64,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let datom_count = pending_datoms.len();
    for datom in pending_datoms {
        // Only validate assertions (added=true), not retractions
        if datom.added {
            validate_datom_constraints(datom, pending_datoms)?;

            // For cardinality-one attributes, automatically retract any existing
            // value before asserting the new one (Datomic upsert semantics).
            // For cardinality-many attributes, allow multiple values - no retraction,
            // but skip if the exact (e, a, v) triple already exists (idempotent).
            if let Some(attr_info) = lookup_attribute_info(datom.a) {
                match attr_info.cardinality.as_str() {
                    "one" => {
                        retract_existing_cardinality_one(
                            datom.e,
                            datom.a,
                            tx_id,
                            &datom.v_bytes,
                            datom.v_type_tag,
                        )?;
                    }
                    "many" => {
                        // For cardinality-many, check if this exact value already
                        // exists. If so, skip inserting a duplicate (idempotent
                        // assertion, matching Datomic semantics).
                        if is_duplicate_cardinality_many(
                            datom.e,
                            datom.a,
                            &datom.v_bytes,
                            datom.v_type_tag,
                        )? {
                            continue;
                        }
                    }
                    _ => {
                        return Err(format!(
                            ":db.error/invalid-cardinality Unknown cardinality '{}' for attribute entid {}. \
                             Valid cardinalities are 'one' and 'many'. This may indicate schema corruption.",
                            attr_info.cardinality, datom.a
                        ).into());
                    }
                }
            }
        }

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

        // Populate mentat.fulltext for fulltext-enabled string attributes.
        // The trigger on mentat.fulltext auto-updates the search_vector column.
        if datom.added && datom.v_type_tag == 7 && is_fulltext_attribute(datom.a) {
            if let Ok(text_value) = String::from_utf8(datom.v_bytes.clone()) {
                Spi::run_with_args(
                    "INSERT INTO mentat.fulltext (text_value) VALUES ($1)",
                    &[DatumWithOid::from(text_value)],
                )?;
            }
        }
    }

    Ok(datom_count)
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
        bootstrap_entids::DB_IS_COMPONENT => {
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

    // Invalidate schema cache after schema changes
    crate::cache::get_cache().invalidate();

    // Invalidate prepared statement cache since schema changes may
    // alter query plans (e.g., new attributes change subquery results).
    crate::functions::query::clear_stmt_cache();

    Ok(())
}

/// Resolve entity place (entid, tempid, ident, or lookup ref)
///
/// Supports:
///   - Integer: direct entity ID
///   - Text: tempid string (allocate or reuse)
///   - Keyword: resolve ident to entity ID
///   - Vector (2 elements): lookup ref like `[:person/email "alice@example.com"]`
///     The attribute must have a unique constraint (:db.unique/identity or :db.unique/value).
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
                    .ok_or(":db.error/allocation-failed Failed to allocate entity ID. \
                         Check that the 'db.part/user' partition exists and has available IDs.")?;
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
            .ok_or_else(|| format!(
                ":db.error/ident-not-found Entity ident '{}' not found in mentat.idents. \
                 Ensure this ident was previously defined via mentat_transact with :db/ident.",
                ident_str
            ))?;
            Ok(entid)
        }
        edn::Value::Vector(ref vec) if vec.len() == 2 => {
            // Lookup ref: [:attribute value]
            // Example: [:person/email "alice@example.com"]
            match &vec[0] {
                edn::Value::Keyword(_) => {}
                other => return Err(format!(
                    ":db.error/invalid-lookup-ref Lookup ref first element must be a keyword attribute, \
                     got {}. Expected format: [:attribute-keyword value]",
                    value_type_name(other)
                ).into()),
            }

            let a = resolve_attribute(&vec[0])?;

            // Validate the attribute has a unique constraint
            let attr_ident_display = crate::cache::get_cache()
                .get_ident(a)
                .unwrap_or_else(|| format!("entid:{}", a));
            let attr_info = lookup_attribute_info(a)
                .ok_or_else(|| format!(
                    ":db.error/attribute-not-found Lookup ref attribute '{}' (entid {}) not found in schema. \
                     {}",
                    attr_ident_display, a, get_available_attributes_hint()
                ))?;
            if attr_info.unique_constraint.is_none() {
                return Err(format!(
                    ":db.error/lookup-ref-requires-unique Lookup ref attribute '{}' does not have \
                     a unique constraint. Only attributes with :db.unique/identity or :db.unique/value \
                     can be used in lookup refs. Add a unique constraint to the attribute definition, e.g.:\n  \
                     [:db/add \"attr\" :db/unique :db.unique/identity]",
                    attr_ident_display
                ).into());
            }

            let (v_bytes, v_type_tag) = encode_value(&vec[1])?;

            // Query for entity with this unique attribute value
            let eid = Spi::get_one_with_args::<i64>(
                "SELECT e FROM mentat.datoms \
                 WHERE a = $1 AND v = $2 AND value_type_tag = $3 AND added = true \
                 LIMIT 1",
                &[
                    DatumWithOid::from(a),
                    DatumWithOid::from(v_bytes),
                    DatumWithOid::from(v_type_tag),
                ],
            )
            .ok()
            .flatten()
            .ok_or_else(|| {
                let attr_ident_display = crate::cache::get_cache()
                    .get_ident(a)
                    .unwrap_or_else(|| format!("entid:{}", a));
                format!(
                    ":db.error/lookup-ref-not-found Lookup ref did not match any existing entity \
                     for attribute '{}' with the given value. \
                     Ensure an entity with this attribute value has been transacted.",
                    attr_ident_display
                )
            })?;

            Ok(eid)
        }
        other => Err(format!(
            ":db.error/invalid-entity-place Invalid entity place: got {} (value: {}). \
             Entity position must be an integer (entity ID), string (tempid), \
             keyword (ident), or 2-element vector (lookup ref like [:attr value]).",
            value_type_name(other), other
        ).into()),
    }
}

/// Resolve attribute (entid or ident) using cache. Errors if the ident is not found.
fn resolve_attribute(value: &edn::Value) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    match value {
        edn::Value::Integer(i) => Ok(*i),
        edn::Value::Keyword(kw) => {
            // Use Display format (:namespace/name) to match schema ident storage
            let ident_str = format!("{}", kw);
            crate::cache::get_cache()
                .resolve_ident(&ident_str)
                .ok_or_else(|| {
                    let hint = get_available_attributes_hint();
                    format!(
                        ":db.error/attribute-not-found Attribute '{}' not found in schema. {}",
                        ident_str, hint
                    ).into()
                })
        }
        other => Err(format!(
            ":db.error/invalid-attribute Invalid attribute: got {} (value: {}). \
             Attribute position must be an integer (entid) or keyword (e.g. :person/name).",
            value_type_name(other), other
        ).into()),
    }
}

/// Try to resolve an attribute, returning None if not found.
/// Used during the schema-scanning pass where user-defined attributes
/// may not yet exist in the database.
fn try_resolve_attribute(value: &edn::Value) -> Option<i64> {
    match value {
        edn::Value::Integer(i) => Some(*i),
        edn::Value::Keyword(kw) => {
            let ident_str = format!("{}", kw);
            crate::cache::get_cache().resolve_ident(&ident_str)
        }
        _ => None,
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
        other => Err(format!(
            ":db.error/unsupported-value-type Cannot encode value of type {} (value: {}). \
             Supported types: boolean, integer (long), string, keyword. \
             For ref values, use an entity ID, tempid string, or keyword ident.",
            value_type_name(other), other
        ).into()),
    }
}

/// Encode a value for a ref-type attribute. The value should be a tempid (string),
/// integer entity ID, or keyword ident. Returns (bytes, type_tag=0) where bytes
/// is the entity ID encoded as little-endian i64.
fn encode_ref_value(
    value: &edn::Value,
    tempid_map: &mut BTreeMap<String, i64>,
) -> Result<(Vec<u8>, i16), Box<dyn std::error::Error + Send + Sync>> {
    let entity_id = resolve_entity_place(value, tempid_map)?;
    Ok((entity_id.to_le_bytes().to_vec(), 0)) // ref = 0
}

/// Look up the value_type of an attribute (using cache).
/// Returns the value_type string (e.g., "string", "long", "ref") or None if not found.
fn lookup_value_type(attr_id: i64) -> Option<String> {
    crate::cache::get_cache()
        .get_attribute(attr_id)
        .map(|info| info.value_type)
}

/// Check if an attribute has fulltext=true (using cache).
fn is_fulltext_attribute(attr_id: i64) -> bool {
    crate::cache::get_cache()
        .get_attribute(attr_id)
        .map(|info| info.fulltext)
        .unwrap_or(false)
}

/// Look up attribute metadata from cache (or database if not cached)
fn lookup_attribute_info(attr_id: i64) -> Option<crate::cache::AttributeInfo> {
    crate::cache::get_cache().get_attribute(attr_id)
}

/// Validate all constraints for a datom before insertion
fn validate_datom_constraints(
    datom: &PendingDatom,
    all_pending: &[PendingDatom],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let attr_info = lookup_attribute_info(datom.a)
        .ok_or_else(|| {
            let ident_name = crate::cache::get_cache()
                .get_ident(datom.a)
                .unwrap_or_else(|| format!("entid:{}", datom.a));
            let hint = get_available_attributes_hint();
            format!(
                ":db.error/attribute-not-found Attribute {} (entid {}) not found in schema. \
                 {} \
                 If this is a new attribute, define it first with :db/ident, :db/valueType, \
                 and :db/cardinality in the same or prior transaction.",
                ident_name, datom.a, hint
            )
        })?;

    // 1. Type validation
    let expected_type_tag = value_type_to_tag(&attr_info.value_type);
    if datom.v_type_tag != expected_type_tag {
        let ident_name = crate::cache::get_cache()
            .get_ident(datom.a)
            .unwrap_or_else(|| format!("entid:{}", datom.a));
        let got_type_name = tag_to_value_type_name(datom.v_type_tag);
        return Err(format!(
            ":db.error/wrong-type-for-attribute Type mismatch for attribute '{}': \
             schema declares :db/valueType :db.type/{} (tag {}), but the asserted value \
             has type {} (tag {}). Ensure the value matches the attribute's declared type.",
            ident_name, attr_info.value_type, expected_type_tag,
            got_type_name, datom.v_type_tag
        )
        .into());
    }

    // 2. Cardinality validation
    match attr_info.cardinality.as_str() {
        "one" => {
            // For cardinality-one, check within this transaction for multiple assertions of same (e, a)
            let count_in_tx = all_pending
                .iter()
                .filter(|d| d.e == datom.e && d.a == datom.a && d.added)
                .count();

            if count_in_tx > 1 {
                let ident_name = crate::cache::get_cache()
                    .get_ident(datom.a)
                    .unwrap_or_else(|| format!("entid:{}", datom.a));
                return Err(format!(
                    ":db.error/cardinality-violation Attribute '{}' has :db/cardinality :db.cardinality/one \
                     but this transaction contains {} assertions for entity {}. \
                     Cardinality-one attributes can only have a single value per entity. \
                     Either remove duplicate assertions or change the attribute to :db.cardinality/many.",
                    ident_name, count_in_tx, datom.e
                )
                .into());
            }

            // Note: existing values for cardinality-one attributes are handled by
            // retract_existing_cardinality_one() during insertion, implementing
            // Datomic's upsert semantics (automatically retract old, assert new).
        }
        "many" => {
            // For cardinality-many, multiple values are allowed.
            // No validation needed - just add new datoms without retracting existing ones.
        }
        _ => {
            return Err(format!(
                ":db.error/invalid-cardinality Unknown cardinality '{}' for attribute entid {}. \
                 Valid cardinalities are 'one' and 'many'. This may indicate schema corruption.",
                attr_info.cardinality, datom.a
            )
            .into());
        }
    }

    // 3. Unique constraint validation
    if let Some(ref unique_type) = attr_info.unique_constraint {
        // Check within this transaction for duplicate values
        let dups_in_tx = all_pending
            .iter()
            .filter(|d| d.a == datom.a && d.v_bytes == datom.v_bytes && d.e != datom.e && d.added)
            .count();

        if dups_in_tx > 0 {
            let ident_name = crate::cache::get_cache()
                .get_ident(datom.a)
                .unwrap_or_else(|| format!("entid:{}", datom.a));
            return Err(format!(
                ":db.error/unique-conflict Unique constraint violation for attribute '{}' \
                 (unique type: :db.unique/{}). This transaction asserts the same value for \
                 {} different entities. Each value must belong to exactly one entity.",
                ident_name, unique_type, dups_in_tx + 1
            )
            .into());
        }

        // Check existing datoms in database (use advisory lock to prevent races)
        // Advisory lock key: hash of (attribute_id, value_bytes)
        let lock_key = (datom.a as i64) ^ (compute_value_hash(&datom.v_bytes) as i64);

        Spi::run_with_args(
            "SELECT pg_advisory_xact_lock($1)",
            &[DatumWithOid::from(lock_key)],
        )?;

        let existing_entity = Spi::get_one_with_args::<i64>(
            "SELECT e FROM mentat.datoms \
             WHERE a = $1 AND v = $2 AND value_type_tag = $3 AND added = true \
             LIMIT 1",
            &[
                DatumWithOid::from(datom.a),
                DatumWithOid::from(datom.v_bytes.clone()),
                DatumWithOid::from(datom.v_type_tag),
            ],
        )
        .ok()
        .flatten();

        if let Some(existing_e) = existing_entity {
            if existing_e != datom.e {
                let ident_name = crate::cache::get_cache()
                    .get_ident(datom.a)
                    .unwrap_or_else(|| format!("entid:{}", datom.a));
                return Err(format!(
                    ":db.error/unique-conflict Unique constraint violation for attribute '{}' \
                     (unique type: :db.unique/{}). The asserted value already exists on entity {} \
                     but is being asserted for entity {}. \
                     To reassign the value, first retract it from entity {}.",
                    ident_name, unique_type, existing_e, datom.e, existing_e
                )
                .into());
            }
        }
    }

    Ok(())
}

/// For cardinality-one attributes, retract any existing value for this (entity, attribute)
/// pair before asserting a new value. This implements Datomic's upsert semantics.
/// If the new value is identical to the existing value, no retraction is needed (idempotent).
fn retract_existing_cardinality_one(
    entity_id: i64,
    attr_id: i64,
    tx_id: i64,
    new_v_bytes: &[u8],
    new_v_type_tag: i16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Find the current value (if any)
    let existing = Spi::connect(|client| {
        let rows = client.select(
            "SELECT v, value_type_tag FROM mentat.datoms \
             WHERE e = $1 AND a = $2 AND added = true \
             ORDER BY tx DESC LIMIT 1",
            None,
            &[DatumWithOid::from(entity_id), DatumWithOid::from(attr_id)],
        )?;

        for row in rows {
            if let (Ok(Some(v_bytes)), Ok(Some(type_tag))) =
                (row.get::<Vec<u8>>(1), row.get::<i16>(2))
            {
                return Ok::<_, pgrx::spi::SpiError>(Some((v_bytes, type_tag)));
            }
        }
        Ok(None)
    })?;

    if let Some((old_v_bytes, old_type_tag)) = existing {
        // If the value is identical, no retraction needed (idempotent assertion)
        if old_v_bytes == new_v_bytes && old_type_tag == new_v_type_tag {
            return Ok(());
        }

        // Insert a retraction datom for the old value
        Spi::run_with_args(
            "INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                DatumWithOid::from(entity_id),
                DatumWithOid::from(attr_id),
                DatumWithOid::from(old_v_bytes),
                DatumWithOid::from(tx_id),
                DatumWithOid::from(false), // added = false (retraction)
                DatumWithOid::from(old_type_tag),
            ],
        )?;
    }

    Ok(())
}

/// For cardinality-many attributes, check if the exact (e, a, v) triple already
/// exists with added=true. If so, the assertion is idempotent and should be
/// skipped to avoid duplicate datoms (matching Datomic semantics).
fn is_duplicate_cardinality_many(
    entity_id: i64,
    attr_id: i64,
    v_bytes: &[u8],
    v_type_tag: i16,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let exists = Spi::get_one_with_args::<bool>(
        "SELECT EXISTS(SELECT 1 FROM mentat.datoms \
         WHERE e = $1 AND a = $2 AND v = $3 AND value_type_tag = $4 AND added = true)",
        &[
            DatumWithOid::from(entity_id),
            DatumWithOid::from(attr_id),
            DatumWithOid::from(v_bytes.to_vec()),
            DatumWithOid::from(v_type_tag),
        ],
    )
    .ok()
    .flatten()
    .unwrap_or(false);

    Ok(exists)
}

/// Map a type tag back to a human-readable value type name (for error messages).
fn tag_to_value_type_name(tag: i16) -> &'static str {
    match tag {
        0 => "ref",
        1 => "boolean",
        2 => "long",
        3 => "double",
        4 => "instant",
        7 => "string",
        8 => "keyword",
        10 => "uuid",
        11 => "bytes",
        _ => "unknown",
    }
}

/// Map value_type string to type tag (matches encoding in encode_value)
fn value_type_to_tag(value_type: &str) -> i16 {
    match value_type {
        "ref" => 0,
        "boolean" => 1,
        "long" => 2,
        "double" => 3,
        "instant" => 4,
        "string" => 7,
        "keyword" => 8,
        "uuid" => 10,
        "bytes" => 11,
        _ => -1,
    }
}

/// Compute a simple hash of value bytes for advisory lock
fn compute_value_hash(bytes: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Format a stored value (bytes + type tag) into a human-readable string for error messages.
fn format_stored_value(v_bytes: &[u8], type_tag: i16) -> String {
    match type_tag {
        0 => {
            // ref: i64 LE
            if v_bytes.len() == 8 {
                let id = i64::from_le_bytes(v_bytes.try_into().unwrap());
                format!("{}", id)
            } else {
                format!("<ref:{} bytes>", v_bytes.len())
            }
        }
        1 => {
            // boolean
            if v_bytes.first() == Some(&1) {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        2 => {
            // long: i64 LE
            if v_bytes.len() == 8 {
                let n = i64::from_le_bytes(v_bytes.try_into().unwrap());
                format!("{}", n)
            } else {
                format!("<long:{} bytes>", v_bytes.len())
            }
        }
        7 => {
            // string: UTF-8
            match std::str::from_utf8(v_bytes) {
                Ok(s) => format!("\"{}\"", s),
                Err(_) => format!("<string:{} bytes>", v_bytes.len()),
            }
        }
        8 => {
            // keyword: UTF-8
            match std::str::from_utf8(v_bytes) {
                Ok(s) => format!(":{}", s),
                Err(_) => format!("<keyword:{} bytes>", v_bytes.len()),
            }
        }
        _ => {
            format!("<{}:{} bytes>", tag_to_value_type_name(type_tag), v_bytes.len())
        }
    }
}
