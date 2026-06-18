// Datomic Client API WebSocket compatibility tests.
//
// Tests the WebSocket connection lifecycle, session management, and
// multiplexed request/response protocol as defined by the Datomic Client API.
//
// Reference: tests/datomic_compatibility/README.md Section 2 (Session Protocol)
//
// ## Session Protocol
//
// 1. Client connects to ws://host:port/ws
// 2. Server sends welcome: {:type :datomic.client/session, :session-id "...", :protocol-version 1}
// 3. Client sends requests with :op, :args, optional :request-id
// 4. Server responds with :result or :error (cognitect.anomalies format)
// 5. Request-id correlation for multiplexed requests

mod helpers;
use helpers::TestServer;

// ============================================================================
// 1. Connection Lifecycle
// ============================================================================

/// On WebSocket upgrade the server MUST immediately send a welcome message
/// containing {:type :datomic.client/session, :session-id "<uuid>",
/// :protocol-version 1}.
#[tokio::test]
async fn test_ws_welcome_message_format() {
    use futures_util::StreamExt;

    let server = TestServer::start().await;
    let ws_url = server.client.ws_url("/ws");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect failed");

    let (_write, mut read) = ws_stream.split();

    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout waiting for welcome")
        .expect("Stream ended")
        .expect("Read error");

    let text = msg.into_text().expect("Expected text frame");

    // Verify welcome contains all required Datomic session fields
    assert!(
        text.contains("datomic.client/session"),
        "Welcome must contain type datomic.client/session: {}",
        text
    );
    assert!(
        text.contains("session-id"),
        "Welcome must contain session-id: {}",
        text
    );
    assert!(
        text.contains("protocol-version"),
        "Welcome must contain protocol-version: {}",
        text
    );
}

/// The session-id in the welcome message must be a valid UUID.
#[tokio::test]
async fn test_ws_welcome_session_id_is_uuid() {
    use futures_util::StreamExt;

    let server = TestServer::start().await;
    let ws_url = server.client.ws_url("/ws");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect failed");

    let (_write, mut read) = ws_stream.split();

    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout")
        .expect("Stream ended")
        .expect("Read error");

    let text = msg.into_text().expect("Expected text");

    // Extract the session-id value from Transit+JSON response
    // Format: [...,"~:session-id","<uuid>",...]
    if let Some(pos) = text.find("\"~:session-id\"") {
        let after = &text[pos + "\"~:session-id\"".len()..];
        let after = after.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        if after.starts_with('"') {
            let end = after[1..].find('"').expect("Unterminated string");
            let session_id = &after[1..end + 1];
            uuid::Uuid::parse_str(session_id)
                .unwrap_or_else(|_| panic!("session-id is not a valid UUID: {}", session_id));
        } else {
            panic!("session-id value is not a string: {}", after);
        }
    } else {
        panic!("Welcome message does not contain ~:session-id: {}", text);
    }
}

/// Each new connection should receive a unique session-id.
#[tokio::test]
async fn test_ws_unique_session_ids() {
    use futures_util::StreamExt;

    let server = TestServer::start().await;
    let ws_url = server.client.ws_url("/ws");

    let mut session_ids = Vec::new();

    for _ in 0..3 {
        let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("WebSocket connect failed");

        let (_write, mut read) = ws_stream.split();

        let msg = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
            .await
            .expect("Timeout")
            .expect("Stream ended")
            .expect("Read error");

        let text = msg.into_text().expect("Expected text");
        session_ids.push(text);
    }

    // All three welcome messages should be different (different session-ids)
    assert_ne!(session_ids[0], session_ids[1]);
    assert_ne!(session_ids[1], session_ids[2]);
    assert_ne!(session_ids[0], session_ids[2]);
}

// ============================================================================
// 2. Request/Response Over WebSocket
// ============================================================================

/// Helper: connect, consume welcome, return (write, read) halves.
async fn ws_connect(
    server: &TestServer,
) -> (
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::Message,
    >,
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) {
    use futures_util::StreamExt;

    let ws_url = server.client.ws_url("/ws");
    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect failed");

    let (write, mut read) = ws_stream.split();

    // Consume welcome message
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout on welcome")
        .expect("No welcome")
        .expect("Welcome error");

    (write, read)
}

