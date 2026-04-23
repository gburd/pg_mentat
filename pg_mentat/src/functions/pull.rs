use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::spi::SpiClient;
use pgrx::JsonB;
use serde_json::json;
use std::collections::HashSet;

/// Type tags matching encode_value in transact.rs.
mod type_tag {
    pub const REF: i16 = 0;
    #[allow(dead_code)]
    pub const BOOLEAN: i16 = 1;
    #[allow(dead_code)]
    pub const LONG: i16 = 2;
    #[allow(dead_code)]
    pub const DOUBLE: i16 = 3;
    #[allow(dead_code)]
    pub const INSTANT: i16 = 4;
    #[allow(dead_code)]
    pub const STRING: i16 = 7;
    #[allow(dead_code)]
    pub const KEYWORD: i16 = 8;
    #[allow(dead_code)]
    pub const UUID: i16 = 10;
    #[allow(dead_code)]
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
        .map_err(|e| -> PullError { format!(
            ":db.error/invalid-pull-pattern Failed to parse pull pattern as EDN: {e}. \
             Expected a vector like [:person/name :person/age] or [*]."
        ).into() })?;
    let pattern_value = parsed.without_spans();

    let specs = match &pattern_value {
        edn::Value::Vector(items) => parse_pull_pattern(items)?,
        _ => return Err(":db.error/invalid-pull-pattern Pull pattern must be a vector. \
                        Expected: [:person/name :person/age] or [*] or [{:person/friends [:person/name]}]. \
                        Got a non-vector EDN value.".into()),
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
                        d.v, d.value_type_tag \
                 FROM mentat.datoms d \
                 JOIN mentat.schema s ON d.a = s.entid \
                 WHERE d.e = $1 AND d.added = true \
                 ORDER BY s.ident";

    // Collect all datom rows first so we can process refs after gathering all values.
    struct DatomRow {
        ident: String,
        cardinality: String,
        value_type: String,
        component: bool,
        v_bytes: Vec<u8>,
        v_type_tag: i16,
    }

    let mut rows = Vec::new();
    for row in client.select(query, None, &[DatumWithOid::from(entity_id)])? {
        rows.push(DatomRow {
            ident: row.get(1)?.ok_or("Missing ident")?,
            cardinality: row.get(2)?.ok_or("Missing cardinality")?,
            value_type: row.get(3)?.ok_or("Missing value_type")?,
            component: row.get(4)?.unwrap_or(false),
            v_bytes: row.get(5)?.ok_or("Missing value")?,
            v_type_tag: row.get(6)?.ok_or("Missing type tag")?,
        });
    }

    for datom in &rows {
        // Skip attributes that have explicit overrides in the pattern.
        if override_idents.contains(&datom.ident) {
            continue;
        }

        if datom.v_type_tag == type_tag::REF {
            let ref_id = decode_ref_id(&datom.v_bytes)?;
            if datom.component {
                // Component ref: recursively pull all attributes of the referenced entity.
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
                // Non-component ref: return just {:db/id ref_id}.
                let ref_obj = json!({":db/id": ref_id});
                insert_value(result_map, &datom.ident, ref_obj, &datom.cardinality);
            }
        } else {
            let decoded = decode_typed_value(&datom.v_bytes, datom.v_type_tag)?;
            insert_value(result_map, &datom.ident, decoded, &datom.cardinality);
        }
    }

    Ok(())
}

/// Pull a single forward attribute.
fn pull_forward_attribute(
    client: &SpiClient<'_>,
    entity_id: i64,
    ident: &str,
    rename: Option<&str>,
    default: Option<&serde_json::Value>,
    limit: Option<&LimitSpec>,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), PullError> {
    let query = "SELECT s.cardinality::TEXT, d.v, d.value_type_tag \
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
        let v_bytes: Vec<u8> = row.get(2)?.ok_or("Missing value")?;
        let v_type_tag: i16 = row.get(3)?.ok_or("Missing type tag")?;

        let decoded = decode_typed_value(&v_bytes, v_type_tag)?;
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
            // Cycle detected: return stub.
            result_map.insert(
                output_key.to_string(),
                json!({":db/id": ref_id, "_cycle": true}),
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
                arr.push(json!({":db/id": *ref_id, "_cycle": true}));
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
    let query = "SELECT d.v \
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
        let v_bytes: Vec<u8> = row.get(1)?.ok_or("Missing value")?;
        ref_ids.push(decode_ref_id(&v_bytes)?);
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
    let entity_bytes = entity_id.to_le_bytes().to_vec();

    let query = "SELECT d.e \
                 FROM mentat.datoms d \
                 JOIN mentat.schema s ON d.a = s.entid \
                 WHERE s.ident = $1 AND d.v = $2 AND d.value_type_tag = $3 AND d.added = true";

    let max_rows = resolve_limit(limit);
    let mut ref_ids = Vec::new();

    for row in client.select(
        query,
        None,
        &[
            DatumWithOid::from(forward_ident),
            DatumWithOid::from(entity_bytes),
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
    let query = "SELECT s.ident, s.cardinality::TEXT, d.v, d.value_type_tag \
                 FROM mentat.datoms d \
                 JOIN mentat.schema s ON d.a = s.entid \
                 WHERE d.e = $1 AND d.added = true \
                 ORDER BY s.ident";

    for row in client.select(query, None, &[DatumWithOid::from(entity_id)])? {
        let ident: String = row.get(1)?.ok_or("Missing ident")?;
        let cardinality: String = row.get(2)?.ok_or("Missing cardinality")?;
        let v_bytes: Vec<u8> = row.get(3)?.ok_or("Missing value")?;
        let v_type_tag: i16 = row.get(4)?.ok_or("Missing type tag")?;

        // Skip ref attributes -- the recursive spec handles those.
        if v_type_tag == type_tag::REF {
            continue;
        }

        let decoded = decode_typed_value(&v_bytes, v_type_tag)?;
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

/// Decode a ref value (type_tag 0) to an entity ID.
fn decode_ref_id(bytes: &[u8]) -> Result<i64, PullError> {
    if bytes.len() != 8 {
        return Err(format!(":db.error/data-corruption Invalid ref value: expected 8 bytes, got {}", bytes.len()).into());
    }
    Ok(i64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

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

/// Decode a BYTEA value based on value_type_tag.
///
/// Type tags (matching encode_value in transact.rs):
///   0 = ref (i64 entity ID, little-endian)
///   1 = boolean
///   2 = long (i64 little-endian)
///   3 = double (f64 little-endian)
///   4 = instant (i64 microseconds since epoch, little-endian)
///   7 = string (UTF-8 bytes)
///   8 = keyword (UTF-8 bytes, stored without leading colon)
///  10 = uuid (16 bytes)
///  11 = bytes (raw)
fn decode_typed_value(bytes: &[u8], type_tag: i16) -> Result<serde_json::Value, PullError> {
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
            // keyword - stored without leading colon
            let s = String::from_utf8(bytes.to_vec())?;
            Ok(json!(format!(":{s}")))
        }
        10 => {
            // uuid (16 bytes)
            if bytes.len() != 16 {
                return Err(
                    format!(":db.error/data-corruption Invalid UUID value: expected 16 bytes, got {}", bytes.len()).into(),
                );
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
            ":db.error/unsupported-type Unsupported value type tag: {type_tag}. \
             Known tags: 0=ref, 1=boolean, 2=long, 3=double, 4=instant, 7=string, \
             8=keyword, 10=uuid, 11=bytes. This may indicate data corruption."
        ).into()),
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
    fn test_decode_ref_id() -> Result<(), PullError> {
        let id: i64 = 42;
        let bytes = id.to_le_bytes().to_vec();
        assert_eq!(super::decode_ref_id(&bytes)?, 42);
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
    /// Creates a graph: A→B→C→A and verifies that recursive pull:
    /// 1. Completes without infinite loop
    /// 2. Marks cycles with _cycle: true
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
                 v BYTEA NOT NULL,
                 tx BIGINT NOT NULL,
                 added BOOLEAN NOT NULL,
                 value_type_tag SMALLINT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_datoms_e ON mentat.datoms(e);
             CREATE INDEX IF NOT EXISTS idx_datoms_a ON mentat.datoms(a);
             CREATE INDEX IF NOT EXISTS idx_datoms_v ON mentat.datoms(v);",
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

        // Create circular graph: A(1000)→B(1001)→C(1002)→A(1000)
        // Entity 1000 (Alice) → friend → 1001
        // Entity 1001 (Bob) → friend → 1002
        // Entity 1002 (Carol) → friend → 1000
        let alice_name_hex = hex::encode("Alice".as_bytes());
        let bob_name_hex = hex::encode("Bob".as_bytes());
        let carol_name_hex = hex::encode("Carol".as_bytes());
        let ref_1000_hex = hex::encode(1000i64.to_le_bytes());
        let ref_1001_hex = hex::encode(1001i64.to_le_bytes());
        let ref_1002_hex = hex::encode(1002i64.to_le_bytes());

        Spi::run(&format!(
            "DELETE FROM mentat.datoms WHERE e IN (1000, 1001, 1002);
             INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag) VALUES
             (1000, 101, decode('{alice_name_hex}', 'hex'), 5000, true, 7),
             (1001, 101, decode('{bob_name_hex}', 'hex'), 5000, true, 7),
             (1002, 101, decode('{carol_name_hex}', 'hex'), 5000, true, 7),
             (1000, 100, decode('{ref_1001_hex}', 'hex'), 5000, true, 0),
             (1001, 100, decode('{ref_1002_hex}', 'hex'), 5000, true, 0),
             (1002, 100, decode('{ref_1000_hex}', 'hex'), 5000, true, 0);",
        ))?;

        // Test 1: Pull with depth 10 - should not infinite loop
        let result = Spi::get_one::<JsonB>("SELECT mentat_pull('[{:person/friend 10}]', 1000)")?;

        // Verify we got a result (didn't infinite loop)
        assert!(
            result.is_some(),
            "Pull should complete without infinite loop"
        );

        if let Some(JsonB(json_val)) = result {
            // Verify the result structure
            let obj = json_val.as_object().expect("result should be an object");

            // Should have :db/id
            assert!(obj.contains_key(":db/id"), "result should contain :db/id");

            // Should have :person/friend
            assert!(
                obj.contains_key(":person/friend"),
                "result should contain :person/friend"
            );

            // Check that a cycle marker appears somewhere in the result
            let json_str = serde_json::to_string(&json_val).unwrap();
            assert!(
                json_str.contains("_cycle"),
                "result should contain _cycle marker: {}",
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
                json_str.contains("_cycle"),
                "unbounded pull should contain _cycle marker: {}",
                json_str
            );
        }

        // Test 3: Pull with depth 1 - should get one level without cycles
        let result = Spi::get_one::<JsonB>("SELECT mentat_pull('[{:person/friend 1}]', 1000)")?;

        assert!(result.is_some(), "Depth-1 pull should complete");

        // Test 4: Verify that non-cyclic paths in the same graph work correctly
        // Create a diamond pattern: D→A, D→B, A→C, B→C
        Spi::run(&format!(
            "INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag) VALUES
             (1003, 100, decode('{ref_1000_hex}', 'hex'), 5001, true, 0),
             (1003, 100, decode('{ref_1001_hex}', 'hex'), 5001, true, 0)
             ON CONFLICT DO NOTHING;",
        ))?;

        // Pull from D - should see both A and B without cycle markers
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
                 v BYTEA NOT NULL,
                 tx BIGINT NOT NULL,
                 added BOOLEAN NOT NULL,
                 value_type_tag SMALLINT NOT NULL
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
        let ref_2000_hex = hex::encode(2000i64.to_le_bytes());
        let ref_2001_hex = hex::encode(2001i64.to_le_bytes());
        let ref_2002_hex = hex::encode(2002i64.to_le_bytes());

        Spi::run(&format!(
            "DELETE FROM mentat.datoms WHERE e IN (2000, 2001, 2002);
             INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag) VALUES
             (2000, 200, decode('{ref_2001_hex}', 'hex'), 6000, true, 0),  -- A → B
             (2000, 200, decode('{ref_2002_hex}', 'hex'), 6000, true, 0),  -- A → C
             (2001, 200, decode('{ref_2002_hex}', 'hex'), 6000, true, 0),  -- B → C
             (2001, 200, decode('{ref_2000_hex}', 'hex'), 6000, true, 0),  -- B → A (cycle)
             (2002, 200, decode('{ref_2000_hex}', 'hex'), 6000, true, 0);  -- C → A (cycle)",
        ))?;

        // Pull from A with depth 5
        let result = Spi::get_one::<JsonB>("SELECT mentat_pull('[{:person/friends 5}]', 2000)")?;

        assert!(
            result.is_some(),
            "Cardinality-many cycle detection should complete"
        );

        if let Some(JsonB(json_val)) = result {
            let json_str = serde_json::to_string(&json_val).unwrap();
            // Should contain cycle markers for the circular references
            assert!(
                json_str.contains("_cycle"),
                "many-cardinality result should contain _cycle markers: {}",
                json_str
            );
        }

        Ok(())
    }
}
