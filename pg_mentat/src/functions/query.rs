use crate::error::MentatError;
use crate::functions::store_management::{get_schema_for_store, quote_ident};
use edn::parse;
use edn::query::{
    Binding, Direction, Element, FindSpec, FnArg, Limit, NonIntegerConstant, OrWhereClause, Order,
    ParsedQuery, PatternNonValuePlace, PatternValuePlace, Predicate, Rule, RuleInvocation,
    VariableOrPlaceholder, WhereClause, WhereFn,
};
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::spi::OwnedPreparedStatement;
use pgrx::JsonB;
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;

// ============================================================================
// Prepared Statement Cache
// ============================================================================

/// Cache entry storing a prepared statement and hit count for diagnostics.
struct CacheEntry {
    stmt: OwnedPreparedStatement,
    hits: u64,
}

/// Thread-local prepared statement cache.
///
/// Uses the generated SQL string as the cache key. Since PostgreSQL backends
/// are single-threaded, a `RefCell` is sufficient (no `Mutex` needed).
/// `OwnedPreparedStatement` uses `SPI_keepplan` to persist the plan in
/// `TopMemoryContext`, so it survives across SPI connection lifetimes.
thread_local! {
    static STMT_CACHE: RefCell<HashMap<String, CacheEntry>> = RefCell::new(HashMap::new());
}

/// Clear the prepared statement cache.
/// Called when schema changes invalidate cached plans.
pub fn clear_stmt_cache() {
    STMT_CACHE.with(|cache| {
        cache.borrow_mut().clear();
    });
}

/// Execute a SQL query using the prepared statement cache.
///
/// If a prepared statement for the given SQL already exists in the cache,
/// reuse it. Otherwise, prepare a new statement, cache it via `SPI_keepplan`,
/// and execute it.
///
/// Returns the `SpiTupleTable` for the caller to iterate over.
fn execute_cached_query<'a>(
    client: &pgrx::spi::SpiClient<'a>,
    sql: &str,
    params: &[DatumWithOid<'_>],
) -> Result<pgrx::spi::SpiTupleTable<'a>, pgrx::spi::SpiError> {
    // Try to execute with a cached statement.
    // The borrow of the RefCell is scoped to the closure and released
    // before the SpiTupleTable is used (it references SPI memory, not the plan).
    let cached_result: Option<Result<pgrx::spi::SpiTupleTable<'a>, _>> =
        STMT_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if let Some(entry) = cache.get_mut(sql) {
                entry.hits += 1;
                // Execute using the cached prepared statement.
                // SPI_execute_plan copies what it needs from the plan;
                // the returned SpiTupleTable only references SPI memory context.
                Some(client.select(&entry.stmt, None, params))
            } else {
                None
            }
        });

    if let Some(result) = cached_result {
        return result;
    }

    // Cache miss: prepare, execute, then cache the plan.
    let arg_types: Vec<PgOid> = params.iter().map(|p| PgOid::from_untagged(p.oid())).collect();
    let prepared = client.prepare(sql, &arg_types)?;

    // Execute using the borrowed prepared statement (does not consume it).
    let result = client.select(&prepared, None, params)?;

    // Keep the plan (moves it to TopMemoryContext) and cache it.
    let owned = prepared.keep();
    STMT_CACHE.with(|cache| {
        cache.borrow_mut().insert(
            sql.to_string(),
            CacheEntry {
                stmt: owned,
                hits: 0,
            },
        );
    });

    Ok(result)
}

/// Value type tags matching the encoding used in transact.rs and pull.rs:
///   0 = ref      (i64 entity ID, little-endian)
///   1 = boolean   (single byte: 0=false, 1=true)
///   2 = long      (i64 little-endian)
///   3 = double    (f64 little-endian)
///   4 = instant   (i64 microseconds since epoch, little-endian)
///   7 = string    (UTF-8 bytes)
///   8 = keyword   (UTF-8 bytes, stored without leading colon)
///  10 = uuid      (16 bytes, big-endian)
///  11 = bytes     (raw binary)
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

/// State accumulated during SQL generation: the parameterized query string
/// and the bound parameter values for safe execution via SPI.
struct SqlBuilder<'a> {
    params: Vec<DatumWithOid<'a>>,
}

impl<'a> SqlBuilder<'a> {
    fn new() -> Self {
        SqlBuilder { params: Vec::new() }
    }

    /// Add a TEXT parameter and return the placeholder string ($N).
    fn bind_text(&mut self, value: String) -> String {
        self.params.push(DatumWithOid::from(value));
        format!("${}", self.params.len())
    }

    /// Add a BIGINT parameter and return the placeholder string ($N).
    fn bind_bigint(&mut self, value: i64) -> String {
        self.params.push(DatumWithOid::from(value));
        format!("${}", self.params.len())
    }

    /// Add a BYTEA parameter and return the placeholder string ($N).
    fn bind_bytea(&mut self, value: Vec<u8>) -> String {
        self.params.push(DatumWithOid::from(value));
        format!("${}", self.params.len())
    }

    /// Add a BOOLEAN parameter and return the placeholder string ($N).
    fn bind_bool(&mut self, value: bool) -> String {
        self.params.push(DatumWithOid::from(value));
        format!("${}", self.params.len())
    }

    /// Add a DOUBLE PRECISION parameter and return the placeholder string ($N).
    fn bind_double(&mut self, value: f64) -> String {
        self.params.push(DatumWithOid::from(value));
        format!("${}", self.params.len())
    }
}

// ============================================================================
// Query Complexity and Optimizer Hints
// ============================================================================

/// Describes the complexity of a generated SQL query, used to decide
/// which optimizer hints (SET LOCAL) to apply before execution.
#[derive(Default)]
struct QueryComplexity {
    /// Number of datom table joins (pattern clauses).
    join_count: usize,
    /// Whether the query uses aggregates (COUNT, SUM, etc.).
    has_aggregates: bool,
    /// Whether the query uses CTEs (recursive rules).
    has_cte: bool,
    /// Whether the query uses UNION (OR-join clauses).
    has_union: bool,
}

impl QueryComplexity {
    /// A query is considered "complex" when it has multiple joins,
    /// aggregates, CTEs, or unions -- situations where extra work_mem
    /// and index-scan encouragement tend to help.
    fn is_complex(&self) -> bool {
        self.join_count > 2 || self.has_aggregates || self.has_cte || self.has_union
    }
}

/// Apply SET LOCAL optimizer hints and resource limits before executing a Mentat query.
///
/// Uses `Spi::run` to issue SET LOCAL statements in the current
/// transaction.  These settings revert automatically at transaction end.
///
/// Resource limits applied:
/// - `statement_timeout`: prevents runaway queries (mentat.query_timeout_ms)
/// - `temp_file_limit`: prevents disk exhaustion (mentat.temp_file_limit)
/// - `max_recursive_iterations`: prevents infinite recursion (mentat.max_recursion_depth)
/// - `enable_seqscan = off`: encourage index usage on datoms table
/// - `work_mem`: increased for complex queries (mentat.default_work_mem)
fn apply_optimizer_hints(
    client: &mut pgrx::spi::SpiClient<'_>,
    complexity: &QueryComplexity,
) {
    // --- Resource limits (always applied, regardless of optimizer hints setting) ---

    // Statement timeout: prevents runaway queries
    let timeout_ms = crate::planner::query_timeout_ms();
    if timeout_ms > 0 {
        let timeout_sql = format!("SET LOCAL statement_timeout = '{}'", timeout_ms);
        let _ = client.update(&timeout_sql, None, &[]);
    }

    // Temp file limit: prevents disk exhaustion from large sorts/hash joins
    let temp_limit = crate::planner::temp_file_limit();
    if temp_limit.chars().all(|c| c.is_ascii_alphanumeric()) {
        let temp_sql = format!("SET LOCAL temp_file_limit = '{}'", temp_limit);
        let _ = client.update(&temp_sql, None, &[]);
    }

    // Recursion depth limit: prevents infinite loops in recursive rules.
    // PostgreSQL 14+ supports max_recursive_iterations; older versions
    // ignore unknown GUCs set via SET LOCAL (no error, just a warning).
    if complexity.has_cte {
        let max_depth = crate::planner::max_recursion_depth();
        if max_depth > 0 {
            let depth_sql = format!("SET LOCAL max_recursive_iterations = {}", max_depth);
            let _ = client.update(&depth_sql, None, &[]);
        }
    }

    // --- Optimizer hints (only when enabled) ---

    if !crate::planner::optimizer_hints_enabled() {
        return;
    }

    // For any Mentat query that touches the datoms table, discourage
    // sequential scans so the planner prefers the covering indexes
    // (EAVT, AEVT, AVET, VAET).
    let _ = client.update("SET LOCAL enable_seqscan = off", None, &[]);

    // For complex queries, bump work_mem to allow larger in-memory
    // sorts and hash tables.
    if complexity.is_complex() {
        let work_mem = crate::planner::default_work_mem();
        // Defensive: only pass values that look like a memory size
        // (digits optionally followed by a unit suffix).
        if work_mem.chars().all(|c| c.is_ascii_alphanumeric()) {
            let set_sql = format!("SET LOCAL work_mem = '{}'", work_mem);
            let _ = client.update(&set_sql, None, &[]);
        }
    }
}

/// Temporal query options parsed from the inputs JSON parameter.
#[derive(Default)]
struct TemporalOption {
    /// If set, only include datoms with tx <= as_of_tx
    as_of: Option<i64>,
    /// If set, only include datoms with tx > since_tx
    since: Option<i64>,
    /// If true, include retracted datoms (added = false) and don't filter by tx
    history: bool,
}

/// Parse temporal options from the inputs JSON parameter.
fn parse_temporal_options(inputs: &serde_json::Value) -> TemporalOption {
    let mut opts = TemporalOption::default();
    if let Some(obj) = inputs.as_object() {
        if let Some(as_of) = obj.get("asOf").and_then(|v| v.as_i64()) {
            opts.as_of = Some(as_of);
        }
        if let Some(since) = obj.get("since").and_then(|v| v.as_i64()) {
            opts.since = Some(since);
        }
        if let Some(history) = obj.get("history").and_then(|v| v.as_bool()) {
            opts.history = history;
        }
    }
    opts
}

/// Pagination options parsed from the inputs JSON parameter.
#[derive(Default)]
struct PaginationOption {
    /// If set, limit the number of result rows returned.
    limit: Option<i64>,
    /// If set, skip this many result rows before returning.
    offset: Option<i64>,
}

/// Parse pagination options from the inputs JSON parameter.
fn parse_pagination_options(inputs: &serde_json::Value) -> PaginationOption {
    let mut opts = PaginationOption::default();
    if let Some(obj) = inputs.as_object() {
        if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
            if limit >= 0 {
                opts.limit = Some(limit);
            }
        }
        if let Some(offset) = obj.get("offset").and_then(|v| v.as_i64()) {
            if offset >= 0 {
                opts.offset = Some(offset);
            }
        }
    }
    opts
}

/// Parse :in clause input bindings from the inputs JSON parameter.
///
/// Matches the "inputs" JSON array positionally against the parsed query's
/// :in variables. For example, given `:in ?name ?age` and
/// `{"inputs": ["Alice", 30]}`, returns `{"?name": "Alice", "?age": 30}`.
fn parse_input_bindings(
    in_vars: &[edn::query::Variable],
    inputs_json: &serde_json::Value,
) -> HashMap<String, serde_json::Value> {
    let mut bindings = HashMap::new();
    if let Some(arr) = inputs_json
        .as_object()
        .and_then(|obj| obj.get("inputs"))
        .and_then(|v| v.as_array())
    {
        for (i, var) in in_vars.iter().enumerate() {
            if let Some(val) = arr.get(i) {
                let var_name = format!("{}", var);
                bindings.insert(var_name, val.clone());
            }
        }
    }
    bindings
}

/// Bind an :in clause variable to a WHERE constraint on a datom value column.
///
/// Encodes the JSON value as BYTEA with the appropriate type tag and adds
/// the constraint to match `alias.v` and `alias.value_type_tag`.
///
/// Also supports lookup ref arrays (e.g., `[":person/email", "alice@example.com"]`)
/// for ref-type attribute values. The lookup ref is resolved to an entity ID
/// and encoded as a ref value (i64 little-endian bytes with type tag 0).
fn bind_input_value(
    alias: &str,
    value: &serde_json::Value,
    builder: &mut SqlBuilder<'_>,
    schema_prefix: &str,
) -> Option<String> {
    match value {
        serde_json::Value::String(s) => {
            // Check if it looks like a keyword (starts with ':')
            if let Some(stripped) = s.strip_prefix(':') {
                let param = builder.bind_text(stripped.to_string());
                Some(format!(
                    "({alias}.v_keyword = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::KEYWORD
                ))
            } else {
                let param = builder.bind_text(s.clone());
                Some(format!(
                    "({alias}.v_text = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::STRING
                ))
            }
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                let param = builder.bind_bigint(i);
                Some(format!(
                    "({alias}.v_long = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::LONG
                ))
            } else if let Some(f) = n.as_f64() {
                let param = builder.bind_double(f);
                Some(format!(
                    "({alias}.v_double = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::DOUBLE
                ))
            } else {
                None
            }
        }
        serde_json::Value::Bool(b) => {
            let param = builder.bind_bool(*b);
            Some(format!(
                "({alias}.v_bool = {param} AND {alias}.value_type_tag = {tag})",
                tag = type_tag::BOOLEAN
            ))
        }
        serde_json::Value::Array(arr) => {
            // Lookup ref in value position: [":person/email", "alice@example.com"]
            let eid = resolve_lookup_ref_to_eid(arr, schema_prefix)?;
            let param = builder.bind_bigint(eid);
            Some(format!(
                "({alias}.v_ref = {param} AND {alias}.value_type_tag = {tag})",
                tag = type_tag::REF
            ))
        }
        _ => None,
    }
}

/// Bind an :in clause variable to a WHERE constraint on an entity column.
///
/// For entity-position variables, the bound value must be an integer entity ID
/// or a lookup ref array like `[":person/email", "alice@example.com"]`.
/// Lookup refs are resolved eagerly against the store's datoms table.
fn bind_input_entity(
    alias: &str,
    value: &serde_json::Value,
    builder: &mut SqlBuilder<'_>,
    schema_prefix: &str,
) -> Option<String> {
    if let Some(i) = value.as_i64() {
        let param = builder.bind_bigint(i);
        Some(format!("{alias}.e = {param}"))
    } else if let Some(arr) = value.as_array() {
        // Lookup ref: [":person/email", "alice@example.com"]
        let eid = resolve_lookup_ref_to_eid(arr, schema_prefix)?;
        let param = builder.bind_bigint(eid);
        Some(format!("{alias}.e = {param}"))
    } else {
        None
    }
}