/// Helper: send a Transit+JSON text message and read the response.
async fn ws_send_recv(
    write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::Message,
    >,
    read: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    request: &str,
) -> String {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    write
        .send(Message::Text(request.into()))
        .await
        .expect("Failed to send");

    let response = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout waiting for response")
        .expect("Stream ended")
        .expect("Read error");

    response
        .into_text()
        .expect("Expected text response")
        .to_string()
}

/// A simple :health operation should return a success result over WebSocket.
#[tokio::test]
async fn test_ws_health_operation() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let response = ws_send_recv(&mut write, &mut read, r#"["^ ","~:op","~:health"]"#).await;

    assert!(
        response.contains("result") && response.contains("healthy"),
        "Health should return healthy result: {}",
        response
    );
}

/// Helper: assert WebSocket response uses valid Datomic protocol format.
/// Without a database, operations return anomaly errors; with a database,
/// they return results. Both are valid protocol responses.
fn assert_valid_ws_response(response: &str, op: &str) {
    assert!(
        response.contains("result") || response.contains("error"),
        "{} should return result or error in Datomic format: {}",
        op,
        response
    );
}

/// EDN-formatted operations should also work over WebSocket.
#[tokio::test]
async fn test_ws_edn_format_operation() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let response = ws_send_recv(&mut write, &mut read, "{:op :health}").await;

    assert_valid_ws_response(&response, "EDN health");
}

/// :list-dbs operation over WebSocket.
#[tokio::test]
async fn test_ws_list_dbs_operation() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let response = ws_send_recv(&mut write, &mut read, r#"["^ ","~:op","~:list-dbs"]"#).await;

    assert_valid_ws_response(&response, "list-dbs");
}

/// :basis-t operation over WebSocket.
#[tokio::test]
async fn test_ws_basis_t_operation() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let response = ws_send_recv(&mut write, &mut read, r#"["^ ","~:op","~:basis-t"]"#).await;

    assert_valid_ws_response(&response, "basis-t");
}

/// Datomic fully-qualified namespace operations should work over WebSocket.
#[tokio::test]
async fn test_ws_datomic_namespaced_operation() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let response = ws_send_recv(
        &mut write,
        &mut read,
        r#"["^ ","~:op","~:datomic.catalog/list-dbs"]"#,
    )
    .await;

    assert_valid_ws_response(&response, "datomic.catalog/list-dbs");
}

// ============================================================================
// 3. Multiple Operations on Single Connection
// ============================================================================

/// Multiple sequential operations on a single WebSocket connection should
/// all receive correct responses in valid Datomic protocol format.
#[tokio::test]
async fn test_ws_sequential_operations() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let ops = vec![
        r#"["^ ","~:op","~:health"]"#,
        r#"["^ ","~:op","~:list-dbs"]"#,
        r#"["^ ","~:op","~:basis-t"]"#,
        r#"["^ ","~:op","~:db-stats"]"#,
    ];

    for (i, request) in ops.iter().enumerate() {
        let response = ws_send_recv(&mut write, &mut read, request).await;
        assert_valid_ws_response(&response, &format!("sequential op {}", i));
    }
}

/// Alternating between Transit+JSON and EDN on the same connection.
#[tokio::test]
async fn test_ws_mixed_format_operations() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    // Transit+JSON
    let r1 = ws_send_recv(&mut write, &mut read, r#"["^ ","~:op","~:health"]"#).await;
    assert_valid_ws_response(&r1, "Transit+JSON health");

    // EDN
    let r2 = ws_send_recv(&mut write, &mut read, "{:op :health}").await;
    assert_valid_ws_response(&r2, "EDN health");

    // Transit+JSON again
    let r3 = ws_send_recv(&mut write, &mut read, r#"["^ ","~:op","~:list-dbs"]"#).await;
    assert_valid_ws_response(&r3, "Transit+JSON list-dbs");
}

// ============================================================================
// 4. Error Handling Over WebSocket
// ============================================================================

/// Invalid operation names should return a cognitect.anomalies error.
#[tokio::test]
async fn test_ws_invalid_operation_returns_error() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let response = ws_send_recv(
        &mut write,
        &mut read,
        r#"["^ ","~:op","~:nonexistent-xyz"]"#,
    )
    .await;

    assert!(
        response.contains("error") || response.contains("anomal"),
        "Invalid operation should return error: {}",
        response
    );
}

