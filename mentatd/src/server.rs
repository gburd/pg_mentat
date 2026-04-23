use crate::cache::QueryCache;
use crate::config::Config;
use crate::metrics;
use crate::pool::DbPool;
use crate::protocol::{
    parser::{parse_request, ParseError},
    serializer::serialize_response,
    transit_serializer::{
        content_type_for_encoding, parse_accept_encoding, serialize_transit_json,
        serialize_transit_msgpack, TransitEncoding,
    },
    Anomaly, AnomalyCategory, Operation, Response, ResponseValue,
};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, error, info};
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
    query_cache: Arc<QueryCache>,
}

impl AppState {
    pub fn new(pool: DbPool, config: Config) -> Self {
        let cache_capacity = if config.cache.enabled {
            config.cache.capacity
        } else {
            0
        };
        let cache_ttl = Duration::from_secs(config.cache.ttl_secs);
        Self {
            pool,
            config: Arc::new(config),
            query_cache: Arc::new(QueryCache::new(cache_capacity, cache_ttl)),
        }
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", post(handle_request))
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_endpoint))
        .with_state(state)
}

async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "mentatd ready")
}

async fn metrics_endpoint(State(state): State<AppState>) -> impl IntoResponse {
    // Update pool gauge before rendering
    let pool_status = state.pool.status();
    metrics::CONNECTION_POOL_SIZE.set(f64::from(pool_status.size as u32));

    let body = metrics::render_metrics();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}

async fn handle_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<AxumResponse, StatusCode> {
    info!("Received request: {}", body);
    metrics::REQUEST_COUNT.inc();

    let response = match parse_request(&body) {
        Ok(request) => match execute_operation(request.op, &state).await {
            Ok(result) => Response::Success { result },
            Err(e) => {
                error!("Operation failed: {}", e);
                metrics::ERROR_COUNT.inc();
                Response::Error { anomaly: e.into() }
            }
        },
        Err(e) => {
            error!("Parse failed: {}", e);
            metrics::ERROR_COUNT.inc();
            Response::Error { anomaly: e.into() }
        }
    };

    // Content-type negotiation: check Accept header for Transit formats
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/edn");

    if let Some(encoding) = parse_accept_encoding(accept) {
        let content_type = content_type_for_encoding(encoding);
        match encoding {
            TransitEncoding::Json => {
                let transit_response = serialize_transit_json(&response);
                info!("Sending Transit+JSON response");
                Ok((
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, content_type)],
                    transit_response,
                )
                    .into_response())
            }
            TransitEncoding::Msgpack => {
                let transit_response = serialize_transit_msgpack(&response);
                info!("Sending Transit+MessagePack response ({} bytes)", transit_response.len());
                Ok((
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, content_type)],
                    transit_response,
                )
                    .into_response())
            }
        }
    } else {
        // Default: EDN format
        let edn_response = serialize_response(&response);
        info!("Sending EDN response: {}", edn_response);
        Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/edn")],
            edn_response,
        )
            .into_response())
    }
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

            let databases: Vec<ResponseValue> = rows
                .iter()
                .map(|row| ResponseValue::String(row.get::<_, String>(0)))
                .collect();

            Ok(ResponseValue::Vector(databases))
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

            Ok(ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("connection-id".to_string()),
                    ResponseValue::String(connection_id),
                ),
                (
                    ResponseValue::Keyword("db-name".to_string()),
                    ResponseValue::String(db_name),
                ),
                (
                    ResponseValue::Keyword("status".to_string()),
                    ResponseValue::String("connected".to_string()),
                ),
            ]))
        }

        Operation::Db { connection_id } => Ok(ResponseValue::Map(vec![
            (
                ResponseValue::Keyword("connection-id".to_string()),
                ResponseValue::String(connection_id.to_string()),
            ),
            (
                ResponseValue::Keyword("status".to_string()),
                ResponseValue::String("active".to_string()),
            ),
        ])),

        Operation::Query { query, args, .. } => {
            info!("Executing query: {} with args: {:?}", query, args);
            metrics::QUERY_COUNT.inc();
            let query_start = Instant::now();

            // Convert args to a stable JSON string for cache key
            let args_json_str = serde_json::to_string(&args)
                .map_err(|e| ServerError::Internal(format!("Failed to serialize args: {}", e)))?;

            // Check cache first -- we cache the raw JSON from PostgreSQL
            if let Some(cached_json) = state.query_cache.get(&query, &args_json_str) {
                debug!("Cache hit for query: {}", query);
                metrics::CACHE_HITS.inc();
                let elapsed = query_start.elapsed().as_secs_f64();
                metrics::QUERY_DURATION.observe(elapsed);
                let result_json: serde_json::Value = serde_json::from_str(&cached_json)
                    .map_err(|e| {
                        ServerError::Internal(format!("Failed to parse cached result: {}", e))
                    })?;
                let result = parse_query_results(&result_json)?;
                return Ok(ResponseValue::Vector(result));
            }

            debug!("Cache miss for query: {}", query);
            metrics::CACHE_MISSES.inc();

            let client = state.pool.get().await?;

            // Convert args Vec<String> to a JSON value for the JSONB parameter
            let args_json = serde_json::to_value(&args)
                .map_err(|e| ServerError::Internal(format!("Failed to serialize args: {}", e)))?;

            let row = client
                .query_one("SELECT mentat_query($1, $2::jsonb)", &[&query, &args_json])
                .await?;

            let result_json: serde_json::Value = row.get(0);

            // Cache the raw JSON result from PostgreSQL
            let json_str = result_json.to_string();
            state.query_cache.insert(&query, &args_json_str, json_str);

            let result = parse_query_results(&result_json)?;

            let elapsed = query_start.elapsed().as_secs_f64();
            metrics::QUERY_DURATION.observe(elapsed);

            Ok(ResponseValue::Vector(result))
        }

        Operation::Transact {
            connection_id,
            tx_data,
        } => {
            info!(
                "Executing transaction: {} with data: {}",
                connection_id, tx_data
            );
            metrics::TRANSACTION_COUNT.inc();

            let client = state.pool.get().await?;

            let row = client
                .query_one("SELECT mentat_transact($1)", &[&tx_data])
                .await?;

            let report_str: String = row.get(0);
            let result = parse_tx_report(&report_str)?;

            // Invalidate query cache after successful transaction since
            // data has changed and cached query results may be stale
            state.query_cache.invalidate();
            debug!("Query cache invalidated after transaction");

            Ok(result)
        }
    }
}

