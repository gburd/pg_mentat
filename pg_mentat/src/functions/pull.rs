use crate::error::MentatError;
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::spi::SpiClient;
use pgrx::JsonB;
use serde_json::json;
use std::collections::{HashMap, HashSet};

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

/// Default limit for cardinality-many results (Datomic default is 1000).
const DEFAULT_MANY_LIMIT: usize = 1000;

/// Maximum recursion depth to prevent runaway queries.
const MAX_RECURSION_DEPTH: usize = 100;

// ---------------------------------------------------------------------------
// Pull pattern AST
// ---------------------------------------------------------------------------

/// A single element in a pull pattern.
#[derive(Debug, Clone)]
enum PullAttrSpec {
    /// Simple keyword attribute, e.g. `:person/name`
    Attribute {
        ident: String,
        /// True if this is a reverse lookup (underscore prefix on name).
        reverse: bool,
        /// The forward ident used for schema lookups when `reverse` is true.
        forward_ident: String,
        rename: Option<String>,
        default: Option<serde_json::Value>,
        limit: Option<LimitSpec>,
    },
    /// Map specification for following refs, e.g. `{:person/friends [:person/name]}`
    MapSpec {
        ident: String,
        reverse: bool,
        forward_ident: String,
        sub_pattern: Vec<PullAttrSpec>,
        rename: Option<String>,
        limit: Option<LimitSpec>,
    },
    /// Recursive specification, e.g. `{:person/friends ...}` or `{:person/friends 6}`
    RecursiveSpec {
        ident: String,
        reverse: bool,
        forward_ident: String,
        depth: RecursionDepth,
        rename: Option<String>,
    },
    /// Wildcard `*` — pull all attributes.
    Wildcard,
}

#[derive(Debug, Clone)]
enum LimitSpec {
    /// A specific numeric limit.
    Count(usize),
    /// No limit (`:limit nil`).
    Unlimited,
}

#[derive(Debug, Clone)]
enum RecursionDepth {
    /// Fixed depth limit.
    Bounded(usize),
    /// Unlimited recursion (`...`).
    Unbounded,
}

type PullError = Box<dyn std::error::Error + Send + Sync>;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Pull entity data using a pull pattern.
///
/// Supports:
///   - Simple attributes: `[:person/name :person/age]`
///   - Wildcard: `[*]`
///   - Nested/map specs: `[{:person/friends [:person/name]}]`
///   - Reverse lookups: `[:person/_friends]`
///   - Recursion: `[{:person/friends ...}]` or `[{:person/friends 6}]`
///   - Limits: `[(:person/friends :limit 5)]`
///   - Defaults: `[(:person/email :default "none")]`
///   - Rename: `[(:person/name :as "Name")]`
///   - Wildcard with map overrides: `[* {:person/friends [:person/name]}]`
#[pg_extern]
pub fn mentat_pull(pattern: &str, entity_id: i64) -> Result<JsonB, PullError> {
    let parsed = edn::parse::value(pattern)
        .map_err(|e| -> PullError { MentatError::InvalidPullPattern {
            message: format!(
                "Failed to parse pull pattern as EDN: {e}. \
                 Expected a vector like [:person/name :person/age] or [*]."
            ),
        }.into() })?;
    let pattern_value = parsed.without_spans();

    let specs = match &pattern_value {
        edn::Value::Vector(items) => parse_pull_pattern(items)?,
        _ => return Err(MentatError::InvalidPullPattern {
            message: "Pull pattern must be a vector. \
                      Expected: [:person/name :person/age] or [*] or [{:person/friends [:person/name]}]. \
                      Got a non-vector EDN value.".to_string(),
        }.into()),
    };

    let mut result_map = serde_json::Map::new();
    result_map.insert(":db/id".to_string(), json!(entity_id));

    let mut visited = HashSet::new();
    visited.insert(entity_id);

    Spi::connect(|client| {
        execute_pull(&client, entity_id, &specs, &mut result_map, &mut visited, 0)
    })?;

    Ok(JsonB(serde_json::Value::Object(result_map)))
}

/// Pull entity data for multiple entities using a pull pattern.
///
/// This is the batched counterpart to `mentat_pull`. Instead of pulling one entity
/// at a time (N+1 queries), this function batches attribute lookups for all entities,
/// resulting in significantly fewer database round-trips.
///
/// Returns a JSONB array with one result per entity ID.
///
/// Example:
/// ```sql
/// SELECT mentat_pull_many('[:person/name :person/age]', ARRAY[100, 101, 102]);
/// ```
#[pg_extern]
pub fn mentat_pull_many(pattern: &str, entity_ids: Vec<i64>) -> Result<JsonB, PullError> {
    let parsed = edn::parse::value(pattern)
        .map_err(|e| -> PullError { MentatError::InvalidPullPattern {
            message: format!(
                "Failed to parse pull pattern as EDN: {e}. \
                 Expected a vector like [:person/name :person/age] or [*]."
            ),
        }.into() })?;
    let pattern_value = parsed.without_spans();

    let specs = match &pattern_value {
        edn::Value::Vector(items) => parse_pull_pattern(items)?,
        _ => return Err(MentatError::InvalidPullPattern {
            message: "Pull pattern must be a vector.".to_string(),
        }.into()),
    };

    let has_wildcard = specs.iter().any(|s| matches!(s, PullAttrSpec::Wildcard));
    let has_map_or_recursive = specs.iter().any(|s| matches!(
        s,
        PullAttrSpec::MapSpec { .. } | PullAttrSpec::RecursiveSpec { .. }
    ));

    // For simple patterns (no wildcard, no map specs, no recursion), use a
    // batched query that fetches all entities' attributes in one round-trip.
    if !has_wildcard && !has_map_or_recursive {
        return pull_many_batched(&specs, &entity_ids);
    }

    // For complex patterns, fall back to per-entity pull (still sharing one SPI connection).
    let mut results = Vec::with_capacity(entity_ids.len());

    Spi::connect(|client| {
        for &eid in &entity_ids {
            let mut result_map = serde_json::Map::new();
            result_map.insert(":db/id".to_string(), json!(eid));
            let mut visited = HashSet::new();
            visited.insert(eid);
            execute_pull(&client, eid, &specs, &mut result_map, &mut visited, 0)?;
            results.push(serde_json::Value::Object(result_map));
        }
        Ok::<(), PullError>(())
    })?;

    Ok(JsonB(serde_json::Value::Array(results)))
}

