use crate::config::Config;
use crate::pool::{DbPool, PoolError};
use crate::protocol::{
    parser::{parse_request, ParseError},
    serializer::serialize_response,
    Anomaly, AnomalyCategory, Operation, Response, ResponseValue,
};
use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
    Router,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("Database pool error: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),
    #[error("Database query error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<ServerError> for Anomaly {
    fn from(err: ServerError) -> Self {
        match err {
            ServerError::Parse(e) => e.into(),
            ServerError::Pool(_) | ServerError::Database(_) => Self {
                category: AnomalyCategory::Unavailable,
                message: "Database unavailable".to_string(),
                db_error: Some("db.error/unavailable".to_string()),
            },
            ServerError::Internal(msg) => Self {
                category: AnomalyCategory::Fault,
                message: msg,
                db_error: Some("db.error/fault".to_string()),
            },
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pool: DbPool,
    config: Arc<Config>,
}

impl AppState {
    pub fn new(pool: DbPool, config: Config) -> Self {
        Self {
            pool,
            config: Arc::new(config),
        }
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", post(handle_request))
        .route("/health", get(health_check))
        .with_state(state)
}

async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "mentatd ready")
}

async fn handle_request(
    State(state): State<AppState>,
    body: String,
) -> Result<AxumResponse, StatusCode> {
    info!("Received request: {}", body);

    let response = match parse_request(&body) {
        Ok(request) => match execute_operation(request.op, &state).await {
            Ok(result) => Response::Success { result },
            Err(e) => {
                error!("Operation failed: {}", e);
                Response::Error { anomaly: e.into() }
            }
        },
        Err(e) => {
            error!("Parse failed: {}", e);
            Response::Error { anomaly: e.into() }
        }
    };

    let edn_response = serialize_response(&response);
    info!("Sending response: {}", edn_response);

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/edn")],
        edn_response,
    )
        .into_response())
}

async fn execute_operation(op: Operation, state: &AppState) -> Result<ResponseValue, ServerError> {
    match op {
        Operation::Health => Ok(ResponseValue::String("healthy".to_string())),

        Operation::ListDatabases => {
            let client = state.pool.get().await?;

            let rows = client
                .query(
                    "SELECT datname FROM pg_database WHERE datistemplate = false",
                    &[],
                )
                .await?;

            let databases: Vec<String> = rows.iter().map(|row| row.get::<_, String>(0)).collect();

            Ok(ResponseValue::List(databases))
        }

        Operation::CreateDatabase { db_name } => {
            if !is_valid_db_name(&db_name) {
                return Err(ServerError::Internal("Invalid database name".to_string()));
            }

            let client = state.pool.get().await?;

            client
                .execute(&format!("CREATE DATABASE {}", db_name), &[])
                .await?;

            Ok(ResponseValue::Boolean(true))
        }

        Operation::DeleteDatabase { db_name } => {
            if !is_valid_db_name(&db_name) {
                return Err(ServerError::Internal("Invalid database name".to_string()));
            }

            let client = state.pool.get().await?;

            client
                .execute(&format!("DROP DATABASE {}", db_name), &[])
                .await?;

            Ok(ResponseValue::Boolean(true))
        }

        Operation::Connect { db_name } => {
            let client = state.pool.get().await?;

            let exists = client
                .query_one(
                    "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)",
                    &[&db_name],
                )
                .await?
                .get::<_, bool>(0);

            if !exists {
                return Err(ServerError::Internal(format!(
                    "Database '{}' not found",
                    db_name
                )));
            }

            let connection_id = Uuid::new_v4().to_string();

            let mut result = BTreeMap::new();
            result.insert("connection-id".to_string(), connection_id);
            result.insert("db-name".to_string(), db_name);
            result.insert("status".to_string(), "connected".to_string());

            Ok(ResponseValue::Map(result))
        }

        Operation::Db { connection_id } => {
            let mut result = BTreeMap::new();
            result.insert("connection-id".to_string(), connection_id.to_string());
            result.insert("status".to_string(), "active".to_string());

            Ok(ResponseValue::Map(result))
        }

        Operation::Query { query, args, .. } => {
            info!("Executing query: {} with args: {:?}", query, args);

            let client = state.pool.get().await?;

            // Convert args Vec<String> to a JSON array for the JSONB parameter
            let args_json = serde_json::to_value(&args)
                .map_err(|e| ServerError::Internal(format!("Failed to serialize args: {}", e)))?;

            let row = client
                .query_one("SELECT mentat_query($1, $2::jsonb)", &[&query, &args_json])
                .await?;

            let result_json: serde_json::Value = row.get(0);
            let result = parse_query_results(&result_json)?;

            Ok(ResponseValue::List(result))
        }

        Operation::Transact {
            connection_id,
            tx_data,
        } => {
            info!(
                "Executing transaction: {} with data: {}",
                connection_id, tx_data
            );

            let client = state.pool.get().await?;

            let row = client
                .query_one("SELECT mentat_transact($1)", &[&tx_data])
                .await?;

            let report_str: String = row.get(0);
            let result = parse_tx_report(&report_str)?;

            Ok(ResponseValue::Map(result))
        }
    }
}

