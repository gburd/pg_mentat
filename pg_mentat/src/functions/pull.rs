use crate::error::MentatError;
use crate::functions::store_management::get_schema_for_store;
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::spi::SpiClient;
use pgrx::JsonB;
use serde_json::json;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Store ID resolution
// ---------------------------------------------------------------------------

/// Get store_id from a PostgreSQL schema name (e.g. "mentat" -> 0, "mentat_foo" -> lookup).
/// This mirrors the logic in transact.rs but is local to pull.
fn get_store_id_from_schema(schema: &str) -> Result<i64, PullError> {
    let store_name = if schema == "mentat" {
        "default"
    } else if let Some(name) = schema.strip_prefix("mentat_") {
        name
    } else {
        return Err(MentatError::InvalidStoreName {
            store_name: schema.to_string(),
            reason: "Schema must be 'mentat' or 'mentat_*'".to_string(),
        }
        .into());
    };

    let store_id: Option<i64> = Spi::get_one_with_args(
        "SELECT store_id FROM mentat.stores WHERE store_name = $1",
        &[DatumWithOid::from(store_name)],
    )?;

    store_id.ok_or_else(|| {
        MentatError::InvalidStoreName {
            store_name: store_name.to_string(),
            reason: "Store not found in mentat.stores".to_string(),
        }
        .into()
    })
}

// ---------------------------------------------------------------------------
// UNION ALL query builders for type-specific tables
// ---------------------------------------------------------------------------

/// Build a UNION ALL query across all type-specific tables that returns rows
/// in the same column layout as the old wide datoms table:
///   (value_type_tag, v_ref, v_bool, v_long, v_double, v_text, v_keyword,
///    v_instant_micros, v_uuid_text, v_bytes)
///
/// Each sub-query filters by store_id and applies the given WHERE clause suffix
/// (which should reference `e`, `a`, `added` etc.).
///
/// `extra_select_prefix` allows prepending columns (e.g., "e, " for multi-entity queries).
/// `where_clause` is the filter applied to each sub-select (excluding store_id which is always added).
fn build_union_all_datoms_query(
    store_id: i64,
    extra_select_prefix: &str,
    where_clause: &str,
    order_clause: &str,
) -> String {
    // pull resolves CURRENT attribute values. In the append-only model the
    // log retains every historical assertion plus its later retraction, so a
    // `... added = true` scan of the log would return superseded and
    // retracted values. Read the current-state projection
    // (mentat.current_<type>) instead: it holds exactly the live datoms, has
    // no `added` column, and is maintained in lock-step with the log.
    // Callers still pass `added = true` for the legacy log shape, so strip
    // that predicate (invalid against the projection, which has no `added`).
    let whr = sanitize_added_predicate(where_clause);
    // Every arm's tag must be cast to ::SMALLINT. Casting only the first arm
    // does NOT force the UNION column type: Postgres resolves the common type
    // across ALL arms, and untyped integer literals elsewhere promote the
    // result to INTEGER. That produced 'integer (Oid 23) is not compatible
    // with Rust type i16' at row.get::<i16>() time in pull_wildcard and
    // friends. Cast per-arm, defensively.
    format!(
        "SELECT * FROM (\
            SELECT {pfx}{ref_tag}::SMALLINT AS value_type_tag, \
                   v AS v_ref, NULL::BOOLEAN AS v_bool, NULL::BIGINT AS v_long, \
                   NULL::DOUBLE PRECISION AS v_double, NULL::TEXT AS v_text, \
                   NULL::TEXT AS v_keyword, \
                   NULL::BIGINT AS v_instant_micros, NULL::TEXT AS v_uuid, NULL::BYTEA AS v_bytes \
            FROM mentat.current_ref \
            WHERE store_id = {sid} AND {whr} \
            UNION ALL \
            SELECT {pfx}{bool_tag}::SMALLINT, \
                   NULL, v, NULL, NULL, NULL, NULL, NULL, NULL, NULL \
            FROM mentat.current_boolean \
            WHERE store_id = {sid} AND {whr} \
            UNION ALL \
            SELECT {pfx}{long_tag}::SMALLINT, \
                   NULL, NULL, v, NULL, NULL, NULL, NULL, NULL, NULL \
            FROM mentat.current_long \
            WHERE store_id = {sid} AND {whr} \
            UNION ALL \
            SELECT {pfx}{double_tag}::SMALLINT, \
                   NULL, NULL, NULL, v, NULL, NULL, NULL, NULL, NULL \
            FROM mentat.current_double \
            WHERE store_id = {sid} AND {whr} \
            UNION ALL \
            SELECT {pfx}{instant_tag}::SMALLINT, \
                   NULL, NULL, NULL, NULL, NULL, NULL, \
                   EXTRACT(EPOCH FROM v)::BIGINT * 1000000 + \
                   EXTRACT(MICROSECOND FROM v)::BIGINT % 1000000, NULL, NULL \
            FROM mentat.current_instant \
            WHERE store_id = {sid} AND {whr} \
            UNION ALL \
            SELECT {pfx}{text_tag}::SMALLINT, \
                   NULL, NULL, NULL, NULL, v, NULL, NULL, NULL, NULL \
            FROM mentat.current_text \
            WHERE store_id = {sid} AND {whr} \
            UNION ALL \
            SELECT {pfx}{kw_tag}::SMALLINT, \
                   NULL, NULL, NULL, NULL, NULL, v, NULL, NULL, NULL \
            FROM mentat.current_keyword \
            WHERE store_id = {sid} AND {whr} \
            UNION ALL \
            SELECT {pfx}{uuid_tag}::SMALLINT, \
                   NULL, NULL, NULL, NULL, NULL, NULL, NULL, v::TEXT, NULL \
            FROM mentat.current_uuid \
            WHERE store_id = {sid} AND {whr} \
            UNION ALL \
            SELECT {pfx}{bytes_tag}::SMALLINT, \
                   NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, v \
            FROM mentat.current_bytes \
            WHERE store_id = {sid} AND {whr} \
        ) AS _datoms {order}",
        pfx = extra_select_prefix,
        sid = store_id,
        whr = whr,
        order = order_clause,
        ref_tag = type_tag::REF,
        bool_tag = type_tag::BOOLEAN,
        long_tag = type_tag::LONG,
        double_tag = type_tag::DOUBLE,
        instant_tag = type_tag::INSTANT,
        text_tag = type_tag::STRING,
        kw_tag = type_tag::KEYWORD,
        uuid_tag = type_tag::UUID,
        bytes_tag = type_tag::BYTES,
    )
}