/// Resolve a lookup ref JSON array to an entity ID.
///
/// The array must have exactly 2 elements: a keyword string (attribute ident
/// starting with ':') and a value. The attribute must have a unique constraint.
///
/// Returns the resolved entity ID, or None if the lookup ref is malformed
/// or cannot be resolved.
fn resolve_lookup_ref_to_eid(arr: &[serde_json::Value], schema_prefix: &str) -> Option<i64> {
    if arr.len() != 2 {
        return None;
    }

    // First element must be a keyword string (e.g., ":person/email")
    let attr_str = arr[0].as_str()?;
    if !attr_str.starts_with(':') {
        return None;
    }

    // Resolve the attribute ident to an entid via cache
    let attr_entid = crate::cache::get_cache().resolve_ident(attr_str)?;

    // Query for the entity with this unique attribute value using typed columns
    lookup_ref_query(attr_entid, &arr[1], schema_prefix)
}

/// Perform a lookup ref query against the typed value columns.
fn lookup_ref_query(attr_entid: i64, value: &serde_json::Value, schema_prefix: &str) -> Option<i64> {
    match value {
        serde_json::Value::String(s) => {
            if let Some(stripped) = s.strip_prefix(':') {
                Spi::get_one_with_args::<i64>(
                    &format!(
                        "SELECT e FROM {schema_prefix}datoms \
                         WHERE a = $1 AND v_keyword = $2 AND value_type_tag = 8 AND added = true LIMIT 1"
                    ),
                    &[DatumWithOid::from(attr_entid), DatumWithOid::from(stripped)],
                ).ok().flatten()
            } else {
                Spi::get_one_with_args::<i64>(
                    &format!(
                        "SELECT e FROM {schema_prefix}datoms \
                         WHERE a = $1 AND v_text = $2 AND value_type_tag = 7 AND added = true LIMIT 1"
                    ),
                    &[DatumWithOid::from(attr_entid), DatumWithOid::from(s.as_str())],
                ).ok().flatten()
            }
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Spi::get_one_with_args::<i64>(
                    &format!(
                        "SELECT e FROM {schema_prefix}datoms \
                         WHERE a = $1 AND v_long = $2 AND value_type_tag = 2 AND added = true LIMIT 1"
                    ),
                    &[DatumWithOid::from(attr_entid), DatumWithOid::from(i)],
                ).ok().flatten()
            } else if let Some(f) = n.as_f64() {
                Spi::get_one_with_args::<i64>(
                    &format!(
                        "SELECT e FROM {schema_prefix}datoms \
                         WHERE a = $1 AND v_double = $2 AND value_type_tag = 3 AND added = true LIMIT 1"
                    ),
                    &[DatumWithOid::from(attr_entid), DatumWithOid::from(f)],
                ).ok().flatten()
            } else {
                None
            }
        }
        serde_json::Value::Bool(b) => {
            Spi::get_one_with_args::<i64>(
                &format!(
                    "SELECT e FROM {schema_prefix}datoms \
                     WHERE a = $1 AND v_bool = $2 AND value_type_tag = 1 AND added = true LIMIT 1"
                ),
                &[DatumWithOid::from(attr_entid), DatumWithOid::from(*b)],
            ).ok().flatten()
        }
        _ => None,
    }
}

/// Resolve a store name to a qualified schema prefix (e.g., "mentat." or "mentat_my_store.").
///
/// The prefix includes the trailing dot, ready to be prepended to table names.
fn resolve_schema_prefix(store_name: &str) -> String {
    let schema = get_schema_for_store(store_name);
    format!("{}.", quote_ident(&schema))
}

/// Internal implementation of the Datalog query executor, parameterized by schema prefix.
///
/// All public query entry points delegate to this function.
pub(crate) fn mentat_query_internal(
    query: &str,
    inputs: JsonB,
    schema_prefix: &str,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let _parsed_value = parse::value(query)?;
    let parsed_query = mentat_core::parse_query(query)?;

    let temporal = parse_temporal_options(&inputs.0);
    let input_bindings = parse_input_bindings(&parsed_query.in_vars, &inputs.0);
    let has_aggregates = find_spec_has_aggregates(&parsed_query.find_spec);
    let find_vars = extract_find_variables(&parsed_query.find_spec);
    let pagination = parse_pagination_options(&inputs.0);

    let mut builder = SqlBuilder::new();
    let (mut sql_query, complexity) = build_sql_from_datalog(
        &parsed_query,
        &find_vars,
        &mut builder,
        &temporal,
        &input_bindings,
        schema_prefix,
    )?;

    // Apply pagination from inputs JSON. This appends LIMIT/OFFSET to the
    // generated SQL, overriding any Datalog :limit if both are present.
    let has_explicit_limit = pagination.limit.is_some()
        || sql_query.contains(" LIMIT ");
    if let Some(limit) = pagination.limit {
        // Remove any existing LIMIT clause (from Datalog :limit) to avoid
        // a SQL syntax error from duplicate LIMIT. The generated SQL always
        // uses uppercase " LIMIT " so we can search directly.
        if let Some(pos) = sql_query.rfind(" LIMIT ") {
            sql_query.truncate(pos);
        }
        sql_query.push_str(&format!(" LIMIT {}", limit));
    }
    if let Some(offset) = pagination.offset {
        sql_query.push_str(&format!(" OFFSET {}", offset));
    }

    // Enforce max result rows as a safety net. If no explicit LIMIT is set
    // and the GUC mentat.max_result_rows is positive, append a LIMIT clause
    // to prevent cartesian explosions from returning unbounded results.
    let max_rows = crate::planner::max_result_rows();
    if !has_explicit_limit && max_rows > 0 {
        // Request one extra row to detect truncation
        sql_query.push_str(&format!(" LIMIT {}", i64::from(max_rows) + 1));
    }

    let params = builder.params;
    let results = Spi::connect_mut(|client| {
        // Apply optimizer hints and resource limits (SET LOCAL) before
        // executing the query. These are transaction-local and revert
        // automatically.
        apply_optimizer_hints(client, &complexity);

        let mut rows_json = Vec::new();
        let row_limit = if !has_explicit_limit && max_rows > 0 {
            max_rows as usize
        } else {
            usize::MAX
        };

        for row in execute_cached_query(client, &sql_query, &params)
            .map_err(|e| Box::new(crate::error::MentatError::InvalidQuery {
                message: format!("SPI execution error: {}", e),
                suggestion: None,
            }) as Box<dyn std::error::Error + Send + Sync>)? {
            if rows_json.len() >= row_limit {
                return Err(Box::new(crate::error::MentatError::ResultLimitExceeded {
                    limit: max_rows,
                    message: format!(
                        "Query returned more than {} rows. \
                         Use :limit in your query, add more specific :where clauses, \
                         or increase mentat.max_result_rows",
                        max_rows
                    ),
                }) as Box<dyn std::error::Error + Send + Sync>);
            }

            let mut row_values = Vec::new();

            for (idx, _var) in find_vars.iter().enumerate() {
                let col_idx = (idx + 1) as usize;

                if let Ok(Some(val)) = row.get::<String>(col_idx) {
                    row_values.push(decode_text_result(&val));
                } else {
                    row_values.push(json!(null));
                }
            }

            rows_json.push(json!(row_values));
        }

        Ok(rows_json)
    })?;

    let response =
        format_find_response(&parsed_query.find_spec, &find_vars, results, has_aggregates);

    Ok(JsonB(response))
}

/// Execute a Datalog query and return results as JSON (default store)
///
/// Supports temporal options via the inputs JSON parameter:
/// - `{"asOf": <tx_id>}` - return datoms as of transaction tx_id
/// - `{"since": <tx_id>}` - return datoms since transaction tx_id
/// - `{"history": true}` - return all datom versions including retractions
///
/// Supports pagination via the inputs JSON parameter:
/// - `{"limit": 1000}` - return at most 1000 results
/// - `{"offset": 100}` - skip the first 100 results
/// - `{"limit": 1000, "offset": 100}` - return results 101-1100
///
/// When both a Datalog `:limit` clause and an inputs `limit` are specified,
/// the inputs `limit` takes precedence (it wraps the generated SQL).
#[pg_extern]
pub fn mentat_query(
    query: &str,
    inputs: JsonB,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    mentat_query_internal(query, inputs, "mentat.")
}

/// Execute a Datalog query against a named store and return results as JSON.
///
/// This is the 3-parameter version that allows specifying which store to query.
/// Use 'default' to query the default store.
///
/// # Example
/// ```sql
/// SELECT mentat_q_store('my_store', '[:find ?e ?name :where [?e :person/name ?name]]', '{}'::jsonb);
/// ```
#[pg_extern]
pub fn mentat_q_store(
    store_name: &str,
    query: &str,
    inputs: JsonB,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let schema_prefix = resolve_schema_prefix(store_name);
    mentat_query_internal(query, inputs, &schema_prefix)
}

/// Execute a Datalog query against a named store with explicit temporal control.
///
/// This is the full 4-parameter version. The `as_of_tx` parameter sets the
/// as-of transaction ID, overriding any `asOf` key in the inputs JSON.
///
/// # Example
/// ```sql
/// SELECT mentat_q_full('my_store', '[:find ?e ?name :where [?e :person/name ?name]]', '{}'::jsonb, 1000042);
/// ```
#[pg_extern]
pub fn mentat_q_full(
    store_name: &str,
    query: &str,
    inputs: JsonB,
    as_of_tx: i64,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    // Merge as_of_tx into the inputs JSON
    let mut inputs_obj = match inputs.0 {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    inputs_obj.insert("asOf".to_string(), json!(as_of_tx));
    let merged_inputs = JsonB(serde_json::Value::Object(inputs_obj));

    let schema_prefix = resolve_schema_prefix(store_name);
    mentat_query_internal(query, merged_inputs, &schema_prefix)
}

/// Execute a Datalog query against the default store (backwards-compatible alias).
///
/// Equivalent to `mentat_query(query, inputs)`.
///
/// # Example
/// ```sql
/// SELECT mentat_q_default('[:find ?e ?name :where [?e :person/name ?name]]', '{}'::jsonb);
/// ```
#[pg_extern]
pub fn mentat_q_default(
    query: &str,
    inputs: JsonB,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    mentat_query_internal(query, inputs, "mentat.")
}

/// Internal implementation of EXPLAIN for a Datalog query, parameterized by schema prefix.
fn mentat_explain_internal(
    query: &str,
    inputs: JsonB,
    schema_prefix: &str,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let _parsed_value = parse::value(query)?;
    let parsed_query = mentat_core::parse_query(query)?;

    let temporal = parse_temporal_options(&inputs.0);
    let input_bindings = parse_input_bindings(&parsed_query.in_vars, &inputs.0);
    let find_vars = extract_find_variables(&parsed_query.find_spec);
    let pagination = parse_pagination_options(&inputs.0);

    let mut builder = SqlBuilder::new();
    let (mut sql_query, _complexity) = build_sql_from_datalog(
        &parsed_query,
        &find_vars,
        &mut builder,
        &temporal,
        &input_bindings,
        schema_prefix,
    )?;

    // Apply pagination (same logic as mentat_query)
    if let Some(limit) = pagination.limit {
        if let Some(pos) = sql_query.rfind(" LIMIT ") {
            sql_query.truncate(pos);
        }
        sql_query.push_str(&format!(" LIMIT {}", limit));
    }
    if let Some(offset) = pagination.offset {
        sql_query.push_str(&format!(" OFFSET {}", offset));
    }

    // Prepend EXPLAIN (FORMAT JSON) to the query
    let explain_sql = format!("EXPLAIN (FORMAT JSON, VERBOSE) {}", sql_query);
    let params = builder.params;

    let plan_json = Spi::connect(|client| {
        let mut plan_rows = Vec::new();

        for row in client.select(&explain_sql, None, &params)? {
            if let Ok(Some(plan_str)) = row.get::<String>(1) {
                plan_rows.push(plan_str);
            }
        }

        // EXPLAIN returns multiple rows, concatenate them
        let full_plan = plan_rows.join("\n");
        let parsed_plan: serde_json::Value = serde_json::from_str(&full_plan)?;

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(json!({
            "datalog_query": query,
            "generated_sql": sql_query,
            "explain_plan": parsed_plan
        }))
    })?;

    Ok(JsonB(plan_json))
}

/// Get PostgreSQL query plan for a Datalog query (for debugging slow queries)
///
/// Returns EXPLAIN output showing how PostgreSQL will execute the generated SQL.
/// Useful for understanding index usage, join strategies, and query costs.
///
/// Example usage:
/// ```sql
/// SELECT mentat.mentat_explain(
///     '[:find ?e ?name :where [?e :person/name ?name]]',
///     '{}'::jsonb
/// );
/// ```
#[pg_extern]
pub fn mentat_explain(
    query: &str,
    inputs: JsonB,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    mentat_explain_internal(query, inputs, "mentat.")
}

/// Get PostgreSQL query plan for a Datalog query against a named store.
///
/// # Example
/// ```sql
/// SELECT mentat.mentat_explain_store('my_store',
///     '[:find ?e ?name :where [?e :person/name ?name]]',
///     '{}'::jsonb
/// );
/// ```
#[pg_extern]
pub fn mentat_explain_store(
    store_name: &str,
    query: &str,
    inputs: JsonB,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let schema_prefix = resolve_schema_prefix(store_name);
    mentat_explain_internal(query, inputs, &schema_prefix)
}

/// Return prepared statement cache statistics as JSON.
///
/// Returns: `{"size": <num_cached>, "total_hits": <total_reuse_count>,
///            "entries": [{"sql": "...", "hits": N}, ...]}`
#[pg_extern]
pub fn mentat_stmt_cache_stats() -> JsonB {
    let stats = STMT_CACHE.with(|cache| {
        let cache = cache.borrow();
        let entries: Vec<serde_json::Value> = cache
            .iter()
            .map(|(sql, entry)| {
                let prefix: &str = if sql.len() > 120 {
                    // Find a safe UTF-8 boundary at or before byte 120
                    let mut end = 120;
                    while end > 0 && !sql.is_char_boundary(end) {
                        end -= 1;
                    }
                    &sql[..end]
                } else {
                    sql.as_str()
                };
                json!({
                    "sql_prefix": prefix,
                    "hits": entry.hits,
                })
            })
            .collect();
        let total_hits: u64 = cache.values().map(|e| e.hits).sum();
        json!({
            "size": cache.len(),
            "total_hits": total_hits,
            "entries": entries,
        })
    });
    JsonB(stats)
}

/// Clear the prepared statement cache.
///
/// Should be called after schema changes (e.g., new attributes defined via
/// `mentat_transact`) that may invalidate cached query plans.
#[pg_extern]
pub fn mentat_stmt_cache_clear() -> &'static str {
    clear_stmt_cache();
    "ok"
}

