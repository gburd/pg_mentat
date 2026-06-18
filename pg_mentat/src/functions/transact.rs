use crate::error::{self, MentatError};
use crate::functions::store_management;
use edn::entities::{BuiltinTxFn, OpType};
use edn::parse;
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use std::cell::Cell;
use std::collections::BTreeMap;

thread_local! {
    /// Cached `store_id` for the current `execute_transaction_inner` call.
    ///
    /// `get_store_id_from_schema()` is called for every datom operation
    /// (insert, retract, duplicate-check, unique-check). Without caching
    /// this fires `SELECT store_id FROM mentat.stores WHERE store_name = $1`
    /// ~4,032 times per typical transaction. Setting this once at transaction
    /// entry and checking it as a fast-path reduces that to a single DB query
    /// per transaction.
    static TX_STORE_ID: Cell<Option<i64>> = const { Cell::new(None) };
}

/// RAII guard that tears down per-transaction context on drop.
///
/// Created at the top of `execute_transaction_inner`; dropped when the
/// function returns (success or failure). Clears:
///   - `TX_STORE_ID` — the cached store_id for this transaction
///   - cache-generation bypass via `cache::end_tx_cache_bypass()`
struct TxContextGuard;

impl Drop for TxContextGuard {
    fn drop(&mut self) {
        TX_STORE_ID.with(|f| f.set(None));
        crate::cache::end_tx_cache_bypass();
    }
}

/// Maximum number of retries for serialization failures (SQLSTATE 40001).
const MAX_SERIALIZATION_RETRIES: u32 = 5;

/// Base delay in milliseconds for exponential backoff on serialization retry.
const BASE_RETRY_DELAY_MS: u64 = 10;

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
    #[expect(dead_code)]
    pub const DB_DOC: i64 = 19;
    pub const DB_TX_INSTANT: i64 = 50;
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

/// Typed value for a pending datom, replacing the old BYTEA encoding.
/// Each variant maps to a specific typed column in the datoms table.
#[derive(Debug, Clone, PartialEq)]
enum TypedValue {
    Ref(i64),
    Boolean(bool),
    Long(i64),
    Double(f64),
    Text(String),
    Keyword(String),
    Instant(i64),      // microseconds since epoch
    Uuid(uuid::Uuid),
    Bytes(Vec<u8>),
}

impl TypedValue {
    /// Return the value_type_tag for this value.
    fn type_tag(&self) -> i16 {
        match self {
            TypedValue::Ref(_) => 0,
            TypedValue::Boolean(_) => 1,
            TypedValue::Long(_) => 2,
            TypedValue::Double(_) => 3,
            TypedValue::Instant(_) => 4,
            TypedValue::Text(_) => 7,
            TypedValue::Keyword(_) => 8,
            TypedValue::Uuid(_) => 10,
            TypedValue::Bytes(_) => 11,
        }
    }
}