/// Strip an `added = true` predicate (in its common forms) from a where
/// clause built for the legacy log tables, so the same clause is valid
/// against the current-state projection (which has no `added` column).
/// Leaves all other predicates intact. If removing the predicate would
/// leave an empty clause, returns `true` so the WHERE stays well-formed.
fn sanitize_added_predicate(where_clause: &str) -> String {
    let mut s = where_clause.to_string();
    for pat in [
        "",
        " AND added = true",
        "d.added = true AND ",
        "added = true AND ",
        "d.added = true",
        "added = true",
    ] {
        s = s.replace(pat, "");
    }
    let trimmed = s.trim().trim_end_matches("AND").trim_start_matches("AND").trim();
    if trimmed.is_empty() {
        "true".to_string()
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// Schema cache — loaded once per pull call to eliminate repeated SPI lookups
// ---------------------------------------------------------------------------

/// Cached schema information for an attribute, loaded in bulk.
#[derive(Debug, Clone)]
#[expect(dead_code, reason = "All fields populated during load; not all read yet")]
struct SchemaAttrInfo {
    entid: i64,
    ident: String,
    cardinality: String,
    is_component: bool,
}

/// A cache of all schema attributes, keyed by ident.
/// Loaded with a single SPI query at the start of a pull call.
struct SchemaCache {
    by_ident: HashMap<String, SchemaAttrInfo>,
    /// Reverse lookup: entid -> ident. Used to render ref values as their
    /// `:db/ident` keyword (e.g. `:db.type/ref`) instead of a raw entid,
    /// matching Datomic's `d/entity` display.
    ident_by_entid: HashMap<i64, String>,
}

impl SchemaCache {
    /// Load all schema attributes in one query from the given store schema.
    fn load(client: &SpiClient<'_>, db_schema: &str) -> Result<Self, PullError> {
        let query = format!(
            "SELECT entid, ident, cardinality::TEXT, component \
             FROM {}.schema",
            db_schema
        );
        let mut by_ident = HashMap::new();
        let mut ident_by_entid = HashMap::new();
        for row in client.select(&query, None, &[])? {
            let entid: i64 = row.get(1)?.ok_or("Missing entid in schema")?;
            let ident: String = row.get(2)?.ok_or("Missing ident in schema")?;
            let cardinality: String = row.get(3)?.ok_or("Missing cardinality in schema")?;
            let is_component: bool = row.get(4)?.unwrap_or(false);
            by_ident.insert(
                ident.clone(),
                SchemaAttrInfo { entid, ident: ident.clone(), cardinality, is_component },
            );
            ident_by_entid.insert(entid, ident);
        }
        Ok(SchemaCache { by_ident, ident_by_entid })
    }

    #[expect(dead_code, reason = "Public API for attribute lookup by ident")]
    fn get(&self, ident: &str) -> Option<&SchemaAttrInfo> {
        self.by_ident.get(ident)
    }

    fn cardinality(&self, ident: &str) -> String {
        self.by_ident
            .get(ident)
            .map(|a| a.cardinality.clone())
            .unwrap_or_else(|| "one".to_string())
    }

    /// Resolve a ref-target entid to its `:db/ident` keyword, if the target
    /// is a named schema entity. Returns `None` for refs to user-level entities
    /// (which should render as `{":db/id": N}`).
    fn ident_for_entid(&self, entid: i64) -> Option<&str> {
        self.ident_by_entid.get(&entid).map(String::as_str)
    }
}

use crate::types::constants::type_tag;

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
    pull("default", pattern, entity_id)
}

/// Pull entity data using a pull pattern from a named store.
///
/// Supports the same pull patterns as mentat_pull but operates on the specified store.
///
/// # Example
/// ```sql
/// SELECT mentat_pull_in_store('my_store', '[:person/name :person/age]', 123);
/// ```
#[pg_extern]
pub fn pull(store: &str, pattern: &str, entity_id: i64) -> Result<JsonB, PullError> {
    let db_schema = get_schema_for_store(store);
    let store_id = get_store_id_from_schema(&db_schema)?;
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
        let schema = SchemaCache::load(&client, &db_schema)?;
        execute_pull(&client, &schema, &db_schema, store_id, entity_id, &specs, &mut result_map, &mut visited, 0)
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
    pull_many("default", pattern, entity_ids)
}

/// Pull entity data for multiple entities from a named store using a pull pattern.
///
/// This is the batched counterpart to `mentat_pull_in_store`.
///
/// # Example
/// ```sql
/// SELECT mentat_pull_many_in_store('my_store', '[:person/name :person/age]', ARRAY[100, 101, 102]);
/// ```
#[pg_extern]
pub fn pull_many(store: &str, pattern: &str, entity_ids: Vec<i64>) -> Result<JsonB, PullError> {
    let db_schema = get_schema_for_store(store);
    let store_id = get_store_id_from_schema(&db_schema)?;
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
        return pull_many_batched(&db_schema, store_id, &specs, &entity_ids);
    }

    // For complex patterns, fall back to per-entity pull (still sharing one SPI connection).
    let mut results = Vec::with_capacity(entity_ids.len());

    Spi::connect(|client| {
        let schema = SchemaCache::load(&client, &db_schema)?;
        for &eid in &entity_ids {
            let mut result_map = serde_json::Map::new();
            result_map.insert(":db/id".to_string(), json!(eid));
            let mut visited = HashSet::new();
            visited.insert(eid);
            execute_pull(&client, &schema, &db_schema, store_id, eid, &specs, &mut result_map, &mut visited, 0)?;
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
///
/// Component ref attributes are automatically expanded by issuing a second
/// batched query for referenced component entities.
fn pull_many_batched(
    db_schema: &str,
    store_id: i64,
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
        let datoms_subquery = build_union_all_datoms_query(
            store_id,
            "e, a, ",
            &format!("e IN ({eid_csv}) AND added = true"),
            "",
        );
        let query = format!(
            "SELECT d.e, s.ident, s.cardinality::TEXT, s.component, d.value_type_tag, \
                    d.v_ref, d.v_bool, d.v_long, d.v_double, \
                    d.v_text, d.v_keyword, \
                    d.v_instant_micros, d.v_uuid, d.v_bytes \
             FROM ({datoms_subquery}) AS d \
             JOIN {schema}.schema s ON d.a = s.entid \
             WHERE s.ident IN ({ident_csv}) \
             ORDER BY d.e, s.ident",
            schema = db_schema
        );

        // Track component ref IDs for a second-pass expansion.
        struct PendingComponentRef {
            parent_eid: i64,
            output_key: String,
            cardinality: String,
            ref_id: i64,
        }
        let mut pending_components: Vec<PendingComponentRef> = Vec::new();

        for row in client.select(&query, None, &[])? {
            let eid: i64 = row.get(1)?.ok_or("Missing entity id")?;
            let ident: String = row.get(2)?.ok_or("Missing ident")?;
            let cardinality: String = row.get(3)?.ok_or("Missing cardinality")?;
            let is_component: bool = row.get(4)?.unwrap_or(false);
            let v_type_tag: i16 = row.get(5)?.ok_or("Missing type tag")?;

            let (decoded_val, ref_id) = decode_row_typed_value(&row, v_type_tag, 6)?;

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

            if v_type_tag == type_tag::REF {
                let rid = ref_id.ok_or("Missing ref ID")?;
                if is_component {
                    // Defer component expansion to a batched second pass.
                    pending_components.push(PendingComponentRef {
                        parent_eid: eid,
                        output_key,
                        cardinality,
                        ref_id: rid,
                    });
                } else {
                    let ref_obj = json!({":db/id": rid});
                    if let Some(result_map) = results_by_eid.get_mut(&eid) {
                        insert_value(result_map, &output_key, ref_obj, &cardinality);
                    }
                }
            } else {
                if let Some(result_map) = results_by_eid.get_mut(&eid) {
                    insert_value(result_map, &output_key, decoded_val, &cardinality);
                }
            }
        }

        // Second pass: batch-expand component refs.
        if !pending_components.is_empty() {
            let component_ids: HashSet<i64> = pending_components.iter().map(|p| p.ref_id).collect();
            let comp_eid_csv: String = component_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");

            let comp_datoms_subquery = build_union_all_datoms_query(
                store_id,
                "e, a, ",
                &format!("e IN ({comp_eid_csv}) AND added = true"),
                "",
            );
            let comp_query = format!(
                "SELECT d.e, s.ident, s.cardinality::TEXT, s.component, d.value_type_tag, \
                        d.v_ref, d.v_bool, d.v_long, d.v_double, \
                        d.v_text, d.v_keyword, \
                        d.v_instant_micros, d.v_uuid, d.v_bytes \
                 FROM ({comp_datoms_subquery}) AS d \
                 JOIN {schema}.schema s ON d.a = s.entid \
                 ORDER BY d.e, s.ident",
                schema = db_schema
            );

            // Build sub-maps for each component entity.
            let mut comp_maps: HashMap<i64, serde_json::Map<String, serde_json::Value>> = HashMap::new();
            for &cid in &component_ids {
                let mut m = serde_json::Map::new();
                m.insert(":db/id".to_string(), json!(cid));
                comp_maps.insert(cid, m);
            }

            for row in client.select(&comp_query, None, &[])? {
                let cid: i64 = row.get(1)?.ok_or("Missing entity id")?;
                let c_ident: String = row.get(2)?.ok_or("Missing ident")?;
                let c_cardinality: String = row.get(3)?.ok_or("Missing cardinality")?;
                let c_type_tag: i16 = row.get(5)?.ok_or("Missing type tag")?;
                let (c_decoded, c_ref_id) = decode_row_typed_value(&row, c_type_tag, 6)?;

                let value = if c_type_tag == type_tag::REF {
                    let rid = c_ref_id.ok_or("Missing ref ID")?;
                    json!({":db/id": rid})
                } else {
                    c_decoded
                };

                if let Some(cm) = comp_maps.get_mut(&cid) {
                    insert_value(cm, &c_ident, value, &c_cardinality);
                }
            }

            // Attach expanded component maps to parent results.
            for pc in &pending_components {
                if let Some(parent_map) = results_by_eid.get_mut(&pc.parent_eid) {
                    let comp_val = comp_maps
                        .get(&pc.ref_id)
                        .map(|m| serde_json::Value::Object(m.clone()))
                        .unwrap_or_else(|| json!({":db/id": pc.ref_id}));
                    insert_value(parent_map, &pc.output_key, comp_val, &pc.cardinality);
                }
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
///
/// Collected info for a forward attribute to be fetched in a batch.
struct ForwardAttrSpec<'a> {
    forward_ident: &'a str,
    rename: Option<&'a str>,
    default: Option<&'a serde_json::Value>,
    limit: Option<&'a LimitSpec>,
}

/// Collected info for a reverse attribute to be fetched in a batch.
struct ReverseAttrSpec<'a> {
    display_ident: &'a str,
    forward_ident: &'a str,
    rename: Option<&'a str>,
    default: Option<&'a serde_json::Value>,
    limit: Option<&'a LimitSpec>,
}

/// **Batched approach**: Instead of issuing one SPI query per attribute (N+1),
/// this function collects all simple forward attributes, fetches them in a
/// single query, and then handles complex specs (wildcard, map, recursive,
/// reverse) individually. This reduces the common case of pulling 50 simple
/// attributes from ~50 SPI calls down to 1.
fn execute_pull(
    client: &SpiClient<'_>,
    schema: &SchemaCache,
    db_schema: &str,
    store_id: i64,
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

    // -----------------------------------------------------------------------
    // Phase 1: Collect all simple forward (non-reverse) attributes and fetch
    //          them in ONE batched query.
    // -----------------------------------------------------------------------
    let mut forward_attrs: Vec<ForwardAttrSpec<'_>> = Vec::new();
    let mut reverse_attrs: Vec<ReverseAttrSpec<'_>> = Vec::new();

    for spec in specs {
        match spec {
            PullAttrSpec::Attribute {
                ident,
                reverse,
                forward_ident,
                rename,
                default,
                limit,
            } => {
                if *reverse {
                    reverse_attrs.push(ReverseAttrSpec {
                        display_ident: ident,
                        forward_ident,
                        rename: rename.as_deref(),
                        default: default.as_ref(),
                        limit: limit.as_ref(),
                    });
                } else {
                    forward_attrs.push(ForwardAttrSpec {
                        forward_ident,
                        rename: rename.as_deref(),
                        default: default.as_ref(),
                        limit: limit.as_ref(),
                    });
                }
            }
            _ => {} // Handled below
        }
    }

    // Batch-fetch all forward attributes in a single query.
    if !forward_attrs.is_empty() {
        pull_forward_attributes_batched(
            client,
            schema,
            db_schema,
            store_id,
            entity_id,
            &forward_attrs,
            result_map,
            visited,
            depth,
        )?;
    }

    // Batch-fetch all reverse attributes in a single query.
    if !reverse_attrs.is_empty() {
        pull_reverse_attributes_batched(
            client,
            schema,
            db_schema,
            store_id,
            entity_id,
            &reverse_attrs,
            result_map,
        )?;
    }

    // -----------------------------------------------------------------------
    // Phase 2: Handle wildcard, map specs, and recursive specs individually.
    //          These are less common and inherently complex; they still benefit
    //          from the schema cache eliminating per-lookup queries.
    // -----------------------------------------------------------------------
    for spec in specs {
        match spec {
            PullAttrSpec::Wildcard => {
                pull_wildcard(
                    client,
                    schema,
                    db_schema,
                    store_id,
                    entity_id,
                    result_map,
                    &override_idents,
                    visited,
                    depth,
                )?;
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
                        schema,
                        db_schema,
                        store_id,
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
                        schema,
                        db_schema,
                        store_id,
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
                    schema,
                    db_schema,
                    store_id,
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
            // Attributes already handled in Phase 1.
            PullAttrSpec::Attribute { .. } => {}
        }
    }

    Ok(())
}

/// Fetch ALL forward attributes for a single entity in ONE query.
///
/// This is the core N+1 fix: instead of issuing one SPI call per attribute,
/// we build a single `WHERE e = $1 AND s.ident IN (...)` query that returns
/// all requested attribute values at once. The results are then distributed
/// to the appropriate output keys.
fn pull_forward_attributes_batched(
    client: &SpiClient<'_>,
    schema: &SchemaCache,
    db_schema: &str,
    store_id: i64,
    entity_id: i64,
    attrs: &[ForwardAttrSpec<'_>],
    result_map: &mut serde_json::Map<String, serde_json::Value>,
    visited: &mut HashSet<i64>,
    depth: usize,
) -> Result<(), PullError> {
    if attrs.is_empty() {
        return Ok(());
    }

    // Build the IN clause for idents (escape single quotes for safety).
    let ident_csv: String = attrs
        .iter()
        .map(|a| format!("'{}'", a.forward_ident.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");

    let datoms_subquery = build_union_all_datoms_query(
        store_id,
        "a, ",
        &format!("e = {} AND added = true", entity_id),
        "",
    );
    let query = format!(
        "SELECT s.ident, s.cardinality::TEXT, s.component, d.value_type_tag, \
                d.v_ref, d.v_bool, d.v_long, d.v_double, \
                d.v_text, d.v_keyword, \
                d.v_instant_micros, d.v_uuid, d.v_bytes \
         FROM ({datoms_subquery}) AS d \
         JOIN {schema}.schema s ON d.a = s.entid \
         WHERE s.ident IN ({ident_csv}) \
         ORDER BY s.ident",
        schema = db_schema
    );

    // Build a lookup from forward_ident -> spec for output key resolution and limits.
    let mut spec_by_ident: HashMap<&str, &ForwardAttrSpec<'_>> = HashMap::new();
    for attr in attrs {
        spec_by_ident.insert(attr.forward_ident, attr);
    }

    // Track which idents we found, and per-ident row counts for limit enforcement.
    let mut found_idents: HashSet<String> = HashSet::new();
    let mut ident_counts: HashMap<String, usize> = HashMap::new();

    // Collect rows that need component expansion (deferred to avoid nested SPI issues).
    struct PendingComponentRef {
        output_key: String,
        cardinality: String,
        ref_id: i64,
    }
    let mut pending_components: Vec<PendingComponentRef> = Vec::new();

    for row in client.select(&query, None, &[])? {
        let ident: String = row.get(1)?.ok_or("Missing ident")?;
        let cardinality: String = row.get(2)?.ok_or("Missing cardinality")?;
        let is_component: bool = row.get(3)?.unwrap_or(false);
        let v_type_tag: i16 = row.get(4)?.ok_or("Missing type tag")?;

        // Enforce per-attribute limit.
        let spec = spec_by_ident.get(ident.as_str());
        let max_rows = spec.map(|s| resolve_limit(s.limit)).unwrap_or(DEFAULT_MANY_LIMIT);
        let count = ident_counts.entry(ident.clone()).or_insert(0);
        if *count >= max_rows {
            continue;
        }
        *count += 1;

        found_idents.insert(ident.clone());

        let (decoded_val, ref_id) = decode_row_typed_value(&row, v_type_tag, 5)?;

        // Determine the output key (handle :as renames).
        let output_key = spec
            .and_then(|s| s.rename)
            .unwrap_or(&ident)
            .to_string();

        if v_type_tag == type_tag::REF {
            let rid = ref_id.ok_or("Missing ref ID")?;
            if is_component && depth < MAX_RECURSION_DEPTH && !visited.contains(&rid) {
                // Defer component expansion.
                pending_components.push(PendingComponentRef {
                    output_key,
                    cardinality,
                    ref_id: rid,
                });
            } else {
                let ref_obj = json!({":db/id": rid});
                insert_value(result_map, &output_key, ref_obj, &cardinality);
            }
        } else {
            insert_value(result_map, &output_key, decoded_val, &cardinality);
        }
    }

    // Expand component refs (recursive wildcard pull).
    for pc in pending_components {
        let mut sub_map = serde_json::Map::new();
        sub_map.insert(":db/id".to_string(), json!(pc.ref_id));
        visited.insert(pc.ref_id);
        pull_wildcard(
            client,
            schema,
            db_schema,
            store_id,
            pc.ref_id,
            &mut sub_map,
            &HashSet::new(),
            visited,
            depth + 1,
        )?;
        let value = serde_json::Value::Object(sub_map);
        insert_value(result_map, &pc.output_key, value, &pc.cardinality);
    }

    // Apply defaults for missing attributes.
    for attr in attrs {
        if !found_idents.contains(attr.forward_ident) {
            if let Some(def) = attr.default {
                let output_key = attr.rename.unwrap_or(attr.forward_ident);
                result_map.insert(output_key.to_string(), def.clone());
            }
        }
    }

    Ok(())
}

/// Fetch ALL reverse attributes for a single entity in ONE query.
///
/// Instead of issuing one SPI call per reverse attribute, this collects all
/// forward idents for reverse lookups and issues a single query.
fn pull_reverse_attributes_batched(
    client: &SpiClient<'_>,
    _schema: &SchemaCache,
    db_schema: &str,
    store_id: i64,
    entity_id: i64,
    attrs: &[ReverseAttrSpec<'_>],
    result_map: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), PullError> {
    if attrs.is_empty() {
        return Ok(());
    }

    // Build the IN clause for forward idents.
    let ident_csv: String = attrs
        .iter()
        .map(|a| format!("'{}'", a.forward_ident.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");

    // Reverse lookups are always ref-type, so query only datoms_ref_new.
    let query = format!(
        "SELECT s.ident, d.e \
         FROM mentat.current_ref d \
         JOIN {schema}.schema s ON d.a = s.entid \
         WHERE d.store_id = {store_id} AND s.ident IN ({ident_csv}) \
               AND d.v = {entity_id} \
         ORDER BY s.ident, d.e",
        schema = db_schema,
        store_id = store_id,
        entity_id = entity_id,
    );

    // Build lookup for limit/rename per forward_ident.
    let mut spec_by_ident: HashMap<&str, &ReverseAttrSpec<'_>> = HashMap::new();
    for attr in attrs {
        spec_by_ident.insert(attr.forward_ident, attr);
    }

    // Collect results grouped by ident.
    let mut refs_by_ident: HashMap<String, Vec<i64>> = HashMap::new();
    let mut ident_counts: HashMap<String, usize> = HashMap::new();

    for row in client.select(&query, None, &[])? {
        let ident: String = row.get(1)?.ok_or("Missing ident")?;
        let e: i64 = row.get(2)?.ok_or("Missing entity")?;

        let spec = spec_by_ident.get(ident.as_str());
        let max_rows = spec.map(|s| resolve_limit(s.limit)).unwrap_or(DEFAULT_MANY_LIMIT);
        let count = ident_counts.entry(ident.clone()).or_insert(0);
        if *count >= max_rows {
            continue;
        }
        *count += 1;
        refs_by_ident.entry(ident).or_default().push(e);
    }

    // Insert results into the output map.
    for attr in attrs {
        let output_key = attr.rename.unwrap_or(attr.display_ident);
        if let Some(ref_ids) = refs_by_ident.get(attr.forward_ident) {
            let arr: Vec<serde_json::Value> =
                ref_ids.iter().map(|id| json!({":db/id": *id})).collect();
            result_map.insert(output_key.to_string(), json!(arr));
        } else if let Some(def) = attr.default {
            result_map.insert(output_key.to_string(), def.clone());
        }
    }

    Ok(())
}

/// Pull all attributes for an entity (wildcard).
/// For ref attributes: non-component refs return just {:db/id N},
/// component refs recursively pull all nested attributes.
fn pull_wildcard(
    client: &SpiClient<'_>,
    schema: &SchemaCache,
    db_schema: &str,
    store_id: i64,
    entity_id: i64,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
    override_idents: &HashSet<String>,
    visited: &mut HashSet<i64>,
    depth: usize,
) -> Result<(), PullError> {
    let datoms_subquery = build_union_all_datoms_query(
        store_id,
        "a, ",
        &format!("e = {} AND added = true", entity_id),
        "",
    );
    let query = format!(
        "SELECT s.ident, s.cardinality::TEXT, s.value_type::TEXT, s.component, \
                d.value_type_tag, d.v_ref, d.v_bool, d.v_long, d.v_double, \
                d.v_text, d.v_keyword, \
                d.v_instant_micros, d.v_uuid, d.v_bytes \
         FROM ({datoms_subquery}) AS d \
         JOIN {schema}.schema s ON d.a = s.entid \
         WHERE true \
         ORDER BY s.ident",
        schema = db_schema
    );

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
    for row in client.select(&query, None, &[])? {
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
                        schema,
                        db_schema,
                        store_id,
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
                // If the ref target is a named schema entity (has `:db/ident`),
                // render its keyword instead of the raw entid. Matches Datomic's
                // `d/entity` display and makes `:db/valueType` look like
                // `:db.type/ref` rather than the integer `70`.
                let value = match schema.ident_for_entid(ref_id) {
                    Some(ident) => json!(ident),
                    None => json!({":db/id": ref_id}),
                };
                insert_value(result_map, &datom.ident, value, &datom.cardinality);
            }
        } else {
            insert_value(result_map, &datom.ident, datom.decoded.clone(), &datom.cardinality);
        }
    }

    Ok(())
}

/// Pull a forward map spec: follow ref values and recursively pull sub-pattern.
fn pull_forward_map_spec(
    client: &SpiClient<'_>,
    schema: &SchemaCache,
    db_schema: &str,
    store_id: i64,
    entity_id: i64,
    ident: &str,
    sub_pattern: &[PullAttrSpec],
    rename: Option<&str>,
    limit: Option<&LimitSpec>,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
    visited: &mut HashSet<i64>,
    depth: usize,
) -> Result<(), PullError> {
    let ref_ids = query_forward_refs(client, db_schema, store_id, entity_id, ident, limit)?;
    let output_key = rename.unwrap_or(ident);

    if ref_ids.is_empty() {
        return Ok(());
    }

    let cardinality = schema.cardinality(ident);

    if cardinality == "one" {
        // Cardinality one: return a single map.
        let ref_id = ref_ids[0];
        let mut sub_map = serde_json::Map::new();
        sub_map.insert(":db/id".to_string(), json!(ref_id));
        let was_new = visited.insert(ref_id);
        if depth < MAX_RECURSION_DEPTH {
            execute_pull(
                client,
                schema,
                db_schema,
                store_id,
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
                    schema,
                    db_schema,
                    store_id,
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
    schema: &SchemaCache,
    db_schema: &str,
    store_id: i64,
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
    let ref_ids = query_reverse_refs(client, db_schema, store_id, entity_id, forward_ident, limit)?;
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
                schema,
                db_schema,
                store_id,
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
    schema: &SchemaCache,
    db_schema: &str,
    store_id: i64,
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
        query_reverse_refs(client, db_schema, store_id, entity_id, forward_ident, None)?
    } else {
        query_forward_refs(client, db_schema, store_id, entity_id, forward_ident, None)?
    };

    let output_key = rename.unwrap_or(display_ident);

    if ref_ids.is_empty() {
        return Ok(());
    }

    let cardinality = if reverse {
        // Reverse lookups are always multi-valued.
        "many".to_string()
    } else {
        schema.cardinality(forward_ident)
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
            // Pull all attributes of the target (excluding the recursive one), plus recurse.
            pull_all_attributes_for_recursive(
                client, schema, db_schema, store_id, ref_id, forward_ident, &mut sub_map, visited, current_depth + 1,
            )?;
            execute_pull(
                client,
                schema,
                db_schema,
                store_id,
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
                pull_all_attributes_for_recursive(
                    client, schema, db_schema, store_id, *ref_id, forward_ident, &mut sub_map, visited, current_depth + 1,
                )?;
                execute_pull(
                    client,
                    schema,
                    db_schema,
                    store_id,
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
    db_schema: &str,
    store_id: i64,
    entity_id: i64,
    ident: &str,
    limit: Option<&LimitSpec>,
) -> Result<Vec<i64>, PullError> {
    // Forward refs only come from datoms_ref_new.
    let escaped_ident = ident.replace('\'', "''");
    let query = format!(
        "SELECT d.v \
         FROM mentat.current_ref d \
         JOIN {schema}.schema s ON d.a = s.entid \
         WHERE d.store_id = {store_id} AND d.e = {entity_id} \
               AND s.ident = '{escaped_ident}'",
        schema = db_schema,
        store_id = store_id,
        entity_id = entity_id,
    );

    let max_rows = resolve_limit(limit);
    let mut ref_ids = Vec::new();

    for row in client.select(&query, None, &[])? {
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
    db_schema: &str,
    store_id: i64,
    entity_id: i64,
    forward_ident: &str,
    limit: Option<&LimitSpec>,
) -> Result<Vec<i64>, PullError> {
    // Reverse refs only come from datoms_ref_new.
    let escaped_ident = forward_ident.replace('\'', "''");
    let query = format!(
        "SELECT d.e \
         FROM mentat.current_ref d \
         JOIN {schema}.schema s ON d.a = s.entid \
         WHERE d.store_id = {store_id} AND s.ident = '{escaped_ident}' \
               AND d.v = {entity_id}",
        schema = db_schema,
        store_id = store_id,
        entity_id = entity_id,
    );

    let max_rows = resolve_limit(limit);
    let mut ref_ids = Vec::new();

    for row in client.select(&query, None, &[])? {
        if ref_ids.len() >= max_rows {
            break;
        }
        let e: i64 = row.get(1)?.ok_or("Missing entity")?;
        ref_ids.push(e);
    }

    Ok(ref_ids)
}

/// Batch query forward ref values for multiple entities at once.
/// Returns a map from source entity ID to the list of referenced entity IDs.
///
/// This eliminates the N+1 problem when following refs across many entities,
/// e.g., for map specs in `pull_many`.
#[expect(dead_code)]
fn query_forward_refs_batched(
    client: &SpiClient<'_>,
    db_schema: &str,
    store_id: i64,
    entity_ids: &[i64],
    ident: &str,
    limit: Option<&LimitSpec>,
) -> Result<HashMap<i64, Vec<i64>>, PullError> {
    if entity_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let max_rows = resolve_limit(limit);
    let eid_csv: String = entity_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
    let escaped_ident = ident.replace('\'', "''");

    // Batched forward refs only come from datoms_ref_new.
    let query = format!(
        "SELECT d.e, d.v \
         FROM mentat.current_ref d \
         JOIN {schema}.schema s ON d.a = s.entid \
         WHERE d.store_id = {store_id} AND d.e IN ({eid_csv}) \
               AND s.ident = '{escaped_ident}' \
         ORDER BY d.e",
        schema = db_schema,
        store_id = store_id,
    );

    let mut result: HashMap<i64, Vec<i64>> = HashMap::new();
    for row in client.select(&query, None, &[])? {
        let eid: i64 = row.get(1)?.ok_or("Missing entity id")?;
        let ref_id: i64 = row.get(2)?.ok_or("Missing v_ref")?;
        let entry = result.entry(eid).or_default();
        if entry.len() < max_rows {
            entry.push(ref_id);
        }
    }

    Ok(result)
}

/// Pull all attributes for an entity during recursive pulls.
///
/// Non-ref attributes are included directly. Ref attributes that are NOT the
/// recursive attribute itself are included as `{:db/id N}` stubs (or
/// recursively expanded if the attribute is `:db/isComponent`). The recursive
/// attribute is excluded because the caller handles it via its own
/// `RecursiveSpec`.
fn pull_all_attributes_for_recursive(
    client: &SpiClient<'_>,
    schema: &SchemaCache,
    db_schema: &str,
    store_id: i64,
    entity_id: i64,
    recursive_forward_ident: &str,
    result_map: &mut serde_json::Map<String, serde_json::Value>,
    visited: &mut HashSet<i64>,
    depth: usize,
) -> Result<(), PullError> {
    let datoms_subquery = build_union_all_datoms_query(
        store_id,
        "a, ",
        &format!("e = {} AND added = true", entity_id),
        "",
    );
    let query = format!(
        "SELECT s.ident, s.cardinality::TEXT, s.component, d.value_type_tag, \
                d.v_ref, d.v_bool, d.v_long, d.v_double, \
                d.v_text, d.v_keyword, \
                d.v_instant_micros, d.v_uuid, d.v_bytes \
         FROM ({datoms_subquery}) AS d \
         JOIN {schema}.schema s ON d.a = s.entid \
         WHERE true \
         ORDER BY s.ident",
        schema = db_schema
    );

    // Collect rows first so we can process component refs after releasing the cursor.
    struct AttrRow {
        ident: String,
        cardinality: String,
        is_component: bool,
        v_type_tag: i16,
        decoded: serde_json::Value,
        ref_id: Option<i64>,
    }

    let mut rows = Vec::new();
    for row in client.select(&query, None, &[])? {
        let ident: String = row.get(1)?.ok_or("Missing ident")?;
        let cardinality: String = row.get(2)?.ok_or("Missing cardinality")?;
        let is_component: bool = row.get(3)?.unwrap_or(false);
        let v_type_tag: i16 = row.get(4)?.ok_or("Missing type tag")?;
        let (decoded, ref_id) = decode_row_typed_value(&row, v_type_tag, 5)?;
        rows.push(AttrRow { ident, cardinality, is_component, v_type_tag, decoded, ref_id });
    }

    for attr_row in &rows {
        // Skip the recursive attribute -- the caller handles it.
        if attr_row.ident == recursive_forward_ident {
            continue;
        }

        if attr_row.v_type_tag == type_tag::REF {
            let rid = attr_row.ref_id.ok_or("Missing ref ID")?;
            if attr_row.is_component && depth < MAX_RECURSION_DEPTH && !visited.contains(&rid) {
                // Component ref: recursively pull all attributes.
                let mut sub_map = serde_json::Map::new();
                sub_map.insert(":db/id".to_string(), json!(rid));
                visited.insert(rid);
                pull_wildcard(client, schema, db_schema, store_id, rid, &mut sub_map, &HashSet::new(), visited, depth + 1)?;
                let value = serde_json::Value::Object(sub_map);
                insert_value(result_map, &attr_row.ident, value, &attr_row.cardinality);
            } else {
                // Non-component ref: return as {:db/id N}.
                let ref_obj = json!({":db/id": rid});
                insert_value(result_map, &attr_row.ident, ref_obj, &attr_row.cardinality);
            }
        } else {
            insert_value(result_map, &attr_row.ident, attr_row.decoded.clone(), &attr_row.cardinality);
        }
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
        crate::ensure_extension_loaded();
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
        crate::ensure_extension_loaded();
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
        crate::ensure_extension_loaded();
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
        crate::ensure_extension_loaded();
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
        crate::ensure_extension_loaded();
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
        crate::ensure_extension_loaded();
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
        crate::ensure_extension_loaded();
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
        crate::ensure_extension_loaded();
        let mut map = serde_json::Map::new();
        insert_value(&mut map, ":name", json!("Alice"), "one");
        assert_eq!(map.get(":name"), Some(&json!("Alice")));
        // Second insert overwrites.
        insert_value(&mut map, ":name", json!("Bob"), "one");
        assert_eq!(map.get(":name"), Some(&json!("Bob")));
    }

    #[pg_test]
    fn test_insert_value_cardinality_many() {
        crate::ensure_extension_loaded();
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
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;

        // Define the ref + string attributes through the real transact path so
        // the schema cache, idents, narrow tables and current-state projection
        // all stay consistent (mentat.datoms is a VIEW now -- raw INSERT/INDEX
        // against it is no longer valid).
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :person/friend :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )?;

        // Create circular graph: A(1000)->B(1001)->C(1002)->A(1000) using
        // explicit entity IDs.
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add 1000 :person/name \"Alice\"]
                [:db/add 1001 :person/name \"Bob\"]
                [:db/add 1002 :person/name \"Carol\"]
                [:db/add 1000 :person/friend 1001]
                [:db/add 1001 :person/friend 1002]
                [:db/add 1002 :person/friend 1000]
            ]'::TEXT)",
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
            "SELECT mentat_transact('[
                [:db/add 1003 :person/friend 1000]
            ]'::TEXT)",
        )?;

        let result = Spi::get_one::<JsonB>("SELECT mentat_pull('[{:person/friend 5}]', 1003)")?;
        assert!(result.is_some(), "Diamond pattern pull should complete");

        Ok(())
    }

    /// Test that cycle detection works with cardinality-many attributes.
    #[pg_test]
    fn test_recursive_pull_many_cycles() -> spi::Result<()> {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;

        // :person/friends is a cardinality-many ref.
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :person/friends :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        )?;

        // Create graph with multiple cycles:
        // A has friends [B, C]
        // B has friends [C, A] (cycle to A)
        // C has friends [A]     (cycle to A)
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add 2000 :person/friends 2001]
                [:db/add 2000 :person/friends 2002]
                [:db/add 2001 :person/friends 2002]
                [:db/add 2001 :person/friends 2000]
                [:db/add 2002 :person/friends 2000]
            ]'::TEXT)",
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
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;

        // Schema: :order/items is a component ref (many), :item/name is a string,
        // :item/qty is a long, :order/name is a string.
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :order/items :db/valueType :db.type/ref :db/cardinality :db.cardinality/many :db/isComponent true}
                {:db/ident :item/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/ident :item/qty :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/ident :order/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )?;

        // Order 3000 has two line items: 3001 and 3002
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add 3000 :order/name \"Order-1\"]
                [:db/add 3001 :item/name \"Widget\"]
                [:db/add 3002 :item/name \"Gadget\"]
                [:db/add 3001 :item/qty 5]
                [:db/add 3002 :item/qty 3]
                [:db/add 3000 :order/items 3001]
                [:db/add 3000 :order/items 3002]
            ]'::TEXT)",
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
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;

        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/ident :person/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )?;

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add 4000 :person/name \"Alice\"]
                [:db/add 4001 :person/name \"Bob\"]
                [:db/add 4002 :person/name \"Carol\"]
                [:db/add 4000 :person/age 30]
                [:db/add 4001 :person/age 25]
                [:db/add 4002 :person/age 35]
            ]'::TEXT)",
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
        crate::ensure_extension_loaded();
        let result = mentat_pull_many("[:person/name]", vec![])?;
        let arr = result.0.as_array().expect("should be array");
        assert!(arr.is_empty(), "empty input should produce empty output");
        Ok(())
    }

    /// Test recursive pull with mixed attributes: name + recursive friends.
    ///
    /// Pattern: `[:person/name {:person/friend ...}]`
    /// Verifies that at each level of recursion, the entity's :person/name
    /// attribute is included alongside the recursive :person/friend traversal.
    #[pg_test]
    fn test_recursive_pull_with_mixed_attrs() -> spi::Result<()> {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;

        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :person/friend :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )?;

        // Chain: Alice -> Bob -> Carol (no cycle)
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add 5000 :person/name \"Alice\"]
                [:db/add 5001 :person/name \"Bob\"]
                [:db/add 5002 :person/name \"Carol\"]
                [:db/add 5000 :person/friend 5001]
                [:db/add 5001 :person/friend 5002]
            ]'::TEXT)",
        )?;

        let result = Spi::get_one::<JsonB>(
            "SELECT mentat_pull('[{:person/friend ...}]', 5000)",
        )?;

        assert!(result.is_some(), "Mixed recursive pull should succeed");
        if let Some(JsonB(json_val)) = result {
            let obj = json_val.as_object().expect("root should be object");
            assert_eq!(obj.get(":db/id"), Some(&json!(5000)));

            // The pattern [{:person/friend ...}] requests only :person/friend at
            // the root, so the root carries :db/id + :person/friend and not
            // :person/name. Recursed children, however, are expanded with all of
            // their attributes (this is what test_recursive_bounded_depth also
            // relies on).
            assert!(
                obj.get(":person/name").is_none(),
                "Root should not carry :person/name for a bare recursive spec, got: {:?}",
                obj
            );

            // Follow to Bob
            let bob = obj.get(":person/friend").expect("should have :person/friend");
            let bob_obj = bob.as_object().expect("friend should be object");
            assert_eq!(bob_obj.get(":db/id"), Some(&json!(5001)));
            assert_eq!(
                bob_obj.get(":person/name"),
                Some(&json!("Bob")),
                "Bob should have :person/name"
            );

            // Follow to Carol
            let carol = bob_obj.get(":person/friend").expect("Bob should have :person/friend");
            let carol_obj = carol.as_object().expect("Carol should be object");
            assert_eq!(carol_obj.get(":db/id"), Some(&json!(5002)));
            assert_eq!(
                carol_obj.get(":person/name"),
                Some(&json!("Carol")),
                "Carol should have :person/name"
            );
        }

        Ok(())
    }

    /// Test bounded recursion depth limit.
    ///
    /// Pattern: `{:person/friend 2}` on chain A -> B -> C -> D
    /// Should stop at depth 2, so D should not appear.
    #[pg_test]
    fn test_recursive_bounded_depth() -> spi::Result<()> {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;

        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :node/next :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/ident :node/label :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )?;

        // Chain: N0 -> N1 -> N2 -> N3
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add 6000 :node/label \"N0\"]
                [:db/add 6001 :node/label \"N1\"]
                [:db/add 6002 :node/label \"N2\"]
                [:db/add 6003 :node/label \"N3\"]
                [:db/add 6000 :node/next 6001]
                [:db/add 6001 :node/next 6002]
                [:db/add 6002 :node/next 6003]
            ]'::TEXT)",
        )?;

        // Depth 2: should get N0 -> N1 -> N2, but N2 should NOT have :node/next
        let result = Spi::get_one::<JsonB>(
            "SELECT mentat_pull('[{:node/next 2}]', 6000)",
        )?;

        assert!(result.is_some(), "Bounded depth pull should succeed");
        if let Some(JsonB(json_val)) = result {
            let n0 = json_val.as_object().expect("root should be object");
            assert_eq!(n0.get(":db/id"), Some(&json!(6000)));

            let n1 = n0.get(":node/next")
                .expect("N0 should have :node/next")
                .as_object()
                .expect("N1 should be object");
            assert_eq!(n1.get(":db/id"), Some(&json!(6001)));
            assert_eq!(n1.get(":node/label"), Some(&json!("N1")));

            let n2 = n1.get(":node/next")
                .expect("N1 should have :node/next")
                .as_object()
                .expect("N2 should be object");
            assert_eq!(n2.get(":db/id"), Some(&json!(6002)));
            assert_eq!(n2.get(":node/label"), Some(&json!("N2")));

            // N2 should NOT have :node/next because we hit depth limit 2.
            assert!(
                n2.get(":node/next").is_none(),
                "N2 should NOT have :node/next at depth 2, got: {:?}",
                n2
            );
        }

        Ok(())
    }

    /// Test reverse recursive pulls.
    ///
    /// Pattern: `{:person/_friend ...}` follows reverse refs recursively.
    #[pg_test]
    fn test_reverse_recursive_pull() -> spi::Result<()> {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;

        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :mgr/reports-to :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/ident :mgr/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )?;

        // Org chart: Employee(7002) reports-to Manager(7001) reports-to VP(7000)
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add 7000 :mgr/name \"VP\"]
                [:db/add 7001 :mgr/name \"Manager\"]
                [:db/add 7002 :mgr/name \"Employee\"]
                [:db/add 7001 :mgr/reports-to 7000]
                [:db/add 7002 :mgr/reports-to 7001]
            ]'::TEXT)",
        )?;

        // Reverse recursive: from VP, find who reports to them recursively
        let result = Spi::get_one::<JsonB>(
            "SELECT mentat_pull('[{:mgr/_reports-to 3}]', 7000)",
        )?;

        assert!(result.is_some(), "Reverse recursive pull should succeed");
        if let Some(JsonB(json_val)) = result {
            let vp = json_val.as_object().expect("root should be object");
            assert_eq!(vp.get(":db/id"), Some(&json!(7000)));

            // VP should have reverse lookup results
            let reports = vp.get(":mgr/_reports-to")
                .expect("VP should have :mgr/_reports-to");
            let reports_arr = reports.as_array()
                .expect("reverse recursive should produce array");
            assert!(!reports_arr.is_empty(), "VP should have at least one report");

            // Find the manager in the reports
            let manager = reports_arr.iter()
                .find(|r| r.get(":db/id") == Some(&json!(7001)))
                .expect("Manager should appear in VP's reports");
            let manager_obj = manager.as_object().expect("manager should be object");
            assert_eq!(
                manager_obj.get(":mgr/name"),
                Some(&json!("Manager")),
                "Manager should have their name"
            );
        }

        Ok(())
    }

    /// Test component with recursive combined: component attribute within a recursively-pulled entity.
    ///
    /// Setup: Person has :person/friend (recursive) and :person/address (component).
    /// Pattern: `{:person/friend ...}` should at each level include addresses as expanded components.
    #[pg_test]
    fn test_component_within_recursive_pull() -> spi::Result<()> {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;

        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :person/friend :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/ident :person/address :db/valueType :db.type/ref :db/cardinality :db.cardinality/one :db/isComponent true}
                {:db/ident :address/city :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )?;

        // Alice(8000) -> addr(8010, city=NYC)
        // Alice friends Bob(8001) -> addr(8011, city=LA)
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add 8000 :person/name \"Alice\"]
                [:db/add 8001 :person/name \"Bob\"]
                [:db/add 8010 :address/city \"NYC\"]
                [:db/add 8011 :address/city \"LA\"]
                [:db/add 8000 :person/friend 8001]
                [:db/add 8000 :person/address 8010]
                [:db/add 8001 :person/address 8011]
            ]'::TEXT)",
        )?;

        let result = Spi::get_one::<JsonB>(
            "SELECT mentat_pull('[{:person/friend ...}]', 8000)",
        )?;

        assert!(result.is_some(), "Component+recursive pull should succeed");
        if let Some(JsonB(json_val)) = result {
            let alice = json_val.as_object().expect("root should be object");
            assert_eq!(alice.get(":db/id"), Some(&json!(8000)));

            // The bare recursive spec [{:person/friend ...}] requests only
            // :person/friend at the root, so the root is not auto-expanded with
            // its other attributes (:person/name, :person/address). The
            // component expansion happens on the recursed child below.
            assert!(
                alice.get(":person/name").is_none(),
                "Root should not carry :person/name for a bare recursive spec, got: {:?}",
                alice
            );

            // Bob is a recursed child, so he is expanded with all attributes,
            // and his component :person/address is fully expanded.
            let bob = alice.get(":person/friend")
                .expect("Alice should have :person/friend");
            let bob_obj = bob.as_object().expect("Bob should be object");
            assert_eq!(bob_obj.get(":person/name"), Some(&json!("Bob")));

            let bob_addr = bob_obj.get(":person/address")
                .expect("Bob should have :person/address");
            let bob_addr_obj = bob_addr.as_object()
                .expect("Bob's address should be object (component expanded)");
            assert_eq!(
                bob_addr_obj.get(":address/city"),
                Some(&json!("LA")),
                "Bob's address should have city=LA"
            );
        }

        Ok(())
    }

    /// Test pull_many with recursive map specs.
    ///
    /// Verifies that mentat_pull_many correctly handles map specs
    /// by falling back to per-entity pulls.
    #[pg_test]
    fn test_pull_many_with_map_spec() -> spi::Result<()> {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;

        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :team/members :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/ident :team/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )?;

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add 9000 :team/name \"Alpha\"]
                [:db/add 9001 :team/name \"Beta\"]
                [:db/add 9100 :person/name \"Alice\"]
                [:db/add 9101 :person/name \"Bob\"]
                [:db/add 9000 :team/members 9100]
                [:db/add 9001 :team/members 9101]
            ]'::TEXT)",
        )?;

        let result = Spi::get_one::<JsonB>(
            "SELECT mentat_pull_many('[:team/name {:team/members [:person/name]}]', ARRAY[9000, 9001])",
        )?;

        assert!(result.is_some(), "Pull many with map spec should succeed");
        if let Some(JsonB(json_val)) = result {
            let arr = json_val.as_array().expect("result should be array");
            assert_eq!(arr.len(), 2, "should have 2 teams");

            let alpha = arr[0].as_object().expect("first should be object");
            assert_eq!(alpha.get(":team/name"), Some(&json!("Alpha")));
            let members = alpha.get(":team/members")
                .expect("Alpha should have members")
                .as_array()
                .expect("members should be array");
            assert_eq!(members.len(), 1);
            let alice = members[0].as_object().expect("member should be object");
            assert_eq!(alice.get(":person/name"), Some(&json!("Alice")));

            let beta = arr[1].as_object().expect("second should be object");
            assert_eq!(beta.get(":team/name"), Some(&json!("Beta")));
        }

        Ok(())
    }

    /// Test that self-referencing entity (points to itself) terminates.
    #[pg_test]
    fn test_recursive_self_reference() -> spi::Result<()> {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;

        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :node/self :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/ident :node/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )?;

        // Entity 10000 points to itself
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add 10000 :node/val 42]
                [:db/add 10000 :node/self 10000]
            ]'::TEXT)",
        )?;

        let result = Spi::get_one::<JsonB>(
            "SELECT mentat_pull('[{:node/self ...}]', 10000)",
        )?;

        assert!(result.is_some(), "Self-referencing pull should complete");
        if let Some(JsonB(json_val)) = result {
            let obj = json_val.as_object().expect("root should be object");
            assert_eq!(obj.get(":db/id"), Some(&json!(10000)));
            assert_eq!(obj.get(":node/val"), Some(&json!(42)));

            // The :node/self should be a {:db/id 10000} stub (cycle)
            let self_ref = obj.get(":node/self")
                .expect("should have :node/self");
            let self_obj = self_ref.as_object()
                .expect(":node/self should be an object");
            assert_eq!(
                self_obj.get(":db/id"),
                Some(&json!(10000)),
                "Self-reference should be a {{:db/id}} stub"
            );
        }

        Ok(())
    }
}