/// Convert a JSON value from `mentat_query()` to a native `ResponseValue`.
fn json_to_response_value(val: &serde_json::Value) -> ResponseValue {
    match val {
        serde_json::Value::String(s) => {
            if let Some(stripped) = s.strip_prefix(':') {
                ResponseValue::Keyword(stripped.to_string())
            } else {
                ResponseValue::String(s.clone())
            }
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                ResponseValue::Integer(i)
            } else if let Some(u) = n.as_u64() {
                // Fallback: treat as i64 if it fits, otherwise as string
                #[allow(clippy::cast_possible_wrap)]
                if u <= i64::MAX as u64 {
                    ResponseValue::Integer(u as i64)
                } else {
                    ResponseValue::String(u.to_string())
                }
            } else {
                // Floating point -- represent as string since EDN floats need special handling
                ResponseValue::String(n.to_string())
            }
        }
        serde_json::Value::Bool(b) => ResponseValue::Boolean(*b),
        serde_json::Value::Null => ResponseValue::Nil,
        serde_json::Value::Array(arr) => {
            let items = arr.iter().map(json_to_response_value).collect();
            ResponseValue::Vector(items)
        }
        serde_json::Value::Object(obj) => {
            let entries = obj
                .iter()
                .map(|(k, v)| {
                    (
                        ResponseValue::Keyword(k.clone()),
                        json_to_response_value(v),
                    )
                })
                .collect();
            ResponseValue::Map(entries)
        }
    }
}