/// Internal implementation of query SQL generation, parameterized by schema prefix.
fn mentat_query_sql_internal(
    query: &str,
    inputs: JsonB,
    schema_prefix: &str,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let _parsed_value = parse::value(query)?;
    let parsed_query = mentat_core::parse_query(query)?;

    let temporal = parse_temporal_options(&inputs.0);
    let input_bindings = parse_input_bindings(&parsed_query.in_vars, &inputs.0);
    let find_vars = extract_find_variables(&parsed_query.find_spec);

    let mut builder = SqlBuilder::new();
    let (sql_query, _complexity) = build_sql_from_datalog(
        &parsed_query,
        &find_vars,
        &mut builder,
        &temporal,
        &input_bindings,
        schema_prefix,
    )?;

    // Build clean column names from the :find variables
    let columns: Vec<String> = find_vars
        .iter()
        .map(|v| {
            // Strip the leading '?' and any aggregate wrapper for SQL column names
            let name = v.trim_start_matches('?');
            // Replace special chars with underscore for valid SQL identifiers
            name.replace('/', "_")
                .replace('-', "_")
                .replace('.', "_")
        })
        .collect();

    Ok(JsonB(json!({
        "sql": sql_query,
        "columns": columns,
        "datalog": query,
    })))
}

/// Return the generated SQL for a Datalog query without executing it.
///
/// This is useful for creating SQL VIEWs backed by Datalog queries.
/// The returned SQL can be used directly in a CREATE VIEW statement.
///
/// Returns a JSON object with `sql` (the generated SQL) and `columns`
/// (the list of column names from the :find clause).
///
/// # Example
/// ```sql
/// SELECT mentat.mentat_query_sql(
///     '[:find ?e ?name :where [?e :person/name ?name]]',
///     '{}'::jsonb
/// );
/// -- Returns: {"sql": "SELECT ...", "columns": ["?e", "?name"]}
/// ```
#[pg_extern]
pub fn mentat_query_sql(
    query: &str,
    inputs: JsonB,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    mentat_query_sql_internal(query, inputs, "mentat.")
}

/// Return the generated SQL for a Datalog query against a named store.
///
/// # Example
/// ```sql
/// SELECT mentat.mentat_query_sql_store('my_store',
///     '[:find ?e ?name :where [?e :person/name ?name]]',
///     '{}'::jsonb
/// );
/// ```
#[pg_extern]
pub fn mentat_query_sql_store(
    store_name: &str,
    query: &str,
    inputs: JsonB,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let schema_prefix = resolve_schema_prefix(store_name);
    mentat_query_sql_internal(query, inputs, &schema_prefix)
}

/// Row type for query_view results (row_num, col1..col8).
type QueryViewRow = (
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Internal query view implementation parameterized by schema prefix.
fn mentat_query_view_internal(
    query: &str,
    inputs: JsonB,
    schema_prefix: &str,
) -> Result<Vec<QueryViewRow>, Box<dyn std::error::Error + Send + Sync>> {
    let _parsed_value = parse::value(query)?;
    let parsed_query = mentat_core::parse_query(query)?;

    let temporal = parse_temporal_options(&inputs.0);
    let input_bindings = parse_input_bindings(&parsed_query.in_vars, &inputs.0);
    let find_vars = extract_find_variables(&parsed_query.find_spec);
    let pagination = parse_pagination_options(&inputs.0);

    let num_cols = find_vars.len();
    if num_cols > 8 {
        return Err(Box::new(MentatError::InvalidQuery {
            message: format!(
                "query_view supports up to 8 columns, but this query has {}",
                num_cols
            ),
            suggestion: Some(
                "Use mentat_query() for queries with more than 8 columns".to_string(),
            ),
        }));
    }

    let mut builder = SqlBuilder::new();
    let (mut sql_query, complexity) = build_sql_from_datalog(
        &parsed_query,
        &find_vars,
        &mut builder,
        &temporal,
        &input_bindings,
        schema_prefix,
    )?;

    // Apply pagination
    if let Some(limit) = pagination.limit {
        if let Some(pos) = sql_query.rfind(" LIMIT ") {
            sql_query.truncate(pos);
        }
        sql_query.push_str(&format!(" LIMIT {}", limit));
    }
    if let Some(offset) = pagination.offset {
        sql_query.push_str(&format!(" OFFSET {}", offset));
    }

    let params = builder.params;
    let rows = Spi::connect_mut(|client| {
        apply_optimizer_hints(client, &complexity);

        let mut result_rows: Vec<QueryViewRow> = Vec::new();

        let mut row_num: i64 = 1;
        for row in execute_cached_query(client, &sql_query, &params)
            .map_err(|e| {
                Box::new(MentatError::InvalidQuery {
                    message: format!("SPI execution error: {}", e),
                    suggestion: None,
                }) as Box<dyn std::error::Error + Send + Sync>
            })?
        {
            let mut cols: [Option<String>; 8] = Default::default();
            for idx in 0..num_cols {
                let col_idx = (idx + 1) as usize;
                if let Ok(Some(val)) = row.get::<String>(col_idx) {
                    let decoded = decode_text_result(&val);
                    cols[idx] = Some(match &decoded {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    });
                }
            }
            let [c1, c2, c3, c4, c5, c6, c7, c8] = cols;
            result_rows.push((row_num, c1, c2, c3, c4, c5, c6, c7, c8));
            row_num += 1;
        }

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(result_rows)
    })?;

    Ok(rows)
}

/// Execute a Datalog query and return results as a set of rows.
///
/// Each row is returned as `(row_num, values)` where `values` is a JSON
/// array of the column values as text. This is suitable for building
/// SQL VIEWs via a wrapper function.
///
/// # Example
/// ```sql
/// SELECT * FROM mentat.mentat_query_view(
///     '[:find ?e ?name :where [?e :person/name ?name]]',
///     '{}'::jsonb
/// );
/// ```
#[pg_extern]
pub fn mentat_query_view(
    query: &str,
    inputs: JsonB,
) -> Result<
    TableIterator<
        'static,
        (
            name!(row_num, i64),
            name!(col1, Option<String>),
            name!(col2, Option<String>),
            name!(col3, Option<String>),
            name!(col4, Option<String>),
            name!(col5, Option<String>),
            name!(col6, Option<String>),
            name!(col7, Option<String>),
            name!(col8, Option<String>),
        ),
    >,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let rows = mentat_query_view_internal(query, inputs, "mentat.")?;
    Ok(TableIterator::new(rows))
}

/// Execute a Datalog query against a named store and return results as a set of rows.
///
/// # Example
/// ```sql
/// SELECT * FROM mentat.mentat_query_view_store('my_store',
///     '[:find ?e ?name :where [?e :person/name ?name]]',
///     '{}'::jsonb
/// );
/// ```
#[pg_extern]
pub fn mentat_query_view_store(
    store_name: &str,
    query: &str,
    inputs: JsonB,
) -> Result<
    TableIterator<
        'static,
        (
            name!(row_num, i64),
            name!(col1, Option<String>),
            name!(col2, Option<String>),
            name!(col3, Option<String>),
            name!(col4, Option<String>),
            name!(col5, Option<String>),
            name!(col6, Option<String>),
            name!(col7, Option<String>),
            name!(col8, Option<String>),
        ),
    >,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let schema_prefix = resolve_schema_prefix(store_name);
    let rows = mentat_query_view_internal(query, inputs, &schema_prefix)?;
    Ok(TableIterator::new(rows))
}

/// Format the query response based on the FindSpec variant.
fn format_find_response(
    find_spec: &FindSpec,
    find_vars: &[String],
    results: Vec<serde_json::Value>,
    has_aggregates: bool,
) -> serde_json::Value {
    match find_spec {
        FindSpec::FindRel(_) => {
            if has_aggregates && results.len() == 1 && find_vars.len() == 1 {
                if let Some(arr) = results[0].as_array() {
                    return json!({"result": arr[0]});
                }
            }
            json!({
                "columns": find_vars,
                "results": results
            })
        }
        FindSpec::FindColl(_) => {
            let scalars: Vec<serde_json::Value> = results
                .into_iter()
                .filter_map(|row| row.as_array().and_then(|arr| arr.first().cloned()))
                .collect();
            json!({"result": scalars})
        }
        FindSpec::FindTuple(_) => {
            if let Some(first) = results.into_iter().next() {
                json!({"result": first})
            } else {
                json!({"result": null})
            }
        }
        FindSpec::FindScalar(_) => {
            if let Some(first_row) = results.into_iter().next() {
                if let Some(arr) = first_row.as_array() {
                    if let Some(val) = arr.first() {
                        return json!({"result": val});
                    }
                }
            }
            json!({"result": null})
        }
    }
}

/// Check if a FindSpec contains any aggregate elements.
fn find_spec_has_aggregates(find_spec: &FindSpec) -> bool {
    for elem in find_spec.columns() {
        if matches!(elem, Element::Aggregate(_)) {
            return true;
        }
    }
    false
}

/// Decode a TEXT result from the SQL CASE expression into the appropriate JSON type.
fn decode_text_result(val: &str) -> serde_json::Value {
    if let Some(bits_str) = val.strip_prefix("d:") {
        if let Ok(bits) = bits_str.parse::<i64>() {
            let f = f64::from_bits(bits as u64);
            return json!(f);
        }
    }

    if val == "true" {
        return json!(true);
    }
    if val == "false" {
        return json!(false);
    }

    if let Ok(i) = val.parse::<i64>() {
        return json!(i);
    }

    // Try parsing as float (for aggregate results like ts_rank)
    if let Ok(f) = val.parse::<f64>() {
        return json!(f);
    }

    json!(val)
}

/// Extract variable names from FindSpec (handles both variables and aggregates).
fn extract_find_variables(find_spec: &FindSpec) -> Vec<String> {
    match find_spec {
        FindSpec::FindRel(elems) => elems.iter().map(|e| format!("{}", e)).collect(),
        FindSpec::FindColl(e) => vec![format!("{}", e)],
        FindSpec::FindTuple(elems) => elems.iter().map(|e| format!("{}", e)).collect(),
        FindSpec::FindScalar(e) => vec![format!("{}", e)],
    }
}

/// Extract the inner variable name from an Element, handling aggregates.
fn element_to_var_name(elem: &Element) -> Option<String> {
    match elem {
        Element::Variable(v) => Some(format!("{}", v)),
        Element::Aggregate(agg) => {
            // Return the variable inside the aggregate for binding lookup
            agg.args.iter().find_map(|arg| {
                if let FnArg::Variable(v) = arg {
                    Some(format!("{}", v))
                } else {
                    None
                }
            })
        }
        Element::Corresponding(v) => Some(format!("{}", v)),
        Element::Pull(_) => None,
    }
}

/// Extract a variable name string from a PatternNonValuePlace, if it is a variable.
fn non_value_var_name(place: &PatternNonValuePlace) -> Option<String> {
    match place {
        PatternNonValuePlace::Variable(v) => Some(format!("{}", v)),
        _ => None,
    }
}

/// Format a keyword ident for schema lookup.
fn keyword_to_ident(kw: &edn::Keyword) -> String {
    format!("{}", kw)
}

/// Build a SQL expression that reads a numeric value from the typed columns.
/// Returns the value as BIGINT, using COALESCE across ref/long columns.
fn build_numeric_value_decode_expr(alias: &str) -> String {
    format!(
        "COALESCE({alias}.v_ref, {alias}.v_long, \
         {alias}.v_double::BIGINT, \
         EXTRACT(EPOCH FROM {alias}.v_instant)::BIGINT * 1000000)"
    )
}

/// Build a SQL CASE expression that reads from typed value columns and returns TEXT.
/// Each type-specific column is read directly with appropriate formatting.
fn build_value_decode_expr(alias: &str) -> String {
    format!(
        "CASE {alias}.value_type_tag \
         WHEN {ref_tag} THEN {alias}.v_ref::TEXT \
         WHEN {bool_tag} THEN {alias}.v_bool::TEXT \
         WHEN {long_tag} THEN {alias}.v_long::TEXT \
         WHEN {double_tag} THEN 'd:' || {alias}.v_double::BIGINT::TEXT \
         WHEN {instant_tag} THEN to_char({alias}.v_instant, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
         WHEN {str_tag} THEN {alias}.v_text \
         WHEN {kw_tag} THEN ':' || {alias}.v_keyword \
         WHEN {uuid_tag} THEN {alias}.v_uuid::TEXT \
         WHEN {bytes_tag} THEN encode({alias}.v_bytes, 'hex') \
         ELSE NULL::TEXT \
         END",
        alias = alias,
        ref_tag = type_tag::REF,
        bool_tag = type_tag::BOOLEAN,
        long_tag = type_tag::LONG,
        double_tag = type_tag::DOUBLE,
        instant_tag = type_tag::INSTANT,
        str_tag = type_tag::STRING,
        kw_tag = type_tag::KEYWORD,
        uuid_tag = type_tag::UUID,
        bytes_tag = type_tag::BYTES,
    )
}