/// Batched pull for simple attribute-only patterns.
///
/// Fetches all requested attributes for all entities in a single SQL query
/// using `WHERE d.e IN (...)`, then groups the results client-side.
/// This eliminates the N+1 query problem when pulling many entities.
fn pull_many_batched(
    specs: &[PullAttrSpec],
    entity_ids: &[i64],
) -> Result<JsonB, PullError> {
    if entity_ids.is_empty() {
        return Ok(JsonB(json!([])));
    }

    // Collect the idents we need.
    let mut ident_list: Vec<String> = Vec::new();
    let mut spec_map: HashMap<String, &PullAttrSpec> = HashMap::new();
    for spec in specs {
        if let PullAttrSpec::Attribute { forward_ident, .. } = spec {
            ident_list.push(forward_ident.clone());
            spec_map.insert(forward_ident.clone(), spec);
        }
    }

    // Build per-entity result maps.
    let mut results_by_eid: HashMap<i64, serde_json::Map<String, serde_json::Value>> =
        HashMap::with_capacity(entity_ids.len());
    for &eid in entity_ids {
        let mut m = serde_json::Map::new();
        m.insert(":db/id".to_string(), json!(eid));
        results_by_eid.insert(eid, m);
    }

    // Build the IN clause from entity IDs (integers, safe from injection).
    let eid_csv: String = entity_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    // Build the IN clause for idents (escape single quotes for safety).
    let ident_csv: String = ident_list
        .iter()
        .map(|s| format!("'{}'", s.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");

    Spi::connect(|client| {
        let query = format!(
            "SELECT d.e, s.ident, s.cardinality::TEXT, d.value_type_tag, \
                    d.v_ref, d.v_bool, d.v_long, d.v_double, \
                    d.v_text, d.v_keyword, \
                    EXTRACT(EPOCH FROM d.v_instant)::BIGINT * 1000000 + \
                    EXTRACT(MICROSECOND FROM d.v_instant)::BIGINT % 1000000 AS v_instant_micros, \
                    d.v_uuid::TEXT, d.v_bytes \
             FROM mentat.datoms d \
             JOIN mentat.schema s ON d.a = s.entid \
             WHERE d.e IN ({eid_csv}) AND s.ident IN ({ident_csv}) AND d.added = true \
             ORDER BY d.e, s.ident"
        );

        for row in client.select(&query, None, &[])? {
            let eid: i64 = row.get(1)?.ok_or("Missing entity id")?;
            let ident: String = row.get(2)?.ok_or("Missing ident")?;
            let cardinality: String = row.get(3)?.ok_or("Missing cardinality")?;
            let v_type_tag: i16 = row.get(4)?.ok_or("Missing type tag")?;

            let (decoded_val, ref_id) = decode_row_typed_value(&row, v_type_tag, 5)?;

            // For ref attributes in simple pulls, return {:db/id N}.
            let decoded = if v_type_tag == type_tag::REF {
                let rid = ref_id.ok_or("Missing ref ID")?;
                json!({":db/id": rid})
            } else {
                decoded_val
            };

            // Determine the output key (handle :as renames).
            let output_key = if let Some(spec) = spec_map.get(&ident) {
                if let PullAttrSpec::Attribute { rename, forward_ident, .. } = spec {
                    rename.as_deref().unwrap_or(forward_ident).to_string()
                } else {
                    ident.clone()
                }
            } else {
                ident.clone()
            };

            if let Some(result_map) = results_by_eid.get_mut(&eid) {
                insert_value(result_map, &output_key, decoded, &cardinality);
            }
        }

        // Apply defaults for missing attributes.
        for spec in specs {
            if let PullAttrSpec::Attribute { forward_ident, rename, default: Some(def), .. } = spec {
                let output_key = rename.as_deref().unwrap_or(forward_ident);
                for result_map in results_by_eid.values_mut() {
                    if !result_map.contains_key(output_key) {
                        result_map.insert(output_key.to_string(), def.clone());
                    }
                }
            }
        }

        Ok::<(), PullError>(())
    })?;

    // Preserve input order.
    let results: Vec<serde_json::Value> = entity_ids
        .iter()
        .map(|eid| {
            results_by_eid
                .remove(eid)
                .map(serde_json::Value::Object)
                .unwrap_or_else(|| json!({":db/id": eid}))
        })
        .collect();

    Ok(JsonB(serde_json::Value::Array(results)))
}

// ---------------------------------------------------------------------------
// Pattern parsing
// ---------------------------------------------------------------------------

/// Parse a pull pattern vector into a list of PullAttrSpec.
fn parse_pull_pattern(items: &[edn::Value]) -> Result<Vec<PullAttrSpec>, PullError> {
    let mut specs = Vec::new();

    for item in items {
        match item {
            edn::Value::PlainSymbol(ref sym) if sym.name() == "*" => {
                specs.push(PullAttrSpec::Wildcard);
            }
            edn::Value::Keyword(kw) => {
                let (ident, reverse, forward_ident) = parse_keyword(kw);
                specs.push(PullAttrSpec::Attribute {
                    ident,
                    reverse,
                    forward_ident,
                    rename: None,
                    default: None,
                    limit: None,
                });
            }
            edn::Value::Map(map) => {
                // Map spec: {keyword pattern-or-recursion}
                for (k, v) in map {
                    let kw = match k {
                        edn::Value::Keyword(kw) => kw,
                        _ => return Err(format!(
                            ":db.error/invalid-pull-pattern Map spec keys must be keyword attributes, \
                             got: {k}. Expected format: {{:attribute [:sub-pattern]}}",
                        ).into()),
                    };
                    let (ident, reverse, forward_ident) = parse_keyword(kw);

                    match v {
                        edn::Value::Vector(sub_items) => {
                            let sub_pattern = parse_pull_pattern(sub_items)?;
                            specs.push(PullAttrSpec::MapSpec {
                                ident,
                                reverse,
                                forward_ident,
                                sub_pattern,
                                rename: None,
                                limit: None,
                            });
                        }
                        edn::Value::PlainSymbol(ref sym) if sym.name() == "..." => {
                            specs.push(PullAttrSpec::RecursiveSpec {
                                ident,
                                reverse,
                                forward_ident,
                                depth: RecursionDepth::Unbounded,
                                rename: None,
                            });
                        }
                        edn::Value::Integer(n) => {
                            let depth = if *n < 0 { 0 } else { *n as usize };
                            specs.push(PullAttrSpec::RecursiveSpec {
                                ident,
                                reverse,
                                forward_ident,
                                depth: RecursionDepth::Bounded(depth),
                                rename: None,
                            });
                        }
                        _ => {
                            return Err(format!(
                                ":db.error/invalid-pull-pattern Map spec value must be a sub-pattern vector, \
                                 '...' (unbounded recursion), or integer depth limit, got: {v}. \
                                 Examples: {{:attr [:sub-attr]}}, {{:attr ...}}, {{:attr 3}}"
                            )
                            .into());
                        }
                    }
                }
            }
            edn::Value::Vector(ref inner) if !inner.is_empty() => {
                // Attribute expression: (keyword :limit N :default V :as "name")
                // EDN parses (:person/name :limit 5) as a List, but [...] inside a vector
                // would be a nested vector. In Datomic, attribute expressions are actually
                // lists. Let's handle both forms.
                parse_attribute_expression(inner, &mut specs)?;
            }
            edn::Value::List(ref inner) if !inner.is_empty() => {
                let items_vec: Vec<edn::Value> = inner.iter().cloned().collect();
                parse_attribute_expression(&items_vec, &mut specs)?;
            }
            _ => {
                return Err(format!(
                    ":db.error/invalid-pull-pattern Unsupported pull pattern element: {item}. \
                     Valid elements: keyword attributes (:ns/name), wildcard (*), \
                     map specs ({{:attr [...]}}), or attribute expressions ((:attr :limit N))"
                ).into());
            }
        }
    }

    Ok(specs)
}

/// Parse an attribute expression like `(:person/name :limit 5 :default "none" :as "Name")`.
fn parse_attribute_expression(
    items: &[edn::Value],
    specs: &mut Vec<PullAttrSpec>,
) -> Result<(), PullError> {
    if items.is_empty() {
        return Err(":db.error/invalid-pull-pattern Empty attribute expression. \
                    Expected: (:attribute :limit N :default V :as \"name\")".into());
    }

    let kw = match &items[0] {
        edn::Value::Keyword(kw) => kw,
        _ => {
            return Err(format!(
                ":db.error/invalid-pull-pattern Attribute expression must start with a keyword \
                 attribute, got: {}. Expected: (:attr-keyword :limit N :default V :as \"name\")",
                items[0]
            )
            .into())
        }
    };
    let (ident, reverse, forward_ident) = parse_keyword(kw);

    let mut rename = None;
    let mut default = None;
    let mut limit = None;

    // Parse modifier pairs: :limit N, :default V, :as "name"
    let mut i = 1;
    while i < items.len() {
        match &items[i] {
            edn::Value::Keyword(mod_kw) => {
                let mod_name = mod_kw.name();
                match mod_name {
                    "limit" => {
                        i += 1;
                        if i >= items.len() {
                            return Err(":db.error/invalid-pull-pattern :limit modifier requires \
                                        a value (integer or nil). Example: (:attr :limit 10)".into());
                        }
                        limit = Some(parse_limit_value(&items[i])?);
                    }
                    "default" => {
                        i += 1;
                        if i >= items.len() {
                            return Err(":db.error/invalid-pull-pattern :default modifier requires \
                                        a value. Example: (:attr :default \"none\")".into());
                        }
                        default = Some(edn_to_json(&items[i]));
                    }
                    "as" => {
                        i += 1;
                        if i >= items.len() {
                            return Err(":db.error/invalid-pull-pattern :as modifier requires a \
                                        string value. Example: (:attr :as \"Display Name\")".into());
                        }
                        match &items[i] {
                            edn::Value::Text(ref s) => {
                                rename = Some(s.to_string());
                            }
                            _ => return Err(":db.error/invalid-pull-pattern :as value must be \
                                            a string. Example: (:attr :as \"Display Name\")".into()),
                        }
                    }
                    _ => {
                        return Err(format!(
                            ":db.error/invalid-pull-pattern Unknown attribute modifier :{mod_name}. \
                             Valid modifiers: :limit, :default, :as"
                        ).into());
                    }
                }
            }
            _ => {
                return Err(format!(
                    ":db.error/invalid-pull-pattern Expected keyword modifier (:limit, :default, \
                     or :as) in attribute expression, got: {}",
                    items[i]
                )
                .into());
            }
        }
        i += 1;
    }

    specs.push(PullAttrSpec::Attribute {
        ident,
        reverse,
        forward_ident,
        rename,
        default,
        limit,
    });

    Ok(())
}

/// Parse a keyword into (display_ident, is_reverse, forward_ident).
/// For `:person/_friends`, returns (":person/_friends", true, ":person/friends").
fn parse_keyword(kw: &edn::symbols::Keyword) -> (String, bool, String) {
    let name = kw.name();
    let ns = kw.namespace();

    let is_reverse = name.starts_with('_');

    if is_reverse {
        let forward_name = &name[1..];
        let ident = if let Some(ns) = ns {
            format!(":{ns}/{name}")
        } else {
            format!(":{name}")
        };
        let forward_ident = if let Some(ns) = ns {
            format!(":{ns}/{forward_name}")
        } else {
            format!(":{forward_name}")
        };
        (ident, true, forward_ident)
    } else {
        let ident = if let Some(ns) = ns {
            format!(":{ns}/{name}")
        } else {
            format!(":{name}")
        };
        (ident.clone(), false, ident)
    }
}

/// Parse a limit value: integer or nil.
fn parse_limit_value(val: &edn::Value) -> Result<LimitSpec, PullError> {
    match val {
        edn::Value::Integer(n) => {
            if *n < 0 {
                Err(format!(
                    ":db.error/invalid-limit :limit value must be non-negative, got {}. \
                     Use a positive integer or nil for unlimited.",
                    n
                ).into())
            } else {
                Ok(LimitSpec::Count(*n as usize))
            }
        }
        edn::Value::Nil => Ok(LimitSpec::Unlimited),
        _ => Err(format!(
            ":db.error/invalid-limit :limit value must be a non-negative integer or nil (for unlimited), \
             got: {val}"
        ).into()),
    }
}

/// Convert an EDN value to a serde_json::Value (for :default values).
fn edn_to_json(val: &edn::Value) -> serde_json::Value {
    match val {
        edn::Value::Nil => serde_json::Value::Null,
        edn::Value::Boolean(b) => json!(*b),
        edn::Value::Integer(n) => json!(*n),
        edn::Value::Float(f) => json!(f.into_inner()),
        edn::Value::Text(ref s) => json!(s),
        edn::Value::Keyword(kw) => json!(format!("{kw}")),
        _ => json!(format!("{val}")),
    }
}

// ---------------------------------------------------------------------------
// Pull execution engine
// ---------------------------------------------------------------------------

/// Execute a pull pattern against an entity.
fn execute_pull(
    client: &SpiClient<'_>,
    entity_id: i64,
    specs: &[PullAttrSpec],
    result_map: &mut serde_json::Map<String, serde_json::Value>,
    visited: &mut HashSet<i64>,
    depth: usize,
) -> Result<(), PullError> {
    // Collect wildcard overrides: map specs and attribute expressions that follow
    // a wildcard should override the wildcard's default handling for those attributes.
    let has_wildcard = specs.iter().any(|s| matches!(s, PullAttrSpec::Wildcard));
    let mut override_idents: HashSet<String> = HashSet::new();

    if has_wildcard {
        for spec in specs {
            match spec {
                PullAttrSpec::Attribute { forward_ident, .. } => {
                    override_idents.insert(forward_ident.clone());
                }
                PullAttrSpec::MapSpec { forward_ident, .. } => {
                    override_idents.insert(forward_ident.clone());
                }
                PullAttrSpec::RecursiveSpec { forward_ident, .. } => {
                    override_idents.insert(forward_ident.clone());
                }
                PullAttrSpec::Wildcard => {}
            }
        }
    }

    for spec in specs {
        match spec {
            PullAttrSpec::Wildcard => {
                pull_wildcard(
                    client,
                    entity_id,
                    result_map,
                    &override_idents,
                    visited,
                    depth,
                )?;
            }
            PullAttrSpec::Attribute {
                ident,
                reverse,
                forward_ident,
                rename,
                default,
                limit,
            } => {
                if *reverse {
                    pull_reverse_attribute(
                        client,
                        entity_id,
                        ident,
                        forward_ident,
                        rename.as_deref(),
                        default.as_ref(),
                        limit.as_ref(),
                        result_map,
                    )?;
                } else {
                    pull_forward_attribute(
                        client,
                        entity_id,
                        forward_ident,
                        rename.as_deref(),
                        default.as_ref(),
                        limit.as_ref(),
                        result_map,
                    )?;
                }
            }
            PullAttrSpec::MapSpec {
                ident,
                reverse,
                forward_ident,
                sub_pattern,
                rename,
                limit,
            } => {
                if *reverse {
                    pull_reverse_map_spec(
                        client,
                        entity_id,
                        ident,
                        forward_ident,
                        sub_pattern,
                        rename.as_deref(),
                        limit.as_ref(),
                        result_map,
                        visited,
                        depth,
                    )?;
                } else {
                    pull_forward_map_spec(
                        client,
                        entity_id,
                        forward_ident,
                        sub_pattern,
                        rename.as_deref(),
                        limit.as_ref(),
                        result_map,
                        visited,
                        depth,
                    )?;
                }
            }
            PullAttrSpec::RecursiveSpec {
                ident,
                reverse,
                forward_ident,
                depth: rec_depth,
                rename,
            } => {
                pull_recursive(
                    client,
                    entity_id,
                    ident,
                    forward_ident,
                    *reverse,
                    rec_depth,
                    rename.as_deref(),
                    result_map,
                    visited,
                    depth,
                )?;
            }
        }
    }

    Ok(())
}

/// Pull all attributes for an entity (wildcard).
/// For ref attributes: non-component refs return just {:db/id N},
/// component refs recursively pull all nested attributes.
fn pull_wildcard(
    client: &SpiClient<'_>,
    entity_id: i64,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
    override_idents: &HashSet<String>,
    visited: &mut HashSet<i64>,
    depth: usize,
) -> Result<(), PullError> {
    let query = "SELECT s.ident, s.cardinality::TEXT, s.value_type::TEXT, s.component, \
                        d.value_type_tag, d.v_ref, d.v_bool, d.v_long, d.v_double, \
                        d.v_text, d.v_keyword, \
                        EXTRACT(EPOCH FROM d.v_instant)::BIGINT * 1000000 + \
                        EXTRACT(MICROSECOND FROM d.v_instant)::BIGINT % 1000000 AS v_instant_micros, \
                        d.v_uuid::TEXT, d.v_bytes \
                 FROM mentat.datoms d \
                 JOIN mentat.schema s ON d.a = s.entid \
                 WHERE d.e = $1 AND d.added = true \
                 ORDER BY s.ident";

    // Collect all datom rows first so we can process refs after gathering all values.
    struct DatomRow {
        ident: String,
        cardinality: String,
        _value_type: String,
        component: bool,
        v_type_tag: i16,
        decoded: serde_json::Value,
        ref_id: Option<i64>,
    }

    let mut rows = Vec::new();
    for row in client.select(query, None, &[DatumWithOid::from(entity_id)])? {
        let v_type_tag: i16 = row.get(5)?.ok_or("Missing type tag")?;
        let (decoded, ref_id) = decode_row_typed_value(&row, v_type_tag, 6)?;
        rows.push(DatomRow {
            ident: row.get(1)?.ok_or("Missing ident")?,
            cardinality: row.get(2)?.ok_or("Missing cardinality")?,
            _value_type: row.get(3)?.ok_or("Missing value_type")?,
            component: row.get(4)?.unwrap_or(false),
            v_type_tag,
            decoded,
            ref_id,
        });
    }

    for datom in &rows {
        // Skip attributes that have explicit overrides in the pattern.
        if override_idents.contains(&datom.ident) {
            continue;
        }

        if datom.v_type_tag == type_tag::REF {
            let ref_id = datom.ref_id.ok_or("Missing ref ID")?;
            if datom.component {
                let mut sub_map = serde_json::Map::new();
                sub_map.insert(":db/id".to_string(), json!(ref_id));
                if depth < MAX_RECURSION_DEPTH && !visited.contains(&ref_id) {
                    visited.insert(ref_id);
                    pull_wildcard(
                        client,
                        ref_id,
                        &mut sub_map,
                        &HashSet::new(),
                        visited,
                        depth + 1,
                    )?;
                }
                let value = serde_json::Value::Object(sub_map);
                insert_value(result_map, &datom.ident, value, &datom.cardinality);
            } else {
                let ref_obj = json!({":db/id": ref_id});
                insert_value(result_map, &datom.ident, ref_obj, &datom.cardinality);
            }
        } else {
            insert_value(result_map, &datom.ident, datom.decoded.clone(), &datom.cardinality);
        }
    }

    Ok(())
}

/// Pull a single forward attribute.
///
/// When the attribute is a ref type and marked as `:db/isComponent` in the schema,
/// the referenced entity is recursively pulled with all its attributes (Datomic
/// component semantics) rather than returning just `{:db/id N}`.
fn pull_forward_attribute(
    client: &SpiClient<'_>,
    entity_id: i64,
    ident: &str,
    rename: Option<&str>,
    default: Option<&serde_json::Value>,
    limit: Option<&LimitSpec>,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), PullError> {
    let query = "SELECT s.cardinality::TEXT, s.component, d.value_type_tag, \
                        d.v_ref, d.v_bool, d.v_long, d.v_double, \
                        d.v_text, d.v_keyword, \
                        EXTRACT(EPOCH FROM d.v_instant)::BIGINT * 1000000 + \
                        EXTRACT(MICROSECOND FROM d.v_instant)::BIGINT % 1000000 AS v_instant_micros, \
                        d.v_uuid::TEXT, d.v_bytes \
                 FROM mentat.datoms d \
                 JOIN mentat.schema s ON d.a = s.entid \
                 WHERE d.e = $1 AND s.ident = $2 AND d.added = true";

    let output_key = rename.unwrap_or(ident);
    let mut found = false;

    let max_rows = resolve_limit(limit);

    let mut count = 0usize;
    for row in client.select(
        query,
        None,
        &[DatumWithOid::from(entity_id), DatumWithOid::from(ident)],
    )? {
        if count >= max_rows {
            break;
        }
        found = true;
        let cardinality: String = row.get(1)?.ok_or("Missing cardinality")?;
        let is_component: bool = row.get(2)?.unwrap_or(false);
        let v_type_tag: i16 = row.get(3)?.ok_or("Missing type tag")?;
        let (decoded_val, ref_id) = decode_row_typed_value(&row, v_type_tag, 4)?;

        let decoded = if v_type_tag == type_tag::REF {
            let rid = ref_id.ok_or("Missing ref ID")?;
            if is_component {
                // Component refs are recursively pulled with all attributes.
                let mut sub_map = serde_json::Map::new();
                sub_map.insert(":db/id".to_string(), json!(rid));
                let mut visited = HashSet::new();
                visited.insert(entity_id);
                visited.insert(rid);
                pull_wildcard(
                    client,
                    rid,
                    &mut sub_map,
                    &HashSet::new(),
                    &mut visited,
                    1,
                )?;
                serde_json::Value::Object(sub_map)
            } else {
                json!({":db/id": rid})
            }
        } else {
            decoded_val
        };
        insert_value(result_map, output_key, decoded, &cardinality);
        count += 1;
    }

    if !found {
        if let Some(def) = default {
            result_map.insert(output_key.to_string(), def.clone());
        }
        // Datomic omits missing attributes unless a default is specified.
    }

    Ok(())
}

/// Pull a reverse attribute: find all entities that reference `entity_id` via `forward_ident`.
fn pull_reverse_attribute(
    client: &SpiClient<'_>,
    entity_id: i64,
    display_ident: &str,
    forward_ident: &str,
    rename: Option<&str>,
    default: Option<&serde_json::Value>,
    limit: Option<&LimitSpec>,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), PullError> {
    let ref_ids = query_reverse_refs(client, entity_id, forward_ident, limit)?;
    let output_key = rename.unwrap_or(display_ident);

    if ref_ids.is_empty() {
        if let Some(def) = default {
            result_map.insert(output_key.to_string(), def.clone());
        }
        return Ok(());
    }

    // Reverse lookups always return an array of {:db/id N} maps.
    let arr: Vec<serde_json::Value> = ref_ids.iter().map(|id| json!({":db/id": *id})).collect();
    result_map.insert(output_key.to_string(), json!(arr));

    Ok(())
}

/// Pull a forward map spec: follow ref values and recursively pull sub-pattern.
fn pull_forward_map_spec(
    client: &SpiClient<'_>,
    entity_id: i64,
    ident: &str,
    sub_pattern: &[PullAttrSpec],
    rename: Option<&str>,
    limit: Option<&LimitSpec>,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
    visited: &mut HashSet<i64>,
    depth: usize,
) -> Result<(), PullError> {
    let ref_ids = query_forward_refs(client, entity_id, ident, limit)?;
    let output_key = rename.unwrap_or(ident);

    if ref_ids.is_empty() {
        return Ok(());
    }

    let cardinality = lookup_cardinality(client, ident)?;

    if cardinality == "one" {
        // Cardinality one: return a single map.
        let ref_id = ref_ids[0];
        let mut sub_map = serde_json::Map::new();
        sub_map.insert(":db/id".to_string(), json!(ref_id));
        let was_new = visited.insert(ref_id);
        if depth < MAX_RECURSION_DEPTH {
            execute_pull(
                client,
                ref_id,
                sub_pattern,
                &mut sub_map,
                visited,
                depth + 1,
            )?;
        }
        if was_new {
            visited.remove(&ref_id);
        }
        result_map.insert(output_key.to_string(), serde_json::Value::Object(sub_map));
    } else {
        // Cardinality many: return an array of maps.
        let mut arr = Vec::new();
        for ref_id in &ref_ids {
            let mut sub_map = serde_json::Map::new();
            sub_map.insert(":db/id".to_string(), json!(*ref_id));
            let was_new = visited.insert(*ref_id);
            if depth < MAX_RECURSION_DEPTH {
                execute_pull(
                    client,
                    *ref_id,
                    sub_pattern,
                    &mut sub_map,
                    visited,
                    depth + 1,
                )?;
            }
            if was_new {
                visited.remove(ref_id);
            }
            arr.push(serde_json::Value::Object(sub_map));
        }
        result_map.insert(output_key.to_string(), json!(arr));
    }

    Ok(())
}

/// Pull a reverse map spec: find entities referencing this one, then sub-pull.
fn pull_reverse_map_spec(
    client: &SpiClient<'_>,
    entity_id: i64,
    display_ident: &str,
    forward_ident: &str,
    sub_pattern: &[PullAttrSpec],
    rename: Option<&str>,
    limit: Option<&LimitSpec>,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
    visited: &mut HashSet<i64>,
    depth: usize,
) -> Result<(), PullError> {
    let ref_ids = query_reverse_refs(client, entity_id, forward_ident, limit)?;
    let output_key = rename.unwrap_or(display_ident);

    if ref_ids.is_empty() {
        return Ok(());
    }

    // Reverse lookups always return arrays.
    let mut arr = Vec::new();
    for ref_id in &ref_ids {
        let mut sub_map = serde_json::Map::new();
        sub_map.insert(":db/id".to_string(), json!(*ref_id));
        let was_new = visited.insert(*ref_id);
        if depth < MAX_RECURSION_DEPTH {
            execute_pull(
                client,
                *ref_id,
                sub_pattern,
                &mut sub_map,
                visited,
                depth + 1,
            )?;
        }
        if was_new {
            visited.remove(ref_id);
        }
        arr.push(serde_json::Value::Object(sub_map));
    }
    result_map.insert(output_key.to_string(), json!(arr));

    Ok(())
}

/// Handle recursive pull specs.
///
/// For `{:person/friends ...}`, recursively follows the attribute, pulling the same
/// pattern at each level. Cycle detection returns just `{:db/id N}` for previously
/// seen entities.
fn pull_recursive(
    client: &SpiClient<'_>,
    entity_id: i64,
    display_ident: &str,
    forward_ident: &str,
    reverse: bool,
    rec_depth: &RecursionDepth,
    rename: Option<&str>,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
    visited: &mut HashSet<i64>,
    current_depth: usize,
) -> Result<(), PullError> {
    let max_depth = match rec_depth {
        RecursionDepth::Bounded(d) => *d,
        RecursionDepth::Unbounded => MAX_RECURSION_DEPTH,
    };

    if current_depth >= max_depth {
        return Ok(());
    }

    let ref_ids = if reverse {
        query_reverse_refs(client, entity_id, forward_ident, None)?
    } else {
        query_forward_refs(client, entity_id, forward_ident, None)?
    };

    let output_key = rename.unwrap_or(display_ident);

    if ref_ids.is_empty() {
        return Ok(());
    }

    let cardinality = if reverse {
        // Reverse lookups are always multi-valued.
        "many".to_string()
    } else {
        lookup_cardinality(client, forward_ident)?
    };

    // Build a self-referencing recursive spec for sub-pulls.
    let self_spec = PullAttrSpec::RecursiveSpec {
        ident: display_ident.to_string(),
        reverse,
        forward_ident: forward_ident.to_string(),
        depth: rec_depth.clone(),
        rename: rename.map(|s| s.to_string()),
    };

    if cardinality == "one" {
        let ref_id = ref_ids[0];
        if visited.contains(&ref_id) {
            // Cycle detected: return just {:db/id N} per Datomic behavior.
            result_map.insert(
                output_key.to_string(),
                json!({":db/id": ref_id}),
            );
        } else {
            let was_new = visited.insert(ref_id);
            let mut sub_map = serde_json::Map::new();
            sub_map.insert(":db/id".to_string(), json!(ref_id));
            // Pull all non-recursive attributes of the target, plus the recursive one.
            pull_all_attributes_simple(client, ref_id, &mut sub_map)?;
            execute_pull(
                client,
                ref_id,
                &[self_spec],
                &mut sub_map,
                visited,
                current_depth + 1,
            )?;
            if was_new {
                visited.remove(&ref_id);
            }
            result_map.insert(output_key.to_string(), serde_json::Value::Object(sub_map));
        }
    } else {
        let mut arr = Vec::new();
        for ref_id in &ref_ids {
            if visited.contains(ref_id) {
                // Cycle detected: return just {:db/id N} per Datomic behavior.
                arr.push(json!({":db/id": *ref_id}));
            } else {
                let was_new = visited.insert(*ref_id);
                let mut sub_map = serde_json::Map::new();
                sub_map.insert(":db/id".to_string(), json!(*ref_id));
                pull_all_attributes_simple(client, *ref_id, &mut sub_map)?;
                execute_pull(
                    client,
                    *ref_id,
                    &[self_spec.clone()],
                    &mut sub_map,
                    visited,
                    current_depth + 1,
                )?;
                if was_new {
                    visited.remove(ref_id);
                }
                arr.push(serde_json::Value::Object(sub_map));
            }
        }
        result_map.insert(output_key.to_string(), json!(arr));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Database query helpers
// ---------------------------------------------------------------------------

/// Query forward ref values for an entity's attribute.
/// Returns the referenced entity IDs.
fn query_forward_refs(
    client: &SpiClient<'_>,
    entity_id: i64,
    ident: &str,
    limit: Option<&LimitSpec>,
) -> Result<Vec<i64>, PullError> {
    let query = "SELECT d.v_ref \
                 FROM mentat.datoms d \
                 JOIN mentat.schema s ON d.a = s.entid \
                 WHERE d.e = $1 AND s.ident = $2 AND d.value_type_tag = $3 AND d.added = true";

    let max_rows = resolve_limit(limit);
    let mut ref_ids = Vec::new();

    for row in client.select(
        query,
        None,
        &[
            DatumWithOid::from(entity_id),
            DatumWithOid::from(ident),
            DatumWithOid::from(type_tag::REF),
        ],
    )? {
        if ref_ids.len() >= max_rows {
            break;
        }
        let ref_id: i64 = row.get(1)?.ok_or("Missing v_ref")?;
        ref_ids.push(ref_id);
    }

    Ok(ref_ids)
}

/// Query reverse refs: find all entities whose `forward_ident` attribute points to `entity_id`.
fn query_reverse_refs(
    client: &SpiClient<'_>,
    entity_id: i64,
    forward_ident: &str,
    limit: Option<&LimitSpec>,
) -> Result<Vec<i64>, PullError> {
    let query = "SELECT d.e \
                 FROM mentat.datoms d \
                 JOIN mentat.schema s ON d.a = s.entid \
                 WHERE s.ident = $1 AND d.v_ref = $2 AND d.value_type_tag = $3 AND d.added = true";

    let max_rows = resolve_limit(limit);
    let mut ref_ids = Vec::new();

    for row in client.select(
        query,
        None,
        &[
            DatumWithOid::from(forward_ident),
            DatumWithOid::from(entity_id),
            DatumWithOid::from(type_tag::REF),
        ],
    )? {
        if ref_ids.len() >= max_rows {
            break;
        }
        let e: i64 = row.get(1)?.ok_or("Missing entity")?;
        ref_ids.push(e);
    }

    Ok(ref_ids)
}

/// Look up the cardinality of an attribute.
fn lookup_cardinality(client: &SpiClient<'_>, ident: &str) -> Result<String, PullError> {
    let query = "SELECT cardinality::TEXT FROM mentat.schema WHERE ident = $1";
    let result = client.select(query, None, &[DatumWithOid::from(ident)])?;

    for row in result {
        let cardinality: String = row.get(1)?.ok_or("Missing cardinality")?;
        return Ok(cardinality);
    }

    // Default to "one" if attribute not found in schema.
    Ok("one".to_string())
}

/// Pull all non-ref attributes for an entity (used during recursive pulls).
fn pull_all_attributes_simple(
    client: &SpiClient<'_>,
    entity_id: i64,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), PullError> {
    let query = "SELECT s.ident, s.cardinality::TEXT, d.value_type_tag, \
                        d.v_ref, d.v_bool, d.v_long, d.v_double, \
                        d.v_text, d.v_keyword, \
                        EXTRACT(EPOCH FROM d.v_instant)::BIGINT * 1000000 + \
                        EXTRACT(MICROSECOND FROM d.v_instant)::BIGINT % 1000000 AS v_instant_micros, \
                        d.v_uuid::TEXT, d.v_bytes \
                 FROM mentat.datoms d \
                 JOIN mentat.schema s ON d.a = s.entid \
                 WHERE d.e = $1 AND d.added = true \
                 ORDER BY s.ident";

    for row in client.select(query, None, &[DatumWithOid::from(entity_id)])? {
        let ident: String = row.get(1)?.ok_or("Missing ident")?;
        let cardinality: String = row.get(2)?.ok_or("Missing cardinality")?;
        let v_type_tag: i16 = row.get(3)?.ok_or("Missing type tag")?;

        // Skip ref attributes -- the recursive spec handles those.
        if v_type_tag == type_tag::REF {
            continue;
        }

        let (decoded, _) = decode_row_typed_value(&row, v_type_tag, 4)?;
        insert_value(result_map, &ident, decoded, &cardinality);
    }

    Ok(())
}

/// Resolve a LimitSpec to a concrete maximum row count.
fn resolve_limit(limit: Option<&LimitSpec>) -> usize {
    match limit {
        Some(LimitSpec::Count(n)) => *n,
        Some(LimitSpec::Unlimited) => usize::MAX,
        None => DEFAULT_MANY_LIMIT,
    }
}

// ---------------------------------------------------------------------------
// Value decoding
// ---------------------------------------------------------------------------

/// Insert a decoded value into the result map, handling cardinality.
/// For cardinality "many", values are accumulated into a JSON array.
/// For cardinality "one", the last value wins.
fn insert_value(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: serde_json::Value,
    cardinality: &str,
) {
    if cardinality == "many" {
        if let Some(existing) = map.get_mut(key) {
            if let Some(arr) = existing.as_array_mut() {
                arr.push(value);
            } else {
                let prev = existing.clone();
                *existing = json!([prev, value]);
            }
        } else {
            map.insert(key.to_string(), json!([value]));
        }
    } else {
        map.insert(key.to_string(), value);
    }
}

/// Decode a typed value from an SPI row based on value_type_tag.
///
/// The row is expected to have the typed value columns starting at `col_offset`:
///   col_offset + 0 = v_ref (BIGINT)
///   col_offset + 1 = v_bool (BOOLEAN)
///   col_offset + 2 = v_long (BIGINT)
///   col_offset + 3 = v_double (DOUBLE PRECISION)
///   col_offset + 4 = v_text (TEXT)
///   col_offset + 5 = v_keyword (TEXT)
///   col_offset + 6 = v_instant_micros (BIGINT, pre-extracted in SQL)
///   col_offset + 7 = v_uuid (TEXT, cast in SQL)
///   col_offset + 8 = v_bytes (BYTEA)
///
/// Returns (decoded_json_value, optional_ref_id).
fn decode_row_typed_value(
    row: &pgrx::spi::SpiHeapTupleData<'_>,
    type_tag: i16,
    col_offset: usize,
) -> Result<(serde_json::Value, Option<i64>), PullError> {
    match type_tag {
        type_tag::REF => {
            let ref_id: i64 = row.get(col_offset)?.ok_or("Missing v_ref")?;
            Ok((json!(ref_id), Some(ref_id)))
        }
        type_tag::BOOLEAN => {
            let b: bool = row.get(col_offset + 1)?.ok_or("Missing v_bool")?;
            Ok((json!(b), None))
        }
        type_tag::LONG => {
            let n: i64 = row.get(col_offset + 2)?.ok_or("Missing v_long")?;
            Ok((json!(n), None))
        }
        type_tag::DOUBLE => {
            let f: f64 = row.get(col_offset + 3)?.ok_or("Missing v_double")?;
            Ok((json!(f), None))
        }
        type_tag::STRING => {
            let s: String = row.get(col_offset + 4)?.ok_or("Missing v_text")?;
            Ok((json!(s), None))
        }
        type_tag::KEYWORD => {
            let s: String = row.get(col_offset + 5)?.ok_or("Missing v_keyword")?;
            Ok((json!(format!(":{s}")), None))
        }
        type_tag::INSTANT => {
            let micros: i64 = row.get(col_offset + 6)?.ok_or("Missing v_instant_micros")?;
            Ok((json!(micros), None))
        }
        type_tag::UUID => {
            let s: String = row.get(col_offset + 7)?.ok_or("Missing v_uuid")?;
            Ok((json!(s), None))
        }
        type_tag::BYTES => {
            let b: Vec<u8> = row.get(col_offset + 8)?.ok_or("Missing v_bytes")?;
            Ok((json!(hex::encode(b)), None))
        }
        _ => Err(MentatError::UnsupportedType { type_tag }.into()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use super::*;

    #[pg_test]
    fn test_parse_simple_pattern() -> Result<(), PullError> {
        let parsed = edn::parse::value("[:person/name :person/age]")
            .map_err(|e| -> PullError { format!("{e}").into() })?;
        let items = match parsed.without_spans() {
            edn::Value::Vector(v) => v,
            _ => panic!("expected vector"),
        };
        let specs = parse_pull_pattern(&items)?;
        assert_eq!(specs.len(), 2);
        match &specs[0] {
            PullAttrSpec::Attribute { ident, reverse, .. } => {
                assert_eq!(ident, ":person/name");
                assert!(!reverse);
            }
            _ => panic!("expected Attribute"),
        }
        Ok(())
    }

    #[pg_test]
    fn test_parse_wildcard_pattern() -> Result<(), PullError> {
        let parsed =
            edn::parse::value("[*]").map_err(|e| -> PullError { format!("{e}").into() })?;
        let items = match parsed.without_spans() {
            edn::Value::Vector(v) => v,
            _ => panic!("expected vector"),
        };
        let specs = parse_pull_pattern(&items)?;
        assert_eq!(specs.len(), 1);
        assert!(matches!(&specs[0], PullAttrSpec::Wildcard));
        Ok(())
    }

    #[pg_test]
    fn test_parse_reverse_lookup() -> Result<(), PullError> {
        let parsed = edn::parse::value("[:person/_friends]")
            .map_err(|e| -> PullError { format!("{e}").into() })?;
        let items = match parsed.without_spans() {
            edn::Value::Vector(v) => v,
            _ => panic!("expected vector"),
        };
        let specs = parse_pull_pattern(&items)?;
        assert_eq!(specs.len(), 1);
        match &specs[0] {
            PullAttrSpec::Attribute {
                ident,
                reverse,
                forward_ident,
                ..
            } => {
                assert_eq!(ident, ":person/_friends");
                assert!(reverse);
                assert_eq!(forward_ident, ":person/friends");
            }
            _ => panic!("expected Attribute"),
        }
        Ok(())
    }

    #[pg_test]
    fn test_parse_map_spec() -> Result<(), PullError> {
        let parsed = edn::parse::value("[{:person/friends [:person/name]}]")
            .map_err(|e| -> PullError { format!("{e}").into() })?;
        let items = match parsed.without_spans() {
            edn::Value::Vector(v) => v,
            _ => panic!("expected vector"),
        };
        let specs = parse_pull_pattern(&items)?;
        assert_eq!(specs.len(), 1);
        match &specs[0] {
            PullAttrSpec::MapSpec {
                ident, sub_pattern, ..
            } => {
                assert_eq!(ident, ":person/friends");
                assert_eq!(sub_pattern.len(), 1);
            }
            _ => panic!("expected MapSpec"),
        }
        Ok(())
    }

    #[pg_test]
    fn test_parse_recursive_unbounded() -> Result<(), PullError> {
        let parsed = edn::parse::value("[{:person/friends ...}]")
            .map_err(|e| -> PullError { format!("{e}").into() })?;
        let items = match parsed.without_spans() {
            edn::Value::Vector(v) => v,
            _ => panic!("expected vector"),
        };
        let specs = parse_pull_pattern(&items)?;
        assert_eq!(specs.len(), 1);
        match &specs[0] {
            PullAttrSpec::RecursiveSpec { depth, .. } => {
                assert!(matches!(depth, RecursionDepth::Unbounded));
            }
            _ => panic!("expected RecursiveSpec"),
        }
        Ok(())
    }

    #[pg_test]
    fn test_parse_recursive_bounded() -> Result<(), PullError> {
        let parsed = edn::parse::value("[{:person/friends 3}]")
            .map_err(|e| -> PullError { format!("{e}").into() })?;
        let items = match parsed.without_spans() {
            edn::Value::Vector(v) => v,
            _ => panic!("expected vector"),
        };
        let specs = parse_pull_pattern(&items)?;
        assert_eq!(specs.len(), 1);
        match &specs[0] {
            PullAttrSpec::RecursiveSpec { depth, .. } => {
                assert!(matches!(depth, RecursionDepth::Bounded(3)));
            }
            _ => panic!("expected RecursiveSpec"),
        }
        Ok(())
    }

    #[pg_test]
    fn test_edn_to_json() {
        assert_eq!(edn_to_json(&edn::Value::Integer(42)), json!(42));
        assert_eq!(edn_to_json(&edn::Value::Boolean(true)), json!(true));
        assert_eq!(
            edn_to_json(&edn::Value::Text("hello".into())),
            json!("hello")
        );
        assert_eq!(edn_to_json(&edn::Value::Nil), serde_json::Value::Null);
    }

    #[pg_test]
    fn test_insert_value_cardinality_one() {
        let mut map = serde_json::Map::new();
        insert_value(&mut map, ":name", json!("Alice"), "one");
        assert_eq!(map.get(":name"), Some(&json!("Alice")));
        // Second insert overwrites.
        insert_value(&mut map, ":name", json!("Bob"), "one");
        assert_eq!(map.get(":name"), Some(&json!("Bob")));
    }

    #[pg_test]
    fn test_insert_value_cardinality_many() {
        let mut map = serde_json::Map::new();
        insert_value(&mut map, ":tags", json!("a"), "many");
        assert_eq!(map.get(":tags"), Some(&json!(["a"])));
        insert_value(&mut map, ":tags", json!("b"), "many");
        assert_eq!(map.get(":tags"), Some(&json!(["a", "b"])));
    }

    /// Test that cycle detection prevents infinite loops on circular references.
    ///
    /// Creates a graph: A->B->C->A and verifies that recursive pull:
    /// 1. Completes without infinite loop
    /// 2. Returns {:db/id N} stubs for cycles (Datomic-compatible)
    /// 3. Works at various recursion depths
    #[pg_test]
    fn test_recursive_pull_cycle_detection() -> spi::Result<()> {
        // Initialize schema with person/friend attribute
        Spi::run(
            "CREATE SCHEMA IF NOT EXISTS mentat;
             CREATE TABLE IF NOT EXISTS mentat.schema (
                 entid BIGINT PRIMARY KEY,
                 ident TEXT UNIQUE NOT NULL,
                 value_type TEXT NOT NULL,
                 cardinality TEXT NOT NULL,
                 unique_identity BOOLEAN DEFAULT FALSE,
                 index_av BOOLEAN DEFAULT FALSE,
                 index_fulltext BOOLEAN DEFAULT FALSE,
                 component BOOLEAN DEFAULT FALSE
             );
             CREATE TABLE IF NOT EXISTS mentat.datoms (
                 e BIGINT NOT NULL,
                 a BIGINT NOT NULL,
                 value_type_tag SMALLINT NOT NULL,
                 v_ref BIGINT,
                 v_bool BOOLEAN,
                 v_long BIGINT,
                 v_double DOUBLE PRECISION,
                 v_text TEXT,
                 v_keyword TEXT,
                 v_instant TIMESTAMPTZ,
                 v_uuid UUID,
                 v_bytes BYTEA,
                 tx BIGINT NOT NULL,
                 added BOOLEAN NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_datoms_e ON mentat.datoms(e);
             CREATE INDEX IF NOT EXISTS idx_datoms_a ON mentat.datoms(a);",
        )?;

        // Insert schema for :person/friend attribute
        Spi::run(
            "INSERT INTO mentat.schema (entid, ident, value_type, cardinality, component)
             VALUES (100, ':person/friend', 'ref', 'one', false)
             ON CONFLICT (ident) DO NOTHING;
             INSERT INTO mentat.schema (entid, ident, value_type, cardinality, component)
             VALUES (101, ':person/name', 'string', 'one', false)
             ON CONFLICT (ident) DO NOTHING;",
        )?;

        // Create circular graph: A(1000)->B(1001)->C(1002)->A(1000)
        Spi::run(
            "DELETE FROM mentat.datoms WHERE e IN (1000, 1001, 1002);
             INSERT INTO mentat.datoms (e, a, value_type_tag, v_text, tx, added) VALUES
             (1000, 101, 7, 'Alice', 5000, true),
             (1001, 101, 7, 'Bob', 5000, true),
             (1002, 101, 7, 'Carol', 5000, true);
             INSERT INTO mentat.datoms (e, a, value_type_tag, v_ref, tx, added) VALUES
             (1000, 100, 0, 1001, 5000, true),
             (1001, 100, 0, 1002, 5000, true),
             (1002, 100, 0, 1000, 5000, true);",
        )?;

        // Test 1: Pull with depth 10 - should not infinite loop
        let result = Spi::get_one::<JsonB>("SELECT mentat_pull('[{:person/friend 10}]', 1000)")?;

        assert!(
            result.is_some(),
            "Pull should complete without infinite loop"
        );

        if let Some(JsonB(json_val)) = result {
            let obj = json_val.as_object().expect("result should be an object");
            assert!(obj.contains_key(":db/id"), "result should contain :db/id");
            assert!(
                obj.contains_key(":person/friend"),
                "result should contain :person/friend"
            );
            let json_str = serde_json::to_string(&json_val).unwrap();
            assert!(
                json_str.contains(":db/id"),
                "result should contain :db/id references: {}",
                json_str
            );
        }

        // Test 2: Pull with unbounded recursion (...) - should hit MAX_RECURSION_DEPTH
        let result = Spi::get_one::<JsonB>("SELECT mentat_pull('[{:person/friend ...}]', 1000)")?;

        assert!(
            result.is_some(),
            "Unbounded pull should complete without infinite loop"
        );

        if let Some(JsonB(json_val)) = result {
            let json_str = serde_json::to_string(&json_val).unwrap();
            assert!(
                json_str.contains(":db/id"),
                "unbounded pull should contain :db/id stubs for cycles: {}",
                json_str
            );
            assert!(
                !json_str.contains("_cycle"),
                "should not contain non-standard _cycle markers: {}",
                json_str
            );
        }

        // Test 3: Pull with depth 1 - should get one level without cycles
        let result = Spi::get_one::<JsonB>("SELECT mentat_pull('[{:person/friend 1}]', 1000)")?;
        assert!(result.is_some(), "Depth-1 pull should complete");

        // Test 4: Verify that non-cyclic paths in the same graph work correctly
        Spi::run(
            "INSERT INTO mentat.datoms (e, a, value_type_tag, v_ref, tx, added) VALUES
             (1003, 100, 0, 1000, 5001, true),
             (1003, 100, 0, 1001, 5001, true)
             ON CONFLICT DO NOTHING;",
        )?;

        let result = Spi::get_one::<JsonB>("SELECT mentat_pull('[{:person/friend 5}]', 1003)")?;
        assert!(result.is_some(), "Diamond pattern pull should complete");

        Ok(())
    }

    /// Test that cycle detection works with cardinality-many attributes.
    #[pg_test]
    fn test_recursive_pull_many_cycles() -> spi::Result<()> {
        // Initialize schema
        Spi::run(
            "CREATE SCHEMA IF NOT EXISTS mentat;
             CREATE TABLE IF NOT EXISTS mentat.schema (
                 entid BIGINT PRIMARY KEY,
                 ident TEXT UNIQUE NOT NULL,
                 value_type TEXT NOT NULL,
                 cardinality TEXT NOT NULL,
                 unique_identity BOOLEAN DEFAULT FALSE,
                 index_av BOOLEAN DEFAULT FALSE,
                 index_fulltext BOOLEAN DEFAULT FALSE,
                 component BOOLEAN DEFAULT FALSE
             );
             CREATE TABLE IF NOT EXISTS mentat.datoms (
                 e BIGINT NOT NULL,
                 a BIGINT NOT NULL,
                 value_type_tag SMALLINT NOT NULL,
                 v_ref BIGINT,
                 v_bool BOOLEAN,
                 v_long BIGINT,
                 v_double DOUBLE PRECISION,
                 v_text TEXT,
                 v_keyword TEXT,
                 v_instant TIMESTAMPTZ,
                 v_uuid UUID,
                 v_bytes BYTEA,
                 tx BIGINT NOT NULL,
                 added BOOLEAN NOT NULL
             );",
        )?;

        // Insert schema for :person/friends (cardinality many)
        Spi::run(
            "INSERT INTO mentat.schema (entid, ident, value_type, cardinality, component)
             VALUES (200, ':person/friends', 'ref', 'many', false)
             ON CONFLICT (ident) DO NOTHING;",
        )?;

        // Create graph with multiple cycles:
        // A has friends [B, C]
        // B has friends [C, A] (cycle to A)
        // C has friends [A]     (cycle to A)
        Spi::run(
            "DELETE FROM mentat.datoms WHERE e IN (2000, 2001, 2002);
             INSERT INTO mentat.datoms (e, a, value_type_tag, v_ref, tx, added) VALUES
             (2000, 200, 0, 2001, 6000, true),
             (2000, 200, 0, 2002, 6000, true),
             (2001, 200, 0, 2002, 6000, true),
             (2001, 200, 0, 2000, 6000, true),
             (2002, 200, 0, 2000, 6000, true);",
        )?;

        // Pull from A with depth 5
        let result = Spi::get_one::<JsonB>("SELECT mentat_pull('[{:person/friends 5}]', 2000)")?;

        assert!(
            result.is_some(),
            "Cardinality-many cycle detection should complete"
        );

        if let Some(JsonB(json_val)) = result {
            let json_str = serde_json::to_string(&json_val).unwrap();
            assert!(
                json_str.contains(":db/id"),
                "many-cardinality result should contain :db/id stubs: {}",
                json_str
            );
            assert!(
                !json_str.contains("_cycle"),
                "should not contain non-standard _cycle markers: {}",
                json_str
            );
        }

        Ok(())
    }

    /// Test that component attributes are automatically pulled recursively.
    ///
    /// Creates an Order entity with component LineItem entities. When pulling
    /// :order/items (marked as component), the referenced line items should be
    /// fully expanded with all their attributes, not returned as bare {:db/id N}.
    #[pg_test]
    fn test_component_auto_pull() -> spi::Result<()> {
        Spi::run(
            "CREATE SCHEMA IF NOT EXISTS mentat;
             CREATE TABLE IF NOT EXISTS mentat.schema (
                 entid BIGINT PRIMARY KEY,
                 ident TEXT UNIQUE NOT NULL,
                 value_type TEXT NOT NULL,
                 cardinality TEXT NOT NULL,
                 unique_identity BOOLEAN DEFAULT FALSE,
                 index_av BOOLEAN DEFAULT FALSE,
                 index_fulltext BOOLEAN DEFAULT FALSE,
                 component BOOLEAN DEFAULT FALSE
             );
             CREATE TABLE IF NOT EXISTS mentat.datoms (
                 e BIGINT NOT NULL,
                 a BIGINT NOT NULL,
                 value_type_tag SMALLINT NOT NULL,
                 v_ref BIGINT,
                 v_bool BOOLEAN,
                 v_long BIGINT,
                 v_double DOUBLE PRECISION,
                 v_text TEXT,
                 v_keyword TEXT,
                 v_instant TIMESTAMPTZ,
                 v_uuid UUID,
                 v_bytes BYTEA,
                 tx BIGINT NOT NULL,
                 added BOOLEAN NOT NULL
             );",
        )?;

        // Schema: :order/items is a component ref (many), :item/name is a string
        Spi::run(
            "INSERT INTO mentat.schema (entid, ident, value_type, cardinality, component)
             VALUES (300, ':order/items', 'ref', 'many', true)
             ON CONFLICT (ident) DO NOTHING;
             INSERT INTO mentat.schema (entid, ident, value_type, cardinality, component)
             VALUES (301, ':item/name', 'string', 'one', false)
             ON CONFLICT (ident) DO NOTHING;
             INSERT INTO mentat.schema (entid, ident, value_type, cardinality, component)
             VALUES (302, ':item/qty', 'long', 'one', false)
             ON CONFLICT (ident) DO NOTHING;
             INSERT INTO mentat.schema (entid, ident, value_type, cardinality, component)
             VALUES (303, ':order/name', 'string', 'one', false)
             ON CONFLICT (ident) DO NOTHING;",
        )?;

        // Order 3000 has two line items: 3001 and 3002
        Spi::run(
            "DELETE FROM mentat.datoms WHERE e IN (3000, 3001, 3002);
             INSERT INTO mentat.datoms (e, a, value_type_tag, v_text, tx, added) VALUES
             (3000, 303, 7, 'Order-1', 7000, true),
             (3001, 301, 7, 'Widget', 7000, true),
             (3002, 301, 7, 'Gadget', 7000, true);
             INSERT INTO mentat.datoms (e, a, value_type_tag, v_long, tx, added) VALUES
             (3001, 302, 2, 5, 7000, true),
             (3002, 302, 2, 3, 7000, true);
             INSERT INTO mentat.datoms (e, a, value_type_tag, v_ref, tx, added) VALUES
             (3000, 300, 0, 3001, 7000, true),
             (3000, 300, 0, 3002, 7000, true);",
        )?;

        // Pull :order/items -- should recursively expand component entities
        let result = Spi::get_one::<JsonB>(
            "SELECT mentat_pull('[:order/name :order/items]', 3000)",
        )?;

        assert!(result.is_some(), "Component pull should succeed");
        if let Some(JsonB(json_val)) = result {
            let obj = json_val.as_object().expect("result should be object");

            // :order/name should be present
            assert_eq!(obj.get(":order/name"), Some(&json!("Order-1")));

            // :order/items should be an array of fully-expanded component entities
            let items = obj.get(":order/items").expect("should have :order/items");
            let arr = items.as_array().expect("items should be array");
            assert_eq!(arr.len(), 2, "should have 2 line items");

            // Each item should have :db/id, :item/name, :item/qty (fully expanded)
            for item in arr {
                let item_obj = item.as_object().expect("item should be object");
                assert!(item_obj.contains_key(":db/id"), "item should have :db/id");
                assert!(item_obj.contains_key(":item/name"), "item should have :item/name");
                assert!(item_obj.contains_key(":item/qty"), "item should have :item/qty");
            }
        }

        Ok(())
    }

    /// Test mentat_pull_many: batched pull of multiple entities.
    #[pg_test]
    fn test_pull_many_basic() -> spi::Result<()> {
        Spi::run(
            "CREATE SCHEMA IF NOT EXISTS mentat;
             CREATE TABLE IF NOT EXISTS mentat.schema (
                 entid BIGINT PRIMARY KEY,
                 ident TEXT UNIQUE NOT NULL,
                 value_type TEXT NOT NULL,
                 cardinality TEXT NOT NULL,
                 unique_identity BOOLEAN DEFAULT FALSE,
                 index_av BOOLEAN DEFAULT FALSE,
                 index_fulltext BOOLEAN DEFAULT FALSE,
                 component BOOLEAN DEFAULT FALSE
             );
             CREATE TABLE IF NOT EXISTS mentat.datoms (
                 e BIGINT NOT NULL,
                 a BIGINT NOT NULL,
                 value_type_tag SMALLINT NOT NULL,
                 v_ref BIGINT,
                 v_bool BOOLEAN,
                 v_long BIGINT,
                 v_double DOUBLE PRECISION,
                 v_text TEXT,
                 v_keyword TEXT,
                 v_instant TIMESTAMPTZ,
                 v_uuid UUID,
                 v_bytes BYTEA,
                 tx BIGINT NOT NULL,
                 added BOOLEAN NOT NULL
             );",
        )?;

        Spi::run(
            "INSERT INTO mentat.schema (entid, ident, value_type, cardinality, component)
             VALUES (400, ':person/name', 'string', 'one', false)
             ON CONFLICT (ident) DO NOTHING;
             INSERT INTO mentat.schema (entid, ident, value_type, cardinality, component)
             VALUES (401, ':person/age', 'long', 'one', false)
             ON CONFLICT (ident) DO NOTHING;",
        )?;

        Spi::run(
            "DELETE FROM mentat.datoms WHERE e IN (4000, 4001, 4002);
             INSERT INTO mentat.datoms (e, a, value_type_tag, v_text, tx, added) VALUES
             (4000, 400, 7, 'Alice', 8000, true),
             (4001, 400, 7, 'Bob', 8000, true),
             (4002, 400, 7, 'Carol', 8000, true);
             INSERT INTO mentat.datoms (e, a, value_type_tag, v_long, tx, added) VALUES
             (4000, 401, 2, 30, 8000, true),
             (4001, 401, 2, 25, 8000, true),
             (4002, 401, 2, 35, 8000, true);",
        )?;

        // Pull multiple entities at once
        let result = Spi::get_one::<JsonB>(
            "SELECT mentat_pull_many('[:person/name :person/age]', ARRAY[4000, 4001, 4002])",
        )?;

        assert!(result.is_some(), "Pull many should return results");
        if let Some(JsonB(json_val)) = result {
            let arr = json_val.as_array().expect("result should be array");
            assert_eq!(arr.len(), 3, "should have 3 entities");

            // Results should be in input order
            let first = arr[0].as_object().expect("first should be object");
            assert_eq!(first.get(":person/name"), Some(&json!("Alice")));
            assert_eq!(first.get(":person/age"), Some(&json!(30)));

            let second = arr[1].as_object().expect("second should be object");
            assert_eq!(second.get(":person/name"), Some(&json!("Bob")));

            let third = arr[2].as_object().expect("third should be object");
            assert_eq!(third.get(":person/name"), Some(&json!("Carol")));
        }

        Ok(())
    }

    /// Test mentat_pull_many with empty entity list.
    #[pg_test]
    fn test_pull_many_empty() -> Result<(), PullError> {
        let result = mentat_pull_many("[:person/name]", vec![])?;
        let arr = result.0.as_array().expect("should be array");
        assert!(arr.is_empty(), "empty input should produce empty output");
        Ok(())
    }
}