/// Parse the JSONB query result from `mentat_query()` into a vector of native EDN vectors.
///
/// The extension returns JSON like:
/// ```json
/// {"columns": ["?name", "?age"], "results": [["Alice", 30], ["Bob", 25]]}
/// ```
///
/// This converts each result row to a native `ResponseValue::Vector`, e.g. `[10001 "Alice"]`.
fn parse_query_results(json: &serde_json::Value) -> Result<Vec<ResponseValue>, ServerError> {
    let results = json
        .get("results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| ServerError::Internal("Missing 'results' in query response".to_string()))?;

    let mut edn_rows = Vec::with_capacity(results.len());
    for row in results {
        let row_arr = row
            .as_array()
            .ok_or_else(|| ServerError::Internal("Expected array for result row".to_string()))?;

        let row_values: Vec<ResponseValue> = row_arr.iter().map(json_to_response_value).collect();
        edn_rows.push(ResponseValue::Vector(row_values));
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
/// This converts it to a `ResponseValue::Map` with native EDN types.
fn parse_tx_report(report_str: &str) -> Result<ResponseValue, ServerError> {
    let report: serde_json::Value = serde_json::from_str(report_str)
        .map_err(|e| ServerError::Internal(format!("Failed to parse tx report: {}", e)))?;

    let mut entries = Vec::new();

    if let Some(tx_id) = report.get("tx-id") {
        entries.push((
            ResponseValue::Keyword("tx-id".to_string()),
            json_to_response_value(tx_id),
        ));
    }

    if let Some(tx_instant) = report.get("tx-instant") {
        entries.push((
            ResponseValue::Keyword("tx-instant".to_string()),
            json_to_response_value(tx_instant),
        ));
    }

    if let Some(tempids) = report.get("tempids") {
        entries.push((
            ResponseValue::Keyword("tempids".to_string()),
            json_to_response_value(tempids),
        ));
    }

    if let Some(datoms) = report.get("datoms-inserted") {
        entries.push((
            ResponseValue::Keyword("datoms-inserted".to_string()),
            json_to_response_value(datoms),
        ));
    }

    entries.push((
        ResponseValue::Keyword("status".to_string()),
        ResponseValue::String("committed".to_string()),
    ));

    Ok(ResponseValue::Map(entries))
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

        // Verify first row is a Vector with native types
        match &rows[0] {
            ResponseValue::Vector(vals) => {
                assert_eq!(vals.len(), 2);
                match &vals[0] {
                    ResponseValue::String(s) => assert_eq!(s, "Alice"),
                    other => panic!("Expected String, got {:?}", other),
                }
                match &vals[1] {
                    ResponseValue::Integer(i) => assert_eq!(*i, 30),
                    other => panic!("Expected Integer, got {:?}", other),
                }
            }
            other => panic!("Expected Vector, got {:?}", other),
        }

        // Verify second row
        match &rows[1] {
            ResponseValue::Vector(vals) => {
                assert_eq!(vals.len(), 2);
                match &vals[0] {
                    ResponseValue::String(s) => assert_eq!(s, "Bob"),
                    other => panic!("Expected String, got {:?}", other),
                }
                match &vals[1] {
                    ResponseValue::Integer(i) => assert_eq!(*i, 25),
                    other => panic!("Expected Integer, got {:?}", other),
                }
            }
            other => panic!("Expected Vector, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_query_results_serialized_format() {
        use crate::protocol::serializer::serialize_response;
        use crate::protocol::Response;

        let json = serde_json::json!({
            "columns": ["?name", "?age"],
            "results": [
                ["Alice", 30],
                ["Bob", 25]
            ]
        });
        let rows = parse_query_results(&json).unwrap_or_default();
        let response = Response::Success {
            result: ResponseValue::Vector(rows),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result [["Alice" 30] ["Bob" 25]]}"#);
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

        match &rows[0] {
            ResponseValue::Vector(vals) => match &vals[0] {
                ResponseValue::String(s) => assert_eq!(s, "hello"),
                other => panic!("Expected String, got {:?}", other),
            },
            other => panic!("Expected Vector, got {:?}", other),
        }
        match &rows[1] {
            ResponseValue::Vector(vals) => match &vals[0] {
                ResponseValue::Integer(i) => assert_eq!(*i, 42),
                other => panic!("Expected Integer, got {:?}", other),
            },
            other => panic!("Expected Vector, got {:?}", other),
        }
        match &rows[2] {
            ResponseValue::Vector(vals) => match &vals[0] {
                ResponseValue::Boolean(b) => assert!(*b),
                other => panic!("Expected Boolean, got {:?}", other),
            },
            other => panic!("Expected Vector, got {:?}", other),
        }
        match &rows[3] {
            ResponseValue::Vector(vals) => match &vals[0] {
                ResponseValue::Nil => {}
                other => panic!("Expected Nil, got {:?}", other),
            },
            other => panic!("Expected Vector, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_query_results_mixed_types_serialized() {
        use crate::protocol::serializer::serialize_response;
        use crate::protocol::Response;

        let json = serde_json::json!({
            "columns": ["?v"],
            "results": [
                ["hello"],
                [42],
                [true],
                [null]
            ]
        });
        let rows = parse_query_results(&json).unwrap_or_default();
        let response = Response::Success {
            result: ResponseValue::Vector(rows),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result [["hello"] [42] [true] [nil]]}"#);
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

        // Verify the map structure
        match result.unwrap_or(ResponseValue::Nil) {
            ResponseValue::Map(entries) => {
                // tx-id should be an integer
                let tx_id_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tx-id"));
                assert!(tx_id_entry.is_some());
                match &tx_id_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Integer(i) => assert_eq!(*i, 12345),
                    other => panic!("Expected Integer for tx-id, got {:?}", other),
                }

                // tx-instant should be nil
                let tx_instant_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tx-instant"));
                assert!(tx_instant_entry.is_some());
                match &tx_instant_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Nil => {}
                    other => panic!("Expected Nil for tx-instant, got {:?}", other),
                }

                // datoms-inserted should be an integer
                let datoms_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "datoms-inserted"));
                assert!(datoms_entry.is_some());
                match &datoms_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Integer(i) => assert_eq!(*i, 3),
                    other => panic!("Expected Integer for datoms-inserted, got {:?}", other),
                }

                // status should be a string
                let status_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "status"));
                assert!(status_entry.is_some());
                match &status_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::String(s) => assert_eq!(s, "committed"),
                    other => panic!("Expected String for status, got {:?}", other),
                }

                // tempids should be a map
                let tempids_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tempids"));
                assert!(tempids_entry.is_some());
                match &tempids_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Map(inner) => {
                        assert_eq!(inner.len(), 1);
                    }
                    other => panic!("Expected Map for tempids, got {:?}", other),
                }
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tx_report_serialized_format() {
        use crate::protocol::serializer::serialize_response;
        use crate::protocol::Response;

        let report_str =
            r#"{"tx-id":12345,"tx-instant":null,"tempids":{},"datoms-inserted":3}"#;
        let result = parse_tx_report(report_str).unwrap_or(ResponseValue::Nil);
        let response = Response::Success { result };
        let output = serialize_response(&response);
        assert!(output.contains(":tx-id 12345"));
        assert!(output.contains(":tx-instant nil"));
        assert!(output.contains(":datoms-inserted 3"));
        assert!(output.contains(":tempids {}"));
        assert!(output.contains(r#":status "committed""#));
    }

    #[test]
    fn test_parse_tx_report_empty_tempids() {
        let report_str = r#"{"tx-id":1,"tx-instant":null,"tempids":{},"datoms-inserted":0}"#;
        let result = parse_tx_report(report_str);
        assert!(result.is_ok());
        match result.unwrap_or(ResponseValue::Nil) {
            ResponseValue::Map(entries) => {
                let tempids_entry = entries.iter().find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "tempids"));
                assert!(tempids_entry.is_some());
                match &tempids_entry.unwrap_or(&(ResponseValue::Nil, ResponseValue::Nil)).1 {
                    ResponseValue::Map(inner) => assert!(inner.is_empty()),
                    other => panic!("Expected empty Map for tempids, got {:?}", other),
                }
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tx_report_invalid_json() {
        let result = parse_tx_report("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_json_to_response_value_keyword_detection() {
        let val = serde_json::json!(":db/name");
        match json_to_response_value(&val) {
            ResponseValue::Keyword(k) => assert_eq!(k, "db/name"),
            other => panic!("Expected Keyword, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_string() {
        let val = serde_json::json!("hello");
        match json_to_response_value(&val) {
            ResponseValue::String(s) => assert_eq!(s, "hello"),
            other => panic!("Expected String, got {:?}", other),
        }
    }

    #[test]
    fn test_json_to_response_value_nested_array() {
        let val = serde_json::json!([1, "two", [3]]);
        match json_to_response_value(&val) {
            ResponseValue::Vector(items) => {
                assert_eq!(items.len(), 3);
                match &items[2] {
                    ResponseValue::Vector(inner) => {
                        assert_eq!(inner.len(), 1);
                        match &inner[0] {
                            ResponseValue::Integer(i) => assert_eq!(*i, 3),
                            other => panic!("Expected Integer, got {:?}", other),
                        }
                    }
                    other => panic!("Expected Vector, got {:?}", other),
                }
            }
            other => panic!("Expected Vector, got {:?}", other),
        }
    }
}