/// Bind a constant value from a pattern's value position to the appropriate typed column.
/// Returns a WHERE clause fragment comparing against the correct typed column.
fn bind_constant_value(
    alias: &str,
    place: &PatternValuePlace,
    builder: &mut SqlBuilder<'_>,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    match place {
        PatternValuePlace::EntidOrInteger(i) => {
            let param = builder.bind_bigint(*i);
            Ok(Some(format!(
                "({alias}.v_long = {param} AND {alias}.value_type_tag = {tag})",
                tag = type_tag::LONG
            )))
        }
        PatternValuePlace::IdentOrKeyword(kw) => {
            let ident_str = keyword_to_ident(kw);
            let stored = if ident_str.starts_with(':') {
                ident_str[1..].to_string()
            } else {
                ident_str
            };
            let param = builder.bind_text(stored);
            Ok(Some(format!(
                "({alias}.v_keyword = {param} AND {alias}.value_type_tag = {tag})",
                tag = type_tag::KEYWORD
            )))
        }
        PatternValuePlace::Constant(constant) => match constant {
            NonIntegerConstant::Boolean(b) => {
                let param = builder.bind_bool(*b);
                Ok(Some(format!(
                    "({alias}.v_bool = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::BOOLEAN
                )))
            }
            NonIntegerConstant::Float(f) => {
                let param = builder.bind_double(f.into_inner());
                Ok(Some(format!(
                    "({alias}.v_double = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::DOUBLE
                )))
            }
            NonIntegerConstant::Text(s) => {
                let param = builder.bind_text(s.as_ref().clone());
                Ok(Some(format!(
                    "({alias}.v_text = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::STRING
                )))
            }
            NonIntegerConstant::Instant(dt) => {
                let micros = dt.timestamp_micros();
                let param = builder.bind_bigint(micros);
                Ok(Some(format!(
                    "({alias}.v_instant = to_timestamp({param}::DOUBLE PRECISION / 1000000.0) AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::INSTANT
                )))
            }
            NonIntegerConstant::Uuid(u) => {
                let uuid_str = u.to_string();
                let param = builder.bind_text(uuid_str);
                Ok(Some(format!(
                    "({alias}.v_uuid = {param}::UUID AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::UUID
                )))
            }
            NonIntegerConstant::BigInteger(_) => {
                Err(":db.error/unsupported-constant BigInteger constants are not supported \
                     in query patterns. Use a regular integer (long) value instead.".into())
            }
        },
        PatternValuePlace::Variable(_) | PatternValuePlace::Placeholder => Ok(None),
    }
}

// ============================================================================
// SQL Generation: Main entry point
// ============================================================================

/// Build SQL query from Datalog clauses.
///
/// Supports: patterns, OR, NOT, predicates, where-functions (fulltext,
/// arithmetic), aggregates, ORDER BY, LIMIT, and temporal options.
///
/// Returns the generated SQL string and a `QueryComplexity` descriptor
/// so the caller can apply appropriate optimizer hints.
fn build_sql_from_datalog(
    parsed: &ParsedQuery,
    find_vars: &[String],
    builder: &mut SqlBuilder<'_>,
    temporal: &TemporalOption,
    input_bindings: &HashMap<String, serde_json::Value>,
    schema_prefix: &str,
) -> Result<(String, QueryComplexity), Box<dyn std::error::Error + Send + Sync>> {
    // Separate clause types
    let mut pattern_clauses = Vec::new();
    let mut or_joins = Vec::new();
    let mut not_joins = Vec::new();
    let mut predicates = Vec::new();
    let mut where_fns: Vec<&WhereFn> = Vec::new();
    let mut rule_invocations: Vec<&RuleInvocation> = Vec::new();

    for clause in &parsed.where_clauses {
        match clause {
            WhereClause::Pattern(p) => pattern_clauses.push(p),
            WhereClause::OrJoin(oj) => or_joins.push(oj),
            WhereClause::NotJoin(nj) => not_joins.push(nj),
            WhereClause::Pred(p) => predicates.push(p),
            WhereClause::WhereFn(wf) => where_fns.push(wf),
            WhereClause::RuleExpr(ri) => rule_invocations.push(ri),
            WhereClause::TypeAnnotation(_) => {
                // Type annotations are hints; silently ignore
            }
        }
    }

    // Handle fulltext where-functions
    let mut fts_joins: Vec<FtsJoin> = Vec::new();
    let mut extra_var_bindings: HashMap<String, String> = HashMap::new();
    for (fts_idx, wf) in where_fns.iter().enumerate() {
        let op_name = wf.operator.0.as_str();
        if op_name == "fulltext" {
            let fj = build_fulltext_join(wf, fts_idx, builder, &mut extra_var_bindings, schema_prefix)?;
            fts_joins.push(fj);
        } else {
            // Arithmetic binding functions: [(* ?age 2) ?double-age]
            if let Some((var_name, expr)) = build_where_fn_binding(wf)? {
                extra_var_bindings.insert(var_name, expr);
            } else {
                return Err(format!(
                    ":db.error/unsupported-where-fn Where-function '{}' is not supported. \
                     Supported functions: fulltext, *, +, -, /",
                    op_name
                )
                .into());
            }
        }
    }

    // Build CTEs from rule definitions and rule invocations
    let mut cte_prefix = String::new();
    let mut rule_cte_info: Option<RuleCteInfo> = None;
    if !rule_invocations.is_empty() && !parsed.rules.is_empty() {
        let (cte_sql, cte_info) =
            build_rule_ctes(&parsed.rules, &rule_invocations, builder, temporal, schema_prefix)?;
        cte_prefix = cte_sql;
        rule_cte_info = Some(cte_info);
    }

    // Build the base query (skip if we only have OR clauses)
    let (base_sql, base_var_to_alias) = if pattern_clauses.is_empty() && !or_joins.is_empty() {
        // No base patterns, only OR clauses - will be handled below
        (String::new(), HashMap::new())
    } else {
        build_extended_pattern_query(
            &pattern_clauses,
            &not_joins,
            &predicates,
            &fts_joins,
            &extra_var_bindings,
            find_vars,
            &parsed.find_spec,
            builder,
            temporal,
            &rule_cte_info,
            input_bindings,
            schema_prefix,
        )?
    };

    // Handle OR-joins using Datalog union semantics.
    //
    // Each OR branch is compiled into an independent SQL query that includes
    // the base pattern clauses (shared context) plus the branch-specific
    // patterns.  The branches are combined with UNION (not UNION ALL) to
    // provide set-semantic deduplication: the same tuple may be produced by
    // multiple branches, and Datalog treats the result as a set.
    let (query_sql, has_union) = if or_joins.is_empty() {
        (base_sql, false)
    } else {
        if or_joins.len() > 1 {
            return Err(":db.error/unsupported-query Multiple OR-join clauses in a single query \
                        are not yet supported. Combine conditions into a single (or ...) clause \
                        or split into separate queries.".into());
        }

        let or_join = or_joins[0];
        let mut union_parts = Vec::new();

        for or_clause in &or_join.clauses {
            // Extract patterns, predicates, and where-functions from each OR branch
            let mut arm_patterns: Vec<&edn::query::Pattern> = Vec::new();
            let mut arm_predicates: Vec<&Predicate> = Vec::new();
            let mut arm_where_fns: Vec<&WhereFn> = Vec::new();

            match or_clause {
                OrWhereClause::Clause(clause) => {
                    match clause {
                        WhereClause::Pattern(p) => arm_patterns.push(p),
                        WhereClause::Pred(pred) => arm_predicates.push(pred),
                        WhereClause::WhereFn(wf) => arm_where_fns.push(wf),
                        WhereClause::NotJoin(_) => return Err(
                            ":db.error/unsupported-query NOT clauses inside OR branches are not yet supported."
                                .into(),
                        ),
                        WhereClause::RuleExpr(_) => return Err(
                            ":db.error/unsupported-query Rule invocations inside OR branches are not yet supported."
                                .into(),
                        ),
                        _ => {} // Ignore type annotations
                    }
                }
                OrWhereClause::And(clauses) => {
                    for c in clauses {
                        match c {
                            WhereClause::Pattern(p) => arm_patterns.push(p),
                            WhereClause::Pred(pred) => arm_predicates.push(pred),
                            WhereClause::WhereFn(wf) => arm_where_fns.push(wf),
                            WhereClause::NotJoin(_) => return Err(
                                ":db.error/unsupported-query NOT clauses inside (or (and ...)) are not yet supported."
                                    .into(),
                            ),
                            WhereClause::RuleExpr(_) => return Err(
                                ":db.error/unsupported-query Rule invocations inside (or (and ...)) are not yet supported."
                                    .into(),
                            ),
                            _ => {} // Ignore type annotations
                        }
                    }
                }
            };

            // Check groundedness: all variables in predicates must be bound by patterns
            for pred in &arm_predicates {
                for arg in &pred.args {
                    if let FnArg::Variable(v) = arg {
                        let var_name = format!("{}", v);
                        // Check if this variable will be bound by patterns in this branch
                        let bound_in_base = pattern_clauses.iter().any(|p| pattern_binds_var(p, &var_name));
                        let bound_in_arm = arm_patterns.iter().any(|p| pattern_binds_var(p, &var_name));
                        if !bound_in_base && !bound_in_arm {
                            return Err(format!(
                                ":db.error/unbound-var Variable '{}' used in predicate inside OR branch \
                                 is not bound by any pattern. All variables in predicates must appear in \
                                 a pattern first. Add a pattern like [?e :some-attr {}] to bind it.",
                                var_name, var_name
                            ).into());
                        }
                    }
                }
            }

            // Each arm gets the base patterns plus its own patterns.
            // This ensures variable bindings from the shared context
            // (e.g. [?p :person/name ?name]) are correctly included in
            // every branch, maintaining consistent bindings across the
            // UNION.
            let mut combined_patterns: Vec<&edn::query::Pattern> = pattern_clauses.clone();
            combined_patterns.extend(arm_patterns);

            // Combine predicates from base query and this OR branch
            let mut combined_predicates = predicates.clone();
            combined_predicates.extend(arm_predicates);

            // Process where-functions for this branch
            let mut arm_fts_joins = fts_joins.clone();
            let mut arm_extra_var_bindings = extra_var_bindings.clone();
            let mut arm_builder = SqlBuilder::new();

            for (idx, wf) in arm_where_fns.iter().enumerate() {
                if wf.operator.0.as_str() == "fulltext" {
                    let fts_idx = fts_joins.len() + idx;
                    let fts_join = build_fulltext_join(wf, fts_idx, &mut arm_builder, &mut arm_extra_var_bindings, schema_prefix)?;
                    arm_fts_joins.push(fts_join);
                } else {
                    return Err(format!(
                        ":db.error/unsupported-query Function '{}' is not supported inside OR branches. \
                         Only fulltext and predicates are currently supported.",
                        wf.operator.0.as_str()
                    ).into());
                }
            }

            let (arm_sql, _arm_var_to_alias) = build_extended_pattern_query(
                &combined_patterns,
                &not_joins,
                &combined_predicates,
                &arm_fts_joins,
                &arm_extra_var_bindings,
                find_vars,
                &parsed.find_spec,
                &mut arm_builder,
                temporal,
                &rule_cte_info,
                input_bindings,
                schema_prefix,
            )?;

            // Remap $N parameter placeholders so they don't collide
            // when we concatenate multiple arms into a single query.
            let offset = builder.params.len();
            let remapped = if offset > 0 {
                remap_param_indices(&arm_sql, offset)
            } else {
                arm_sql
            };

            builder.params.extend(arm_builder.params);
            union_parts.push(format!("({})", remapped));
        }

        (union_parts.join(" UNION "), true)
    };

    // Prepend CTEs if we have rules
    let query_sql = if cte_prefix.is_empty() {
        query_sql
    } else {
        format!("{} {}", cte_prefix, query_sql)
    };

    // Append ORDER BY
    // For non-UNION queries, pass var_to_alias so numeric columns (e, a, tx)
    // are ordered numerically rather than lexicographically as TEXT.
    let var_alias_ref = if has_union {
        None
    } else {
        Some(&base_var_to_alias)
    };
    let query_sql = append_order_by(query_sql, &parsed.order, find_vars, var_alias_ref);

    // Append LIMIT
    let query_sql = append_limit(query_sql, &parsed.limit, &parsed.find_spec);

    let complexity = QueryComplexity {
        join_count: pattern_clauses.len(),
        has_aggregates: find_spec_has_aggregates(&parsed.find_spec),
        has_cte: !cte_prefix.is_empty(),
        has_union,
    };

    Ok((query_sql, complexity))
}

// ============================================================================
// Fulltext search support
// ============================================================================

/// Represents a fulltext search join with its FROM and WHERE fragments.
#[derive(Clone)]
struct FtsJoin {
    from_fragment: String,
    where_parts: Vec<String>,
}

