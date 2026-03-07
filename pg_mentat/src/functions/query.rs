use edn::parse;
use edn::query::{
    Binding, Direction, Element, FindSpec, FnArg, Limit, NonIntegerConstant, OrWhereClause, Order,
    ParsedQuery, PatternNonValuePlace, PatternValuePlace, Predicate, Rule, RuleInvocation,
    VariableOrPlaceholder, WhereClause, WhereFn,
};
use pgrx::datum::DatumWithOid;
use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::json;
use std::collections::HashMap;

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

/// Execute a Datalog query and return results as JSON
///
/// Supports temporal options via the inputs JSON parameter:
/// - `{"asOf": <tx_id>}` - return datoms as of transaction tx_id
/// - `{"since": <tx_id>}` - return datoms since transaction tx_id
/// - `{"history": true}` - return all datom versions including retractions
#[pg_extern]
pub fn mentat_query(
    query: &str,
    inputs: JsonB,
) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    let _parsed_value = parse::value(query)?;
    let parsed_query = mentat_core::parse_query(query)?;

    let temporal = parse_temporal_options(&inputs.0);
    let has_aggregates = find_spec_has_aggregates(&parsed_query.find_spec);
    let find_vars = extract_find_variables(&parsed_query.find_spec);

    let mut builder = SqlBuilder::new();
    let sql_query = build_sql_from_datalog(&parsed_query, &find_vars, &mut builder, &temporal)?;

    let params = builder.params;
    let results = Spi::connect(|client| {
        let mut rows_json = Vec::new();

        for row in client.select(&sql_query, None, &params)? {
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

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(rows_json)
    })?;

    let response =
        format_find_response(&parsed_query.find_spec, &find_vars, results, has_aggregates);

    Ok(JsonB(response))
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

/// Build a SQL CASE expression that decodes a BYTEA value column based on
/// the value_type_tag for the given table alias.
fn build_value_decode_expr(alias: &str) -> String {
    let i64_decode = format!(
        "(get_byte({alias}.v, 0)::BIGINT | \
         (get_byte({alias}.v, 1)::BIGINT << 8) | \
         (get_byte({alias}.v, 2)::BIGINT << 16) | \
         (get_byte({alias}.v, 3)::BIGINT << 24) | \
         (get_byte({alias}.v, 4)::BIGINT << 32) | \
         (get_byte({alias}.v, 5)::BIGINT << 40) | \
         (get_byte({alias}.v, 6)::BIGINT << 48) | \
         (get_byte({alias}.v, 7)::BIGINT << 56))"
    );

    let double_decode = format!("'d:' || ({i64_decode})::TEXT");

    format!(
        "CASE {alias}.value_type_tag \
         WHEN {ref_tag} THEN {i64_expr}::TEXT \
         WHEN {bool_tag} THEN (get_byte({alias}.v, 0) != 0)::TEXT \
         WHEN {long_tag} THEN {i64_expr}::TEXT \
         WHEN {double_tag} THEN {double_expr} \
         WHEN {instant_tag} THEN {i64_expr}::TEXT \
         WHEN {str_tag} THEN convert_from({alias}.v, 'UTF8') \
         WHEN {kw_tag} THEN ':' || convert_from({alias}.v, 'UTF8') \
         WHEN {uuid_tag} THEN encode({alias}.v, 'hex') \
         WHEN {bytes_tag} THEN encode({alias}.v, 'hex') \
         ELSE NULL::TEXT \
         END",
        alias = alias,
        ref_tag = type_tag::REF,
        bool_tag = type_tag::BOOLEAN,
        long_tag = type_tag::LONG,
        double_tag = type_tag::DOUBLE,
        double_expr = double_decode,
        instant_tag = type_tag::INSTANT,
        str_tag = type_tag::STRING,
        kw_tag = type_tag::KEYWORD,
        uuid_tag = type_tag::UUID,
        bytes_tag = type_tag::BYTES,
        i64_expr = i64_decode,
    )
}

/// Encode a constant value from a pattern's value position into BYTEA + type tag,
/// and bind it as a parameter. Returns a WHERE clause fragment.
fn bind_constant_value(
    alias: &str,
    place: &PatternValuePlace,
    builder: &mut SqlBuilder<'_>,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    match place {
        PatternValuePlace::EntidOrInteger(i) => {
            let bytes = i.to_le_bytes().to_vec();
            let param = builder.bind_bytea(bytes);
            Ok(Some(format!(
                "({alias}.v = {param} AND {alias}.value_type_tag = {tag})",
                tag = type_tag::LONG
            )))
        }
        PatternValuePlace::IdentOrKeyword(kw) => {
            let ident_str = keyword_to_ident(kw);
            let stored = if ident_str.starts_with(':') {
                &ident_str[1..]
            } else {
                &ident_str
            };
            let bytes = stored.as_bytes().to_vec();
            let param = builder.bind_bytea(bytes);
            Ok(Some(format!(
                "({alias}.v = {param} AND {alias}.value_type_tag = {tag})",
                tag = type_tag::KEYWORD
            )))
        }
        PatternValuePlace::Constant(constant) => match constant {
            NonIntegerConstant::Boolean(b) => {
                let bytes = vec![if *b { 1u8 } else { 0u8 }];
                let param = builder.bind_bytea(bytes);
                Ok(Some(format!(
                    "({alias}.v = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::BOOLEAN
                )))
            }
            NonIntegerConstant::Float(f) => {
                let bytes = f.into_inner().to_le_bytes().to_vec();
                let param = builder.bind_bytea(bytes);
                Ok(Some(format!(
                    "({alias}.v = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::DOUBLE
                )))
            }
            NonIntegerConstant::Text(s) => {
                let bytes = s.as_ref().as_bytes().to_vec();
                let param = builder.bind_bytea(bytes);
                Ok(Some(format!(
                    "({alias}.v = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::STRING
                )))
            }
            NonIntegerConstant::Instant(dt) => {
                let micros = dt.timestamp_micros();
                let bytes = micros.to_le_bytes().to_vec();
                let param = builder.bind_bytea(bytes);
                Ok(Some(format!(
                    "({alias}.v = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::INSTANT
                )))
            }
            NonIntegerConstant::Uuid(u) => {
                let bytes = u.as_bytes().to_vec();
                let param = builder.bind_bytea(bytes);
                Ok(Some(format!(
                    "({alias}.v = {param} AND {alias}.value_type_tag = {tag})",
                    tag = type_tag::UUID
                )))
            }
            NonIntegerConstant::BigInteger(_) => {
                Err("BigInteger constants are not supported in query patterns".into())
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
fn build_sql_from_datalog(
    parsed: &ParsedQuery,
    find_vars: &[String],
    builder: &mut SqlBuilder<'_>,
    temporal: &TemporalOption,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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
            let fj = build_fulltext_join(wf, fts_idx, builder, &mut extra_var_bindings)?;
            fts_joins.push(fj);
        } else {
            // Arithmetic binding functions: [(* ?age 2) ?double-age]
            if let Some((var_name, expr)) = build_where_fn_binding(wf)? {
                extra_var_bindings.insert(var_name, expr);
            } else {
                return Err(format!(
                    "Where-function '{}' is not yet supported in query translation",
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
            build_rule_ctes(&parsed.rules, &rule_invocations, builder, temporal)?;
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
        )?
    };

    // Handle OR-joins
    let (query_sql, has_union) = if or_joins.is_empty() {
        (base_sql, false)
    } else {
        if or_joins.len() > 1 {
            return Err("Multiple OR-join clauses in a single query are not yet supported".into());
        }

        let or_join = or_joins[0];
        let mut union_parts = Vec::new();

        for or_clause in &or_join.clauses {
            let arm_patterns: Vec<&edn::query::Pattern> = match or_clause {
                OrWhereClause::Clause(WhereClause::Pattern(p)) => vec![p],
                OrWhereClause::And(clauses) => {
                    let mut ps = Vec::new();
                    for c in clauses {
                        match c {
                            WhereClause::Pattern(p) => ps.push(p),
                            _ => return Err(
                                "Non-pattern clauses inside (or (and ...)) are not yet supported"
                                    .into(),
                            ),
                        }
                    }
                    ps
                }
                _ => return Err("Non-pattern clauses inside (or ...) are not yet supported".into()),
            };

            let mut combined: Vec<&edn::query::Pattern> = pattern_clauses.clone();
            combined.extend(arm_patterns);

            let mut arm_builder = SqlBuilder::new();
            let (arm_sql, _arm_var_to_alias) = build_extended_pattern_query(
                &combined,
                &not_joins,
                &predicates,
                &fts_joins,
                &extra_var_bindings,
                find_vars,
                &parsed.find_spec,
                &mut arm_builder,
                temporal,
                &rule_cte_info,
            )?;

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
    let var_alias_ref = if has_union { None } else { Some(&base_var_to_alias) };
    let query_sql = append_order_by(query_sql, &parsed.order, find_vars, var_alias_ref);

    // Append LIMIT
    let query_sql = append_limit(query_sql, &parsed.limit, &parsed.find_spec);

    Ok(query_sql)
}

// ============================================================================
// Fulltext search support
// ============================================================================

/// Represents a fulltext search join with its FROM and WHERE fragments.
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
) -> Result<FtsJoin, Box<dyn std::error::Error + Send + Sync>> {
    if wf.args.len() < 3 {
        return Err("fulltext requires at least 3 arguments: (fulltext $ :attr \"term\")".into());
    }

    let attr_ident = match &wf.args[1] {
        FnArg::IdentOrKeyword(kw) => keyword_to_ident(kw),
        _ => return Err("fulltext second argument must be a keyword attribute".into()),
    };

    let search_term = match &wf.args[2] {
        FnArg::Constant(NonIntegerConstant::Text(s)) => s.as_ref().clone(),
        _ => return Err("fulltext third argument must be a string search term".into()),
    };

    let fts_alias = format!("fts{}", fts_idx);
    let datoms_alias = format!("fts_d{}", fts_idx);

    let attr_param = builder.bind_text(attr_ident);

    let mut where_parts = Vec::new();
    where_parts.push(format!(
        "{datoms_alias}.a = (SELECT entid FROM mentat.schema WHERE ident = {attr_param})"
    ));
    where_parts.push(format!(
        "{datoms_alias}.value_type_tag = {}",
        type_tag::STRING
    ));
    where_parts.push(format!(
        "{fts_alias}.text_value = convert_from({datoms_alias}.v, 'UTF8')"
    ));

    if !search_term.is_empty() {
        let search_param = builder.bind_text(search_term.clone());
        where_parts.push(format!(
            "{fts_alias}.search_vector @@ plainto_tsquery('english', {search_param})"
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
                        let score_param = builder.bind_text(search_term.clone());
                        var_bindings.insert(
                            var_name,
                            format!(
                                "ts_rank({fts_alias}.search_vector, plainto_tsquery('english', {score_param}))::TEXT"
                            ),
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    let from_fragment = format!("mentat.datoms {datoms_alias}, mentat.fulltext {fts_alias}");

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
        return Err(format!("Arithmetic function '{}' requires exactly 2 arguments", op).into());
    }

    let result_var = match &wf.binding {
        Binding::BindScalar(v) => format!("{}", v),
        _ => return Err(format!("Arithmetic function '{}' requires a scalar binding", op).into()),
    };

    let arg0 = fn_arg_to_placeholder(&wf.args[0]);
    let arg1 = fn_arg_to_placeholder(&wf.args[1]);

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
                        .map_or(false, |(_, col)| {
                            *col == "e" || *col == "a" || *col == "tx"
                        });
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
                let col_aliases: Vec<String> = (1..=find_vars.len())
                    .map(|i| format!("_c{}", i))
                    .collect();
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
        Limit::None => {
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
) -> Result<(String, HashMap<String, (String, &'static str)>), Box<dyn std::error::Error + Send + Sync>> {
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
                    where_clauses.push(format!(
                        "{alias}.e = {existing}.{col}",
                        existing = existing_alias
                    ));
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
                    "{alias}.e = (SELECT entid FROM mentat.idents WHERE ident = {param})"
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
                    "{alias}.a = (SELECT entid FROM mentat.schema WHERE ident = {param})"
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
                            "{alias}.v = {existing}.v AND {alias}.value_type_tag = {existing}.value_type_tag",
                            existing = existing_alias
                        ));
                    } else {
                        where_clauses.push(format!(
                            "{alias}.v = {existing}.{col}",
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
        }

        if let Some(since_tx) = temporal.since {
            let param = builder.bind_bigint(since_tx);
            where_clauses.push(format!("{alias}.tx > {param}"));
        }

        // Handle "added" variable binding for history queries (5th position in pattern)
        // Check if the pattern has an "added" variable: [?e ?a ?v ?tx ?added]
        // The EDN parser puts the 5th element in the tx position; for 5-element patterns
        // we handle this by checking find_vars for an "?added" variable.
        // Actually, the parser only supports 4-element patterns [e a v tx], so "?added"
        // bindings need special handling in the SELECT.

        joins.push(format!("mentat.datoms {alias}"));
    }

    // Add FTS joins
    for fj in fts_joins {
        joins.push(fj.from_fragment.clone());
        where_clauses.extend(fj.where_parts.iter().cloned());
    }

    // Handle NOT clauses as NOT EXISTS subqueries
    for not_join in not_joins {
        let not_sql = build_not_exists_subquery(not_join, &var_to_alias, builder, temporal)?;
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
                // Check for special ?added variable in history mode
                if inner_var == "?added" && temporal.history {
                    // Use the first datom alias's added column
                    if let Some(first_alias) = joins.first() {
                        let alias = first_alias
                            .strip_prefix("mentat.datoms ")
                            .unwrap_or("datoms0");
                        select_exprs.push(format!("{alias}.added::TEXT"));
                        if has_aggregates {
                            group_by_exprs.push(format!("{}", col_idx + 1));
                        }
                    } else {
                        select_exprs.push("NULL::TEXT".to_string());
                    }
                } else {
                    select_exprs.push("NULL::TEXT".to_string());
                }
            }
        }
    }

    if select_exprs.is_empty() {
        return Err("No find variables could be resolved to pattern bindings".into());
    }

    if joins.is_empty() && fts_joins.is_empty() {
        return Err("No where clauses produced any datom table joins".into());
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
        _ => return Err(format!("Unsupported aggregate function: {}", func_name).into()),
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
    // Find all VAR_REF:?xxx occurrences and replace them
    while let Some(start) = result.find("VAR_REF:") {
        let rest = &result[start + 8..];
        // Variable names end at space, ), or end of string
        let end = rest
            .find(|c: char| {
                c == ' ' || c == ')' || c == ',' || c == '+' || c == '-' || c == '*' || c == '/'
            })
            .unwrap_or(rest.len());
        let var_name = &rest[..end];

        let replacement = if let Some((alias, col)) = var_to_alias.get(var_name) {
            if *col == "v" {
                format!("({})", build_value_decode_expr(alias))
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
            &result[start + 8 + end..]
        );
    }
    result
}

// ============================================================================
// NOT EXISTS subquery builder
// ============================================================================

/// Build a NOT EXISTS subquery from a NotJoin clause.
fn build_not_exists_subquery(
    not_join: &edn::query::NotJoin,
    outer_var_to_alias: &HashMap<String, (String, &'static str)>,
    builder: &mut SqlBuilder<'_>,
    temporal: &TemporalOption,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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
                            "{alias}.e = (SELECT entid FROM mentat.idents WHERE ident = {param})"
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
                            "{alias}.a = (SELECT entid FROM mentat.schema WHERE ident = {param})"
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
                                    "{alias}.v = {outer_alias}.v AND {alias}.value_type_tag = {outer_alias}.value_type_tag"
                                ));
                            } else {
                                sub_where.push(format!("{alias}.v = {outer_alias}.{outer_col}"));
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
                }
                if let Some(since_tx) = temporal.since {
                    let param = builder.bind_bigint(since_tx);
                    sub_where.push(format!("{alias}.tx > {param}"));
                }

                sub_joins.push(format!("mentat.datoms {alias}"));
            }
            _ => {
                return Err("Only pattern clauses are supported inside NOT".into());
            }
        }
    }

    if sub_joins.is_empty() {
        return Err("NOT clause must contain at least one pattern".into());
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
        _ => return Err(format!("Unsupported predicate operator: {}", op).into()),
    };

    if pred.args.len() != 2 {
        return Err(format!("Predicate '{}' requires exactly 2 arguments", op).into());
    }

    let left = pred_arg_to_sql(&pred.args[0], var_to_alias)?;
    let right = pred_arg_to_sql(&pred.args[1], var_to_alias)?;

    // For value column comparisons, we need to cast the decoded value to numeric
    // so that comparisons work correctly on the underlying values
    Ok(format!("({} {} {})", left, sql_op, right))
}

/// Convert a predicate argument to a SQL expression.
fn pred_arg_to_sql(
    arg: &FnArg,
    var_to_alias: &HashMap<String, (String, &'static str)>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match arg {
        FnArg::Variable(v) => {
            let var_name = format!("{}", v);
            if let Some((alias, col)) = var_to_alias.get(var_name.as_str()) {
                if *col == "v" {
                    // For value comparisons, decode to text then cast to numeric
                    // This works for integer and double types
                    Ok(format!("({})", build_value_decode_expr(alias)))
                } else {
                    Ok(format!("{}.{}", alias, col))
                }
            } else {
                Err(format!("Unbound variable in predicate: {}", var_name).into())
            }
        }
        FnArg::EntidOrInteger(i) => Ok(format!("'{}'", i)),
        FnArg::Constant(NonIntegerConstant::Float(f)) => Ok(format!("'{}'", f.into_inner())),
        FnArg::Constant(NonIntegerConstant::Text(s)) => Ok(format!("'{}'", s.as_ref())),
        FnArg::Constant(NonIntegerConstant::Boolean(b)) => Ok(format!("'{}'", b)),
        _ => Err("Unsupported predicate argument type".into()),
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
            .ok_or_else(|| format!("No rule definition found for '{}'", rule_name))?;

        // Determine the arity (number of arguments) from the first clause head
        let arity = if let Some(first_clause) = rule.clauses.first() {
            first_clause.head.args.len()
        } else {
            return Err(format!("Rule '{}' has no clauses", rule_name).into());
        };

        // Generate column names for the CTE: col0, col1, ...
        let cte_cols: Vec<String> = (0..arity).map(|i| format!("col{}", i)).collect();
        let cte_col_list = cte_cols.join(", ");

        // Build UNION of each rule clause body
        let mut union_parts = Vec::new();
        for clause in &rule.clauses {
            let clause_sql =
                build_rule_clause_sql(clause, &cte_cols, builder, temporal, rule_name)?;
            union_parts.push(clause_sql);
        }

        let cte_body = union_parts.join(" UNION ALL ");

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
        static CTE_COLS: [&str; 8] = ["col0", "col1", "col2", "col3", "col4", "col5", "col6", "col7"];
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
                            "{alias}.e = (SELECT entid FROM mentat.idents WHERE ident = {param})"
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
                            "{alias}.a = (SELECT entid FROM mentat.schema WHERE ident = {param})"
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
                                    "{alias}.v = {existing}.v AND {alias}.value_type_tag = {existing}.value_type_tag"
                                ));
                            } else {
                                where_parts.push(format!("{alias}.v = {existing}.{col}"));
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
                }
                if let Some(since) = temporal.since {
                    let param = builder.bind_bigint(since);
                    where_parts.push(format!("{alias}.tx > {param}"));
                }

                pattern_joins.push(format!("mentat.datoms {alias}"));
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
                                // Value column: need to compare decoded value
                                // For ref-type values (entity IDs stored as BYTEA),
                                // decode and compare
                                where_parts.push(format!("({}::TEXT) = {alias}.e::TEXT", col_ref));
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
            _ => {
                return Err(
                    "Only patterns and recursive rule invocations are supported in rule bodies"
                        .into(),
                );
            }
        }
    }

    // Build SELECT expressions: map head variables to body columns
    let mut select_parts = Vec::new();
    for (i, arg) in clause.head.args.iter().enumerate() {
        if let FnArg::Variable(v) = arg {
            let var_name = format!("{}", v);
            if let Some((alias, col)) = body_var_to_alias.get(var_name.as_str()) {
                if alias == &recursive_alias && *col == "computed" {
                    // Recursive variable mapped directly
                    select_parts.push(format!("{}.col{}::BIGINT", recursive_alias, i));
                } else if *col == "v" {
                    // Value column: for ref-type, decode the entity reference
                    let i64_decode = format!(
                        "(get_byte({alias}.v, 0)::BIGINT | \
                         (get_byte({alias}.v, 1)::BIGINT << 8) | \
                         (get_byte({alias}.v, 2)::BIGINT << 16) | \
                         (get_byte({alias}.v, 3)::BIGINT << 24) | \
                         (get_byte({alias}.v, 4)::BIGINT << 32) | \
                         (get_byte({alias}.v, 5)::BIGINT << 40) | \
                         (get_byte({alias}.v, 6)::BIGINT << 48) | \
                         (get_byte({alias}.v, 7)::BIGINT << 56))"
                    );
                    select_parts.push(i64_decode);
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
        return Err("Rule clause body has no patterns".into());
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