/// Parse the JSONB query result from `mentat_query()` into a list of EDN-formatted strings.
///
/// The extension returns JSON like:
/// ```json
/// {"columns": ["?name", "?age"], "results": [["Alice", 30], ["Bob", 25]]}
/// ```
///
/// This converts each result row to an EDN vector string, e.g. `["Alice" 30]`.
fn parse_query_results(json: &serde_json::Value) -> Result<Vec<String>, ServerError> {
    let results = json
        .get("results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| ServerError::Internal("Missing 'results' in query response".to_string()))?;

    let mut edn_rows = Vec::with_capacity(results.len());
    for row in results {
        let row_arr = row
            .as_array()
            .ok_or_else(|| ServerError::Internal("Expected array for result row".to_string()))?;

        let mut edn_row = String::from("[");
        for (i, val) in row_arr.iter().enumerate() {
            if i > 0 {
                edn_row.push(' ');
            }
            match val {
                serde_json::Value::String(s) => {
                    edn_row.push('"');
                    edn_row.push_str(s);
                    edn_row.push('"');
                }
                serde_json::Value::Number(n) => {
                    edn_row.push_str(&n.to_string());
                }
                serde_json::Value::Bool(b) => {
                    edn_row.push_str(if *b { "true" } else { "false" });
                }
                serde_json::Value::Null => {
                    edn_row.push_str("nil");
                }
                other => {
                    edn_row.push_str(&other.to_string());
                }
            }
        }
        edn_row.push(']');
        edn_rows.push(edn_row);
    }

    Ok(edn_rows)
}

/// Parse the JSON string returned by `mentat_transact()` into a response map.
///
/// The extension returns a JSON string like:
/// ```json
/// {"tx-id": 12345, "tx-instant": null, "tempids": {"tempid1": 100}, "datoms-inserted": 3}
/// ```
///
/// This converts it to a `BTreeMap` suitable for `ResponseValue::Map`.
fn parse_tx_report(report_str: &str) -> Result<BTreeMap<String, String>, ServerError> {
    let report: serde_json::Value = serde_json::from_str(report_str)
        .map_err(|e| ServerError::Internal(format!("Failed to parse tx report: {}", e)))?;

    let mut result = BTreeMap::new();

    if let Some(tx_id) = report.get("tx-id") {
        result.insert("tx-id".to_string(), tx_id.to_string());
    }

    if let Some(tx_instant) = report.get("tx-instant") {
        if tx_instant.is_null() {
            result.insert("tx-instant".to_string(), "nil".to_string());
        } else {
            result.insert("tx-instant".to_string(), tx_instant.to_string());
        }
    }

    if let Some(tempids) = report.get("tempids") {
        // Serialize tempids as an EDN map string
        if let Some(obj) = tempids.as_object() {
            let mut tempid_edn = String::from("{");
            for (i, (k, v)) in obj.iter().enumerate() {
                if i > 0 {
                    tempid_edn.push_str(", ");
                }
                tempid_edn.push('"');
                tempid_edn.push_str(k);
                tempid_edn.push_str("\" ");
                tempid_edn.push_str(&v.to_string());
            }
            tempid_edn.push('}');
            result.insert("tempids".to_string(), tempid_edn);
        }
    }

    if let Some(datoms) = report.get("datoms-inserted") {
        result.insert("datoms-inserted".to_string(), datoms.to_string());
    }

    result.insert("status".to_string(), "committed".to_string());

    Ok(result)
}

fn is_valid_db_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name
            .chars()
            .next()
            .map_or(false, |c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_db_name() {
        assert!(is_valid_db_name("test_db"));
        assert!(is_valid_db_name("mydb123"));
        assert!(!is_valid_db_name("123db"));
        assert!(!is_valid_db_name("my-db"));
        assert!(!is_valid_db_name(""));
    }

    #[test]
    fn test_parse_query_results_basic() {
        let json = serde_json::json!({
            "columns": ["?name", "?age"],
            "results": [
                ["Alice", 30],
                ["Bob", 25]
            ]
        });
        let result = parse_query_results(&json);
        assert!(result.is_ok());
        let rows = result.unwrap_or_default();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], r#"["Alice" 30]"#);
        assert_eq!(rows[1], r#"["Bob" 25]"#);
    }

    #[test]
    fn test_parse_query_results_empty() {
        let json = serde_json::json!({
            "columns": ["?name"],
            "results": []
        });
        let result = parse_query_results(&json);
        assert!(result.is_ok());
        assert!(result.unwrap_or_default().is_empty());
    }

    #[test]
    fn test_parse_query_results_mixed_types() {
        let json = serde_json::json!({
            "columns": ["?v"],
            "results": [
                ["hello"],
                [42],
                [true],
                [null]
            ]
        });
        let result = parse_query_results(&json);
        assert!(result.is_ok());
        let rows = result.unwrap_or_default();
        assert_eq!(rows[0], r#"["hello"]"#);
        assert_eq!(rows[1], "[42]");
        assert_eq!(rows[2], "[true]");
        assert_eq!(rows[3], "[nil]");
    }

    #[test]
    fn test_parse_query_results_missing_results_field() {
        let json = serde_json::json!({"columns": ["?x"]});
        let result = parse_query_results(&json);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tx_report_basic() {
        let report_str =
            r#"{"tx-id":12345,"tx-instant":null,"tempids":{"tempid1":100},"datoms-inserted":3}"#;
        let result = parse_tx_report(report_str);
        assert!(result.is_ok());
        let map = result.unwrap_or_default();
        assert_eq!(map.get("tx-id").map(|s| s.as_str()), Some("12345"));
        assert_eq!(map.get("tx-instant").map(|s| s.as_str()), Some("nil"));
        assert_eq!(map.get("datoms-inserted").map(|s| s.as_str()), Some("3"));
        assert_eq!(map.get("status").map(|s| s.as_str()), Some("committed"));
        assert!(map.get("tempids").is_some());
        let tempids = map.get("tempids").cloned().unwrap_or_default();
        assert!(tempids.contains("tempid1"));
        assert!(tempids.contains("100"));
    }

    #[test]
    fn test_parse_tx_report_empty_tempids() {
        let report_str = r#"{"tx-id":1,"tx-instant":null,"tempids":{},"datoms-inserted":0}"#;
        let result = parse_tx_report(report_str);
        assert!(result.is_ok());
        let map = result.unwrap_or_default();
        assert_eq!(map.get("tempids").map(|s| s.as_str()), Some("{}"));
    }

    #[test]
    fn test_parse_tx_report_invalid_json() {
        let result = parse_tx_report("not json");
        assert!(result.is_err());
    }
}
