use pgrx::prelude::*;
use pgrx::JsonB;
use edn::parse;
use edn::query::{FindSpec, ParsedQuery, WhereClause};
use serde_json::json;
use std::collections::HashMap;

/// Execute a Datalog query and return results as JSON
///
/// Accepts a Datalog query like:
/// ```datalog
/// [:find ?name ?age
///  :where
///  [?e :person/name ?name]
///  [?e :person/age ?age]]
/// ```
///
/// Returns results as JSON:
/// ```json
/// {
///   "columns": ["?name", "?age"],
///   "results": [
///     ["Alice", 30],
///     ["Bob", 25]
///   ]
/// }
/// ```
#[pg_extern]
fn mentat_query(query: &str, _inputs: JsonB) -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    // Parse the EDN query
    let parsed_value = parse::value(query)?;
    let parsed_query = mentat_core::parse_query(query)?;

    // Extract find variables
    let find_vars = extract_find_variables(&parsed_query.find_spec);

    // Build SQL query from datalog clauses
    let sql_query = build_sql_from_datalog(&parsed_query, &find_vars)?;

    // Execute the SQL query and collect results
    let results = Spi::connect(|client| {
        let mut rows_json = Vec::new();

        for row in client.select(&sql_query, None, &[])? {
            let mut row_values = Vec::new();

            for (idx, _var) in find_vars.iter().enumerate() {
                // Try to get the value - handle different types
                let col_idx = (idx + 1) as usize;

                // Try as string first (most common)
                if let Ok(Some(val)) = row.get::<String>(col_idx) {
                    row_values.push(json!(val));
                } else if let Ok(Some(val)) = row.get::<i64>(col_idx) {
                    row_values.push(json!(val));
                } else if let Ok(Some(val)) = row.get::<f64>(col_idx) {
                    row_values.push(json!(val));
                } else if let Ok(Some(val)) = row.get::<bool>(col_idx) {
                    row_values.push(json!(val));
                } else {
                    row_values.push(json!(null));
                }
            }

            rows_json.push(json!(row_values));
        }

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(rows_json)
    })?;

    let response = json!({
        "columns": find_vars,
        "results": results
    });

    Ok(JsonB(response))
}

/// Extract variable names from FindSpec
fn extract_find_variables(find_spec: &FindSpec) -> Vec<String> {
    match find_spec {
        FindSpec::FindRel(vars) => vars.iter().map(|e| format!("{}", e)).collect(),
        FindSpec::FindColl(e) => vec![format!("{}", e)],
        FindSpec::FindTuple(vars) => vars.iter().map(|e| format!("{}", e)).collect(),
        FindSpec::FindScalar(e) => vec![format!("{}", e)],
    }
}

/// Build SQL query from Datalog clauses
/// This is a simplified implementation that handles basic patterns
fn build_sql_from_datalog(
    parsed: &ParsedQuery,
    find_vars: &[String],
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Track variable bindings to datom table aliases
    let mut var_to_alias: HashMap<String, String> = HashMap::new();
    let mut alias_counter = 0;
    let mut joins = Vec::new();
    let mut where_clauses = Vec::new();

    // Process where clauses
    for clause in &parsed.where_clauses {
        if let WhereClause::Pattern(pattern) = clause {
            let alias = format!("d{}", alias_counter);
            alias_counter += 1;

            // Handle entity variable
            let e_var = format!("{:?}", pattern.entity);
            if e_var.starts_with('?') {
                if let Some(existing_alias) = var_to_alias.get(&e_var) {
                    where_clauses.push(format!("{}.e = {}.e", existing_alias, alias));
                } else {
                    var_to_alias.insert(e_var.clone(), alias.clone());
                }
            }

            // Handle attribute - must be a keyword
            let attr_constraint = format!("{}.a = (SELECT entid FROM mentat.schema WHERE ident = '{}')",
                alias, format!("{:?}", pattern.attribute));
            where_clauses.push(attr_constraint);

            // Handle value variable or constant
            let v_pattern = format!("{:?}", pattern.value);
            if v_pattern.starts_with('?') {
                // This is a variable we need to return
                // We'll decode it in the SELECT clause
                var_to_alias.insert(v_pattern, alias.clone());
            }

            // Add join
            joins.push(format!("mentat.datoms {}", alias));
            where_clauses.push(format!("{}.added = true", alias));
        }
    }

    // Build SELECT clause with value decoding
    let mut select_exprs = Vec::new();
    for var in find_vars {
        if let Some(alias) = var_to_alias.get(var) {
            // Decode the value from BYTEA based on type
            let decode_expr = format!(
                "CASE {alias}.value_type_tag \
                 WHEN 1 THEN (get_byte({alias}.v, 0) != 0)::TEXT \
                 WHEN 2 THEN (get_byte({alias}.v, 0)::BIGINT | \
                             (get_byte({alias}.v, 1)::BIGINT << 8) | \
                             (get_byte({alias}.v, 2)::BIGINT << 16) | \
                             (get_byte({alias}.v, 3)::BIGINT << 24) | \
                             (get_byte({alias}.v, 4)::BIGINT << 32) | \
                             (get_byte({alias}.v, 5)::BIGINT << 40) | \
                             (get_byte({alias}.v, 6)::BIGINT << 48) | \
                             (get_byte({alias}.v, 7)::BIGINT << 56))::TEXT \
                 WHEN 7 THEN convert_from({alias}.v, 'UTF8') \
                 WHEN 8 THEN convert_from({alias}.v, 'UTF8') \
                 ELSE NULL::TEXT \
                 END",
                alias = alias
            );
            select_exprs.push(decode_expr);
        }
    }

    // Construct final SQL
    let sql = format!(
        "SELECT {} FROM {} WHERE {}",
        select_exprs.join(", "),
        joins.join(", "),
        where_clauses.join(" AND ")
    );

    Ok(sql)
}