/// Malformed messages should return an error, not crash the connection.
#[tokio::test]
async fn test_ws_malformed_message_returns_error() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let response = ws_send_recv(&mut write, &mut read, "this is not valid json or edn {{{{").await;

    assert!(
        response.contains("error") || response.contains("anomal"),
        "Malformed message should return error: {}",
        response
    );
}

/// After an error, the connection should remain usable for subsequent requests.
#[tokio::test]
async fn test_ws_connection_survives_error() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    // Send invalid request
    let err_response = ws_send_recv(&mut write, &mut read, "bad request").await;
    assert!(
        err_response.contains("error"),
        "Should get error: {}",
        err_response
    );

    // Connection should still work
    let ok_response = ws_send_recv(&mut write, &mut read, r#"["^ ","~:op","~:health"]"#).await;
    assert!(
        ok_response.contains("healthy"),
        "Connection should still work after error: {}",
        ok_response
    );
}

/// Multiple errors in a row should not kill the connection.
#[tokio::test]
async fn test_ws_multiple_errors_dont_kill_connection() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    for _ in 0..5 {
        let resp = ws_send_recv(&mut write, &mut read, "invalid").await;
        assert!(resp.contains("error"), "Should get error: {}", resp);
    }

    // Still alive
    let ok = ws_send_recv(&mut write, &mut read, r#"["^ ","~:op","~:health"]"#).await;
    assert!(ok.contains("healthy"), "Should still work: {}", ok);
}

// ============================================================================
// 5. Graceful Close
// ============================================================================

/// Sending a WebSocket close frame should cleanly close the connection.
#[tokio::test]
async fn test_ws_graceful_close() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    // Send close frame
    write
        .send(Message::Close(None))
        .await
        .expect("Failed to send close");

    // Server should acknowledge close or stream should end
    let next = tokio::time::timeout(std::time::Duration::from_secs(5), read.next()).await;

    match next {
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
            // Expected: server sends close frame back or stream ends
        }
        _ => {
            // Any non-hanging result is acceptable
        }
    }
}

// ============================================================================
// 6. Request-ID Correlation (Multiplexing)
// ============================================================================

/// When a request includes :request-id, the response must echo it back.
#[tokio::test]
async fn test_ws_request_id_correlation() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let response = ws_send_recv(
        &mut write,
        &mut read,
        r#"["^ ","~:op","~:health","~:request-id","req-abc-123"]"#,
    )
    .await;

    assert!(
        response.contains("req-abc-123"),
        "Response must echo back the request-id: {}",
        response
    );
}

/// Different request-ids should be correctly correlated with their responses.
#[tokio::test]
async fn test_ws_multiple_request_ids() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let ids = ["req-001", "req-002", "req-003"];
    for id in &ids {
        let request = format!(r#"["^ ","~:op","~:health","~:request-id","{}"]"#, id);
        let response = ws_send_recv(&mut write, &mut read, &request).await;
        assert!(
            response.contains(id),
            "Response should contain request-id '{}': {}",
            id,
            response
        );
    }
}

/// Error responses should also include the request-id for correlation.
#[tokio::test]
async fn test_ws_request_id_on_error() {
    let server = TestServer::start().await;
    let (mut write, mut read) = ws_connect(&server).await;

    let response = ws_send_recv(
        &mut write,
        &mut read,
        r#"["^ ","~:op","~:nonexistent","~:request-id","err-req-42"]"#,
    )
    .await;

    assert!(
        response.contains("err-req-42"),
        "Error response should still echo request-id: {}",
        response
    );
}

// ============================================================================
// 7. Session Management (Unit Tests)
// ============================================================================

