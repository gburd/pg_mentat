//! WebSocket handler for Datomic Client API protocol.
//!
//! Provides persistent WebSocket connections for the Datomic Client API,
//! allowing clients to send multiple operations over a single connection
//! without the overhead of HTTP request/response per operation.
//!
//! ## Protocol
//!
//! Messages are Transit+JSON or Transit+MessagePack encoded maps:
//!
//! Request:
//! ```edn
//! {:op :q
//!  :args {:query "[:find ?e :where [?e :name]]" :args []}
//!  :request-id "abc-123"}
//! ```
//!
//! Response:
//! ```edn
//! {:result [[42] [43]]
//!  :request-id "abc-123"}
//! ```
//!
//! The optional `:request-id` allows clients to correlate responses with
//! requests when sending multiple operations concurrently.
//!
//! ## Connection lifecycle
//!
//! 1. Client opens WebSocket to `/ws`
//! 2. Server creates a session and sends a welcome message
//! 3. Client sends operations as Transit messages
//! 4. Server processes each and sends back results
//! 5. Client closes connection or it times out

use crate::metrics;
use crate::protocol::transit_parser::{detect_input_format, parse_transit_json, InputFormat};
use crate::protocol::transit_serializer::serialize_transit_json;
use crate::protocol::{parser::parse_request, AnomalyCategory, Response, ResponseValue};
use crate::server::AppState;
use crate::session::SessionStore;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Maximum idle time for a WebSocket connection before it is closed (seconds).
const WS_IDLE_TIMEOUT_SECS: u64 = 300;

/// Maximum message size for WebSocket frames (16 MiB, matching HTTP body limit).
const _WS_MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Application state extended with session management for WebSocket connections.
#[derive(Clone)]
pub struct WsState {
    pub app_state: AppState,
    pub session_store: Arc<SessionStore>,
}

/// Create the WebSocket router with all WS endpoints.
pub fn create_ws_router(state: WsState) -> Router {
    Router::new()
        .route("/ws", get(ws_upgrade))
        .with_state(state)
}