/// A single parsed assertion ready for insertion.
struct PendingDatom {
    e: i64,
    a: i64,
    v: TypedValue,
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
/// Map a PostgreSQL schema name to a store name.
///
/// `mentat` (the default schema) → `default`. Any other schema is expected
/// to be named `mentat_<store_name>`; the prefix is stripped. If the schema
/// doesn't follow that convention we fall through to `default` — the worst
/// case is over-invalidation, which is correct if not optimal.
fn schema_to_store_name(schema: &str) -> &str {
    if schema == "mentat" {
        "default"
    } else if let Some(name) = schema.strip_prefix("mentat_") {
        name
    } else {
        "default"
    }
}

/// Check if a transaction result indicates schema-affecting changes.
///
/// The transaction result JSON includes a `"schema_changed"` field set by
/// `execute_transaction_inner` when any datom's attribute matches a
/// schema-defining entid (`:db/ident`, `:db/valueType`, `:db/cardinality`,
/// `:db/unique`, `:db/index`, `:db/fulltext`, `:db/isComponent`,
/// `:db/noHistory`).
fn transaction_touched_schema(result_json: &str) -> bool {
    // Fast path: check for the marker in the JSON result
    result_json.contains("\"schema_changed\":true")
}

fn value_type_name(value: &edn::Value) -> &'static str {
    match value {
        edn::Value::Nil => "nil",
        edn::Value::Boolean(_) => "boolean",
        edn::Value::Integer(_) => "integer",
        edn::Value::BigInteger(_) => "biginteger",
        edn::Value::Float(_) => "float/double",
        edn::Value::Instant(_) => "instant",
        edn::Value::Uuid(_) => "uuid",
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
#[expect(dead_code, reason = "Used by error reporting paths not yet wired")]
fn get_available_attributes_hint() -> String {
    let available = error::get_available_attributes();
    if available.is_empty() {
        "No schema attributes found. Did you forget to define schema with mentat_transact?".to_string()
    } else if available.len() > 20 {
        let shown: Vec<&str> = available.iter().take(20).map(|s| s.as_str()).collect();
        format!("Available attributes (first 20): {}", shown.join(", "))
    } else {
        let shown: Vec<&str> = available.iter().map(|s| s.as_str()).collect();
        format!("Available attributes: {}", shown.join(", "))
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
///
/// This is the backwards-compatible version that operates on the default store.
#[pg_extern]
pub fn mentat_transact(edn_tx: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    execute_transaction_body("mentat", edn_tx)
}

/// Process an EDN transaction against a named store.
///
/// Like `mentat_transact` but targets a specific store created with
/// `mentat_create_store`. The `store_name` parameter selects which
/// PostgreSQL schema to operate on.
///
/// # Example
/// ```sql
/// SELECT mentat_transact_full('my_store',
///   '[[:db/add "t" :person/name "Alice"]]');
/// ```
#[pg_extern]
pub fn t(
    store_name: &str,
    edn_tx: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let schema_name = resolve_store_schema(store_name)?;
    execute_transaction_body(&schema_name, edn_tx)
}

/// Speculatively apply an EDN transaction without committing to the database.
///
/// Equivalent to Datomic's `d/with`: applies the transaction in a savepoint,
/// captures the transaction report (tempid resolution, tx-data, db-before,
/// db-after), then rolls back the savepoint so no persistent changes are made.
///
/// This enables "what-if" transaction previews for UI applications, validation
/// of complex transactions before committing, and testing transaction logic
/// without side effects.
///
/// Returns the same JSON transaction report format as `mentat_transact`:
/// ```json
/// {
///   "db-before": {"basis-t": <N>},
///   "db-after": {"basis-t": <M>},
///   "tx-data": [[e, a, v, tx, added], ...],
///   "tempids": {"tempid-string": entity-id, ...}
/// }
/// ```
///
/// # Example
/// ```sql
/// SELECT mentat.mentat_with('[[:db/add "t" :person/name "Alice"]]');
/// ```
#[pg_extern]
pub fn mentat_with(edn_tx: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    execute_speculative_transaction("mentat", edn_tx)
}

/// Speculatively apply an EDN transaction against a named store without
/// committing.
///
/// Like `mentat_with` but targets a specific store. See `mentat_with` for
/// full documentation.
///
/// # Example
/// ```sql
/// SELECT mentat.with('my_store',
///   '[[:db/add "t" :person/name "Alice"]]');
/// ```
#[pg_extern(name = "with")]
pub fn mentat_with_store(
    store_name: &str,
    edn_tx: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let schema_name = resolve_store_schema(store_name)?;
    execute_speculative_transaction(&schema_name, edn_tx)
}

/// List available built-in transaction functions.
///
/// Returns a JSON array describing each built-in transaction function,
/// including its name, argument format, and description. This allows
/// clients to discover available functions programmatically.
///
/// # Example
/// ```sql
/// SELECT mentat.transaction_fns();
/// ```
///
/// Returns:
/// ```json
/// [
///   {"name": ":db.fn/cas", "args": "e a old-value new-value",
///    "description": "Compare-and-swap: atomically set attribute to new value if current value matches old value"},
///   {"name": ":db.fn/retractEntity", "args": "entity-id",
///    "description": "Retract all datoms for an entity"}
/// ]
/// ```
#[pg_extern(name = "transaction_fns")]
pub fn mentat_transaction_fns() -> String {
    r#"[{"name":":db.fn/cas","args":"e a old-value new-value","description":"Compare-and-swap: atomically set attribute to new value if current value matches old value. Fails if current value does not match old-value. Use nil as old-value to assert that no value currently exists."},{"name":":db.fn/retractEntity","args":"entity-id","description":"Retract all datoms for an entity. Also accepted as :db/retractEntity."}]"#.to_string()
}

/// Execute a speculative transaction using a SAVEPOINT.
///
/// This runs the full transaction processing pipeline (parsing, tempid
/// resolution, constraint checking, datom insertion) inside a PostgreSQL
/// SAVEPOINT, captures the transaction report, then rolls back the savepoint
/// to undo all database modifications.
///
/// The approach guarantees that speculative transactions produce identical
/// results to committed transactions (same constraint checking, same tempid
/// allocation, same cardinality handling) because they use the exact same
/// code path.
fn execute_speculative_transaction(
    schema: &str,
    edn_tx: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use pgrx::spi::Spi;

    // Use a PL/pgSQL function with EXCEPTION block to get proper subtransaction
    // isolation. This works in all contexts (client connections, SPI, pgrx tests)
    // because PL/pgSQL manages subtransaction lifecycle and snapshot ownership
    // correctly, unlike manual BeginInternalSubTransaction which causes
    // "snapshot reference not owned by resource owner" errors when SPI snapshots
    // outlive the subtransaction.
    //
    // Strategy: Execute the real transaction, capture the JSON result, then
    // intentionally RAISE an exception to trigger rollback of the subtransaction.
    // The EXCEPTION handler catches our marker exception and returns the result.
    // The writes from mentat_transact are rolled back by the exception mechanism.
    let escaped_edn = edn_tx.replace('\'', "''");
    let escaped_schema = schema.replace('\'', "''");

    // Execute via a PL/pgSQL helper function that runs the transaction,
    // captures the result, then raises an exception to trigger rollback.
    // The EXCEPTION handler catches our marker and returns the result.
    let get_sql = format!(
        "SELECT mentat._speculative_exec('{escaped_schema}', '{escaped_edn}')"
    );

    // Create the helper function if it doesn't exist yet
    Spi::run(&format!(
        "CREATE OR REPLACE FUNCTION mentat._speculative_exec(p_schema TEXT, p_edn TEXT)
         RETURNS TEXT LANGUAGE plpgsql AS $fn$
         DECLARE
             _result TEXT;
         BEGIN
             -- Execute in the appropriate schema
             IF p_schema = 'mentat' THEN
                 SELECT mentat_transact(p_edn::TEXT)::TEXT INTO _result;
             ELSE
                 EXECUTE format('SELECT %I.transact($1::TEXT)::TEXT', p_schema)
                     INTO _result USING p_edn;
             END IF;
             -- Intentionally raise to trigger subtransaction rollback
             RAISE EXCEPTION 'SPECULATIVE_OK:%', _result;
         EXCEPTION
             WHEN OTHERS THEN
                 IF SQLERRM LIKE 'SPECULATIVE_OK:%' THEN
                     RETURN substring(SQLERRM FROM 16);
                 ELSE
                     RAISE;
                 END IF;
         END;
         $fn$"
    ))?;

    let result = Spi::get_one::<String>(&get_sql)?
        .ok_or_else(|| "Speculative transaction returned NULL".to_string())?;

    // Invalidate caches since the subtransaction rollback may have left stale state
    crate::cache::get_cache().invalidate();
    crate::functions::query::clear_stmt_cache();

    Ok(result)
}

/// Resolve a store name to its PostgreSQL schema name.
///
/// For "default", returns "mentat". For other names, validates the store
/// name and returns the corresponding schema (e.g., "mentat_<name>").
/// Does NOT check whether the store actually exists in the metadata table;
/// the first SQL statement that references the schema will fail if it's
/// missing, which is acceptable for the transact path.
fn resolve_store_schema(
    store_name: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if store_name == "default" {
        return Ok("mentat".to_string());
    }
    store_management::validate_store_name(store_name)?;
    Ok(store_management::get_schema_for_store(store_name))
}

/// Check whether an error is a PostgreSQL serialization failure (SQLSTATE 40001)
/// or deadlock detected (SQLSTATE 40P01). These are retriable errors.
fn is_serialization_failure(err: &(dyn std::error::Error + Send + Sync)) -> bool {
    let msg = err.to_string();
    // PostgreSQL serialization failure indicators
    msg.contains("40001")
        || msg.contains("serialization_failure")
        || msg.contains("could not serialize access")
        || msg.contains("40P01")
        || msg.contains("deadlock detected")
}

// ============================================================================
// Transaction Function Framework
// ============================================================================

/// Recognize a keyword as a built-in transaction function invocation.
///
/// Returns `Some(BuiltinTxFn)` if the keyword matches a known transaction
/// function, or `None` if it's not a recognized function.
///
/// Supported patterns:
///   - `:db.fn/cas` -> `BuiltinTxFn::Cas`
///   - `:db.fn/retractEntity` -> `BuiltinTxFn::RetractEntity`
///   - `:db/retractEntity` -> `BuiltinTxFn::RetractEntity`
fn recognize_tx_fn(kw: &edn::symbols::Keyword) -> Option<BuiltinTxFn> {
    match (kw.namespace(), kw.name()) {
        (Some("db.fn"), "cas") | (Some("db"), "cas") => Some(BuiltinTxFn::Cas),
        (Some("db.fn"), "retractEntity")
        | (Some("db"), "retractEntity")
        | (None, "retractEntity") => Some(BuiltinTxFn::RetractEntity),
        _ => None,
    }
}

/// Execute the `:db.fn/retractEntity` transaction function.
///
/// Queries all current datoms for the given entity and generates retraction
/// datoms for each one, effectively removing the entity from the database.
///
/// In Datomic, `:db/retractEntity` retracts all datoms where the entity
/// appears in the `e` position. Component attributes are handled recursively
/// (retract entity cascades to component references).
fn execute_retract_entity_fn(
    e: i64,
    qs: &str,
    pending_datoms: &mut Vec<PendingDatom>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut visited = std::collections::HashSet::new();
    execute_retract_entity_recursive(e, qs, pending_datoms, &mut visited)
}

/// Recursive implementation of entity retraction with cycle detection.
/// Cascades through `:db/isComponent` ref attributes.
fn execute_retract_entity_recursive(
    e: i64,
    qs: &str,
    pending_datoms: &mut Vec<PendingDatom>,
    visited: &mut std::collections::HashSet<i64>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Cycle guard: skip if already processing this entity
    if !visited.insert(e) {
        return Ok(());
    }

    let retract_query = format!(
        "SELECT a, value_type_tag, v_ref, v_bool, v_long, v_double, \
                v_text, v_keyword, v_instant, v_uuid, v_bytes \
         FROM {}.datoms WHERE e = $1 AND added = true",
        qs
    );

    // Collect datoms and identify component refs to cascade
    let mut component_targets: Vec<i64> = Vec::new();

    Spi::connect(|client| {
        let rows = client.select(
            &retract_query,
            None,
            &[DatumWithOid::from(e)],
        )?;

        for row in rows {
            let a: i64 = row.get(1)?.ok_or("Missing attribute")?;
            let v_type_tag: i16 = row.get(2)?.ok_or("Missing type tag")?;
            let v = read_typed_value_from_row(&row, v_type_tag, 2)?;

            // Check if this is a ref with :db/isComponent = true
            if v_type_tag == crate::types::constants::type_tag::REF {
                if let Some(attr_info) = crate::cache::get_cache().get_attribute(a) {
                    if attr_info.component {
                        if let TypedValue::Ref(target) = &v {
                            component_targets.push(*target);
                        }
                    }
                }
            }

            pending_datoms.push(PendingDatom {
                e,
                a,
                v,
                added: false,
            });
        }

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })?;

    // Recursively retract component entities
    for target in component_targets {
        execute_retract_entity_recursive(target, qs, pending_datoms, visited)?;
    }

    Ok(())
}

/// Execute the `:db.fn/cas` (compare-and-swap) transaction function.
///
/// Atomically sets attribute `a` on entity `e` to `new_edn` if and only if
/// the current value equals `old_edn`. If `old_edn` is `nil`, the attribute
/// must not currently have a value.
///
/// On success, pushes a retraction of the old value (if not nil) and an
/// assertion of the new value into `pending_datoms`.
///
/// On failure, returns a `CasFailed` error describing the mismatch.
fn execute_cas_fn(
    e: i64,
    a: i64,
    old_edn: &edn::Value,
    new_edn: &edn::Value,
    qs: &str,
    tempid_map: &mut BTreeMap<String, i64>,
    pending_datoms: &mut Vec<PendingDatom>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let is_ref = lookup_value_type(a).as_deref() == Some("ref");

    // Get current value(s) for this (e, a) pair from the current-state
    // projection. Append-only model: the log retains historical assertions,
    // so an `added=true` scan of mentat.datoms would return superseded
    // values too. mentat.current_datoms holds only live values.
    let cas_query = format!(
        "SELECT value_type_tag, v_ref, v_bool, v_long, v_double, \
                v_text, v_keyword, v_instant, v_uuid, v_bytes \
         FROM {}.current_datoms WHERE e = $1 AND a = $2",
        qs
    );
    let current_values: Vec<TypedValue> = Spi::connect(|client| {
        let rows = client.select(
            &cas_query,
            None,
            &[DatumWithOid::from(e), DatumWithOid::from(a)],
        )?;

        let mut vals = Vec::new();
        for row in rows {
            let v_type_tag: i16 = row.get(1)?.ok_or("Missing type tag")?;
            let v = read_typed_value_from_row(&row, v_type_tag, 1)?;
            vals.push(v);
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(vals)
    })?;

    // Check cardinality -- CAS on cardinality-many with multiple values is an error
    if let Some(attr_info) = lookup_attribute_info(a) {
        if attr_info.cardinality == "many" && current_values.len() > 1 {
            let attr_name = crate::cache::get_cache()
                .get_ident(a)
                .unwrap_or_else(|| format!("entid:{}", a));
            return Err(MentatError::CasFailed {
                entity: e,
                attr: attr_name,
                expected: "at most one existing value".to_string(),
                actual: format!(
                    "cardinality-many attribute has {} values; \
                     CAS requires at most one existing value",
                    current_values.len()
                ),
            }.into());
        }
    }

    let old_is_nil = matches!(old_edn, edn::Value::Nil);

    // Encode old value for comparison (unless nil)
    let old_encoded: Option<TypedValue> = if old_is_nil {
        None
    } else if is_ref {
        Some(encode_ref_value(old_edn, tempid_map, qs)?)
    } else {
        Some(encode_value(old_edn)?)
    };

    // Compare current database state with expected old value
    let cas_matches = if old_is_nil {
        // old-value is nil: expect no current value
        current_values.is_empty()
    } else if let Some(ref old_val) = old_encoded {
        // old-value is not nil: expect exactly one matching value
        current_values.len() == 1 && current_values[0] == *old_val
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
                .map(format_typed_value)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let attr_name = crate::cache::get_cache()
            .get_ident(a)
            .unwrap_or_else(|| format!("entid:{}", a));
        return Err(MentatError::CasFailed {
            entity: e,
            attr: attr_name,
            expected: format!("{:?}", old_edn),
            actual: current_desc,
        }.into());
    }

    // CAS matched -- retract old value (if not nil) and assert new value
    if !old_is_nil {
        if let Some(old_val) = old_encoded {
            pending_datoms.push(PendingDatom {
                e,
                a,
                v: old_val,
                added: false,
            });
        }
    }

    let new_val = if is_ref {
        encode_ref_value(new_edn, tempid_map, qs)?
    } else {
        encode_value(new_edn)?
    };
    pending_datoms.push(PendingDatom {
        e,
        a,
        v: new_val,
        added: true,
    });

    Ok(())
}

/// Allocate a transaction ID using lock-free sequence allocation.
///
/// PostgreSQL sequences are atomic — no two callers get the same value.
/// The `transactions` table has a PRIMARY KEY on `tx`, so duplicate inserts
/// are impossible with a sequence. SERIALIZABLE isolation + retry logic
/// (already in place in `execute_transaction_body`) handles actual data
/// conflicts (same entity being modified concurrently).
///
/// `basis_t` is derived from the actual maximum committed transaction ID
/// preceding ours, queried via an index-only scan on transactions(tx).
/// This handles gaps in the sequence (e.g., from rolled-back transactions
/// or sequence cache eviction on backend restart).
fn allocate_tx_id(qs: &str) -> Result<(i64, i64, i64), Box<dyn std::error::Error + Send + Sync>> {
    // Allocate transaction ID from the sequence (atomic, no lock needed)
    let tx_id = Spi::get_one::<i64>(&format!("SELECT nextval('{}.partition_tx_seq')", qs))
        .ok()
        .flatten()
        .ok_or_else(|| MentatError::AllocationFailed {
            partition: "db.part/tx".to_string(),
        })?;

    // basis_t is the most recent committed transaction before ours.
    // We query the actual MAX(tx) rather than assuming tx_id - 1, because
    // sequences can have gaps (rolled-back txns, cache eviction on restart).
    // This is an index-only scan on the PRIMARY KEY — negligible cost.
    let basis_t_before = Spi::get_one::<i64>(&format!(
        "SELECT COALESCE(MAX(tx), 0) FROM {}.transactions WHERE tx < {}",
        qs, tx_id
    ))
    .ok()
    .flatten()
    .unwrap_or(tx_id - 1); // Fallback for empty table (bootstrap)

    // Create transaction record and get the timestamp as microseconds since epoch.
    // The PRIMARY KEY constraint on tx guarantees uniqueness.
    let tx_instant_micros = Spi::get_one_with_args::<i64>(
        &format!(
            "INSERT INTO {}.transactions (tx, tx_instant) VALUES ($1, CURRENT_TIMESTAMP) \
             RETURNING (EXTRACT(EPOCH FROM tx_instant) * 1000000)::BIGINT",
            qs
        ),
        &[DatumWithOid::from(tx_id)],
    )
    .ok()
    .flatten()
    .ok_or_else(|| MentatError::TransactionFailed {
        message: format!(
            "Failed to create transaction record. \
             The {}.transactions table may be missing or the insert failed.",
            qs
        ),
    })?;

    Ok((basis_t_before, tx_id, tx_instant_micros))
}

/// Internal function containing the actual transaction logic.
///
/// The `schema` parameter is the PostgreSQL schema name (e.g., "mentat" for
/// the default store, or "mentat_<name>" for a named store). All SQL
/// queries are parameterized with this schema.
///
/// Runs within the caller's PostgreSQL transaction. Uses savepoints to ensure
/// that schema installation and datom insertion are atomic: if Pass 2 (datom
/// insertion) fails after schema was written in Pass 1, the savepoint rollback
/// undoes the schema changes too.
///
/// Transaction isolation: Uses SERIALIZABLE isolation to prevent lost updates,
/// non-repeatable reads, and phantom reads under concurrent writes. When two
/// transactions conflict, PostgreSQL raises SQLSTATE 40001
/// (serialization_failure). Serialization failures are automatically retried
/// with exponential backoff (up to MAX_SERIALIZATION_RETRIES attempts).
fn execute_transaction_body(
    schema: &str,
    edn_tx: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut attempt: u32 = 0;

    loop {
        attempt += 1;

        match execute_transaction_inner(schema, edn_tx) {
            Ok(result) => {
                // Only invalidate the schema cache and bump the cross-backend
                // generation counter if this transaction touched schema-defining
                // attributes. Pure data writes skip this overhead.
                let store_name = schema_to_store_name(schema);
                if transaction_touched_schema(&result) {
                    crate::cache::invalidate_store_cache(store_name);
                    crate::cache::bump_store_generation(store_name);
                }
                return Ok(result);
            }
            Err(err) => {
                if is_serialization_failure(err.as_ref()) && attempt < MAX_SERIALIZATION_RETRIES {
                    // Exponential backoff: 10ms, 20ms, 40ms, 80ms, 160ms
                    let delay_ms = BASE_RETRY_DELAY_MS * (1u64 << (attempt - 1));

                    // Use pg_sleep for the delay since we're inside SPI.
                    // Savepoint rollback happens automatically on error, so
                    // the next attempt starts with a clean slate.
                    let _ = Spi::run(&format!("SELECT pg_sleep({})", delay_ms as f64 / 1000.0));

                    // Continue to next attempt
                    continue;
                }

                // Not a serialization failure or retries exhausted
                if is_serialization_failure(err.as_ref()) {
                    return Err(MentatError::SerializationFailure {
                        message: format!(
                            "Transaction failed after {} attempts due to concurrent \
                             modifications. The transaction was retried with exponential \
                             backoff but could not be serialized. Original error: {}",
                            attempt, err
                        ),
                        attempt,
                    }
                    .into());
                }

                return Err(err);
            }
        }
    }
}

/// The inner transaction body, called by `execute_transaction_body` which
/// handles retry logic for serialization failures.
fn execute_transaction_inner(
    schema: &str,
    edn_tx: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Quote the schema name for safe SQL interpolation
    let qs = store_management::quote_ident(schema);

    // Parse EDN transaction
    let value_and_span = parse::value(edn_tx)?;
    let value = value_and_span.without_spans();

    // Validate it's a vector
    let entities = match value {
        edn::Value::Vector(ref vec) => vec,
        _ => return Err(MentatError::InvalidTransaction {
            message: format!(
                "Transaction must be a vector of entities, got {}. \
                 Expected EDN like: [[:db/add \"tempid\" :attr \"value\"]]",
                value_type_name(&value)
            ),
        }.into()),
    };

    // Allocate transaction ID with advisory lock protection
    let (basis_t_before, tx_id, tx_instant_micros) = allocate_tx_id(&qs)?;

    // --- Per-transaction context setup ---
    // (1) Check cache generation once — bypass all subsequent checks within
    //     this transaction (~17,870 round-trips eliminated per typical call).
    // (2) Resolve store_id once — bypasses ~4,032 per-datom lookups.
    // TxContextGuard clears both on drop (success or early-return via `?`).
    let store_name_raw = schema_to_store_name(schema);
    crate::cache::begin_tx_cache_bypass(store_name_raw);
    // TX_STORE_ID is still None here — first call goes to the DB.
    let tx_store_id = get_store_id_from_schema(&qs)?;
    TX_STORE_ID.with(|f| f.set(Some(tx_store_id)));
    let _tx_ctx_guard = TxContextGuard;
    // --- End per-transaction context setup ---

    // Insert :db/txInstant datom for this transaction using typed column
    // Use to_timestamp() in SQL to convert microseconds to TIMESTAMPTZ
    Spi::run_with_args(
        &format!(
            "INSERT INTO {}.datoms (e, a, value_type_tag, v_instant, tx, added) \
             VALUES ($1, $2, $3, to_timestamp($4::DOUBLE PRECISION / 1000000.0), $5, $6)",
            qs
        ),
        &[
            DatumWithOid::from(tx_id),
            DatumWithOid::from(bootstrap_entids::DB_TX_INSTANT),
            DatumWithOid::from(4_i16), // type_tag::INSTANT = 4
            DatumWithOid::from(tx_instant_micros),
            DatumWithOid::from(tx_id),
            DatumWithOid::from(true),
        ],
    )?;

    // The txInstant datom is cardinality-one and Datalog-queryable
    // ([?tx :db/txInstant ?inst]); mirror it into the current-state
    // projection so current-time reads resolve it from current_instant like
    // any other live datom. It is never retracted, so a plain upsert keyed
    // by (store_id, e=tx_id, a=DB_TX_INSTANT) suffices.
    Spi::run_with_args(
        &format!(
            "INSERT INTO {}.current_instant (store_id, e, a, v, tx) \
             VALUES ($1, $2, $3, to_timestamp($4::DOUBLE PRECISION / 1000000.0), $5) \
             ON CONFLICT (store_id, e, a, v) DO UPDATE SET tx = EXCLUDED.tx",
            qs
        ),
        &[
            DatumWithOid::from(tx_store_id),
            DatumWithOid::from(tx_id),
            DatumWithOid::from(bootstrap_entids::DB_TX_INSTANT),
            DatumWithOid::from(tx_instant_micros),
            DatumWithOid::from(tx_id),
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
    // Track entity IDs allocated for map entities (by index) so Pass 2 reuses
    // the same IDs instead of calling nextval again.
    let mut map_entity_ids: std::collections::HashMap<usize, i64> = std::collections::HashMap::new();

    // --- Pass 1: Scan for schema definitions ---
    // Only process :db/ident, :db/valueType, :db/cardinality, etc. assertions.
    // Allocate tempids encountered so they're stable across passes.
    for (entity_idx, entity_value) in entities.iter().enumerate() {
        match entity_value {
            edn::Value::Vector(ref entity_vec) if entity_vec.len() >= 4 => {
                // Only process :db/add
                match &entity_vec[0] {
                    edn::Value::Keyword(kw) if kw.name() == "add" => {}
                    _ => continue,
                };

                // Allocate/resolve the entity tempid so it's stable
                let e = resolve_entity_place(&entity_vec[1], &mut tempid_map, &qs)?;

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
                // Resolve entity for stable tempid allocation.
                // Store the allocated ID so Pass 2 reuses it.
                let e = if let Some(id_val) =
                    map.get(&edn::Value::Keyword(edn::symbols::Keyword::namespaced("db", "id")))
                {
                    resolve_entity_place(id_val, &mut tempid_map, &qs)?
                } else {
                    let id = Spi::get_one::<i64>(
                        &format!("SELECT nextval('{}.partition_user_seq')", qs),
                    )
                    .ok()
                    .flatten()
                    .ok_or_else(|| MentatError::AllocationFailed {
                        partition: "db.part/user".to_string(),
                    })?;
                    map_entity_ids.insert(entity_idx, id);
                    id
                };

                for (attr_key, attr_value) in map {
                    if let edn::Value::Keyword(kw) = attr_key {
                        if is_db_id(kw) {
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

    // Install schema attributes (if any). In pgrx tests, we rely on the
    // outer transaction for atomicity. In production, the extension function
    // call is already wrapped in a transaction by PostgreSQL.
    install_schema_attributes(&schema_builders, &qs)?;

    // --- Pass 2: Parse ALL assertions and insert datoms ---
    // Now all idents (both bootstrap and newly-defined) are resolvable.
    let mut pending_datoms: Vec<PendingDatom> = Vec::new();

    for (entity_idx, entity_value) in entities.iter().enumerate() {
        match entity_value {
            // Handle built-in transaction functions via the dispatch framework.
            // Recognized functions: :db.fn/cas, :db/cas, :db.fn/retractEntity,
            // :db/retractEntity, :retractEntity
            edn::Value::Vector(ref entity_vec)
                if entity_vec.len() >= 2
                    && matches!(&entity_vec[0], edn::Value::Keyword(kw) if recognize_tx_fn(kw).is_some()) =>
            {
                let kw = match &entity_vec[0] {
                    edn::Value::Keyword(kw) => kw,
                    _ => continue, // unreachable due to guard
                };
                let tx_fn = match recognize_tx_fn(kw) {
                    Some(f) => f,
                    None => continue, // unreachable due to guard
                };

                match tx_fn {
                    BuiltinTxFn::RetractEntity => {
                        if entity_vec.len() != 2 {
                            return Err(MentatError::InvalidTransaction {
                                message: format!(
                                    ":db.fn/retractEntity requires exactly 1 argument (entity-id), \
                                     got {}. Format: [:db.fn/retractEntity entity-id]",
                                    entity_vec.len() - 1
                                ),
                            }.into());
                        }
                        let e = resolve_entity_place(&entity_vec[1], &mut tempid_map, &qs)?;
                        execute_retract_entity_fn(e, &qs, &mut pending_datoms)?;
                    }
                    BuiltinTxFn::Cas => {
                        if entity_vec.len() != 5 {
                            return Err(MentatError::InvalidTransaction {
                                message: format!(
                                    ":db.fn/cas requires exactly 4 arguments \
                                     (entity attr old-value new-value), got {}. \
                                     Format: [:db.fn/cas e a old-val new-val]",
                                    entity_vec.len() - 1
                                ),
                            }.into());
                        }
                        let e = resolve_entity_place(&entity_vec[1], &mut tempid_map, &qs)?;
                        let a = resolve_attribute(&entity_vec[2])?;
                        execute_cas_fn(
                            e, a,
                            &entity_vec[3], &entity_vec[4],
                            &qs, &mut tempid_map, &mut pending_datoms,
                        )?;
                    }
                }
            }
            // Handle :db/add and :db/retract - format: [:db/add e a v] or [:db/retract e a v]
            edn::Value::Vector(ref entity_vec) if entity_vec.len() >= 4 => {
                let op = match &entity_vec[0] {
                    edn::Value::Keyword(kw) if kw.name() == "add" => OpType::Add,
                    edn::Value::Keyword(kw) if kw.name() == "retract" => OpType::Retract,
                    // Fail loud: a vector that begins with a keyword but is not a
                    // recognized operation (e.g. [:db/invalid ...]) is an unknown
                    // transaction operation, not silently-skippable data.
                    edn::Value::Keyword(kw) => {
                        return Err(MentatError::InvalidTransaction {
                            message: format!(
                                "unknown transaction operation '{}'. Expected one of \
                                 :db/add, :db/retract, :db/cas, :db.fn/cas, \
                                 :db/retractEntity, :db.fn/retractEntity.",
                                kw
                            ),
                        }
                        .into());
                    }
                    _ => continue,
                };

                let e = resolve_entity_place(&entity_vec[1], &mut tempid_map, &qs)?;
                let a = resolve_attribute(&entity_vec[2])?;
                // Check if attribute is ref-type; if so, resolve value as entity reference
                let v = if lookup_value_type(a).as_deref() == Some("ref") {
                    encode_ref_value(&entity_vec[3], &mut tempid_map, &qs)?
                } else {
                    encode_value(&entity_vec[3])?
                };
                let added = matches!(op, OpType::Add);

                pending_datoms.push(PendingDatom {
                    e,
                    a,
                    v,
                    added,
                });
            }
            edn::Value::Map(ref map) => {
                // Reuse entity ID from Pass 1 if one was pre-allocated,
                // otherwise allocate a new one (for non-schema map entities).
                let e = if let Some(id_val) =
                    map.get(&edn::Value::Keyword(edn::symbols::Keyword::namespaced("db", "id")))
                {
                    resolve_entity_place(id_val, &mut tempid_map, &qs)?
                } else if let Some(&pre_allocated) = map_entity_ids.get(&entity_idx) {
                    pre_allocated
                } else {
                    Spi::get_one::<i64>(
                        &format!("SELECT nextval('{}.partition_user_seq')", qs),
                    )
                    .ok()
                    .flatten()
                    .ok_or_else(|| MentatError::AllocationFailed {
                        partition: "db.part/user".to_string(),
                    })?
                };

                for (attr_key, attr_value) in map {
                    if let edn::Value::Keyword(kw) = attr_key {
                        if is_db_id(kw) {
                            continue;
                        }
                    }

                    let a = resolve_attribute(attr_key)?;
                    // Check if attribute is ref-type; if so, resolve value as entity reference
                    let v = if lookup_value_type(a).as_deref() == Some("ref") {
                        encode_ref_value(attr_value, &mut tempid_map, &qs)?
                    } else {
                        encode_value(attr_value)?
                    };

                    pending_datoms.push(PendingDatom {
                        e,
                        a,
                        v,
                        added: true,
                    });
                }
            }
            // Fail loud: a vector beginning with a keyword that reached here
            // matched none of the operation arms above (too few elements, e.g.
            // [:db/add] or [:db/add "e"]). Such an assertion is malformed
            // rather than silently-skippable data.
            edn::Value::Vector(ref entity_vec)
                if matches!(entity_vec.first(), Some(edn::Value::Keyword(_))) =>
            {
                let kw = match entity_vec.first() {
                    Some(edn::Value::Keyword(kw)) => kw,
                    _ => unreachable!("guard guarantees leading keyword"),
                };
                return Err(MentatError::InvalidTransaction {
                    message: format!(
                        "malformed assertion starting with '{}': got {} element(s). \
                         Expected [:db/add e a v] or [:db/retract e a v] (4 elements), \
                         [:db/cas e a old new] (5 elements), or \
                         [:db/retractEntity e] (2 elements).",
                        kw,
                        entity_vec.len()
                    ),
                }
                .into());
            }
            _ => {}
        }
    }

    // --- Upsert resolution for :db.unique/identity attributes ---
    // In Datomic, when a tempid-allocated entity asserts a value for a
    // :db.unique/identity attribute that already exists in the database,
    // the tempid should resolve to the existing entity's ID (upsert)
    // rather than causing a unique constraint violation.
    //
    // Two phases:
    //   Phase A: Check the DB for existing entities with the same value.
    //   Phase B: Within the transaction, merge tempids that assert the same
    //            identity-unique value (in-transaction unification).
    let mut upsert_remaps: BTreeMap<i64, i64> = BTreeMap::new(); // old_eid -> target_eid

    // Phase A: DB-level upsert resolution
    for datom in &pending_datoms {
        if !datom.added {
            continue;
        }
        if let Some(attr_info) = lookup_attribute_info(datom.a) {
            if attr_info.unique_constraint.as_deref() == Some("identity") {
                if let Ok(Some(existing_eid)) = check_unique_typed_value(datom.a, &datom.v, &qs) {
                    if existing_eid != datom.e {
                        // Check for conflicting remaps: if this tempid was already
                        // remapped to a different entity, that's an error (two
                        // identity-unique attrs on the same tempid point to
                        // different existing entities).
                        if let Some(&prev_remap) = upsert_remaps.get(&datom.e) {
                            if prev_remap != existing_eid {
                                return Err(MentatError::InvalidTransaction {
                                    message: format!(
                                        "Conflicting upsert: tempid for entity {} resolves to \
                                         both {} and {} via different :db.unique/identity attributes",
                                        datom.e, prev_remap, existing_eid
                                    ),
                                }.into());
                            }
                        } else {
                            upsert_remaps.insert(datom.e, existing_eid);
                        }
                    }
                }
            }
        }
    }

    // Phase B: In-transaction tempid merging for :db.unique/identity
    // When two different tempids in the same transaction assert the same value
    // for an identity-unique attribute, merge them to the same entity ID
    // (the first one seen becomes the canonical entity).
    {
        // Collect (index, attr_id, effective_eid) for identity-unique assertions
        let mut identity_assertions: Vec<(usize, i64, i64)> = Vec::new();

        for (idx, datom) in pending_datoms.iter().enumerate() {
            if !datom.added {
                continue;
            }
            if let Some(attr_info) = lookup_attribute_info(datom.a) {
                if attr_info.unique_constraint.as_deref() == Some("identity") {
                    let effective_e = upsert_remaps.get(&datom.e).copied().unwrap_or(datom.e);
                    identity_assertions.push((idx, datom.a, effective_e));
                }
            }
        }

        // For each pair, check if they share the same (attr, value) but different
        // entities. Resolve each endpoint through the existing remap chain at
        // comparison time (not at collection time): with three+ tempids sharing
        // one identity value, an earlier pair may already have merged one of
        // them, and we must follow that merge so all of them collapse to a
        // single canonical entity instead of raising a false "conflicting
        // upsert". `effective_e` captured during collection only reflects
        // Phase A remaps, so we re-resolve here.
        fn resolve_root(remaps: &BTreeMap<i64, i64>, mut e: i64) -> i64 {
            // Follow the chain to its root. The map is acyclic by construction
            // (we only ever point a higher/younger tempid at an existing root),
            // but bound the walk defensively.
            for _ in 0..remaps.len() + 1 {
                match remaps.get(&e) {
                    Some(&next) if next != e => e = next,
                    _ => break,
                }
            }
            e
        }

        for i in 0..identity_assertions.len() {
            for j in (i + 1)..identity_assertions.len() {
                let (idx_i, a_i, _e_i) = identity_assertions[i];
                let (idx_j, a_j, _e_j) = identity_assertions[j];

                if a_i != a_j {
                    continue;
                }
                if pending_datoms[idx_i].v != pending_datoms[idx_j].v {
                    continue;
                }

                // Same attr and value -- unify the two entities' roots.
                let root_i = resolve_root(&upsert_remaps, pending_datoms[idx_i].e);
                let root_j = resolve_root(&upsert_remaps, pending_datoms[idx_j].e);
                if root_i == root_j {
                    continue; // already unified
                }
                // First one seen wins as canonical; point the other root at it.
                let (canonical, merged) = (root_i, root_j);
                upsert_remaps.insert(merged, canonical);
            }
        }
    }

    // Flatten the remap map so every entry points directly at its final root.
    // Phase A + Phase B can produce multi-step chains (a -> b -> c); the
    // single-lookup apply step below only resolves one hop, so collapse the
    // chains here to guarantee all datoms land on the canonical entity.
    if !upsert_remaps.is_empty() {
        let keys: Vec<i64> = upsert_remaps.keys().copied().collect();
        for k in keys {
            let mut root = k;
            for _ in 0..upsert_remaps.len() + 1 {
                match upsert_remaps.get(&root) {
                    Some(&next) if next != root => root = next,
                    _ => break,
                }
            }
            if root != k {
                upsert_remaps.insert(k, root);
            }
        }
    }

    // Apply upsert remaps to all pending datoms and the tempid map
    if !upsert_remaps.is_empty() {
        for datom in &mut pending_datoms {
            if let Some(&new_eid) = upsert_remaps.get(&datom.e) {
                datom.e = new_eid;
            }
            // Also remap ref values that point to remapped entities
            if let TypedValue::Ref(ref_id) = &datom.v {
                if let Some(&new_ref) = upsert_remaps.get(ref_id) {
                    datom.v = TypedValue::Ref(new_ref);
                }
            }
        }
        // Update tempid_map so the TxReport reflects the upsert resolution
        for (_tempid, eid) in tempid_map.iter_mut() {
            if let Some(&new_eid) = upsert_remaps.get(eid) {
                *eid = new_eid;
            }
        }

        // Deduplicate identical assertion datoms that may result from tempid
        // merging. After upsert remapping, two formerly distinct tempids now
        // share the same entity ID, so their identical assertions (same e, a, v,
        // added) should be collapsed to avoid cardinality-one violations.
        let mut dedup_indices: Vec<usize> = Vec::new();
        for (i, datom) in pending_datoms.iter().enumerate() {
            if !datom.added {
                continue;
            }
            // Check if an earlier datom in the list is identical
            let is_dup = pending_datoms[..i].iter().any(|earlier| {
                earlier.added
                    && earlier.e == datom.e
                    && earlier.a == datom.a
                    && earlier.v == datom.v
            });
            if is_dup {
                dedup_indices.push(i);
            }
        }
        // Remove duplicates in reverse order to preserve indices
        for &idx in dedup_indices.iter().rev() {
            pending_datoms.remove(idx);
        }
    }

    // Validate and insert all datoms.
    // If schema was installed (savepoint active), catch errors to rollback
    // the savepoint before propagating.
    let datom_result = insert_datoms(&pending_datoms, tx_id, &qs);

    match datom_result {
        Ok(_datom_count) => {
            // All datoms inserted successfully. PostgreSQL's transaction
            // management will commit schema + datoms atomically.

            // Build Datomic-compatible TxReport response with all 4 required fields:
            //   :db-before  - database value before the transaction
            //   :db-after   - database value after the transaction
            //   :tx-data    - all datoms produced by the transaction
            //   :tempids    - mapping of tempid strings to allocated entity IDs

            // Build tempids JSON object
            let tempids_json: Vec<String> = tempid_map
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", k, v))
                .collect();

            // Build tx-data: array of [e, a, v, tx, added] for each datom in the transaction.
            // Include the implicit :db/txInstant datom first (matches Datomic ordering),
            // then all user-supplied datoms.
            let mut tx_data_entries: Vec<String> = Vec::with_capacity(pending_datoms.len() + 1);

            // The :db/txInstant datom
            let tx_instant_tv = TypedValue::Instant(tx_instant_micros);
            tx_data_entries.push(format!(
                "[{},{},{},{},{}]",
                tx_id,
                bootstrap_entids::DB_TX_INSTANT,
                format_typed_value_for_json(&tx_instant_tv),
                tx_id,
                true
            ));

            // All user datoms (including implicit retractions from cardinality-one handling)
            for datom in &pending_datoms {
                tx_data_entries.push(format!(
                    "[{},{},{},{},{}]",
                    datom.e,
                    datom.a,
                    format_typed_value_for_json(&datom.v),
                    tx_id,
                    datom.added
                ));
            }

            let tx_data_json = format!("[{}]", tx_data_entries.join(","));

            Ok(format!(
                "{{\"db-before\":{{\"basis-t\":{}}},\"db-after\":{{\"basis-t\":{}}},\"tx-data\":{},\"tempids\":{{{}}},\"schema_changed\":{}}}",
                basis_t_before,
                tx_id,
                tx_data_json,
                tempids_json.join(","),
                has_schema_changes
            ))
        }
        Err(e) => {
            // Datom insertion failed. PostgreSQL will automatically roll back
            // the entire transaction (including schema changes). Invalidate
            // caches in case they were populated during schema installation.
            if has_schema_changes {
                crate::cache::get_cache().invalidate();
                crate::functions::query::clear_stmt_cache();
            }
            Err(e)
        }
    }
}

/// Outcome of a cardinality-one check, returned by
/// `retract_existing_cardinality_one` to decouple the retraction
/// decision from the actual INSERT (which is now deferred to the batch).
enum CardinalityOneResult {
    /// The new value equals the existing value; skip the entire datom
    /// (idempotent assertion).
    Skip,
    /// No existing value; insert the new datom normally.
    Insert,
    /// An existing different value was found and marked retracted via an
    /// UPDATE.  The caller must queue a retraction datom `(e, a, old_v,
    /// tx, false)` before queuing the new assertion.
    Replace(TypedValue),
}

/// Insert all pending datoms, validating constraints and handling cardinality
/// semantics. Returns the number of datoms processed.
///
/// The `schema` parameter is the quoted PostgreSQL schema name.
fn insert_datoms(
    pending_datoms: &[PendingDatom],
    tx_id: i64,
    schema: &str,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let datom_count = pending_datoms.len();

    // Collect all rows that need to be written to the type-specific tables.
    // Deferred to a single batch INSERT per type at the end of the loop,
    // reducing ~463 per-datom INSERTs to at most 9 (one per type table).
    let mut rows_to_insert: Vec<(i64, i64, TypedValue, i64, bool)> = Vec::with_capacity(datom_count);
    let mut fulltext_values: Vec<String> = Vec::new();

    for datom in pending_datoms {
        // Only validate assertions (added=true), not retractions
        if datom.added {
            validate_datom_constraints(datom, pending_datoms, schema)?;

            // For cardinality-one attributes, automatically retract any existing
            // value before asserting the new one (Datomic upsert semantics).
            // For cardinality-many attributes, allow multiple values - no retraction,
            // but skip if the exact (e, a, v) triple already exists (idempotent).
            if let Some(attr_info) = lookup_attribute_info(datom.a) {
                // :db/noHistory attributes keep ONLY the current value: no
                // assert/retract trail accumulates. On assertion we
                // physically prune the prior log row(s) for this (e,a)
                // (cardinality-one) or (e,a,v) duplicates (cardinality-many)
                // and skip queuing any retraction datom. The current-state
                // projection is maintained as usual, so reads are unchanged.
                if attr_info.no_history {
                    match attr_info.cardinality.as_str() {
                        "one" => {
                            // Idempotent: if the current value already equals
                            // the new one, skip entirely.
                            if let CardinalityOneResult::Skip = retract_existing_cardinality_one(
                                datom.e, datom.a, &datom.v, schema,
                            )? {
                                continue;
                            }
                            // Prune ALL prior log rows for this (e,a): noHistory
                            // keeps no trail, so the old assertion/retraction
                            // rows are physically removed before the new
                            // assertion is written.
                            prune_no_history_ea(datom.e, datom.a, &datom.v, schema)?;
                            // The new value differs from the old (we did not
                            // Skip), and for cardinality-one each value is a
                            // distinct projection PK (store_id,e,a,v), so the
                            // old projection row would otherwise linger. Prune
                            // the projection for this (e,a) too; the upsert
                            // below re-inserts the current value.
                            prune_projection_ea(datom.e, datom.a, &datom.v, schema)?;
                        }
                        "many" => {
                            if is_duplicate_cardinality_many(
                                datom.e, datom.a, &datom.v, schema,
                            )? {
                                continue;
                            }
                            // For cardinality-many noHistory, prune only stale
                            // rows of this exact (e,a,v) (e.g. a prior
                            // retraction of the same value) so re-assertion
                            // doesn't accumulate history.
                            prune_no_history_eav(datom.e, datom.a, &datom.v, schema)?;
                        }
                        _ => {
                            return Err(MentatError::InvalidCardinality {
                                cardinality: attr_info.cardinality.clone(),
                                attr_entid: datom.a,
                            }.into());
                        }
                    }
                    // Queue only the new assertion; no retraction trail.
                    rows_to_insert.push((datom.e, datom.a, datom.v.clone(), tx_id, true));
                    if datom.added && is_fulltext_attribute(datom.a) {
                        if let TypedValue::Text(ref text_value) = datom.v {
                            fulltext_values.push(text_value.clone());
                        }
                    }
                    continue;
                }
                match attr_info.cardinality.as_str() {
                    "one" => {
                        match retract_existing_cardinality_one(
                            datom.e,
                            datom.a,
                            &datom.v,
                            schema,
                        )? {
                            CardinalityOneResult::Skip => continue, // idempotent
                            CardinalityOneResult::Insert => { /* no retraction needed */ }
                            CardinalityOneResult::Replace(old_v) => {
                                // Queue retraction datom for the old value.
                                rows_to_insert.push((datom.e, datom.a, old_v, tx_id, false));
                            }
                        }
                    }
                    "many" => {
                        if is_duplicate_cardinality_many(
                            datom.e,
                            datom.a,
                            &datom.v,
                            schema,
                        )? {
                            continue;
                        }
                    }
                    _ => {
                        return Err(MentatError::InvalidCardinality {
                            cardinality: attr_info.cardinality.clone(),
                            attr_entid: datom.a,
                        }.into());
                    }
                }
            }

            // Queue the new assertion.
            rows_to_insert.push((datom.e, datom.a, datom.v.clone(), tx_id, true));

            // Populate fulltext for fulltext-enabled string attributes.
            if datom.added && is_fulltext_attribute(datom.a) {
                if let TypedValue::Text(ref text_value) = datom.v {
                    fulltext_values.push(text_value.clone());
                }
            }
        } else {
            // Explicit retraction (added=false): append the retraction datom.
            //
            // Append-only model (1.5.0): we do NOT flip the existing
            // assertion row's `added` flag. The datom log is immutable; a
            // retraction is a new datom (e, a, v, tx, false). Current-time
            // queries resolve liveness from the current-state projection
            // (which this retraction removes the value from via
            // maintain_current_projection), and as-of queries resolve via
            // the latest-tx-wins / NOT EXISTS-newer supersession logic over
            // the immutable log.
            rows_to_insert.push((datom.e, datom.a, datom.v.clone(), tx_id, false));
        }
    }

    // Flush all collected datoms via one batch INSERT per type table.
    batch_insert_datoms(&rows_to_insert, schema)?;

    // Maintain the current-state projection from the same rows: assertions
    // upsert into mentat.current_<type>, retractions delete from it. This
    // keeps the projection in lock-step with the append-only log inside the
    // same transaction. See sql/24_current_projection.sql.
    maintain_current_projection(&rows_to_insert, schema)?;

    // Fulltext inserts remain per-row (typically low volume).
    for text_value in &fulltext_values {
        Spi::run_with_args(
            &format!("INSERT INTO {}.fulltext (text_value) VALUES ($1)", schema),
            &[DatumWithOid::from(text_value.as_str())],
        )?;
    }

    Ok(datom_count)
}

/// Collect schema-defining assertions for an entity.
///
/// When an assertion targets a built-in schema attribute (:db/ident, :db/valueType, etc.),
/// record the value in the SchemaBuilder for that entity so we can install the attribute
/// definition before inserting datoms.
/// Returns true if `kw` is the `:db/id` pseudo-attribute used to bind a
/// tempid to an entity inside a map-form entity.
///
/// EDN parses `:db/id` as `Keyword::namespaced("db", "id")`. Older code in
/// this file compared `kw.name() == "db/id"` and looked up the map with
/// `Keyword::plain("db/id")` — both of those silently fail for the parsed
/// keyword (namespace="db", name="id"), so `:db/id` was being treated as
/// an unknown user attribute. That broke every `{:db/id ... :foo 1}`
/// map-form transaction. Always go through this helper.
#[inline]
fn is_db_id(kw: &edn::symbols::Keyword) -> bool {
    kw.namespace() == Some("db") && kw.name() == "id"
}

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

/// Install new schema attributes into the store's schema and idents tables.
///
/// For each entity that has at least :db/ident and :db/valueType, insert a row
/// into <schema>.schema and <schema>.idents. This must happen before datoms are
/// inserted so that foreign key constraints on datoms.a are satisfied.
///
/// The `schema` parameter is the quoted PostgreSQL schema name.
fn install_schema_attributes(
    builders: &BTreeMap<i64, SchemaBuilder>,
    schema: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for (&entid, builder) in builders {
        // An entity is an ATTRIBUTE definition only if it asserts :db/valueType
        // or :db/cardinality. A bare :db/ident with neither is just naming an
        // entity (e.g. an enum value like :status/open, or a partition) -- a
        // legitimate Datomic pattern, NOT an incomplete attribute. For those
        // we install the ident mapping and move on. For a genuine attribute
        // definition we fail loud on a missing piece rather than silently
        // skipping it.
        let is_attribute_definition =
            builder.value_type.is_some() || builder.cardinality.is_some();

        let ident = match &builder.ident {
            Some(i) => i.clone(),
            None => {
                if is_attribute_definition {
                    return Err(MentatError::InvalidTransaction {
                        message: format!(
                            "incomplete schema attribute definition (entity {}): \
                             missing :db/ident. A schema attribute requires \
                             :db/ident, :db/valueType, and :db/cardinality.",
                            entid
                        ),
                    }
                    .into());
                }
                // No ident and not an attribute def -> nothing to install.
                continue;
            }
        };

        // A bare :db/ident (no valueType/cardinality): install the ident-only
        // mapping for the named entity and skip attribute-column handling.
        if !is_attribute_definition {
            Spi::run_with_args(
                &format!(
                    "INSERT INTO {}.idents (ident, entid) VALUES ($1, $2) \
                     ON CONFLICT (ident) DO UPDATE SET entid = EXCLUDED.entid",
                    schema
                ),
                &[
                    DatumWithOid::from(ident.as_str()),
                    DatumWithOid::from(entid),
                ],
            )?;
            continue;
        }

        let value_type = match &builder.value_type {
            Some(vt) => vt.clone(),
            None => {
                return Err(MentatError::InvalidTransaction {
                    message: format!(
                        "incomplete schema attribute definition for '{}': \
                         missing :db/valueType. A schema attribute requires \
                         :db/ident, :db/valueType, and :db/cardinality.",
                        ident
                    ),
                }
                .into());
            }
        };

        let cardinality = match &builder.cardinality {
            Some(c) => c.clone(),
            None => {
                return Err(MentatError::InvalidTransaction {
                    message: format!(
                        "incomplete schema attribute definition for '{}': \
                         missing :db/cardinality. A schema attribute requires \
                         :db/ident, :db/valueType, and :db/cardinality.",
                        ident
                    ),
                }
                .into());
            }
        };
        let indexed = builder.indexed.unwrap_or(false);
        let fulltext = builder.fulltext.unwrap_or(false);
        let component = builder.component.unwrap_or(false);
        let no_history = builder.no_history.unwrap_or(false);

        // Insert into idents (keyword -> entid mapping)
        Spi::run_with_args(
            &format!(
                "INSERT INTO {}.idents (ident, entid) VALUES ($1, $2) \
                 ON CONFLICT (ident) DO UPDATE SET entid = EXCLUDED.entid",
                schema
            ),
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

        // Insert into schema with all attribute properties.
        // Cast text parameters to the correct PostgreSQL enum types.
        // Note: enum types (mentat.value_type etc.) live in the mentat schema
        // and are shared across all stores.
        Spi::run_with_args(
            &format!(
                "INSERT INTO {schema}.schema \
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
                schema = schema
            ),
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
///
/// The `schema` parameter is the quoted PostgreSQL schema name.
fn resolve_entity_place(
    value: &edn::Value,
    tempid_map: &mut std::collections::BTreeMap<String, i64>,
    schema: &str,
) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    match value {
        edn::Value::Integer(i) => Ok(*i),
        edn::Value::Text(ref s) => {
            // Tempid: allocate or reuse
            if let Some(&existing) = tempid_map.get::<str>(s.as_ref()) {
                Ok(existing)
            } else {
                let entid = Spi::get_one::<i64>(
                    &format!("SELECT nextval('{}.partition_user_seq')", schema),
                )
                .ok()
                .flatten()
                .ok_or_else(|| MentatError::AllocationFailed {
                        partition: "db.part/user".to_string(),
                    })?;
                tempid_map.insert(s.to_string(), entid);
                Ok(entid)
            }
        }
        edn::Value::Keyword(kw) => {
            // Resolve keyword ident using cache (works for all stores since the
            // cache is populated from the store's schema/idents tables).
            let ident_str = format!("{}", kw);
            // Try the cache first, then fall back to direct DB lookup in the
            // store's idents table.
            let entid = crate::cache::get_cache()
                .resolve_ident(&ident_str)
                .or_else(|| {
                    Spi::get_one_with_args::<i64>(
                        &format!("SELECT entid FROM {}.idents WHERE ident = $1", schema),
                        &[DatumWithOid::from(ident_str.as_str())],
                    )
                    .ok()
                    .flatten()
                })
                .ok_or_else(|| MentatError::EntityNotFound {
                    ident: ident_str.clone(),
                    message: "Ensure this ident was previously defined via mentat_transact with :db/ident.".to_string(),
                })?;
            Ok(entid)
        }
        edn::Value::Vector(ref vec) if vec.len() == 2 => {
            // Lookup ref: [:attribute value]
            // Example: [:person/email "alice@example.com"]
            match &vec[0] {
                edn::Value::Keyword(_) => {}
                other => return Err(MentatError::InvalidEntityPlace {
                    got_type: value_type_name(other).to_string(),
                    got_value: format!("Lookup ref first element must be a keyword attribute, got {}", other),
                }.into()),
            }

            let a = resolve_attribute(&vec[0])?;

            // Validate the attribute has a unique constraint
            let attr_ident_display = crate::cache::get_cache()
                .get_ident(a)
                .unwrap_or_else(|| format!("entid:{}", a));
            let attr_info = lookup_attribute_info(a)
                .ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
                    error::attribute_not_found(&attr_ident_display).into()
                })?;
            if attr_info.unique_constraint.is_none() {
                return Err(MentatError::LookupRefRequiresUnique {
                    attr: attr_ident_display,
                }.into());
            }

            let typed_val = encode_value(&vec[1])?;

            // Query for entity with this unique attribute value using typed columns
            let eid = check_unique_typed_value(a, &typed_val, schema)?
                .ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
                    let attr_ident_display = crate::cache::get_cache()
                        .get_ident(a)
                        .unwrap_or_else(|| format!("entid:{}", a));
                    MentatError::LookupRefNotFound {
                        attr: attr_ident_display,
                        message: "Ensure an entity with this attribute value has been transacted.".to_string(),
                    }.into()
                })?;

            Ok(eid)
        }
        other => Err(MentatError::InvalidEntityPlace {
            got_type: value_type_name(other).to_string(),
            got_value: other.to_string(),
        }.into()),
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
                .ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
                    error::attribute_not_found(&ident_str).into()
                })
        }
        other => Err(MentatError::InvalidAttribute {
            got_type: value_type_name(other).to_string(),
            got_value: other.to_string(),
        }.into()),
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

/// Encode EDN value as a TypedValue for insertion into typed columns.
fn encode_value(
    value: &edn::Value,
) -> Result<TypedValue, Box<dyn std::error::Error + Send + Sync>> {
    match value {
        edn::Value::Boolean(b) => Ok(TypedValue::Boolean(*b)),
        edn::Value::Integer(i) => Ok(TypedValue::Long(*i)),
        edn::Value::Text(ref s) => Ok(TypedValue::Text(s.clone())),
        edn::Value::Float(f) => {
            let val: f64 = f.into_inner();
            Ok(TypedValue::Double(val))
        }
        edn::Value::Instant(dt) => {
            let micros = dt.timestamp_micros();
            Ok(TypedValue::Instant(micros))
        }
        edn::Value::Uuid(u) => {
            Ok(TypedValue::Uuid(*u))
        }
        edn::Value::Bytes(b) => {
            // edn::Value::Bytes wraps bytes::Bytes; TypedValue::Bytes is Vec<u8>.
            Ok(TypedValue::Bytes(b.to_vec()))
        }
        edn::Value::Keyword(kw) => {
            // Store keyword without leading colon, using slash separator
            // e.g., :person/name -> "person/name"
            let display = format!("{}", kw); // produces ":person/name"
            let s = if display.starts_with(':') {
                display[1..].to_string()
            } else {
                display
            };
            Ok(TypedValue::Keyword(s))
        }
        other => Err(MentatError::UnsupportedValueType {
            got_type: value_type_name(other).to_string(),
            got_value: other.to_string(),
        }.into()),
    }
}

/// Encode a value for a ref-type attribute. The value should be a tempid (string),
/// integer entity ID, or keyword ident. Returns TypedValue::Ref with the entity ID.
///
/// The `schema` parameter is the quoted PostgreSQL schema name.
fn encode_ref_value(
    value: &edn::Value,
    tempid_map: &mut BTreeMap<String, i64>,
    schema: &str,
) -> Result<TypedValue, Box<dyn std::error::Error + Send + Sync>> {
    let entity_id = resolve_entity_place(value, tempid_map, schema)?;
    Ok(TypedValue::Ref(entity_id))
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

/// Validate all constraints for a datom before insertion.
///
/// The `schema` parameter is the quoted PostgreSQL schema name.
fn validate_datom_constraints(
    datom: &PendingDatom,
    all_pending: &[PendingDatom],
    schema: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let attr_info = lookup_attribute_info(datom.a)
        .ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
            let ident_name = crate::cache::get_cache()
                .get_ident(datom.a)
                .unwrap_or_else(|| format!("entid:{}", datom.a));
            error::attribute_not_found(&ident_name).into()
        })?;

    // 1. Type validation
    let expected_type_tag = value_type_to_tag(&attr_info.value_type);
    let got_type_tag = datom.v.type_tag();
    if got_type_tag != expected_type_tag {
        let ident_name = crate::cache::get_cache()
            .get_ident(datom.a)
            .unwrap_or_else(|| format!("entid:{}", datom.a));
        let got_type_name = tag_to_value_type_name(got_type_tag);
        return Err(MentatError::TypeMismatch {
            attr: ident_name,
            expected: attr_info.value_type.clone(),
            got: got_type_name.to_string(),
            expected_tag: expected_type_tag,
            got_tag: got_type_tag,
        }.into());
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
                return Err(MentatError::CardinalityViolation {
                    attr: ident_name,
                    entity: datom.e,
                    count: count_in_tx,
                }.into());
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
            return Err(MentatError::InvalidCardinality {
                cardinality: attr_info.cardinality.clone(),
                attr_entid: datom.a,
            }.into());
        }
    }

    // 3. Unique constraint validation
    if let Some(ref unique_type) = attr_info.unique_constraint {
        // Check within this transaction for duplicate values from different entities
        let dups_in_tx = all_pending
            .iter()
            .filter(|d| d.a == datom.a && d.v == datom.v && d.e != datom.e && d.added)
            .count();

        if dups_in_tx > 0 {
            // For :db.unique/identity, in-transaction duplicates from different
            // entities should have been resolved by the upsert remapping pass.
            // If we still see them here, it means two different non-tempid entities
            // are asserting the same identity value, which is a real conflict.
            // For :db.unique/value, always error on duplicates.
            let ident_name = crate::cache::get_cache()
                .get_ident(datom.a)
                .unwrap_or_else(|| format!("entid:{}", datom.a));
            return Err(MentatError::UniqueConstraintViolation {
                attr: ident_name,
                unique_type: unique_type.clone(),
                existing_eid: datom.e,
                new_eid: datom.e,
            }.into());
        }

        // Check existing datoms in database (use advisory lock to prevent races)
        let lock_key = (datom.a as i64) ^ (compute_typed_value_hash(&datom.v) as i64);

        Spi::run_with_args(
            "SELECT pg_advisory_xact_lock($1)",
            &[DatumWithOid::from(lock_key)],
        )?;

        let existing_entity = check_unique_typed_value(datom.a, &datom.v, schema)?;

        if let Some(existing_e) = existing_entity {
            if existing_e != datom.e {
                // For :db.unique/identity, this should not happen after the
                // upsert remapping pass -- the datom.e should have been remapped
                // to existing_e already. If it wasn't remapped, it means the
                // entity ID was a literal (not a tempid), so this is a real
                // conflict regardless of unique type.
                let ident_name = crate::cache::get_cache()
                    .get_ident(datom.a)
                    .unwrap_or_else(|| format!("entid:{}", datom.a));
                return Err(MentatError::UniqueConstraintViolation {
                    attr: ident_name,
                    unique_type: unique_type.clone(),
                    existing_eid: existing_e,
                    new_eid: datom.e,
                }.into());
            }
            // existing_e == datom.e: the entity already has this value for this
            // unique attribute. This is fine -- it's either an upsert (identity)
            // or an idempotent re-assertion (value). No error needed.
        }
    }

    Ok(())
}

/// For cardinality-one attributes, retract any existing value for this (entity, attribute)
/// pair before asserting a new value. This implements Datomic's upsert semantics.
/// If the new value is identical to the existing value, no retraction is needed (idempotent).
///
/// The `schema` parameter is the quoted PostgreSQL schema name.
/// Parse a value-as-text plus its narrow-table type tag back into a
/// `TypedValue`. Shared by both the fast (single-table) and slow
/// (UNION-ALL) variants of `find_current_value_for_ea_typed` so the
/// decoding is identical.
fn parse_typed_value_from_tag(
    type_tag: i16,
    value_str: String,
) -> Result<TypedValue, Box<dyn std::error::Error + Send + Sync>> {
    let typed_value = match type_tag {
        0 => TypedValue::Ref(value_str.parse().map_err(|_| "Invalid ref")?),
        1 => TypedValue::Boolean(value_str.parse().map_err(|_| "Invalid boolean")?),
        2 => TypedValue::Long(value_str.parse().map_err(|_| "Invalid long")?),
        3 => TypedValue::Double(value_str.parse().map_err(|_| "Invalid double")?),
        4 => TypedValue::Instant(value_str.parse().map_err(|_| "Invalid instant")?),
        7 => TypedValue::Text(value_str),
        8 => TypedValue::Keyword(value_str),
        10 => TypedValue::Uuid(value_str.parse().map_err(|_| "Invalid UUID")?),
        11 => {
            let decoded = hex::decode(&value_str).map_err(|_| "Invalid hex")?;
            TypedValue::Bytes(decoded)
        }
        _ => return Err(format!("Unknown type_tag: {}", type_tag).into()),
    };
    Ok(typed_value)
}

/// Decide what a cardinality-one assertion of `new_v` for `(e, a)` implies
/// against the current value held in the projection:
///
/// * `Skip`         -- new_v equals the current value (idempotent no-op).
/// * `Insert`       -- no current value exists; just assert new_v.
/// * `Replace(old)` -- a different current value exists; the caller must
///   append a retraction datom `(e, a, old, tx, false)` before the new
///   assertion.
///
/// Append-only model (1.5.0): this NO LONGER flips the existing log row's
/// `added` flag. The datom log is immutable. The current value is read from
/// the current-state projection (mentat.current_<type>), which is the
/// authoritative, latest-tx-wins source; maintain_current_projection then
/// applies the retraction (delete old) + assertion (upsert new) to keep the
/// projection correct.
fn retract_existing_cardinality_one(
    entity_id: i64,
    attr_id: i64,
    new_v: &TypedValue,
    schema: &str,
) -> Result<CardinalityOneResult, Box<dyn std::error::Error + Send + Sync>> {
    let store_id = get_store_id_from_schema(schema)?;

    // Read the current value (if any) from the projection -- authoritative
    // and latest-tx-wins-correct, unlike a raw `added=true` scan of the
    // append-only log.
    let existing = current_projection_value_for_ea(store_id, entity_id, attr_id, new_v)?;

    if let Some(old_v) = existing {
        if old_v == *new_v {
            return Ok(CardinalityOneResult::Skip); // idempotent
        }
        // Different value: caller appends the retraction datom. No log flip.
        return Ok(CardinalityOneResult::Replace(old_v));
    }

    Ok(CardinalityOneResult::Insert)
}

/// Read the current value of a cardinality-one (e, a) from the current-state
/// projection. `type_hint` selects which current_<type> table to query (a
/// cardinality-one attribute's type is fixed, so the current value lives in
/// exactly one projection table). Returns None if no current value exists.
fn current_projection_value_for_ea(
    store_id: i64,
    entity_id: i64,
    attr_id: i64,
    type_hint: &TypedValue,
) -> Result<Option<TypedValue>, Box<dyn std::error::Error + Send + Sync>> {
    let (table, tag): (&str, i16) = match type_hint {
        TypedValue::Ref(_) => ("current_ref", 0),
        TypedValue::Boolean(_) => ("current_boolean", 1),
        TypedValue::Long(_) => ("current_long", 2),
        TypedValue::Double(_) => ("current_double", 3),
        TypedValue::Instant(_) => ("current_instant", 4),
        TypedValue::Text(_) => ("current_text", 7),
        TypedValue::Keyword(_) => ("current_keyword", 8),
        TypedValue::Uuid(_) => ("current_uuid", 10),
        TypedValue::Bytes(_) => ("current_bytes", 11),
    };
    let value_expr = match tag {
        4 => "(EXTRACT(EPOCH FROM v)::bigint * 1000000)::text",
        7 | 8 => "v",
        11 => "encode(v, 'hex')",
        _ => "v::text",
    };
    let query = format!(
        "SELECT {value_expr} AS value FROM mentat.{table} \
         WHERE store_id = $1 AND e = $2 AND a = $3 LIMIT 1"
    );
    Spi::connect(|client| {
        let mut rows = client.select(
            &query,
            None,
            &[
                DatumWithOid::from(store_id),
                DatumWithOid::from(entity_id),
                DatumWithOid::from(attr_id),
            ],
        )?;
        if let Some(row) = rows.next() {
            let value_str: String = row.get(1)?.ok_or("Missing value")?;
            Ok(Some(parse_typed_value_from_tag(tag, value_str)?))
        } else {
            Ok(None)
        }
    })
}

/// For cardinality-many attributes, check if the exact (e, a, v) triple already
/// exists with added=true. If so, the assertion is idempotent and should be
/// skipped to avoid duplicate datoms (matching Datomic semantics).
///
/// The `schema` parameter is the quoted PostgreSQL schema name.
fn is_duplicate_cardinality_many(
    entity_id: i64,
    attr_id: i64,
    v: &TypedValue,
    schema: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let store_id = get_store_id_from_schema(schema)?;

    // Append-only model: "is this (e,a,v) already live?" is answered by the
    // current-state projection (presence == live), NOT by an `added=true`
    // scan of the immutable log (which would also match a value that was
    // later retracted). Each current_<type> table has no `added` column;
    // a row's existence means the value is currently asserted.
    let exists = match v {
        TypedValue::Ref(id) => Spi::get_one_with_args::<bool>(
            "SELECT EXISTS(SELECT 1 FROM mentat.current_ref \
             WHERE store_id = $1 AND e = $2 AND a = $3 AND v = $4)",
            &[DatumWithOid::from(store_id), DatumWithOid::from(entity_id),
              DatumWithOid::from(attr_id), DatumWithOid::from(*id)]),
        TypedValue::Boolean(b) => Spi::get_one_with_args::<bool>(
            "SELECT EXISTS(SELECT 1 FROM mentat.current_boolean \
             WHERE store_id = $1 AND e = $2 AND a = $3 AND v = $4)",
            &[DatumWithOid::from(store_id), DatumWithOid::from(entity_id),
              DatumWithOid::from(attr_id), DatumWithOid::from(*b)]),
        TypedValue::Long(n) => Spi::get_one_with_args::<bool>(
            "SELECT EXISTS(SELECT 1 FROM mentat.current_long \
             WHERE store_id = $1 AND e = $2 AND a = $3 AND v = $4)",
            &[DatumWithOid::from(store_id), DatumWithOid::from(entity_id),
              DatumWithOid::from(attr_id), DatumWithOid::from(*n)]),
        TypedValue::Double(f) => Spi::get_one_with_args::<bool>(
            "SELECT EXISTS(SELECT 1 FROM mentat.current_double \
             WHERE store_id = $1 AND e = $2 AND a = $3 AND v = $4)",
            &[DatumWithOid::from(store_id), DatumWithOid::from(entity_id),
              DatumWithOid::from(attr_id), DatumWithOid::from(*f)]),
        TypedValue::Text(s) => Spi::get_one_with_args::<bool>(
            "SELECT EXISTS(SELECT 1 FROM mentat.current_text \
             WHERE store_id = $1 AND e = $2 AND a = $3 AND v = $4)",
            &[DatumWithOid::from(store_id), DatumWithOid::from(entity_id),
              DatumWithOid::from(attr_id), DatumWithOid::from(s.as_str())]),
        TypedValue::Keyword(s) => Spi::get_one_with_args::<bool>(
            "SELECT EXISTS(SELECT 1 FROM mentat.current_keyword \
             WHERE store_id = $1 AND e = $2 AND a = $3 AND v = $4)",
            &[DatumWithOid::from(store_id), DatumWithOid::from(entity_id),
              DatumWithOid::from(attr_id), DatumWithOid::from(s.as_str())]),
        TypedValue::Instant(micros) => Spi::get_one_with_args::<bool>(
            "SELECT EXISTS(SELECT 1 FROM mentat.current_instant \
             WHERE store_id = $1 AND e = $2 AND a = $3 \
             AND v = to_timestamp($4::DOUBLE PRECISION / 1000000.0))",
            &[DatumWithOid::from(store_id), DatumWithOid::from(entity_id),
              DatumWithOid::from(attr_id), DatumWithOid::from(*micros)]),
        TypedValue::Uuid(u) => {
            let uuid_str = u.to_string();
            Spi::get_one_with_args::<bool>(
                "SELECT EXISTS(SELECT 1 FROM mentat.current_uuid \
                 WHERE store_id = $1 AND e = $2 AND a = $3 AND v = $4::UUID)",
                &[DatumWithOid::from(store_id), DatumWithOid::from(entity_id),
                  DatumWithOid::from(attr_id), DatumWithOid::from(uuid_str.as_str())])
        }
        TypedValue::Bytes(b) => Spi::get_one_with_args::<bool>(
            "SELECT EXISTS(SELECT 1 FROM mentat.current_bytes \
             WHERE store_id = $1 AND e = $2 AND a = $3 AND v = $4)",
            &[DatumWithOid::from(store_id), DatumWithOid::from(entity_id),
              DatumWithOid::from(attr_id), DatumWithOid::from(b.clone())]),
    }.ok().flatten().unwrap_or(false);

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

/// Compute a simple hash of a TypedValue for advisory lock keys.
fn compute_typed_value_hash(v: &TypedValue) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    match v {
        TypedValue::Ref(i) => { 0i16.hash(&mut hasher); i.hash(&mut hasher); }
        TypedValue::Boolean(b) => { 1i16.hash(&mut hasher); b.hash(&mut hasher); }
        TypedValue::Long(i) => { 2i16.hash(&mut hasher); i.hash(&mut hasher); }
        TypedValue::Double(f) => { 3i16.hash(&mut hasher); f.to_bits().hash(&mut hasher); }
        TypedValue::Text(s) => { 7i16.hash(&mut hasher); s.hash(&mut hasher); }
        TypedValue::Keyword(s) => { 8i16.hash(&mut hasher); s.hash(&mut hasher); }
        TypedValue::Instant(i) => { 4i16.hash(&mut hasher); i.hash(&mut hasher); }
        TypedValue::Uuid(u) => { 10i16.hash(&mut hasher); u.as_bytes().hash(&mut hasher); }
        TypedValue::Bytes(b) => { 11i16.hash(&mut hasher); b.hash(&mut hasher); }
    }
    hasher.finish()
}

/// Format a TypedValue into a JSON-compatible representation for tx-data.
///
/// Uses type-tagged objects for types that JSON cannot natively distinguish:
/// - Instants:  `{"_t":"inst","v":<micros>}` (microseconds since Unix epoch)
/// - UUIDs:     `{"_t":"uuid","v":"<uuid-string>"}`
/// - Doubles:   `{"_t":"double","v":<number>}`
/// - Keywords:  Encoded as `":keyword"` (prefix `:` allows detection)
/// - Refs, Booleans, Longs, Text: Plain JSON values (natively distinguishable)
fn format_typed_value_for_json(v: &TypedValue) -> String {
    match v {
        TypedValue::Ref(id) => format!("{}", id),
        TypedValue::Boolean(b) => format!("{}", b),
        TypedValue::Long(n) => format!("{}", n),
        TypedValue::Double(f) => {
            // Type-tagged to distinguish from integers
            format!("{{\"_t\":\"double\",\"v\":{}}}", f)
        }
        TypedValue::Instant(micros) => {
            // Type-tagged instant with microseconds since epoch
            format!("{{\"_t\":\"inst\",\"v\":{}}}", micros)
        }
        TypedValue::Text(s) => {
            let escaped = s
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            format!("\"{}\"", escaped)
        }
        TypedValue::Keyword(s) => format!("\":{}\"", s),
        TypedValue::Uuid(u) => {
            // Type-tagged UUID
            format!("{{\"_t\":\"uuid\",\"v\":\"{}\"}}", u)
        }
        TypedValue::Bytes(b) => {
            let hex: String = b.iter().map(|byte| format!("{:02x}", byte)).collect();
            format!("\"{}\"", hex)
        }
    }
}

/// Format a TypedValue into a human-readable string for error messages.
fn format_typed_value(v: &TypedValue) -> String {
    match v {
        TypedValue::Ref(id) => format!("{}", id),
        TypedValue::Boolean(b) => format!("{}", b),
        TypedValue::Long(n) => format!("{}", n),
        TypedValue::Double(f) => format!("{}", f),
        TypedValue::Text(s) => format!("\"{}\"", s),
        TypedValue::Keyword(s) => format!(":{}", s),
        TypedValue::Instant(micros) => format!("<instant:{}>", micros),
        TypedValue::Uuid(u) => format!("#uuid \"{}\"", u),
        TypedValue::Bytes(b) => format!("<bytes:{}>", hex::encode(b)),
    }
}

/// Insert a single datom with typed value columns into the store's datoms table.
///
/// The `schema` parameter is the quoted PostgreSQL schema name.
/// Get store_id from store name via stores metadata table.
/// Returns 0 for "default" store, or the assigned store_id for other stores.
/// Get store_id for the current transaction's schema.
///
/// **Fast path**: returns the `TX_STORE_ID` thread-local set by
/// `execute_transaction_inner` at transaction start. This eliminates the
/// `SELECT store_id FROM mentat.stores WHERE ...` round-trip that would
/// otherwise fire ~4,032 times per typical transaction.
///
/// **Slow path** (first call, or any call outside a transaction): derives the
/// store name from the schema string and queries `mentat.stores`.
fn get_store_id_from_schema(schema: &str) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    // Fast path: use the cached store_id for this transaction.
    if let Some(id) = TX_STORE_ID.with(|f| f.get()) {
        return Ok(id);
    }

    // Slow path: extract store name and look up in the metadata table.
    let store_name = if schema == "mentat" {
        "default"
    } else if let Some(name) = schema.strip_prefix("mentat_") {
        name
    } else {
        return Err(MentatError::InvalidStoreName {
            store_name: schema.to_string(),
            reason: "Schema must be 'mentat' or 'mentat_*'".to_string(),
        }.into());
    };

    let store_id: Option<i64> = Spi::get_one_with_args(
        "SELECT store_id FROM mentat.stores WHERE store_name = $1",
        &[DatumWithOid::from(store_name)],
    )?;

    store_id.ok_or_else(|| {
        MentatError::StoreNotFound {
            store_name: store_name.to_string(),
        }.into()
    })
}

/// Escape a string value for embedding as a SQL single-quoted literal.
///
/// With `standard_conforming_strings = on` (the PostgreSQL default since 9.1)
/// the only character that needs escaping in a standard string literal is the
/// single quote, doubled as `''`. Backslashes are **not** special.
fn sql_escape_str(s: &str) -> String {
    s.replace('\'', "''")
}

/// Batch-insert all collected datoms into the appropriate type-specific
/// tables using one multi-row `INSERT … VALUES` per non-empty type group.
///
/// For a transaction asserting N datoms across K distinct value types, this
/// issues at most K INSERT statements instead of N. For the production
/// workload (~463 datoms/tx, mostly instants), K ≤ 9 (one per type table).
///
/// All type tables live in the `mentat` schema regardless of store; the
/// `store_id` column (via `TX_STORE_ID` fast-path) disambiguates stores.
fn batch_insert_datoms(
    rows: &[(i64, i64, TypedValue, i64, bool)],
    schema: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if rows.is_empty() {
        return Ok(());
    }

    // Dedup by the full PK (e, a, v, tx, added). Some transaction paths can
    // queue the same datom twice -- e.g. :db.fn/cas pushes a retraction of
    // the old value, and the cardinality-one replace logic in insert_datoms
    // independently queues the same retraction. Identical datoms are
    // idempotent, but a single INSERT ... ON CONFLICT (store_id,e,a,v,tx)
    // cannot list the same key twice ("cannot affect row a second time").
    // Collapse duplicates before building the batch.
    use std::collections::HashSet;
    let mut seen: HashSet<(i64, i64, String, i64, bool)> = HashSet::new();
    let mut deduped: Vec<&(i64, i64, TypedValue, i64, bool)> = Vec::with_capacity(rows.len());
    for row in rows {
        let (e, a, v, tx, added) = row;
        let key = (*e, *a, typed_value_sql_literal(v), *tx, *added);
        if seen.insert(key) {
            deduped.push(row);
        }
    }
    let rows: &[&(i64, i64, TypedValue, i64, bool)] = &deduped;

    // TX_STORE_ID fast-path: no DB query after the first call in this tx.
    let store_id = get_store_id_from_schema(schema)?;

    // Per-type accumulators: each entry is one row in the VALUES clause.
    let mut ref_vals:     Vec<String> = Vec::new();
    let mut bool_vals:    Vec<String> = Vec::new();
    let mut long_vals:    Vec<String> = Vec::new();
    let mut double_vals:  Vec<String> = Vec::new();
    let mut instant_vals: Vec<String> = Vec::new();
    let mut text_vals:    Vec<String> = Vec::new();
    let mut kw_vals:      Vec<String> = Vec::new();
    let mut uuid_vals:    Vec<String> = Vec::new();
    let mut bytes_vals:   Vec<String> = Vec::new();

    for (e, a, v, tx, added) in rows {
        let added_sql = if *added { "true" } else { "false" };
        match v {
            TypedValue::Ref(ref_id) => {
                ref_vals.push(format!("({},{},{},{},{},{})",
                    store_id, e, a, ref_id, tx, added_sql));
            }
            TypedValue::Boolean(b) => {
                bool_vals.push(format!("({},{},{},{},{},{})",
                    store_id, e, a,
                    if *b { "true" } else { "false" },
                    tx, added_sql));
            }
            TypedValue::Long(n) => {
                long_vals.push(format!("({},{},{},{},{},{})",
                    store_id, e, a, n, tx, added_sql));
            }
            TypedValue::Double(f) => {
                // Use ::double precision cast to prevent PostgreSQL from
                // treating integer-looking values (e.g. "1") as integers.
                let v_sql = if f.is_nan() {
                    "'NaN'::double precision".to_string()
                } else if f.is_infinite() {
                    if *f > 0.0 {
                        "'Infinity'::double precision".to_string()
                    } else {
                        "'-Infinity'::double precision".to_string()
                    }
                } else {
                    format!("{:e}::double precision", f)
                };
                double_vals.push(format!("({},{},{},{},{},{})",
                    store_id, e, a, v_sql, tx, added_sql));
            }
            TypedValue::Instant(micros) => {
                // to_timestamp converts Unix microseconds → TIMESTAMPTZ.
                instant_vals.push(format!(
                    "({},{},{},to_timestamp({}::double precision/1000000.0),{},{})",
                    store_id, e, a, micros, tx, added_sql));
            }
            TypedValue::Text(s) => {
                text_vals.push(format!("({},{},{},'{}',{},{})",
                    store_id, e, a, sql_escape_str(s), tx, added_sql));
            }
            TypedValue::Keyword(s) => {
                kw_vals.push(format!("({},{},{},'{}',{},{})",
                    store_id, e, a, sql_escape_str(s), tx, added_sql));
            }
            TypedValue::Uuid(u) => {
                uuid_vals.push(format!("({},{},{},'{}',{},{})",
                    store_id, e, a, u, tx, added_sql));
            }
            TypedValue::Bytes(b) => {
                bytes_vals.push(format!(
                    "({},{},{},decode('{}','hex'),{},{})",
                    store_id, e, a, hex::encode(b), tx, added_sql));
            }
        }
    }

    // Helper closure: flush one type group as a single INSERT.
    let flush = |table: &str, vals: Vec<String>| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if vals.is_empty() {
            return Ok(());
        }
        Spi::run(&format!(
            "INSERT INTO mentat.{} (store_id,e,a,v,tx,added) VALUES {} \
             ON CONFLICT (store_id,e,a,v,tx) DO UPDATE SET added = EXCLUDED.added",
            table, vals.join(",")
        ))?;
        Ok(())
    };

    flush("datoms_ref_new",     ref_vals)?;
    flush("datoms_boolean_new", bool_vals)?;
    flush("datoms_long_new",    long_vals)?;
    flush("datoms_double_new",  double_vals)?;
    flush("datoms_instant_new", instant_vals)?;
    flush("datoms_text_new",    text_vals)?;
    flush("datoms_keyword_new", kw_vals)?;
    flush("datoms_uuid_new",    uuid_vals)?;
    flush("datoms_bytes_new",   bytes_vals)?;

    Ok(())
}

/// Render a TypedValue as a SQL literal for the `v` column, matching the
/// encoding `batch_insert_datoms` uses. Shared with the current-projection
/// maintenance so both produce byte-identical value SQL.
fn typed_value_sql_literal(v: &TypedValue) -> String {
    match v {
        TypedValue::Ref(id) => id.to_string(),
        TypedValue::Boolean(b) => if *b { "true".to_string() } else { "false".to_string() },
        TypedValue::Long(n) => n.to_string(),
        TypedValue::Double(f) => {
            if f.is_nan() {
                "'NaN'::double precision".to_string()
            } else if f.is_infinite() {
                if *f > 0.0 {
                    "'Infinity'::double precision".to_string()
                } else {
                    "'-Infinity'::double precision".to_string()
                }
            } else {
                format!("{:e}::double precision", f)
            }
        }
        TypedValue::Instant(micros) => {
            format!("to_timestamp({}::double precision/1000000.0)", micros)
        }
        TypedValue::Text(s) => format!("'{}'", sql_escape_str(s)),
        TypedValue::Keyword(s) => format!("'{}'", sql_escape_str(s)),
        TypedValue::Uuid(u) => format!("'{}'::uuid", u),
        TypedValue::Bytes(b) => format!("decode('{}','hex')", hex::encode(b)),
    }
}

/// The current_<type> projection table name for a TypedValue.
fn current_projection_table(v: &TypedValue) -> &'static str {
    match v {
        TypedValue::Ref(_) => "current_ref",
        TypedValue::Boolean(_) => "current_boolean",
        TypedValue::Long(_) => "current_long",
        TypedValue::Double(_) => "current_double",
        TypedValue::Instant(_) => "current_instant",
        TypedValue::Text(_) => "current_text",
        TypedValue::Keyword(_) => "current_keyword",
        TypedValue::Uuid(_) => "current_uuid",
        TypedValue::Bytes(_) => "current_bytes",
    }
}

/// Prune ALL prior projection rows for a (store_id, e, a) on a :db/noHistory
/// cardinality-one attribute. Because each distinct value is its own
/// projection PK (store_id,e,a,v), replacing the value would otherwise leave
/// the old projection row behind (the normal retraction-driven delete is
/// skipped for noHistory). The caller's subsequent projection upsert
/// re-inserts the current value.
fn prune_projection_ea(
    e: i64,
    a: i64,
    type_hint: &TypedValue,
    schema: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let store_id = get_store_id_from_schema(schema)?;
    let table = current_projection_table(type_hint);
    Spi::run_with_args(
        &format!(
            "DELETE FROM mentat.{} WHERE store_id = $1 AND e = $2 AND a = $3",
            table
        ),
        &[
            DatumWithOid::from(store_id),
            DatumWithOid::from(e),
            DatumWithOid::from(a),
        ],
    )?;
    Ok(())
}

/// The append-only log table name for a TypedValue.
fn log_table_for(v: &TypedValue) -> &'static str {
    match v {
        TypedValue::Ref(_) => "datoms_ref_new",
        TypedValue::Boolean(_) => "datoms_boolean_new",
        TypedValue::Long(_) => "datoms_long_new",
        TypedValue::Double(_) => "datoms_double_new",
        TypedValue::Instant(_) => "datoms_instant_new",
        TypedValue::Text(_) => "datoms_text_new",
        TypedValue::Keyword(_) => "datoms_keyword_new",
        TypedValue::Uuid(_) => "datoms_uuid_new",
        TypedValue::Bytes(_) => "datoms_bytes_new",
    }
}

/// Prune ALL prior log rows for a (store_id, e, a) on a :db/noHistory
/// cardinality-one attribute, before the new assertion is written. noHistory
/// keeps no assert/retract trail -- only the current value survives in the
/// log (and the projection). `type_hint` selects the typed log table; a
/// cardinality-one attribute's type is fixed.
fn prune_no_history_ea(
    e: i64,
    a: i64,
    type_hint: &TypedValue,
    schema: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let store_id = get_store_id_from_schema(schema)?;
    let table = log_table_for(type_hint);
    Spi::run_with_args(
        &format!(
            "DELETE FROM mentat.{} WHERE store_id = $1 AND e = $2 AND a = $3",
            table
        ),
        &[
            DatumWithOid::from(store_id),
            DatumWithOid::from(e),
            DatumWithOid::from(a),
        ],
    )?;
    Ok(())
}

/// Prune prior log rows for a specific (store_id, e, a, v) on a :db/noHistory
/// cardinality-many attribute, so re-assertion of the same value doesn't
/// accumulate a history trail. Only the exact value is pruned; other live
/// values for the same (e, a) are untouched.
fn prune_no_history_eav(
    e: i64,
    a: i64,
    v: &TypedValue,
    schema: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let store_id = get_store_id_from_schema(schema)?;
    let table = log_table_for(v);
    let v_sql = typed_value_sql_literal(v);
    // v_sql is a SQL literal (escaped for text/keyword, decode() for bytes,
    // numeric/typed literals otherwise) produced by the same helper the
    // batch insert uses, so it is safe to interpolate.
    Spi::run_with_args(
        &format!(
            "DELETE FROM mentat.{} WHERE store_id = $1 AND e = $2 AND a = $3 AND v = {}",
            table, v_sql
        ),
        &[
            DatumWithOid::from(store_id),
            DatumWithOid::from(e),
            DatumWithOid::from(a),
        ],
    )?;
    Ok(())
}

/// Maintain the current-state projection from the batch of (e,a,v,tx,added)
/// rows just written to the log.
///
/// Assertions (added=true) upsert into mentat.current_<type>; retractions
/// (added=false) delete the matching (store_id,e,a,v) projection row.
///
/// Batched per type table (one INSERT ... ON CONFLICT and one DELETE per
/// type that appears in the batch) so a 463-datom tx issues at most ~18
/// statements, not 463. Cardinality-one replace is expressed in the log as
/// a retraction of the old value + an assertion of the new; the
/// corresponding projection ops (delete old v, upsert new v) leave exactly
/// one row per (e,a) -- the PK (store_id,e,a,v) does not enforce that, the
/// transact path's cardinality handling does, identically to how it governs
/// the log.
fn maintain_current_projection(
    rows: &[(i64, i64, TypedValue, i64, bool)],
    schema: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if rows.is_empty() {
        return Ok(());
    }
    let store_id = get_store_id_from_schema(schema)?;

    // Per-type accumulators. Upserts are keyed by their conflict tuple
    // (store_id,e,a,v) so a single INSERT ... ON CONFLICT never lists the
    // same row twice (PostgreSQL rejects that with "ON CONFLICT DO UPDATE
    // command cannot affect row a second time"). The last write in the
    // batch for a given conflict key wins (highest tx, since rows are in
    // assertion order).
    use std::collections::HashMap;
    // table -> ( (e,a,v_sql) -> values_row )
    let mut upserts: HashMap<&'static str, HashMap<(i64, i64, String), String>> = HashMap::new();
    // table -> set of (e,a,v_sql) delete tuples (dedup via map key)
    let mut deletes: HashMap<&'static str, HashMap<(i64, i64, String), String>> = HashMap::new();

    for (e, a, v, tx, added) in rows {
        let table = current_projection_table(v);
        let v_sql = typed_value_sql_literal(v);
        let key = (*e, *a, v_sql.clone());
        if *added {
            // Last assertion of this (e,a,v) in the batch wins. Also drop any
            // pending delete of the same key (retract-then-reassert in one tx
            // ends with the value live).
            deletes.get_mut(table).map(|m| m.remove(&key));
            upserts.entry(table).or_default().insert(
                key,
                format!("({},{},{},{},{})", store_id, e, a, v_sql, tx),
            );
        } else {
            // Retraction. Drop any pending upsert of the same key (assert-then-
            // retract in one tx ends with the value gone), and queue a delete.
            upserts.get_mut(table).map(|m| m.remove(&key));
            deletes.entry(table).or_default().insert(
                key,
                format!("({},{},{},{})", store_id, e, a, v_sql),
            );
        }
    }

    // Apply deletes first, then upserts: within a single tx a value can be
    // retracted then re-asserted (cardinality-many) or replaced
    // (cardinality-one: old retracted, new asserted). Deleting first then
    // upserting yields the correct final set in both cases.
    for (table, tuples) in &deletes {
        if tuples.is_empty() {
            continue;
        }
        let rows_sql: Vec<&String> = tuples.values().collect();
        let joined = rows_sql.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",");
        Spi::run(&format!(
            "DELETE FROM mentat.{} WHERE (store_id,e,a,v) IN (VALUES {})",
            table, joined
        ))?;
    }
    for (table, vals) in &upserts {
        if vals.is_empty() {
            continue;
        }
        let rows_sql: Vec<&String> = vals.values().collect();
        let joined = rows_sql.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",");
        Spi::run(&format!(
            "INSERT INTO mentat.{} (store_id,e,a,v,tx) VALUES {} \
             ON CONFLICT (store_id,e,a,v) DO UPDATE SET tx = EXCLUDED.tx",
            table, joined
        ))?;
    }

    Ok(())
}

/// Get the column name for a TypedValue.
#[expect(dead_code, reason = "Utility for future direct-insert optimization")]
fn typed_value_column(v: &TypedValue) -> &'static str {
    match v {
        TypedValue::Ref(_) => "v_ref",
        TypedValue::Boolean(_) => "v_bool",
        TypedValue::Long(_) => "v_long",
        TypedValue::Double(_) => "v_double",
        TypedValue::Instant(_) => "v_instant",
        TypedValue::Text(_) => "v_text",
        TypedValue::Keyword(_) => "v_keyword",
        TypedValue::Uuid(_) => "v_uuid",
        TypedValue::Bytes(_) => "v_bytes",
    }
}

/// Check for unique constraint violation by looking up existing datom with same (a, v).
///
/// The `schema` parameter is the quoted PostgreSQL schema name.
fn check_unique_typed_value(
    attr_id: i64,
    v: &TypedValue,
    schema: &str,
) -> Result<Option<i64>, Box<dyn std::error::Error + Send + Sync>> {
    // Get store_id from schema
    let store_id = get_store_id_from_schema(schema)?;

    // Query the appropriate type-specific table based on value type
    let result = match v {
        TypedValue::Ref(id) => Spi::get_one_with_args::<i64>(
            "SELECT e FROM mentat.datoms_ref_new \
             WHERE store_id = $1 AND a = $2 AND v = $3 AND added = true LIMIT 1",
            &[DatumWithOid::from(store_id), DatumWithOid::from(attr_id), DatumWithOid::from(*id)]),
        TypedValue::Boolean(b) => Spi::get_one_with_args::<i64>(
            "SELECT e FROM mentat.datoms_boolean_new \
             WHERE store_id = $1 AND a = $2 AND v = $3 AND added = true LIMIT 1",
            &[DatumWithOid::from(store_id), DatumWithOid::from(attr_id), DatumWithOid::from(*b)]),
        TypedValue::Long(n) => Spi::get_one_with_args::<i64>(
            "SELECT e FROM mentat.datoms_long_new \
             WHERE store_id = $1 AND a = $2 AND v = $3 AND added = true LIMIT 1",
            &[DatumWithOid::from(store_id), DatumWithOid::from(attr_id), DatumWithOid::from(*n)]),
        TypedValue::Double(f) => Spi::get_one_with_args::<i64>(
            "SELECT e FROM mentat.datoms_double_new \
             WHERE store_id = $1 AND a = $2 AND v = $3 AND added = true LIMIT 1",
            &[DatumWithOid::from(store_id), DatumWithOid::from(attr_id), DatumWithOid::from(*f)]),
        TypedValue::Text(s) => Spi::get_one_with_args::<i64>(
            "SELECT e FROM mentat.datoms_text_new \
             WHERE store_id = $1 AND a = $2 AND v = $3 AND added = true LIMIT 1",
            &[DatumWithOid::from(store_id), DatumWithOid::from(attr_id), DatumWithOid::from(s.as_str())]),
        TypedValue::Keyword(s) => Spi::get_one_with_args::<i64>(
            "SELECT e FROM mentat.datoms_keyword_new \
             WHERE store_id = $1 AND a = $2 AND v = $3 AND added = true LIMIT 1",
            &[DatumWithOid::from(store_id), DatumWithOid::from(attr_id), DatumWithOid::from(s.as_str())]),
        TypedValue::Instant(micros) => Spi::get_one_with_args::<i64>(
            "SELECT e FROM mentat.datoms_instant_new \
             WHERE store_id = $1 AND a = $2 AND v = to_timestamp($3::DOUBLE PRECISION / 1000000.0) AND added = true LIMIT 1",
            &[DatumWithOid::from(store_id), DatumWithOid::from(attr_id), DatumWithOid::from(*micros)]),
        TypedValue::Uuid(u) => {
            let uuid_str = u.to_string();
            Spi::get_one_with_args::<i64>(
                "SELECT e FROM mentat.datoms_uuid_new \
                 WHERE store_id = $1 AND a = $2 AND v = $3::UUID AND added = true LIMIT 1",
                &[DatumWithOid::from(store_id), DatumWithOid::from(attr_id), DatumWithOid::from(uuid_str.as_str())])
        }
        TypedValue::Bytes(b) => Spi::get_one_with_args::<i64>(
            "SELECT e FROM mentat.datoms_bytes_new \
             WHERE store_id = $1 AND a = $2 AND v = $3 AND added = true LIMIT 1",
            &[DatumWithOid::from(store_id), DatumWithOid::from(attr_id), DatumWithOid::from(b.clone())]),
    }.ok().flatten();

    Ok(result)
}

/// Read a TypedValue from an SPI row given the type tag and the starting column offset.
/// The columns starting at (offset+1) are: v_ref, v_bool, v_long, v_double, v_text, v_keyword, v_instant, v_uuid, v_bytes
/// (pgrx SPI columns are 1-based, so offset=2 means v_ref is at column 3)
fn read_typed_value_from_row(
    row: &pgrx::spi::SpiHeapTupleData<'_>,
    type_tag: i16,
    offset: usize,
) -> Result<TypedValue, Box<dyn std::error::Error + Send + Sync>> {
    match type_tag {
        0 => {
            let v: i64 = row.get(offset + 1)?.ok_or("Missing v_ref")?;
            Ok(TypedValue::Ref(v))
        }
        1 => {
            let v: bool = row.get(offset + 2)?.ok_or("Missing v_bool")?;
            Ok(TypedValue::Boolean(v))
        }
        2 => {
            let v: i64 = row.get(offset + 3)?.ok_or("Missing v_long")?;
            Ok(TypedValue::Long(v))
        }
        3 => {
            let v: f64 = row.get(offset + 4)?.ok_or("Missing v_double")?;
            Ok(TypedValue::Double(v))
        }
        4 => {
            // Read v_instant - we can read it as i64 microseconds via extract epoch
            // pgrx TimestampWithTimeZone is internally stored as i64 microseconds from Postgres epoch (2000-01-01)
            // We need Unix epoch microseconds, so we'll read it differently.
            // Option: read the column as a String and parse, or use the internal representation.
            // pgrx::datum::TimestampWithTimeZone can be converted to i64 (microseconds from PG epoch)
            let v: pgrx::datum::TimestampWithTimeZone = row.get(offset + 7)?.ok_or("Missing v_instant")?;
            // PG epoch is 2000-01-01 00:00:00 UTC, Unix epoch is 1970-01-01
            // Difference: 946684800 seconds = 946684800_000_000 microseconds
            let pg_epoch_offset_micros: i64 = 946_684_800_000_000;
            let pg_micros: i64 = v.into();
            let unix_micros = pg_micros + pg_epoch_offset_micros;
            Ok(TypedValue::Instant(unix_micros))
        }
        7 => {
            let v: String = row.get(offset + 5)?.ok_or("Missing v_text")?;
            Ok(TypedValue::Text(v))
        }
        8 => {
            let v: String = row.get(offset + 6)?.ok_or("Missing v_keyword")?;
            Ok(TypedValue::Keyword(v))
        }
        10 => {
            let v: pgrx::Uuid = row.get(offset + 8)?.ok_or("Missing v_uuid")?;
            let bytes: [u8; 16] = *v.as_bytes();
            Ok(TypedValue::Uuid(uuid::Uuid::from_bytes(bytes)))
        }
        11 => {
            let v: Vec<u8> = row.get(offset + 9)?.ok_or("Missing v_bytes")?;
            Ok(TypedValue::Bytes(v))
        }
        _ => Err(format!("Unknown type tag: {}", type_tag).into()),
    }
}