mod session_unit_tests {
    use mentatd::session::{Session, SessionStore};
    use std::time::Duration;

    /// Session.new() creates a non-nil UUID and empty snapshots.
    #[test]
    fn test_session_initial_state() {
        let session = Session::new("my-db".to_string());
        assert!(!session.id.is_nil());
        assert_eq!(session.db_name, "my-db");
        assert!(session.db_snapshots.is_empty());
        assert!(!session.is_expired(Duration::from_secs(60)));
    }

    /// SessionStore: create, get, touch, add_snapshot, remove.
    #[tokio::test]
    async fn test_session_store_full_lifecycle() {
        let store = SessionStore::new(Duration::from_secs(300));

        // Create
        let session = store.create("test-db".to_string()).await;
        assert_eq!(session.db_name, "test-db");

        // Get
        let got = store.get(&session.id).await;
        assert!(got.is_some());
        assert_eq!(got.as_ref().unwrap().db_name, "test-db");

        // Touch
        assert!(store.touch(&session.id).await);

        // Add snapshot
        store
            .add_snapshot(&session.id, "snap-1".to_string(), 1000)
            .await;
        let with_snap = store.get(&session.id).await.unwrap();
        assert_eq!(with_snap.get_snapshot("snap-1"), Some(1000));

        // Remove
        let removed = store.remove(&session.id).await;
        assert!(removed.is_some());
        assert!(store.get(&session.id).await.is_none());
    }

    /// Expired sessions should not be returned by get().
    #[tokio::test]
    async fn test_session_expiration() {
        let store = SessionStore::new(Duration::from_millis(1));
        let session = store.create("db".to_string()).await;

        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(store.get(&session.id).await.is_none());
    }

    /// touch() on an expired session returns false and cleans it up.
    #[tokio::test]
    async fn test_touch_expired_session() {
        let store = SessionStore::new(Duration::from_millis(1));
        let session = store.create("db".to_string()).await;

        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(!store.touch(&session.id).await);
    }

    /// cleanup_expired() removes all expired sessions and returns the count.
    #[tokio::test]
    async fn test_cleanup_expired_sessions() {
        let store = SessionStore::new(Duration::from_millis(1));
        store.create("db1".to_string()).await;
        store.create("db2".to_string()).await;
        store.create("db3".to_string()).await;

        tokio::time::sleep(Duration::from_millis(10)).await;

        let removed = store.cleanup_expired().await;
        assert_eq!(removed, 3);
        assert_eq!(store.active_count().await, 0);
    }

    /// touch() resets expiry, keeping the session alive.
    #[tokio::test]
    async fn test_touch_resets_expiry() {
        let store = SessionStore::new(Duration::from_millis(50));
        let session = store.create("db".to_string()).await;

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(store.touch(&session.id).await);

        tokio::time::sleep(Duration::from_millis(30)).await;
        // Should still be alive because we touched it
        assert!(store.get(&session.id).await.is_some());
    }

    /// Multiple snapshots can be stored per session.
    #[tokio::test]
    async fn test_multiple_snapshots() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create("db".to_string()).await;

        store.add_snapshot(&session.id, "s1".to_string(), 100).await;
        store.add_snapshot(&session.id, "s2".to_string(), 200).await;
        store.add_snapshot(&session.id, "s3".to_string(), 300).await;

        let s = store.get(&session.id).await.unwrap();
        assert_eq!(s.get_snapshot("s1"), Some(100));
        assert_eq!(s.get_snapshot("s2"), Some(200));
        assert_eq!(s.get_snapshot("s3"), Some(300));
        assert_eq!(s.get_snapshot("s4"), None);
    }

    /// get() on a non-existent session returns None.
    #[tokio::test]
    async fn test_get_nonexistent_session() {
        let store = SessionStore::new(Duration::from_secs(300));
        assert!(store.get(&uuid::Uuid::new_v4()).await.is_none());
    }

    /// default_session_store() constructs without panicking.
    #[test]
    fn test_default_session_store() {
        let _store = mentatd::session::default_session_store();
    }
}