/// Handle a WebSocket upgrade request.
///
/// This is the HTTP handler that upgrades an HTTP connection to a WebSocket.
/// After upgrade, the connection is managed by `handle_ws_connection`.
pub async fn ws_upgrade(ws: WebSocketUpgrade, State(state): State<WsState>) -> impl IntoResponse {
    info!("WebSocket upgrade request received");
    metrics::REQUEST_COUNT.inc();

    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

/// Handle a WebSocket connection after upgrade.
///
/// Creates a session, sends a welcome message, then enters the main
/// request-response loop until the client disconnects or the connection
/// times out.
async fn handle_ws_connection(mut socket: WebSocket, state: WsState) {
    let session_id = Uuid::new_v4();
    info!("WebSocket connection established: session={}", session_id);

    // Send welcome message with session info
    let welcome = Response::Success {
        result: ResponseValue::Map(vec![
            (
                ResponseValue::Keyword("type".to_string()),
                ResponseValue::Keyword("datomic.client/session".to_string()),
            ),
            (
                ResponseValue::Keyword("session-id".to_string()),
                ResponseValue::String(session_id.to_string()),
            ),
            (
                ResponseValue::Keyword("protocol-version".to_string()),
                ResponseValue::Integer(1),
            ),
        ]),
    };

    let welcome_json = serialize_transit_json(&welcome);
    if let Err(e) = socket.send(Message::Text(welcome_json.into())).await {
        error!("Failed to send welcome message: {}", e);
        return;
    }

    // Main message loop
    let idle_timeout = std::time::Duration::from_secs(WS_IDLE_TIMEOUT_SECS);

    loop {
        let msg = tokio::time::timeout(idle_timeout, socket.recv()).await;

        match msg {
            Err(_) => {
                // Idle timeout
                info!(
                    "WebSocket idle timeout ({}s): session={}",
                    WS_IDLE_TIMEOUT_SECS, session_id
                );
                let timeout_msg = serialize_transit_json(&Response::Error {
                    anomaly: crate::protocol::Anomaly {
                        category: AnomalyCategory::Interrupted,
                        message: "Connection idle timeout".to_string(),
                        db_error: Some("db.error/timeout".to_string()),
                    },
                });
                let _ = socket.send(Message::Text(timeout_msg.into())).await;
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
            Ok(None) => {
                // Client disconnected
                info!("WebSocket client disconnected: session={}", session_id);
                break;
            }
            Ok(Some(Err(e))) => {
                error!(
                    "WebSocket receive error: session={}, error={}",
                    session_id, e
                );
                break;
            }
            Ok(Some(Ok(msg))) => {
                match msg {
                    Message::Text(text) => {
                        debug!(
                            "WebSocket text message: session={}, len={}",
                            session_id,
                            text.len()
                        );
                        let response = process_ws_message(&text, &state).await;
                        if let Err(e) = socket.send(Message::Text(response.into())).await {
                            error!("WebSocket send error: session={}, error={}", session_id, e);
                            break;
                        }
                    }
                    Message::Binary(data) => {
                        debug!(
                            "WebSocket binary message: session={}, len={}",
                            session_id,
                            data.len()
                        );
                        // Binary messages are Transit+MessagePack
                        let response = process_ws_binary_message(&data, &state).await;
                        if let Err(e) = socket.send(Message::Text(response.into())).await {
                            error!("WebSocket send error: session={}, error={}", session_id, e);
                            break;
                        }
                    }
                    Message::Ping(data) => {
                        debug!("WebSocket ping: session={}", session_id);
                        if let Err(e) = socket.send(Message::Pong(data)).await {
                            error!(
                                "WebSocket pong send error: session={}, error={}",
                                session_id, e
                            );
                            break;
                        }
                    }
                    Message::Pong(_) => {
                        debug!("WebSocket pong received: session={}", session_id);
                    }
                    Message::Close(_) => {
                        info!("WebSocket close frame received: session={}", session_id);
                        break;
                    }
                }
            }
        }
    }

    // Clean up session
    state.session_store.remove(&session_id).await;
    info!("WebSocket session cleaned up: session={}", session_id);
}

/// Process a text WebSocket message (Transit+JSON or EDN).
///
/// Parses the message, executes the operation, and returns a Transit+JSON
/// response string.
async fn process_ws_message(text: &str, state: &WsState) -> String {
    let request_start = std::time::Instant::now();
    metrics::REQUEST_COUNT.inc();

    // Extract request-id if present for correlation
    let request_id = extract_request_id(text);

    // Try Transit+JSON first, fall back to EDN
    let format = detect_input_format("application/transit+json");
    let parse_result =
        if format == InputFormat::TransitJson && (text.starts_with('[') || text.starts_with('{')) {
            parse_transit_json(text)
        } else {
            parse_request(text)
        };

    let response = match parse_result {
        Ok(request) => {
            match crate::server::execute_operation_public(request.op, &state.app_state).await {
                Ok(result) => Response::Success { result },
                Err(e) => {
                    error!("WebSocket operation failed: {}", e);
                    metrics::ERROR_COUNT.inc();
                    Response::Error { anomaly: e.into() }
                }
            }
        }
        Err(e) => {
            warn!("WebSocket parse error: {}", e);
            metrics::ERROR_COUNT.inc();
            Response::Error { anomaly: e.into() }
        }
    };

    let elapsed = request_start.elapsed();
    debug!("WebSocket request processed in {:?}", elapsed);

    // Wrap response with request-id if present
    let response_json = serialize_transit_json(&response);
    if let Some(rid) = request_id {
        inject_request_id(&response_json, &rid)
    } else {
        response_json
    }
}

/// Process a binary WebSocket message (Transit+MessagePack).
///
/// Parses the binary Transit+MessagePack message, executes the operation,
/// and returns a Transit+JSON response string.
async fn process_ws_binary_message(data: &[u8], state: &WsState) -> String {
    let request_start = std::time::Instant::now();
    metrics::REQUEST_COUNT.inc();

    let parse_result = crate::protocol::transit_parser::parse_transit_msgpack(data);

    let response = match parse_result {
        Ok(request) => {
            match crate::server::execute_operation_public(request.op, &state.app_state).await {
                Ok(result) => Response::Success { result },
                Err(e) => {
                    error!("WebSocket binary operation failed: {}", e);
                    metrics::ERROR_COUNT.inc();
                    Response::Error { anomaly: e.into() }
                }
            }
        }
        Err(e) => {
            warn!("WebSocket binary parse error: {}", e);
            metrics::ERROR_COUNT.inc();
            Response::Error { anomaly: e.into() }
        }
    };

    let elapsed = request_start.elapsed();
    debug!("WebSocket binary request processed in {:?}", elapsed);

    serialize_transit_json(&response)
}

/// Try to extract a `:request-id` value from a Transit+JSON message.
///
/// This does a lightweight string scan rather than a full parse to avoid
/// double-parsing the message.
fn extract_request_id(text: &str) -> Option<String> {
    // Look for "~:request-id" in the Transit+JSON cmap
    let marker = "\"~:request-id\"";
    if let Some(pos) = text.find(marker) {
        let after = &text[pos + marker.len()..];
        // Skip comma and whitespace
        let after = after.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        // Extract the string value
        if after.starts_with('"') {
            let end = after[1..].find('"').map(|i| i + 1)?;
            return Some(after[1..end].to_string());
        }
    }
    None
}

/// Inject a `:request-id` field into a Transit+JSON response.
///
/// Modifies the Transit+JSON cmap to include the request-id for correlation.
fn inject_request_id(response_json: &str, request_id: &str) -> String {
    // Transit+JSON cmap: ["^ ", "~:result", <value>]
    // We insert "~:request-id","<id>" before the closing ]
    if response_json.ends_with(']') {
        let mut result = response_json[..response_json.len() - 1].to_string();
        result.push_str(",\"~:request-id\",\"");
        result.push_str(&request_id.replace('"', "\\\""));
        result.push_str("\"]");
        result
    } else {
        response_json.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_request_id_present() {
        let msg = r#"["^ ","~:op","~:q","~:request-id","req-123","~:args",["^ "]]"#;
        assert_eq!(extract_request_id(msg), Some("req-123".to_string()));
    }

    #[test]
    fn test_extract_request_id_absent() {
        let msg = r#"["^ ","~:op","~:q","~:args",["^ "]]"#;
        assert_eq!(extract_request_id(msg), None);
    }

    #[test]
    fn test_inject_request_id() {
        let response = r#"["^ ","~:result",42]"#;
        let result = inject_request_id(response, "req-456");
        assert_eq!(result, r#"["^ ","~:result",42,"~:request-id","req-456"]"#);
    }

    #[test]
    fn test_inject_request_id_no_bracket() {
        let response = "some-other-format";
        let result = inject_request_id(response, "req-789");
        assert_eq!(result, "some-other-format");
    }

    #[test]
    fn test_inject_request_id_with_special_chars() {
        let response = r#"["^ ","~:result",null]"#;
        let result = inject_request_id(response, "req-with\"quote");
        assert!(result.contains("req-with\\\"quote"));
    }
}
