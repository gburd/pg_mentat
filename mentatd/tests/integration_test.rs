//! Integration tests requiring a running PostgreSQL instance with pg_mentat installed.
//!
//! These tests are ignored by default. Run with:
//!   cargo test -p mentatd --test integration_test -- --ignored
//!
//! Prerequisites:
//!   - PostgreSQL running locally
//!   - DATABASE_URL environment variable set (or defaults to postgresql://localhost/mentat)
//!   - pg_mentat extension available in the PostgreSQL instance

mod helpers;
use helpers::TestServer;

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_server_health_check() {
    let server = TestServer::start().await;
    let response = server.client.get("/health").await;

    assert_eq!(response.status, 200);
    assert_eq!(response.body, "mentatd ready");
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_connect_operation() {
    let server = TestServer::start().await;

    let request = r#"{:op :connect :args {:db-name "test-db"}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":result"));
    assert!(response.body.contains(":connection-id"));
    assert!(response.body.contains(":db-name \"test-db\""));
    assert!(response.body.contains(":status \"connected\""));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_list_databases_operation() {
    let server = TestServer::start().await;

    let request = r#"{:op :list-dbs}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":result"));
    assert!(response.body.contains("[")); // Result is a list
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_query_operation() {
    let server = TestServer::start().await;

    let request = r#"{:op :q :args {:query "[:find ?e :where [?e :name \"Alice\"]]" :args []}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":result"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_transact_operation() {
    let server = TestServer::start().await;

    let request = r#"{:op :transact :args {:connection-id "test-conn-123" :tx-data "[{:db/id -1 :name \"Bob\"}]"}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":result"));
    assert!(response.body.contains(":tx-id"));
    assert!(response.body.contains(":status"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_health_operation() {
    let server = TestServer::start().await;

    let request = r#"{:op :health}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":result"));
    assert!(response.body.contains("healthy"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_invalid_operation() {
    let server = TestServer::start().await;

    let request = r#"{:op :invalid-op}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
    assert!(response.body.contains(":cognitect.anomalies/category"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_missing_op_field() {
    let server = TestServer::start().await;

    let request = r#"{:foo :bar}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
    assert!(response.body.contains("Missing required field"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_invalid_edn_format() {
    let server = TestServer::start().await;

    let request = "not valid edn at all";
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_content_type_header() {
    let server = TestServer::start().await;

    let request = r#"{:op :health}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert_eq!(response.content_type.as_deref(), Some("application/edn"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_db_operation() {
    let server = TestServer::start().await;

    let uuid = "550e8400-e29b-41d4-a716-446655440000";
    let request = format!(r#"{{:op :db :args {{:connection-id "{}"}}}}"#, uuid);
    let response = server.client.post("/", &request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":result"));
    assert!(response.body.contains(":connection-id"));
    assert!(response.body.contains(":status"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_db_operation_invalid_uuid() {
    let server = TestServer::start().await;

    let request = r#"{:op :db :args {:connection-id "not-a-uuid"}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_query_with_timeout() {
    let server = TestServer::start().await;

    let request = r#"{:op :q :args {:query "[:find ?e]" :args [] :timeout 5000}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":result"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_query_with_limit_and_offset() {
    let server = TestServer::start().await;

    let request = r#"{:op :q :args {:query "[:find ?e]" :args [] :limit 10 :offset 5}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":result"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_multiple_concurrent_requests() {
    let server = TestServer::start().await;

    let mut handles = Vec::new();
    for _ in 0..10 {
        let client = server.client.clone();
        let handle = tokio::spawn(async move {
            let request = r#"{:op :health}"#;
            client.post("/", request).await
        });
        handles.push(handle);
    }

    for handle in handles {
        let response = handle.await.unwrap();
        assert_eq!(response.status, 200);
        assert!(response.body.contains("healthy"));
    }
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_edn_response_format() {
    let server = TestServer::start().await;

    let request = r#"{:op :health}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);

    // Verify response parses as valid EDN
    assert!(edn::parse::value(&response.body).is_ok());
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_connect_nonexistent_database() {
    let server = TestServer::start().await;

    let request = r#"{:op :connect :args {:db-name "nonexistent_db_xyz"}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
    assert!(response.body.contains("not found"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_create_and_delete_database() {
    let server = TestServer::start().await;

    let db_name = format!(
        "test_db_{}",
        uuid::Uuid::new_v4().to_string().replace('-', "_")
    );

    // Create database
    let create_request = format!(r#"{{:op :create-db :args {{:db-name "{}"}}}}"#, db_name);
    let create_response = server.client.post("/", &create_request).await;

    assert_eq!(create_response.status, 200);
    assert!(create_response.body.contains(":result"));

    // Delete database
    let delete_request = format!(r#"{{:op :delete-db :args {{:db-name "{}"}}}}"#, db_name);
    let delete_response = server.client.post("/", &delete_request).await;

    assert_eq!(delete_response.status, 200);
    assert!(delete_response.body.contains(":result"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_invalid_database_name() {
    let server = TestServer::start().await;

    let request = r#"{:op :create-db :args {:db-name "invalid-name-with-dashes"}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_datomic_catalog_namespace() {
    let server = TestServer::start().await;

    // Test alternate namespace format
    let request = r#"{:op :datomic.catalog/list-dbs}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":result"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_empty_request_body() {
    let server = TestServer::start().await;

    let request = "";
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_whitespace_only_request() {
    let server = TestServer::start().await;

    let request = "   \n\t  ";
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
}

// ---------------------------------------------------------------------------
// Transit+JSON integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_transit_json_health() {
    let server = TestServer::start().await;

    // Transit+JSON cmap encoding: ["^ ", "~:op", "~:health"]
    let request = r#"["^ ","~:op","~:health"]"#;
    let response = server.client.post_transit_json("/", request).await;

    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/transit+json")
    );
    // Transit+JSON response should contain "~:result" (keyword encoding)
    assert!(
        response.body.contains("result"),
        "Transit+JSON response should contain result: {}",
        response.body
    );
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_transit_json_list_dbs() {
    let server = TestServer::start().await;

    let request = r#"["^ ","~:op","~:list-dbs"]"#;
    let response = server.client.post_transit_json("/", request).await;

    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/transit+json")
    );
    assert!(
        response.body.contains("result"),
        "List databases response should contain result"
    );
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_transit_json_invalid_operation() {
    let server = TestServer::start().await;

    let request = r#"["^ ","~:op","~:nonexistent-op"]"#;
    let response = server.client.post_transit_json("/", request).await;

    assert_eq!(response.status, 200);
    assert!(
        response.body.contains("error"),
        "Invalid operation should return error: {}",
        response.body
    );
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_transit_json_connect() {
    let server = TestServer::start().await;

    let request =
        r#"["^ ","~:op","~:connect","~:args",["^ ","~:db-name","postgres"]]"#;
    let response = server.client.post_transit_json("/", request).await;

    assert_eq!(response.status, 200);
    assert!(
        response.body.contains("result"),
        "Connect response should contain result: {}",
        response.body
    );
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_transit_json_malformed() {
    let server = TestServer::start().await;

    let request = "this is not valid transit json";
    let response = server.client.post_transit_json("/", request).await;

    assert_eq!(response.status, 200);
    assert!(
        response.body.contains("error"),
        "Malformed Transit+JSON should return error"
    );
}

// ---------------------------------------------------------------------------
// Transit+MessagePack integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_transit_msgpack_health() {
    let server = TestServer::start().await;

    // Build msgpack-encoded Transit request: ["^ ", "~:op", "~:health"]
    let mut buf = Vec::new();
    // fixarray(3)
    buf.push(0x93);
    // fixstr "^ " (2 bytes)
    buf.push(0xa2);
    buf.extend_from_slice(b"^ ");
    // fixstr "~:op" (4 bytes)
    buf.push(0xa4);
    buf.extend_from_slice(b"~:op");
    // fixstr "~:health" (8 bytes)
    buf.push(0xa8);
    buf.extend_from_slice(b"~:health");

    let response = server.client.post_transit_msgpack("/", buf).await;

    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/transit+msgpack")
    );
    assert!(
        !response.body.is_empty(),
        "MessagePack response should not be empty"
    );
}

#[tokio::test]
#[ignore = "requires live PostgreSQL"]
async fn test_transit_msgpack_list_dbs() {
    let server = TestServer::start().await;

    // Build msgpack-encoded Transit request: ["^ ", "~:op", "~:list-dbs"]
    let mut buf = Vec::new();
    buf.push(0x93);
    buf.push(0xa2);
    buf.extend_from_slice(b"^ ");
    buf.push(0xa4);
    buf.extend_from_slice(b"~:op");
    buf.push(0xa9);
    buf.extend_from_slice(b"~:list-dbs");

    let response = server.client.post_transit_msgpack("/", buf).await;

    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/transit+msgpack")
    );
    assert!(
        !response.body.is_empty(),
        "MessagePack response should not be empty"
    );
}