/// Build a fulltext search join from a `(fulltext $ :attr "term")` where-function.
fn build_fulltext_join(
    wf: &WhereFn,
    fts_idx: usize,
    builder: &mut SqlBuilder<'_>,
    var_bindings: &mut HashMap<String, String>,
    schema_prefix: &str,
) -> Result<FtsJoin, Box<dyn std::error::Error + Send + Sync>> {
    if wf.args.len() < 3 {
        return Err(":db.error/fulltext-args fulltext requires at least 3 arguments: \
                    (fulltext $ :attr \"search-term\"). Got only {} arguments. \
                    Example: [(fulltext $ :person/bio \"engineer\") [[?e ?val]]]"
            .replace("{}", &wf.args.len().to_string()).into());
    }

    let attr_ident = match &wf.args[1] {
        FnArg::IdentOrKeyword(kw) => keyword_to_ident(kw),
        _ => return Err(":db.error/fulltext-args fulltext second argument must be a keyword \
                        attribute (e.g. :person/bio). Format: (fulltext $ :attr \"term\")".into()),
    };

    let search_term = match &wf.args[2] {
        FnArg::Constant(NonIntegerConstant::Text(s)) => s.as_ref().clone(),
        _ => return Err(":db.error/fulltext-args fulltext third argument must be a string \
                        search term. Format: (fulltext $ :attr \"search words\")".into()),
    };

    let fts_alias = format!("fts{}", fts_idx);
    let datoms_alias = format!("fts_d{}", fts_idx);

    let attr_param = builder.bind_text(attr_ident);

    let mut where_parts = Vec::new();
    where_parts.push(format!(
        "{datoms_alias}.a = (SELECT entid FROM {schema_prefix}schema WHERE ident = {attr_param})"
    ));
    where_parts.push(format!(
        "{datoms_alias}.value_type_tag = {}",
        type_tag::STRING
    ));
    where_parts.push(format!(
        "{fts_alias}.text_value = {datoms_alias}.v_text"
    ));

    if !search_term.is_empty() {
        // Detect phrase search: if the search term is wrapped in quotes, use phraseto_tsquery
        // for proximity matching; otherwise use plainto_tsquery for simple keyword search.
        let is_phrase = search_term.starts_with('"') && search_term.ends_with('"');
        let clean_term = if is_phrase {
            search_term[1..search_term.len() - 1].to_string()
        } else {
            search_term.clone()
        };
        let search_param = builder.bind_text(clean_term);
        let tsquery_fn = if is_phrase {
            "phraseto_tsquery"
        } else {
            "plainto_tsquery"
        };
        where_parts.push(format!(
            "{fts_alias}.search_vector @@ {tsquery_fn}('english', {search_param})"
        ));
    } else {
        where_parts.push("false".to_string());
    }

    where_parts.push(format!("{datoms_alias}.added = true"));

    // Bind result variables from the binding pattern [[?e ?name _ ?score]]
    if let Binding::BindRel(ref vars) = wf.binding {
        for (i, vop) in vars.iter().enumerate() {
            if let VariableOrPlaceholder::Variable(ref v) = vop {
                let var_name = format!("{}", v);
                match i {
                    0 => {
                        var_bindings.insert(var_name, format!("{datoms_alias}.e::TEXT"));
                    }
                    1 => {
                        var_bindings.insert(var_name, format!("{fts_alias}.text_value"));
                    }
                    2 => {
                        var_bindings.insert(var_name, format!("{datoms_alias}.tx::TEXT"));
                    }
                    3 => {
                        let is_phrase_score =
                            search_term.starts_with('"') && search_term.ends_with('"');
                        let clean_score_term = if is_phrase_score {
                            search_term[1..search_term.len() - 1].to_string()
                        } else {
                            search_term.clone()
                        };
                        let score_param = builder.bind_text(clean_score_term);
                        let score_fn = if is_phrase_score {
                            "phraseto_tsquery"
                        } else {
                            "plainto_tsquery"
                        };
                        var_bindings.insert(
                            var_name,
                            format!(
                                "ts_rank({fts_alias}.search_vector, {score_fn}('english', {score_param}))::TEXT"
                            ),
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    let from_fragment = format!("{schema_prefix}datoms {datoms_alias}, {schema_prefix}fulltext {fts_alias}");

    Ok(FtsJoin {
        from_fragment,
        where_parts,
    })
}

// ============================================================================
// Arithmetic where-function bindings
// ============================================================================

/// Build a computed expression from a where-function binding like [(* ?age 2) ?double-age].
fn build_where_fn_binding(
    wf: &WhereFn,
) -> Result<Option<(String, String)>, Box<dyn std::error::Error + Send + Sync>> {
    let op = wf.operator.0.as_str();

    let sql_op = match op {
        "*" => "*",
        "+" => "+",
        "-" => "-",
        "/" => "/",
        _ => return Ok(None),
    };

    if wf.args.len() != 2 {
        return Err(format!(
            ":db.error/fn-arity Arithmetic function '{}' requires exactly 2 arguments, got {}. \
             Example: [({} ?x 2) ?result]",
            op, wf.args.len(), op
        ).into());
    }

    let result_var = match &wf.binding {
        Binding::BindScalar(v) => format!("{}", v),
        _ => return Err(format!(
            ":db.error/fn-binding Arithmetic function '{}' requires a scalar binding (single variable). \
             Example: [({} ?x 2) ?result]",
            op, op
        ).into()),
    };

    let arg0 = fn_arg_to_numeric_placeholder(&wf.args[0]);
    let arg1 = fn_arg_to_numeric_placeholder(&wf.args[1]);

    Ok(Some((
        result_var,
        format!("({} {} {})", arg0, sql_op, arg1),
    )))
}

/// Convert an FnArg to a SQL placeholder expression.
fn fn_arg_to_placeholder(arg: &FnArg) -> String {
    match arg {
        FnArg::Variable(v) => format!("VAR_REF:{}", v),
        FnArg::EntidOrInteger(i) => i.to_string(),
        FnArg::Constant(NonIntegerConstant::Float(f)) => format!("{}", f.into_inner()),
        FnArg::Constant(NonIntegerConstant::Boolean(b)) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        _ => "NULL".to_string(),
    }
}

/// Convert an FnArg to a SQL placeholder for numeric (arithmetic) context.
/// Uses NUM_VAR_REF: prefix so resolve_var_refs produces a numeric expression.
fn fn_arg_to_numeric_placeholder(arg: &FnArg) -> String {
    match arg {
        FnArg::Variable(v) => format!("NUM_VAR_REF:{}", v),
        _ => fn_arg_to_placeholder(arg),
    }
}

// ============================================================================
// ORDER BY and LIMIT
// ============================================================================

/// Append ORDER BY clause to SQL string.
///
/// When `var_to_alias` is provided (non-UNION queries) and a variable maps to a
/// numeric column (e, a, tx), the query is wrapped in a subquery so that the
/// ORDER BY can cast the TEXT column to BIGINT for proper numeric ordering.
/// This avoids the "ORDER BY must appear in select list" error with DISTINCT.
fn append_order_by(
    sql: String,
    order: &Option<Vec<Order>>,
    find_vars: &[String],
    var_to_alias: Option<&HashMap<String, (String, &'static str)>>,
) -> String {
    if let Some(ref orders) = order {
        if orders.is_empty() {
            return sql;
        }

        // Check if any ordered variable is a numeric column (e, a, tx)
        let has_numeric_order = var_to_alias.map_or(false, |vta| {
            orders.iter().any(|Order(_, var)| {
                let var_name = format!("{}", var);
                vta.get(var_name.as_str())
                    .map_or(false, |(_, col)| *col == "e" || *col == "a" || *col == "tx")
            })
        });

        let mut order_parts = Vec::new();
        for Order(direction, var) in orders {
            let var_name = format!("{}", var);
            if let Some(col_pos) = find_vars.iter().position(|v| *v == var_name) {
                let dir = match direction {
                    Direction::Ascending => "ASC",
                    Direction::Descending => "DESC",
                };
                if has_numeric_order {
                    // Use column alias from the subquery wrapper
                    let is_numeric = var_to_alias
                        .and_then(|vta| vta.get(var_name.as_str()))
                        .map_or(false, |(_, col)| *col == "e" || *col == "a" || *col == "tx");
                    if is_numeric {
                        order_parts.push(format!("_c{}::BIGINT {}", col_pos + 1, dir));
                    } else {
                        order_parts.push(format!("_c{} {}", col_pos + 1, dir));
                    }
                } else {
                    order_parts.push(format!("{} {}", col_pos + 1, dir));
                }
            }
        }
        if !order_parts.is_empty() {
            if has_numeric_order {
                // Wrap in subquery with named columns so we can cast in ORDER BY
                let col_aliases: Vec<String> =
                    (1..=find_vars.len()).map(|i| format!("_c{}", i)).collect();
                return format!(
                    "SELECT {cols} FROM ({inner}) AS _q({col_defs}) ORDER BY {order}",
                    cols = col_aliases.join(", "),
                    inner = sql,
                    col_defs = col_aliases.join(", "),
                    order = order_parts.join(", "),
                );
            } else {
                return format!("{} ORDER BY {}", sql, order_parts.join(", "));
            }
        }
    }
    sql
}

/// Append LIMIT clause to SQL string.
fn append_limit(sql: String, limit: &Limit, find_spec: &FindSpec) -> String {
    match limit {
        Limit::Fixed(n) => format!("{} LIMIT {}", sql, n),
        Limit::Unlimited => {
            if find_spec.is_unit_limited() {
                format!("{} LIMIT 1", sql)
            } else {
                sql
            }
        }
        Limit::Variable(_) => sql,
    }
}

// ============================================================================
// Remap parameter indices for UNION queries
// ============================================================================

/// Remap `$1`, `$2`, ... placeholders in a SQL string by adding an offset.
fn remap_param_indices(sql: &str, offset: usize) -> String {
    let mut result = String::with_capacity(sql.len());
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start {
                let n: usize = sql[start..end].parse().unwrap_or(0);
                result.push('$');
                result.push_str(&(n + offset).to_string());
                i = end;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

// ============================================================================
// Extended pattern query builder (supports NOT, predicates, aggregates, FTS, temporal)
// ============================================================================

/// Build a SQL query from patterns plus NOT, predicates, FTS, aggregates, temporal, and rules.
fn build_extended_pattern_query(
    patterns: &[&edn::query::Pattern],
    not_joins: &[&edn::query::NotJoin],
    predicates: &[&Predicate],
    fts_joins: &[FtsJoin],
    extra_var_bindings: &HashMap<String, String>,
    find_vars: &[String],
    find_spec: &FindSpec,
    builder: &mut SqlBuilder<'_>,
    temporal: &TemporalOption,
    rule_cte_info: &Option<RuleCteInfo>,
    input_bindings: &HashMap<String, serde_json::Value>,
    schema_prefix: &str,
) -> Result<
    (String, HashMap<String, (String, &'static str)>),
    Box<dyn std::error::Error + Send + Sync>,
> {
    // Track variable bindings to datom table aliases
    let mut var_to_alias: HashMap<String, (String, &'static str)> = HashMap::new();
    let mut joins = Vec::new();
    let mut where_clauses = Vec::new();

    // Pre-populate var_to_alias with rule CTE bindings
    if let Some(ref cte_info) = rule_cte_info {
        joins.push(cte_info.from_fragment.clone());
        for (var_name, (alias, col)) in &cte_info.var_to_col {
            var_to_alias.insert(var_name.clone(), (alias.clone(), col));
        }
    }

    for (idx, pattern) in patterns.iter().enumerate() {
        let alias = format!("datoms{}", idx);

        // Handle entity position
        match &pattern.entity {
            PatternNonValuePlace::Variable(v) => {
                let var_name = format!("{}", v);
                if let Some((existing_alias, col)) = var_to_alias.get(&var_name) {
                    if *col == "v" {
                        // Variable was bound from a value column (BYTEA ref).
                        // Variable was bound from a value column (ref type).
                        // Use v_ref directly for comparison with entity column.
                        where_clauses.push(format!(
                            "{alias}.e = {existing_alias}.v_ref"
                        ));
                    } else {
                        where_clauses.push(format!(
                            "{alias}.e = {existing}.{col}",
                            existing = existing_alias
                        ));
                    }
                } else {
                    var_to_alias.insert(var_name, (alias.clone(), "e"));
                }
            }
            PatternNonValuePlace::Entid(id) => {
                let param = builder.bind_bigint(*id);
                where_clauses.push(format!("{alias}.e = {param}"));
            }
            PatternNonValuePlace::Ident(kw) => {
                let ident_str = keyword_to_ident(kw);
                let param = builder.bind_text(ident_str);
                where_clauses.push(format!(
                    "{alias}.e = (SELECT entid FROM {schema_prefix}idents WHERE ident = {param})"
                ));
            }
            PatternNonValuePlace::Placeholder => {}
        }

        // Handle attribute position
        match &pattern.attribute {
            PatternNonValuePlace::Ident(kw) => {
                let ident_str = keyword_to_ident(kw);
                let param = builder.bind_text(ident_str);
                where_clauses.push(format!(
                    "{alias}.a = (SELECT entid FROM {schema_prefix}schema WHERE ident = {param})"
                ));
            }
            PatternNonValuePlace::Entid(id) => {
                let param = builder.bind_bigint(*id);
                where_clauses.push(format!("{alias}.a = {param}"));
            }
            PatternNonValuePlace::Variable(v) => {
                let var_name = format!("{}", v);
                if let Some((existing_alias, col)) = var_to_alias.get(&var_name) {
                    where_clauses.push(format!(
                        "{alias}.a = {existing}.{col}",
                        existing = existing_alias
                    ));
                } else {
                    var_to_alias.insert(var_name, (alias.clone(), "a"));
                }
            }
            PatternNonValuePlace::Placeholder => {}
        }

        // Handle value position
        match &pattern.value {
            PatternValuePlace::Variable(v) => {
                let var_name = format!("{}", v);
                if let Some((existing_alias, col)) = var_to_alias.get(&var_name) {
                    if *col == "v" {
                        where_clauses.push(format!(
                            "{alias}.value_type_tag = {existing}.value_type_tag \
AND {alias}.v_ref IS NOT DISTINCT FROM {existing}.v_ref \
AND {alias}.v_bool IS NOT DISTINCT FROM {existing}.v_bool \
AND {alias}.v_long IS NOT DISTINCT FROM {existing}.v_long \
AND {alias}.v_double IS NOT DISTINCT FROM {existing}.v_double \
AND {alias}.v_text IS NOT DISTINCT FROM {existing}.v_text \
AND {alias}.v_keyword IS NOT DISTINCT FROM {existing}.v_keyword \
AND {alias}.v_instant IS NOT DISTINCT FROM {existing}.v_instant \
AND {alias}.v_uuid IS NOT DISTINCT FROM {existing}.v_uuid \
AND {alias}.v_bytes IS NOT DISTINCT FROM {existing}.v_bytes",
                            existing = existing_alias
                        ));
                    } else {
                        // Variable was bound to a non-value column (e, a, tx)
                        // In value position, this means the value is a ref to that entity/attr/tx
                        where_clauses.push(format!(
                            "{alias}.v_ref = {existing}.{col}",
                            existing = existing_alias
                        ));
                    }
                } else {
                    var_to_alias.insert(var_name, (alias.clone(), "v"));
                }
            }
            _ => {
                if let Some(constraint) = bind_constant_value(&alias, &pattern.value, builder)? {
                    where_clauses.push(constraint);
                }
            }
        }

        // Handle tx position
        if let Some(tx_var) = non_value_var_name(&pattern.tx) {
            if let Some((existing_alias, col)) = var_to_alias.get(&tx_var) {
                where_clauses.push(format!(
                    "{alias}.tx = {existing}.{col}",
                    existing = existing_alias
                ));
            } else {
                var_to_alias.insert(tx_var, (alias.clone(), "tx"));
            }
        } else if let PatternNonValuePlace::Entid(tx_id) = &pattern.tx {
            let param = builder.bind_bigint(*tx_id);
            where_clauses.push(format!("{alias}.tx = {param}"));
        }

        // Temporal filtering per datom table
        if temporal.history {
            // History mode: include both added=true and added=false (no filter)
        } else {
            where_clauses.push(format!("{alias}.added = true"));
        }

        if let Some(as_of_tx) = temporal.as_of {
            let param = builder.bind_bigint(as_of_tx);
            where_clauses.push(format!("{alias}.tx <= {param}"));

            // For as-of queries on cardinality-one attributes, exclude datoms
            // that have been superseded by a newer assertion within the as-of
            // window.  Cardinality-many attributes must NOT be filtered this
            // way because multiple values for the same (e, a) are valid.
            //
            // We check cardinality at query-compile time when the attribute is
            // a known constant; when it is a variable we skip the filter
            // entirely (cardinality-one correctness is still guaranteed by the
            // explicit retraction inserted during transact).
            let is_cardinality_many = match &pattern.attribute {
                PatternNonValuePlace::Ident(kw) => {
                    let ident_str = keyword_to_ident(kw);
                    crate::cache::get_cache()
                        .resolve_ident(&ident_str)
                        .and_then(|entid| crate::cache::get_cache().get_attribute(entid))
                        .map(|info| info.cardinality == "many")
                        .unwrap_or(false)
                }
                PatternNonValuePlace::Entid(id) => crate::cache::get_cache()
                    .get_attribute(*id)
                    .map(|info| info.cardinality == "many")
                    .unwrap_or(false),
                _ => false, // variable or placeholder: skip filter (safe due to explicit retractions)
            };

            if !is_cardinality_many {
                let param2 = builder.bind_bigint(as_of_tx);
                where_clauses.push(format!(
                    "NOT EXISTS (SELECT 1 FROM {schema_prefix}datoms newer \
                     WHERE newer.e = {alias}.e AND newer.a = {alias}.a \
                     AND newer.added = true \
                     AND newer.tx > {alias}.tx AND newer.tx <= {param2})"
                ));
            }
        }

        if let Some(since_tx) = temporal.since {
            let param = builder.bind_bigint(since_tx);
            where_clauses.push(format!("{alias}.tx > {param}"));
        }

        // Handle added position (5th element in pattern, e.g. [?e ?a ?v ?tx ?added])
        if let Some(added_var) = non_value_var_name(&pattern.added) {
            if let Some((existing_alias, col)) = var_to_alias.get(&added_var) {
                where_clauses.push(format!(
                    "{alias}.added = {existing}.{col}",
                    existing = existing_alias
                ));
            } else {
                var_to_alias.insert(added_var, (alias.clone(), "added"));
            }
        }

        joins.push(format!("{schema_prefix}datoms {alias}"));
    }

    // Add FTS joins
    for fj in fts_joins {
        joins.push(fj.from_fragment.clone());
        where_clauses.extend(fj.where_parts.iter().cloned());
    }

    // Apply :in clause input bindings as WHERE constraints
    for (var_name, value) in input_bindings {
        if let Some((alias, col)) = var_to_alias.get(var_name.as_str()) {
            let constraint = match *col {
                "v" => bind_input_value(alias, value, builder, schema_prefix),
                "e" => bind_input_entity(alias, value, builder, schema_prefix),
                "a" => {
                    // Attribute column: bind as bigint
                    if let Some(i) = value.as_i64() {
                        let param = builder.bind_bigint(i);
                        Some(format!("{alias}.a = {param}"))
                    } else {
                        None
                    }
                }
                "tx" => {
                    // Transaction column: bind as bigint
                    if let Some(i) = value.as_i64() {
                        let param = builder.bind_bigint(i);
                        Some(format!("{alias}.tx = {param}"))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(c) = constraint {
                where_clauses.push(c);
            }
        }
    }

    // Handle NOT clauses as NOT EXISTS subqueries
    for not_join in not_joins {
        let not_sql = build_not_exists_subquery(not_join, &var_to_alias, builder, temporal, schema_prefix)?;
        where_clauses.push(not_sql);
    }

    // Handle predicate clauses
    for pred in predicates {
        let pred_sql = build_predicate_clause(pred, &var_to_alias)?;
        where_clauses.push(pred_sql);
    }

    // Detect aggregates
    let has_aggregates = find_spec_has_aggregates(find_spec);

    // Build SELECT clause
    let mut select_exprs = Vec::new();
    let mut group_by_exprs = Vec::new();

    for (col_idx, var_display) in find_vars.iter().enumerate() {
        // Check if this is an aggregate element
        let elem = get_find_element(find_spec, col_idx);
        let is_aggregate = elem.map_or(false, |e| matches!(e, Element::Aggregate(_)));

        if is_aggregate {
            // Build aggregate expression
            if let Some(Element::Aggregate(agg)) = elem {
                let agg_sql = build_aggregate_select(agg, &var_to_alias, extra_var_bindings)?;
                select_exprs.push(agg_sql);
            }
        } else if let Some(expr) = extra_var_bindings.get(var_display) {
            // Computed variable (from FTS or arithmetic binding)
            let resolved = resolve_var_refs(expr, &var_to_alias, extra_var_bindings);
            select_exprs.push(format!("({})::TEXT", resolved));
            if has_aggregates {
                group_by_exprs.push(format!("{}", col_idx + 1));
            }
        } else {
            // Extract the inner variable name for lookup
            let inner_var = elem
                .and_then(element_to_var_name)
                .unwrap_or_else(|| var_display.clone());

            if let Some((alias, col)) = var_to_alias.get(inner_var.as_str()) {
                if *col == "v" {
                    select_exprs.push(build_value_decode_expr(alias));
                } else {
                    select_exprs.push(format!("{alias}.{col}::TEXT"));
                }
                if has_aggregates {
                    group_by_exprs.push(format!("{}", col_idx + 1));
                }
            } else {
                select_exprs.push("NULL::TEXT".to_string());
            }
        }
    }

    if select_exprs.is_empty() {
        return Err(MentatError::InvalidQuery {
            message: "No :find variables could be resolved to pattern bindings. \
                      Ensure every variable in :find also appears in a :where pattern.".to_string(),
            suggestion: Some("Example: [:find ?name :where [?e :person/name ?name]]".to_string()),
        }.into());
    }

    if joins.is_empty() && fts_joins.is_empty() {
        return Err(MentatError::InvalidQuery {
            message: "No :where clauses produced any datom table joins. \
                      Ensure your query has at least one data pattern like [?e :attr ?v].".to_string(),
            suggestion: Some("Pure predicate or function-only queries are not supported.".to_string()),
        }.into());
    }

    let distinct = if !has_aggregates && find_spec.requires_distinct() {
        "DISTINCT "
    } else {
        ""
    };

    let mut sql = format!(
        "SELECT {distinct}{select} FROM {from}",
        select = select_exprs.join(", "),
        from = joins.join(", "),
    );

    if !where_clauses.is_empty() {
        sql.push_str(&format!(" WHERE {}", where_clauses.join(" AND ")));
    }

    // GROUP BY for mixed aggregate + regular queries
    if has_aggregates && !group_by_exprs.is_empty() {
        sql.push_str(&format!(" GROUP BY {}", group_by_exprs.join(", ")));
    }

    Ok((sql, var_to_alias))
}

/// Get the Element at the given index from a FindSpec.
fn get_find_element(find_spec: &FindSpec, idx: usize) -> Option<&Element> {
    match find_spec {
        FindSpec::FindRel(elems) => elems.get(idx),
        FindSpec::FindColl(e) => {
            if idx == 0 {
                Some(e)
            } else {
                None
            }
        }
        FindSpec::FindTuple(elems) => elems.get(idx),
        FindSpec::FindScalar(e) => {
            if idx == 0 {
                Some(e)
            } else {
                None
            }
        }
    }
}

/// Build a SQL aggregate expression like COUNT(DISTINCT alias.col)::TEXT.
fn build_aggregate_select(
    agg: &edn::query::Aggregate,
    var_to_alias: &HashMap<String, (String, &'static str)>,
    extra_var_bindings: &HashMap<String, String>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let func_name = agg.func.0 .0.as_str();

    let sql_func = match func_name {
        "count" => "COUNT",
        "sum" => "SUM",
        "avg" => "AVG",
        "min" => "MIN",
        "max" => "MAX",
        _ => return Err(format!(
            ":db.error/unsupported-aggregate Unsupported aggregate function '{}'. \
             Supported aggregates: count, sum, avg, min, max. \
             Example: [:find (count ?e) :where [?e :person/name]]",
            func_name
        ).into()),
    };

    // Get the variable argument
    let var_arg = agg.args.iter().find_map(|arg| {
        if let FnArg::Variable(v) = arg {
            Some(format!("{}", v))
        } else {
            None
        }
    });

    let inner_expr = if let Some(ref var_name) = var_arg {
        if let Some((alias, col)) = var_to_alias.get(var_name.as_str()) {
            if *col == "v" {
                build_value_decode_expr(alias)
            } else {
                format!("{alias}.{col}")
            }
        } else if let Some(expr) = extra_var_bindings.get(var_name.as_str()) {
            resolve_var_refs(expr, var_to_alias, extra_var_bindings)
        } else {
            "NULL".to_string()
        }
    } else {
        "NULL".to_string()
    };

    // COUNT uses DISTINCT to match Datalog set semantics
    if func_name == "count" {
        Ok(format!("{}(DISTINCT {})::TEXT", sql_func, inner_expr))
    } else {
        // For SUM/AVG/MIN/MAX the inner expression is text, so cast to numeric first
        Ok(format!("{}(({})::NUMERIC)::TEXT", sql_func, inner_expr))
    }
}

/// Resolve VAR_REF:?varname placeholders in an expression to actual SQL column references.
fn resolve_var_refs(
    expr: &str,
    var_to_alias: &HashMap<String, (String, &'static str)>,
    extra_var_bindings: &HashMap<String, String>,
) -> String {
    let mut result = expr.to_string();
    // Find all NUM_VAR_REF:?xxx and VAR_REF:?xxx occurrences and replace them
    loop {
        let num_pos = result.find("NUM_VAR_REF:");
        let var_pos = result.find("VAR_REF:");

        let (start, prefix_len, is_numeric) = match (num_pos, var_pos) {
            (Some(n), Some(v)) => {
                if n < v {
                    (n, 12, true) // "NUM_VAR_REF:" is 12 chars
                } else {
                    (v, 8, false) // "VAR_REF:" is 8 chars
                }
            }
            (Some(n), None) => (n, 12, true),
            (None, Some(v)) => (v, 8, false),
            (None, None) => break,
        };

        let rest = &result[start + prefix_len..];
        // Variable names end at space, ), or end of string
        let end = rest
            .find(|c: char| {
                c == ' ' || c == ')' || c == ',' || c == '+' || c == '-' || c == '*' || c == '/'
            })
            .unwrap_or(rest.len());
        let var_name = &rest[..end];

        let replacement = if let Some((alias, col)) = var_to_alias.get(var_name) {
            if *col == "v" {
                if is_numeric {
                    // For arithmetic context, decode as BIGINT
                    format!("({})", build_numeric_value_decode_expr(alias))
                } else {
                    format!("({})", build_value_decode_expr(alias))
                }
            } else {
                format!("{}.{}", alias, col)
            }
        } else if let Some(inner_expr) = extra_var_bindings.get(var_name) {
            inner_expr.clone()
        } else {
            "NULL".to_string()
        };

        result = format!(
            "{}{}{}",
            &result[..start],
            replacement,
            &result[start + prefix_len + end..]
        );
    }
    result
}

// ============================================================================
// NOT EXISTS subquery builder
// ============================================================================

/// Build a NOT EXISTS subquery from a NotJoin clause.
///
/// Validates groundedness: every variable used in the NOT clause must be bound
/// in the outer query scope. An unbound variable in NOT produces semantically
/// unsound results because NOT EXISTS would test against an uncorrelated
/// subquery, effectively filtering out all rows or none.
fn build_not_exists_subquery(
    not_join: &edn::query::NotJoin,
    outer_var_to_alias: &HashMap<String, (String, &'static str)>,
    builder: &mut SqlBuilder<'_>,
    temporal: &TemporalOption,
    schema_prefix: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Groundedness validation: collect all variables mentioned in the NOT
    // clause and verify each one is bound in the outer scope.
    let mut not_vars: Vec<String> = Vec::new();
    for clause in &not_join.clauses {
        if let WhereClause::Pattern(p) = clause {
            if let PatternNonValuePlace::Variable(v) = &p.entity {
                not_vars.push(format!("{}", v));
            }
            if let PatternNonValuePlace::Variable(v) = &p.attribute {
                not_vars.push(format!("{}", v));
            }
            if let PatternValuePlace::Variable(v) = &p.value {
                not_vars.push(format!("{}", v));
            }
            if let PatternNonValuePlace::Variable(v) = &p.tx {
                not_vars.push(format!("{}", v));
            }
        }
    }

    let unbound: Vec<&String> = not_vars
        .iter()
        .filter(|v| !outer_var_to_alias.contains_key(v.as_str()))
        .collect();

    if !unbound.is_empty() {
        // Deduplicate for error message clarity
        let mut unique_unbound: Vec<&str> = unbound.iter().map(|s| s.as_str()).collect();
        unique_unbound.sort();
        unique_unbound.dedup();
        return Err(format!(
            ":db.error/unbound-variable-in-not Variables {} in (not ...) clause are not bound \
             in the outer query. Every variable in a NOT clause must appear in a :where pattern \
             before the NOT clause. Unbound variables in NOT produce semantically unsound results.",
            unique_unbound.join(", ")
        )
        .into());
    }

    let mut sub_joins = Vec::new();
    let mut sub_where = Vec::new();

    for (idx, clause) in not_join.clauses.iter().enumerate() {
        match clause {
            WhereClause::Pattern(p) => {
                let alias = format!("not_d{}", idx);

                // Entity position
                match &p.entity {
                    PatternNonValuePlace::Variable(v) => {
                        let var_name = format!("{}", v);
                        // Correlate with outer query
                        if let Some((outer_alias, outer_col)) = outer_var_to_alias.get(&var_name) {
                            sub_where.push(format!("{alias}.e = {outer_alias}.{outer_col}"));
                        }
                    }
                    PatternNonValuePlace::Entid(id) => {
                        let param = builder.bind_bigint(*id);
                        sub_where.push(format!("{alias}.e = {param}"));
                    }
                    PatternNonValuePlace::Ident(kw) => {
                        let ident_str = keyword_to_ident(kw);
                        let param = builder.bind_text(ident_str);
                        sub_where.push(format!(
                            "{alias}.e = (SELECT entid FROM {schema_prefix}idents WHERE ident = {param})"
                        ));
                    }
                    PatternNonValuePlace::Placeholder => {}
                }

                // Attribute position
                match &p.attribute {
                    PatternNonValuePlace::Ident(kw) => {
                        let ident_str = keyword_to_ident(kw);
                        let param = builder.bind_text(ident_str);
                        sub_where.push(format!(
                            "{alias}.a = (SELECT entid FROM {schema_prefix}schema WHERE ident = {param})"
                        ));
                    }
                    PatternNonValuePlace::Entid(id) => {
                        let param = builder.bind_bigint(*id);
                        sub_where.push(format!("{alias}.a = {param}"));
                    }
                    PatternNonValuePlace::Variable(v) => {
                        let var_name = format!("{}", v);
                        if let Some((outer_alias, outer_col)) = outer_var_to_alias.get(&var_name) {
                            sub_where.push(format!("{alias}.a = {outer_alias}.{outer_col}"));
                        }
                    }
                    PatternNonValuePlace::Placeholder => {}
                }

                // Value position
                match &p.value {
                    PatternValuePlace::Variable(v) => {
                        let var_name = format!("{}", v);
                        if let Some((outer_alias, outer_col)) = outer_var_to_alias.get(&var_name) {
                            if *outer_col == "v" {
                                sub_where.push(format!(
                                    "{alias}.value_type_tag = {outer_alias}.value_type_tag \
                                     AND {alias}.v_ref IS NOT DISTINCT FROM {outer_alias}.v_ref \
                                     AND {alias}.v_bool IS NOT DISTINCT FROM {outer_alias}.v_bool \
                                     AND {alias}.v_long IS NOT DISTINCT FROM {outer_alias}.v_long \
                                     AND {alias}.v_double IS NOT DISTINCT FROM {outer_alias}.v_double \
                                     AND {alias}.v_text IS NOT DISTINCT FROM {outer_alias}.v_text \
                                     AND {alias}.v_keyword IS NOT DISTINCT FROM {outer_alias}.v_keyword \
                                     AND {alias}.v_instant IS NOT DISTINCT FROM {outer_alias}.v_instant \
                                     AND {alias}.v_uuid IS NOT DISTINCT FROM {outer_alias}.v_uuid \
                                     AND {alias}.v_bytes IS NOT DISTINCT FROM {outer_alias}.v_bytes"
                                ));
                            } else {
                                // Non-value column (e, a, tx) used in value position = ref lookup
                                sub_where.push(format!("{alias}.v_ref = {outer_alias}.{outer_col}"));
                            }
                        }
                    }
                    _ => {
                        if let Some(constraint) = bind_constant_value(&alias, &p.value, builder)? {
                            sub_where.push(constraint);
                        }
                    }
                }

                // Temporal filtering in subquery too
                if !temporal.history {
                    sub_where.push(format!("{alias}.added = true"));
                }
                if let Some(as_of_tx) = temporal.as_of {
                    let param = builder.bind_bigint(as_of_tx);
                    sub_where.push(format!("{alias}.tx <= {param}"));

                    // Only add NOT EXISTS for cardinality-one attributes
                    let is_cardinality_many = match &p.attribute {
                        PatternNonValuePlace::Ident(kw) => {
                            let ident_str = keyword_to_ident(kw);
                            crate::cache::get_cache()
                                .resolve_ident(&ident_str)
                                .and_then(|entid| crate::cache::get_cache().get_attribute(entid))
                                .map(|info| info.cardinality == "many")
                                .unwrap_or(false)
                        }
                        PatternNonValuePlace::Entid(id) => crate::cache::get_cache()
                            .get_attribute(*id)
                            .map(|info| info.cardinality == "many")
                            .unwrap_or(false),
                        _ => false,
                    };

                    if !is_cardinality_many {
                        let param2 = builder.bind_bigint(as_of_tx);
                        sub_where.push(format!(
                            "NOT EXISTS (SELECT 1 FROM {schema_prefix}datoms newer \
                             WHERE newer.e = {alias}.e AND newer.a = {alias}.a \
                             AND newer.added = true \
                             AND newer.tx > {alias}.tx AND newer.tx <= {param2})"
                        ));
                    }
                }
                if let Some(since_tx) = temporal.since {
                    let param = builder.bind_bigint(since_tx);
                    sub_where.push(format!("{alias}.tx > {param}"));
                }

                sub_joins.push(format!("{schema_prefix}datoms {alias}"));
            }
            _ => {
                return Err(":db.error/unsupported-query Only pattern clauses (e.g. [?e :attr ?v]) \
                            are supported inside (not ...) / (not-join ...). Predicates and \
                            function calls inside NOT are not yet supported.".into());
            }
        }
    }

    if sub_joins.is_empty() {
        return Err(":db.error/empty-not NOT clause must contain at least one data pattern. \
                    Example: (not [?e :person/retired true])".into());
    }

    Ok(format!(
        "NOT EXISTS (SELECT 1 FROM {} WHERE {})",
        sub_joins.join(", "),
        sub_where.join(" AND ")
    ))
}

// ============================================================================
// Predicate clause builder
// ============================================================================

/// Build a SQL WHERE condition from a Datalog predicate like [(< ?age 30)].
fn build_predicate_clause(
    pred: &Predicate,
    var_to_alias: &HashMap<String, (String, &'static str)>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let op = pred.operator.0.as_str();

    let sql_op = match op {
        "<" => "<",
        ">" => ">",
        "<=" => "<=",
        ">=" => ">=",
        "=" => "=",
        "!=" => "!=",
        _ => return Err(format!(
            ":db.error/unsupported-predicate Unsupported predicate operator '{}'. \
             Supported operators: <, >, <=, >=, =, !=. \
             Example: [(< ?age 30)]",
            op
        ).into()),
    };

    if pred.args.len() != 2 {
        return Err(format!(
            ":db.error/predicate-arity Predicate '{}' requires exactly 2 arguments, got {}. \
             Example: [({} ?var value)]",
            op, pred.args.len(), op
        ).into());
    }

    let left = pred_arg_to_sql(&pred.args[0], var_to_alias)?;
    let right = pred_arg_to_sql(&pred.args[1], var_to_alias)?;

    // For value column comparisons, we need to cast the decoded value to numeric
    // so that comparisons work correctly on the underlying values
    Ok(format!("({} {} {})", left, sql_op, right))
}

/// Convert a predicate argument to a SQL expression.
/// With typed columns, value comparisons use COALESCE across the native typed columns,
/// which gives correct range semantics (numeric < on numbers, text < on strings, etc).
fn pred_arg_to_sql(
    arg: &FnArg,
    var_to_alias: &HashMap<String, (String, &'static str)>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match arg {
        FnArg::Variable(v) => {
            let var_name = format!("{}", v);
            if let Some((alias, col)) = var_to_alias.get(var_name.as_str()) {
                if *col == "v" {
                    // For value comparisons, use COALESCE across typed columns
                    // This produces the native typed value for correct comparisons
                    Ok(format!(
                        "COALESCE({alias}.v_ref::NUMERIC, \
                         CASE WHEN {alias}.v_bool THEN 1 ELSE 0 END::NUMERIC, \
                         {alias}.v_long::NUMERIC, \
                         {alias}.v_double::NUMERIC, \
                         EXTRACT(EPOCH FROM {alias}.v_instant)::NUMERIC, \
                         NULL)"
                    ))
                } else {
                    Ok(format!("{}.{}", alias, col))
                }
            } else {
                Err(format!(
                    ":db.error/unbound-var Unbound variable '{}' in predicate. \
                     Every variable used in a predicate must first appear in a :where pattern. \
                     Add a pattern like [?e :some-attr {}] to bind it.",
                    var_name, var_name
                ).into())
            }
        }
        FnArg::EntidOrInteger(i) => Ok(format!("{}", i)),
        FnArg::Constant(NonIntegerConstant::Float(f)) => Ok(format!("{}", f.into_inner())),
        FnArg::Constant(NonIntegerConstant::Text(s)) => {
            // SECURITY: Escape single quotes to prevent SQL injection
            let escaped = s.as_ref().replace('\'', "''");
            Ok(format!("'{}'", escaped))
        }
        FnArg::Constant(NonIntegerConstant::Boolean(b)) => {
            Ok(format!("{}", if *b { 1 } else { 0 }))
        }
        _ => Err(":db.error/unsupported-pred-arg Unsupported predicate argument type. \
                  Supported types: variables (?x), integers, floats, strings, and booleans.".into()),
    }
}

// ============================================================================
// Rule CTE builder
// ============================================================================

/// Information about a rule CTE needed to join it into the main query.
struct RuleCteInfo {
    /// FROM fragment, e.g., "ancestor"
    from_fragment: String,
    /// Map of variable name to (alias, column_name) for var_to_alias
    var_to_col: HashMap<String, (String, &'static str)>,
}

/// Build WITH RECURSIVE CTE(s) from rule definitions and invocations.
///
/// Returns:
/// - The CTE prefix string (e.g., "WITH RECURSIVE rule_name(col1, col2) AS (...)")
/// - A RuleCteInfo for joining the CTE into the main query
fn build_rule_ctes(
    rules: &[Rule],
    invocations: &[&RuleInvocation],
    builder: &mut SqlBuilder<'_>,
    temporal: &TemporalOption,
    schema_prefix: &str,
) -> Result<(String, RuleCteInfo), Box<dyn std::error::Error + Send + Sync>> {
    let mut cte_parts = Vec::new();
    let mut var_to_col: HashMap<String, (String, &'static str)> = HashMap::new();
    let mut cte_table_name = String::new();

    for invocation in invocations {
        let rule_name = invocation.name.0.as_str();

        // Find the matching rule definition
        let rule = rules
            .iter()
            .find(|r| r.name.0.as_str() == rule_name)
            .ok_or_else(|| {
                let available_rules: Vec<&str> = rules.iter().map(|r| r.name.0.as_str()).collect();
                format!(
                    ":db.error/rule-not-found No rule definition found for '{}'. \
                     Available rules: [{}]. Rules must be defined in the :with section of the query.",
                    rule_name,
                    available_rules.join(", ")
                )
            })?;

        // Determine the arity (number of arguments) from the first clause head
        let arity = if let Some(first_clause) = rule.clauses.first() {
            first_clause.head.args.len()
        } else {
            return Err(format!(
                ":db.error/empty-rule Rule '{}' has no clauses. Each rule must have at least one \
                 clause with a head and body patterns.",
                rule_name
            ).into());
        };

        // Generate column names for the CTE: col0, col1, ...
        let cte_cols: Vec<String> = (0..arity).map(|i| format!("col{}", i)).collect();
        let cte_col_list = cte_cols.join(", ");

        // Build UNION of each rule clause body.
        // Use UNION (not UNION ALL) to eliminate duplicate rows across rule
        // clauses, matching Datalog set-semantics. Multiple rule clauses may
        // produce the same binding tuple, and the result should be a set.
        let mut union_parts = Vec::new();
        for clause in &rule.clauses {
            let clause_sql =
                build_rule_clause_sql(clause, &cte_cols, builder, temporal, rule_name, schema_prefix)?;
            union_parts.push(clause_sql);
        }

        let cte_body = union_parts.join(" UNION ");

        let is_recursive = rule.clauses.iter().any(|clause| {
            clause.body.iter().any(
                |wc| matches!(wc, WhereClause::RuleExpr(ri) if ri.name.0.as_str() == rule_name),
            )
        });

        let recursive_kw = if is_recursive { "RECURSIVE " } else { "" };

        cte_parts.push(format!(
            "WITH {recursive_kw}{rule_name}({cte_col_list}) AS ({cte_body})"
        ));

        // Bind invocation arguments to CTE columns
        // The invocation (ancestor ?anc ?desc) binds ?anc -> ancestor.col0, ?desc -> ancestor.col1
        static CTE_COLS: [&str; 8] = [
            "col0", "col1", "col2", "col3", "col4", "col5", "col6", "col7",
        ];
        for (i, arg) in invocation.args.iter().enumerate() {
            if let FnArg::Variable(v) = arg {
                if i < CTE_COLS.len() {
                    let var_name = format!("{}", v);
                    var_to_col.insert(var_name, (rule_name.to_string(), CTE_COLS[i]));
                }
            }
        }

        // Store the CTE table name for the FROM fragment
        cte_table_name = rule_name.to_string();
    }

    // Join all CTEs (in practice we only support one CTE for now)
    let cte_sql = cte_parts.join(", ");

    let cte_info = RuleCteInfo {
        from_fragment: cte_table_name,
        var_to_col,
    };

    Ok((cte_sql, cte_info))
}

/// Build a SQL WHERE condition from a Datalog predicate in a rule body.
fn build_predicate_clause_for_rule(
    pred: &Predicate,
    var_to_alias: &HashMap<String, (String, &'static str)>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let op = pred.operator.0.as_str();

    let sql_op = match op {
        "<" => "<",
        ">" => ">",
        "<=" => "<=",
        ">=" => ">=",
        "=" => "=",
        "!=" => "!=",
        _ => return Err(format!(
            ":db.error/unsupported-predicate Unsupported predicate operator '{}' in rule. \
             Supported operators: <, >, <=, >=, =, !=. \
             Example: [(< ?age 30)]",
            op
        ).into()),
    };

    if pred.args.len() != 2 {
        return Err(format!(
            ":db.error/predicate-arity Predicate '{}' in rule requires exactly 2 arguments, got {}. \
             Example: [({} ?var value)]",
            op, pred.args.len(), op
        ).into());
    }

    let left = pred_arg_to_sql_for_rule(&pred.args[0], var_to_alias)?;
    let right = pred_arg_to_sql_for_rule(&pred.args[1], var_to_alias)?;

    Ok(format!("({} {} {})", left, sql_op, right))
}

/// Resolve variable references in expressions for rule context.
fn resolve_var_refs_for_rule(
    expr: &str,
    var_to_alias: &HashMap<String, (String, &'static str)>,
) -> String {
    let mut result = expr.to_string();

    // Find all NUM_VAR_REF:?xxx and VAR_REF:?xxx occurrences and replace them
    loop {
        let num_pos = result.find("NUM_VAR_REF:");
        let var_pos = result.find("VAR_REF:");

        let (start, prefix_len, is_numeric) = match (num_pos, var_pos) {
            (Some(n), Some(v)) => {
                if n < v {
                    (n, 12, true) // "NUM_VAR_REF:" is 12 chars
                } else {
                    (v, 8, false) // "VAR_REF:" is 8 chars
                }
            }
            (Some(n), None) => (n, 12, true),
            (None, Some(v)) => (v, 8, false),
            (None, None) => break,
        };

        let rest = &result[start + prefix_len..];
        // Variable names end at space, ), or end of string
        let end = rest
            .find(|c: char| {
                c == ' ' || c == ')' || c == ',' || c == '+' || c == '-' || c == '*' || c == '/'
            })
            .unwrap_or(rest.len());

        let var_name = rest[..end].to_string();

        let replacement = if let Some((alias, col)) = var_to_alias.get(&var_name) {
            if is_numeric {
                // Numeric context
                if *col == "v" {
                    // For value columns, use numeric coalesce for arithmetic
                    format!(
                        "COALESCE({alias}.v_long::NUMERIC, {alias}.v_double::NUMERIC, {alias}.v_ref::NUMERIC, 0)"
                    )
                } else if *col == "computed" && alias.starts_with("rec_") {
                    // Recursive variable - need to determine the right column
                    // For now, assume col0
                    format!("{alias}.col0::NUMERIC")
                } else {
                    format!("{alias}.{col}::NUMERIC")
                }
            } else {
                // Text/regular context
                if *col == "v" {
                    build_value_decode_expr(alias)
                } else if *col == "computed" && alias.starts_with("rec_") {
                    format!("{alias}.col0")
                } else {
                    format!("{alias}.{col}")
                }
            }
        } else {
            // Variable not bound - keep as-is for now
            format!("NULL")
        };

        let prefix = &result[..start];
        let suffix = &result[start + prefix_len + end..];
        result = format!("{}{}{}", prefix, replacement, suffix);
    }

    result
}

/// Convert a predicate argument to SQL expression in rule context.
fn pred_arg_to_sql_for_rule(
    arg: &FnArg,
    var_to_alias: &HashMap<String, (String, &'static str)>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match arg {
        FnArg::Variable(v) => {
            let var_name = format!("{}", v);
            if let Some((alias, col)) = var_to_alias.get(var_name.as_str()) {
                if *col == "v" {
                    // For value comparisons in rules, use COALESCE across typed columns
                    Ok(format!(
                        "COALESCE({alias}.v_ref::NUMERIC, \
                         CASE WHEN {alias}.v_bool THEN 1 ELSE 0 END::NUMERIC, \
                         {alias}.v_long::NUMERIC, \
                         {alias}.v_double::NUMERIC, \
                         EXTRACT(EPOCH FROM {alias}.v_instant)::NUMERIC, \
                         NULL)"
                    ))
                } else if *col == "computed" {
                    // This is from a recursive rule invocation
                    Ok(format!("{}.col0", alias))  // TODO: handle proper column mapping
                } else {
                    Ok(format!("{}.{}", alias, col))
                }
            } else {
                Err(format!(
                    ":db.error/unbound-var Unbound variable '{}' in rule predicate. \
                     Every variable used in a predicate must first appear in a pattern in the rule body. \
                     Add a pattern like [?e :some-attr {}] to bind it.",
                    var_name, var_name
                ).into())
            }
        }
        FnArg::EntidOrInteger(i) => Ok(format!("{}", i)),
        FnArg::Constant(NonIntegerConstant::Float(f)) => Ok(format!("{}", f.into_inner())),
        FnArg::Constant(NonIntegerConstant::Text(s)) => {
            // SECURITY: Escape single quotes to prevent SQL injection
            let escaped = s.as_ref().replace('\'', "''");
            Ok(format!("'{}'", escaped))
        }
        FnArg::Constant(NonIntegerConstant::Boolean(b)) => {
            Ok(format!("{}", if *b { 1 } else { 0 }))
        }
        _ => Err(":db.error/unsupported-pred-arg Unsupported predicate argument type in rule. \
                  Supported types: variables (?x), integers, floats, strings, and booleans.".into()),
    }
}

/// Build SQL for a single rule clause body.
///
/// Each clause has a head (defining result columns) and a body (patterns + optional
/// recursive rule invocations).
fn build_rule_clause_sql(
    clause: &edn::query::RuleClause,
    _cte_cols: &[String],
    builder: &mut SqlBuilder<'_>,
    temporal: &TemporalOption,
    rule_name: &str,
    schema_prefix: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Map head argument variables to CTE column positions
    let mut head_var_to_col: HashMap<String, usize> = HashMap::new();
    for (i, arg) in clause.head.args.iter().enumerate() {
        if let FnArg::Variable(v) = arg {
            head_var_to_col.insert(format!("{}", v), i);
        }
    }

    // Process body patterns
    let mut pattern_joins = Vec::new();
    let mut where_parts = Vec::new();
    let mut body_var_to_alias: HashMap<String, (String, &'static str)> = HashMap::new();
    let mut recursive_join: Option<String> = None;
    let mut recursive_alias = String::new();

    let mut pattern_idx = 0;
    for wc in &clause.body {
        match wc {
            WhereClause::Pattern(p) => {
                let alias = format!("r_d{}", pattern_idx);
                pattern_idx += 1;

                // Entity position
                match &p.entity {
                    PatternNonValuePlace::Variable(v) => {
                        let var_name = format!("{}", v);
                        if let Some((existing, col)) = body_var_to_alias.get(&var_name) {
                            where_parts.push(format!("{alias}.e = {existing}.{col}"));
                        } else {
                            body_var_to_alias.insert(var_name, (alias.clone(), "e"));
                        }
                    }
                    PatternNonValuePlace::Entid(id) => {
                        let param = builder.bind_bigint(*id);
                        where_parts.push(format!("{alias}.e = {param}"));
                    }
                    PatternNonValuePlace::Ident(kw) => {
                        let ident_str = keyword_to_ident(kw);
                        let param = builder.bind_text(ident_str);
                        where_parts.push(format!(
                            "{alias}.e = (SELECT entid FROM {schema_prefix}idents WHERE ident = {param})"
                        ));
                    }
                    PatternNonValuePlace::Placeholder => {}
                }

                // Attribute position
                match &p.attribute {
                    PatternNonValuePlace::Ident(kw) => {
                        let ident_str = keyword_to_ident(kw);
                        let param = builder.bind_text(ident_str);
                        where_parts.push(format!(
                            "{alias}.a = (SELECT entid FROM {schema_prefix}schema WHERE ident = {param})"
                        ));
                    }
                    PatternNonValuePlace::Entid(id) => {
                        let param = builder.bind_bigint(*id);
                        where_parts.push(format!("{alias}.a = {param}"));
                    }
                    PatternNonValuePlace::Variable(v) => {
                        let var_name = format!("{}", v);
                        if let Some((existing, col)) = body_var_to_alias.get(&var_name) {
                            where_parts.push(format!("{alias}.a = {existing}.{col}"));
                        } else {
                            body_var_to_alias.insert(var_name, (alias.clone(), "a"));
                        }
                    }
                    PatternNonValuePlace::Placeholder => {}
                }

                // Value position
                match &p.value {
                    PatternValuePlace::Variable(v) => {
                        let var_name = format!("{}", v);
                        if let Some((existing, col)) = body_var_to_alias.get(&var_name) {
                            if *col == "v" {
                                where_parts.push(format!(
                                    "{alias}.value_type_tag = {existing}.value_type_tag \
AND {alias}.v_ref IS NOT DISTINCT FROM {existing}.v_ref \
AND {alias}.v_bool IS NOT DISTINCT FROM {existing}.v_bool \
AND {alias}.v_long IS NOT DISTINCT FROM {existing}.v_long \
AND {alias}.v_double IS NOT DISTINCT FROM {existing}.v_double \
AND {alias}.v_text IS NOT DISTINCT FROM {existing}.v_text \
AND {alias}.v_keyword IS NOT DISTINCT FROM {existing}.v_keyword \
AND {alias}.v_instant IS NOT DISTINCT FROM {existing}.v_instant \
AND {alias}.v_uuid IS NOT DISTINCT FROM {existing}.v_uuid \
AND {alias}.v_bytes IS NOT DISTINCT FROM {existing}.v_bytes"
                                ));
                            } else {
                                // Non-value column used in value position = ref lookup
                                where_parts.push(format!("{alias}.v_ref = {existing}.{col}"));
                            }
                        } else {
                            body_var_to_alias.insert(var_name, (alias.clone(), "v"));
                        }
                    }
                    _ => {
                        if let Some(constraint) = bind_constant_value(&alias, &p.value, builder)? {
                            where_parts.push(constraint);
                        }
                    }
                }

                // Temporal filtering
                if !temporal.history {
                    where_parts.push(format!("{alias}.added = true"));
                }
                if let Some(as_of) = temporal.as_of {
                    let param = builder.bind_bigint(as_of);
                    where_parts.push(format!("{alias}.tx <= {param}"));

                    // Only add NOT EXISTS for cardinality-one attributes
                    let is_cardinality_many = match &p.attribute {
                        PatternNonValuePlace::Ident(kw) => {
                            let ident_str = keyword_to_ident(kw);
                            crate::cache::get_cache()
                                .resolve_ident(&ident_str)
                                .and_then(|entid| crate::cache::get_cache().get_attribute(entid))
                                .map(|info| info.cardinality == "many")
                                .unwrap_or(false)
                        }
                        PatternNonValuePlace::Entid(id) => crate::cache::get_cache()
                            .get_attribute(*id)
                            .map(|info| info.cardinality == "many")
                            .unwrap_or(false),
                        _ => false,
                    };

                    if !is_cardinality_many {
                        let param2 = builder.bind_bigint(as_of);
                        where_parts.push(format!(
                            "NOT EXISTS (SELECT 1 FROM {schema_prefix}datoms newer \
                             WHERE newer.e = {alias}.e AND newer.a = {alias}.a \
                             AND newer.added = true \
                             AND newer.tx > {alias}.tx AND newer.tx <= {param2})"
                        ));
                    }
                }
                if let Some(since) = temporal.since {
                    let param = builder.bind_bigint(since);
                    where_parts.push(format!("{alias}.tx > {param}"));
                }

                pattern_joins.push(format!("{schema_prefix}datoms {alias}"));
            }
            WhereClause::RuleExpr(ri) if ri.name.0.as_str() == rule_name => {
                // Recursive self-reference: JOIN against the CTE itself
                recursive_alias = format!("rec_{}", rule_name);
                recursive_join = Some(format!("{rule_name} {recursive_alias}"));

                // Bind recursive arguments to body variables
                for (i, arg) in ri.args.iter().enumerate() {
                    if let FnArg::Variable(v) = arg {
                        let var_name = format!("{}", v);
                        let col_ref = format!("{}.col{}", recursive_alias, i);
                        // Link the recursive CTE column to the body variable
                        if let Some((alias, col)) = body_var_to_alias.get(&var_name) {
                            if *col == "v" {
                                // Value column: use v_ref for ref-type recursive joins
                                where_parts.push(format!("{col_ref}::BIGINT = {alias}.v_ref"));
                            } else {
                                where_parts.push(format!("{col_ref}::BIGINT = {alias}.{col}",));
                            }
                        } else {
                            // New variable - bind to recursive column
                            body_var_to_alias
                                .insert(var_name, (recursive_alias.clone(), "computed"));
                        }
                    }
                }
            }
            WhereClause::Pred(pred) => {
                // Handle predicates in rule bodies
                let pred_sql = build_predicate_clause_for_rule(pred, &body_var_to_alias)?;
                where_parts.push(pred_sql);
            }
            WhereClause::WhereFn(wf) => {
                // Handle arithmetic function bindings in rule bodies
                if let Some((result_var, computed_expr)) = build_where_fn_binding(wf)? {
                    // Store the computed expression for later use in SELECT
                    body_var_to_alias.insert(result_var, ("COMPUTED".to_string(), "expr"));
                    // We'll handle this in the SELECT clause
                } else {
                    return Err(format!(
                        ":db.error/unsupported-rule-fn Unsupported function '{}' in rule body. \
                         Supported functions: *, +, -, /",
                        wf.operator.0
                    ).into());
                }
            }
            _ => {
                return Err(
                    ":db.error/unsupported-rule-body Only data patterns, predicates, arithmetic functions, \
                     and recursive rule invocations are supported in rule bodies. \
                     Other clause types are not yet supported."
                        .into(),
                );
            }
        }
    }

    // Build SELECT expressions: map head variables to body columns
    let mut select_parts = Vec::new();
    let mut computed_expressions: HashMap<String, String> = HashMap::new();

    // First pass: collect computed expressions from WhereFn clauses
    for wc in &clause.body {
        if let WhereClause::WhereFn(wf) = wc {
            if let Some((result_var, computed_expr)) = build_where_fn_binding(wf)? {
                // Resolve variable references in the computed expression
                let resolved_expr = resolve_var_refs_for_rule(&computed_expr, &body_var_to_alias);
                computed_expressions.insert(result_var, resolved_expr);
            }
        }
    }

    for (i, arg) in clause.head.args.iter().enumerate() {
        if let FnArg::Variable(v) = arg {
            let var_name = format!("{}", v);

            // Check if this is a computed expression first
            if let Some(expr) = computed_expressions.get(&var_name) {
                select_parts.push(format!("({})", expr));
            } else if let Some((alias, col)) = body_var_to_alias.get(var_name.as_str()) {
                if alias == &recursive_alias && *col == "computed" {
                    // Recursive variable mapped directly
                    select_parts.push(format!("{}.col{}::BIGINT", recursive_alias, i));
                } else if alias == "COMPUTED" && *col == "expr" {
                    // This shouldn't happen as we handle computed expressions above
                    select_parts.push("NULL::BIGINT".to_string());
                } else if *col == "v" {
                    // Value column: for ref-type, use v_ref directly
                    select_parts.push(format!("{alias}.v_ref"));
                } else {
                    select_parts.push(format!("{alias}.{col}"));
                }
            } else {
                select_parts.push("NULL::BIGINT".to_string());
            }
        } else {
            select_parts.push("NULL::BIGINT".to_string());
        }
    }

    // Combine FROM
    let mut from_parts: Vec<String> = pattern_joins;
    if let Some(ref rj) = recursive_join {
        from_parts.push(rj.clone());
    }

    if from_parts.is_empty() {
        return Err(":db.error/empty-rule-body Rule clause body has no data patterns. \
                    Each rule clause must contain at least one pattern like [?e :attr ?v].".into());
    }

    let sql = if where_parts.is_empty() {
        format!(
            "SELECT {} FROM {}",
            select_parts.join(", "),
            from_parts.join(", ")
        )
    } else {
        format!(
            "SELECT {} FROM {} WHERE {}",
            select_parts.join(", "),
            from_parts.join(", "),
            where_parts.join(" AND ")
        )
    };

    Ok(sql)
}

// ============================================================================
// Helper functions for OR-clause predicate support
// ============================================================================

/// Check if a pattern binds a specific variable.
fn pattern_binds_var(pattern: &edn::query::Pattern, var_name: &str) -> bool {
    use edn::query::{PatternNonValuePlace, PatternValuePlace};

    // Check entity position
    if let PatternNonValuePlace::Variable(v) = &pattern.entity {
        if format!("{}", v) == var_name {
            return true;
        }
    }

    // Check attribute position
    if let PatternNonValuePlace::Variable(v) = &pattern.attribute {
        if format!("{}", v) == var_name {
            return true;
        }
    }

    // Check value position
    if let PatternValuePlace::Variable(v) = &pattern.value {
        if format!("{}", v) == var_name {
            return true;
        }
    }

    // Check tx position
    if let PatternNonValuePlace::Variable(v) = &pattern.tx {
        if format!("{}", v) == var_name {
            return true;
        }
    }

    // Check added position (if it exists)
    if let PatternNonValuePlace::Variable(v) = &pattern.added {
        if format!("{}", v) == var_name {
            return true;
        }
    }

    false
}
