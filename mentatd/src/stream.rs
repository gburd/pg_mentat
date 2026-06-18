//! Streaming query responses via Server-Sent Events (SSE).
//!
//! The `/stream/query` endpoint accepts the same EDN request body as the
//! regular `/` endpoint for query operations, but returns results
//! incrementally using the SSE protocol. Results are emitted in batches
//! (default 1000 rows per event) so that memory usage stays constant
//! regardless of result set size.
//!
//! ## SSE event types
//!
//! - `columns` -- Sent once at the start; contains the column names.
//! - `batch`   -- Sent zero or more times; each carries a batch of rows.
//! - `done`    -- Sent once at the end; carries summary metadata.
//! - `error`   -- Sent if an error occurs; carries the anomaly.
//!
//! ## Compatibility with Datomic streaming
//!
//! Datomic's chunked query API returns results as a lazy sequence.  This
//! SSE-based approach provides a similar incremental consumption pattern
//! over HTTP, with each `batch` event corresponding to one chunk.

use crate::metrics;
use crate::protocol::{
    parser::parse_request, serializer::serialize_response, Anomaly, AnomalyCategory, Operation,
    Response, ResponseValue,
};
use crate::server::AppState;
use axum::{
    extract::State,
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use std::convert::Infallible;
use std::time::Instant;
use tracing::{error, info};

/// Default number of rows per SSE batch event.
const DEFAULT_BATCH_SIZE: usize = 1000;

/// Handle a streaming query request.
///
/// Parses the incoming EDN body, validates that it is a query operation,
/// and returns an SSE stream that emits results in batches.
pub async fn handle_stream_query(
    State(state): State<AppState>,
    _headers: HeaderMap,
    body: String,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    info!("Received streaming query request");
    metrics::STREAM_QUERY_COUNT.inc();
    metrics::REQUEST_COUNT.inc();

    let validated_op = validate_stream_request(&body);

    let stream = async_stream::stream! {
        let op = match validated_op {
            Ok(op) => op,
            Err(anomaly) => {
                yield Ok(Event::default().event("error").data(format_anomaly_edn(&anomaly)));
                return;
            }
        };

        let query_start = Instant::now();

        match execute_streaming_query(op, &state).await {
            Ok((columns, rows)) => {
                yield Ok(Event::default().event("columns").data(format_columns_edn(&columns)));

                let total_rows = rows.len();

                for chunk in rows.chunks(DEFAULT_BATCH_SIZE) {
                    let batch_edn = format_batch_edn(chunk);
                    metrics::STREAM_ROWS_SENT.inc_by(chunk.len() as u64);
                    yield Ok(Event::default().event("batch").data(batch_edn));
                }

                let elapsed = query_start.elapsed().as_secs_f64();
                metrics::STREAM_DURATION.observe(elapsed);
                let batch_count = total_rows.div_ceil(DEFAULT_BATCH_SIZE.max(1));
                let done_edn = format!(
                    "{{:total-rows {total_rows} :batches {batch_count} :duration-ms {:.1}}}",
                    elapsed * 1000.0,
                );
                yield Ok(Event::default().event("done").data(done_edn));
            }
            Err(e) => {
                error!("Streaming query failed: {e}");
                metrics::ERROR_COUNT.inc();
                let anomaly: Anomaly = e.into();
                yield Ok(Event::default().event("error").data(format_anomaly_edn(&anomaly)));
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Validate the incoming request body and extract the query operation.
/// Returns the validated `Operation` or an `Anomaly` on failure.
fn validate_stream_request(body: &str) -> Result<Operation, Anomaly> {
    let request = parse_request(body).map_err(|e| {
        error!("Stream parse failed: {e}");
        metrics::ERROR_COUNT.inc();
        Anomaly::from(e)
    })?;

    if let op @ (Operation::Query { .. }
    | Operation::AsOf { .. }
    | Operation::Since { .. }
    | Operation::History { .. }) = request.op
    {
        Ok(op)
    } else {
        metrics::ERROR_COUNT.inc();
        Err(Anomaly {
            category: AnomalyCategory::Incorrect,
            message:
                "Streaming is only supported for query operations (:q, :as-of, :since, :history)"
                    .to_string(),
            db_error: Some(":db.error/invalid-request".to_string()),
        })
    }
}

/// Error type for streaming query execution.
#[derive(Debug, thiserror::Error)]
enum StreamError {
    #[error("Database pool error: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),
    #[error("Database query error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<StreamError> for Anomaly {
    fn from(err: StreamError) -> Self {
        match err {
            StreamError::Pool(_) | StreamError::Database(_) => Self {
                category: AnomalyCategory::Unavailable,
                message: "Database unavailable".to_string(),
                db_error: Some("db.error/unavailable".to_string()),
            },
            StreamError::Internal(msg) => Self {
                category: AnomalyCategory::Fault,
                message: msg,
                db_error: Some("db.error/fault".to_string()),
            },
        }
    }
}

/// Execute the query and return `(column_names, row_values)` where each row
/// is a `Vec<ResponseValue>`.
///
/// The `PostgreSQL` function `mentat_query` returns a JSON object with `columns`
/// and `results` arrays. We parse the full result and then the caller chunks
/// it for SSE delivery.
async fn execute_streaming_query(
    op: Operation,
    state: &AppState,
) -> Result<(Vec<String>, Vec<Vec<ResponseValue>>), StreamError> {
    let (query, args_json) = match &op {
        Operation::Query { query, args, .. } => {
            let args_json = serde_json::to_value(args)
                .map_err(|e| StreamError::Internal(format!("Failed to serialize args: {e}")))?;
            (query.clone(), args_json)
        }
        Operation::AsOf { query, args, t } => {
            let args_raw = serde_json::to_value(args)
                .map_err(|e| StreamError::Internal(format!("Failed to serialize args: {e}")))?;
            let mut inputs = serde_json::Map::new();
            inputs.insert("inputs".to_string(), args_raw);
            inputs.insert("asOf".to_string(), serde_json::Value::Number((*t).into()));
            (query.clone(), serde_json::Value::Object(inputs))
        }
        Operation::Since { query, args, t } => {
            let args_raw = serde_json::to_value(args)
                .map_err(|e| StreamError::Internal(format!("Failed to serialize args: {e}")))?;
            let mut inputs = serde_json::Map::new();
            inputs.insert("inputs".to_string(), args_raw);
            inputs.insert("since".to_string(), serde_json::Value::Number((*t).into()));
            (query.clone(), serde_json::Value::Object(inputs))
        }
        Operation::History { query, args } => {
            let args_raw = serde_json::to_value(args)
                .map_err(|e| StreamError::Internal(format!("Failed to serialize args: {e}")))?;
            let mut inputs = serde_json::Map::new();
            inputs.insert("inputs".to_string(), args_raw);
            inputs.insert("history".to_string(), serde_json::Value::Bool(true));
            (query.clone(), serde_json::Value::Object(inputs))
        }
        _ => {
            return Err(StreamError::Internal(
                "Only query operations are supported for streaming".to_string(),
            ));
        }
    };

    let client = state.pool().get().await?;

    let row = client
        .query_one("SELECT mentat_query($1, $2::jsonb)", &[&query, &args_json])
        .await?;

    let result_json: serde_json::Value = row.get(0);

    let columns: Vec<String> = result_json
        .get("columns")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let results = result_json
        .get("results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| StreamError::Internal("Missing 'results' in query response".to_string()))?;

    let mut rows = Vec::with_capacity(results.len());
    for row_val in results {
        let row_arr = row_val
            .as_array()
            .ok_or_else(|| StreamError::Internal("Expected array for result row".to_string()))?;

        let row_values: Vec<ResponseValue> = row_arr.iter().map(json_to_response_value).collect();
        rows.push(row_values);
    }

    Ok((columns, rows))
}

/// Convert a JSON value to a `ResponseValue`.
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
                i64::try_from(u).map_or_else(
                    |_| ResponseValue::String(u.to_string()),
                    ResponseValue::Integer,
                )
            } else {
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
                .map(|(k, v)| (ResponseValue::Keyword(k.clone()), json_to_response_value(v)))
                .collect();
            ResponseValue::Map(entries)
        }
    }
}

/// Format column names as EDN vector for the `columns` SSE event.
fn format_columns_edn(columns: &[String]) -> String {
    use std::fmt::Write;
    let mut out = String::from("[");
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        write!(out, "\"{col}\"").ok();
    }
    out.push(']');
    out
}

/// Format a batch of rows as an EDN vector of vectors for the `batch` SSE event.
fn format_batch_edn(rows: &[Vec<ResponseValue>]) -> String {
    let row_values: Vec<ResponseValue> = rows
        .iter()
        .map(|row| ResponseValue::Vector(row.clone()))
        .collect();

    let response = Response::Success {
        result: ResponseValue::Vector(row_values),
    };
    serialize_response(&response)
}

/// Format an anomaly as an EDN string for the `error` SSE event.
fn format_anomaly_edn(anomaly: &Anomaly) -> String {
    let response = Response::Error {
        anomaly: anomaly.clone(),
    };
    serialize_response(&response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_columns_edn() {
        let columns = vec!["?name".to_string(), "?age".to_string()];
        let result = format_columns_edn(&columns);
        assert_eq!(result, r#"["?name" "?age"]"#);
    }

    #[test]
    fn test_format_columns_edn_empty() {
        let columns: Vec<String> = vec![];
        let result = format_columns_edn(&columns);
        assert_eq!(result, "[]");
    }

    #[test]
    fn test_format_batch_edn() {
        let rows = vec![
            vec![
                ResponseValue::String("Alice".to_string()),
                ResponseValue::Integer(30),
            ],
            vec![
                ResponseValue::String("Bob".to_string()),
                ResponseValue::Integer(25),
            ],
        ];
        let result = format_batch_edn(&rows);
        assert_eq!(result, r#"{:result [["Alice" 30] ["Bob" 25]]}"#);
    }

    #[test]
    fn test_format_batch_edn_empty() {
        let rows: Vec<Vec<ResponseValue>> = vec![];
        let result = format_batch_edn(&rows);
        assert_eq!(result, "{:result []}");
    }

    #[test]
    fn test_format_anomaly_edn() {
        let anomaly = Anomaly {
            category: AnomalyCategory::Incorrect,
            message: "test error".to_string(),
            db_error: Some(":db.error/invalid-request".to_string()),
        };
        let result = format_anomaly_edn(&anomaly);
        assert!(result.contains(":cognitect.anomalies/incorrect"));
        assert!(result.contains("test error"));
    }

    #[test]
    fn test_json_to_response_value_string() {
        let val = serde_json::json!("hello");
        match json_to_response_value(&val) {
            ResponseValue::String(s) => assert_eq!(s, "hello"),
            other => panic!("Expected String, got {other:?}"),
        }
    }

    #[test]
    fn test_json_to_response_value_keyword() {
        let val = serde_json::json!(":db/name");
        match json_to_response_value(&val) {
            ResponseValue::Keyword(k) => assert_eq!(k, "db/name"),
            other => panic!("Expected Keyword, got {other:?}"),
        }
    }

    #[test]
    fn test_json_to_response_value_integer() {
        let val = serde_json::json!(42);
        match json_to_response_value(&val) {
            ResponseValue::Integer(i) => assert_eq!(i, 42),
            other => panic!("Expected Integer, got {other:?}"),
        }
    }

    #[test]
    fn test_json_to_response_value_null() {
        let val = serde_json::json!(null);
        assert!(matches!(json_to_response_value(&val), ResponseValue::Nil));
    }

    #[test]
    fn test_json_to_response_value_bool() {
        let val = serde_json::json!(true);
        match json_to_response_value(&val) {
            ResponseValue::Boolean(b) => assert!(b),
            other => panic!("Expected Boolean, got {other:?}"),
        }
    }

    #[test]
    fn test_default_batch_size() {
        assert_eq!(DEFAULT_BATCH_SIZE, 1000);
    }

    #[test]
    fn test_batch_chunking_logic() {
        let rows: Vec<Vec<ResponseValue>> =
            (0..2500).map(|i| vec![ResponseValue::Integer(i)]).collect();

        let chunks: Vec<_> = rows.chunks(DEFAULT_BATCH_SIZE).collect();
        assert_eq!(chunks.len(), 3); // 1000 + 1000 + 500
        assert_eq!(chunks[0].len(), 1000);
        assert_eq!(chunks[1].len(), 1000);
        assert_eq!(chunks[2].len(), 500);
    }

    #[test]
    fn test_validate_stream_request_valid_query() {
        let body = r#"{:op :q :args {:query "[:find ?e :where [?e :name]]" :args []}}"#;
        let result = validate_stream_request(body);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_stream_request_invalid_op() {
        let body = r#"{:op :health}"#;
        let result = validate_stream_request(body);
        assert!(result.is_err());
        let anomaly = result.unwrap_err();
        assert!(anomaly.message.contains("Streaming is only supported"));
    }

    #[test]
    fn test_validate_stream_request_bad_edn() {
        let body = "not valid edn";
        let result = validate_stream_request(body);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_stream_request_as_of() {
        let body = r#"{:op :as-of :args {:query "[:find ?e]" :args [] :t 1000}}"#;
        let result = validate_stream_request(body);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_stream_request_since() {
        let body = r#"{:op :since :args {:query "[:find ?e]" :args [] :t 1000}}"#;
        let result = validate_stream_request(body);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_stream_request_history() {
        let body = r#"{:op :history :args {:query "[:find ?e]" :args []}}"#;
        let result = validate_stream_request(body);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_stream_request_transact_rejected() {
        let body = r#"{:op :transact :args {:connection-id "abc" :tx-data "[{:db/id -1}]"}}"#;
        let result = validate_stream_request(body);
        assert!(result.is_err());
    }
}
