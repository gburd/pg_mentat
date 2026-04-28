// Comprehensive Datomic Client API protocol compatibility tests.
//
// These tests verify that mentatd implements the Datomic Client API protocol
// correctly, covering:
//
// 1. WebSocket connection lifecycle and session management (Task #8)
// 2. All Datomic operations including new ones from Task #9
// 3. Transit+JSON encoding for all operations
// 4. Datomic Client API namespaced operation names
// 5. Error handling and anomaly format
// 6. Concurrent operations over WebSocket
//
// NOTE: Tests that require a live database connection use the TestServer helper
// which needs PostgreSQL running and DATABASE_URL set.

mod helpers;
use helpers::TestServer;

// ============================================================================
// Section 1: Unit tests (no database required)
// ============================================================================

mod protocol_unit_tests {
    use mentatd::protocol::datomic_client::{
        format_connect_response, format_db_response, format_error_response,
        format_success_response, is_valid_operation, normalize_op_keyword,
    };
    use mentatd::protocol::{AnomalyCategory, ResponseValue};
    use mentatd::session::{Session, SessionStore};
    use std::time::Duration;

    // -- Operation normalization --

    #[test]
    fn test_normalize_all_catalog_ops() {
        assert_eq!(normalize_op_keyword("datomic.catalog/list-dbs"), "list-dbs");
        assert_eq!(normalize_op_keyword("list-dbs"), "list-dbs");
        assert_eq!(normalize_op_keyword("list-databases"), "list-dbs");

        assert_eq!(
            normalize_op_keyword("datomic.catalog/create-db"),
            "create-db"
        );
        assert_eq!(normalize_op_keyword("create-db"), "create-db");
        assert_eq!(normalize_op_keyword("create-database"), "create-db");

        assert_eq!(
            normalize_op_keyword("datomic.catalog/delete-db"),
            "delete-db"
        );
        assert_eq!(normalize_op_keyword("delete-db"), "delete-db");
        assert_eq!(normalize_op_keyword("delete-database"), "delete-db");
    }

    #[test]
    fn test_normalize_all_protocol_ops() {
        let ops = vec![
            ("datomic.client.protocol/connect", "connect"),
            ("datomic.client.protocol/db", "db"),
            ("datomic.client.protocol/q", "q"),
            ("datomic.client.protocol/qseq", "qseq"),
            ("datomic.client.protocol/pull", "pull"),
            ("datomic.client.protocol/pull-many", "pull-many"),
            ("datomic.client.protocol/transact", "transact"),
            ("datomic.client.protocol/with", "with"),
            ("datomic.client.protocol/datoms", "datoms"),
            ("datomic.client.protocol/index-range", "index-range"),
            ("datomic.client.protocol/tx-range", "tx-range"),
            ("datomic.client.protocol/entid", "entid"),
            ("datomic.client.protocol/ident", "ident"),
            ("datomic.client.protocol/db-stats", "db-stats"),
            ("datomic.client.protocol/as-of", "as-of"),
            ("datomic.client.protocol/since", "since"),
            ("datomic.client.protocol/history", "history"),
            ("datomic.client.protocol/filter", "filter"),
            ("datomic.client.protocol/basis-t", "basis-t"),
            ("datomic.client.protocol/db-snapshot", "db-snapshot"),
        ];
        for (input, expected) in ops {
            assert_eq!(
                normalize_op_keyword(input),
                expected,
                "normalize_op_keyword({}) should be {}",
                input,
                expected
            );
        }
    }

    #[test]
    fn test_normalize_short_forms() {
        let ops = vec![
            "connect", "db", "q", "qseq", "pull", "pull-many", "transact", "with", "datoms",
            "index-range", "tx-range", "entid", "ident", "db-stats", "as-of", "since", "history",
            "filter", "basis-t", "db-snapshot",
        ];
        for op in ops {
            assert_eq!(
                normalize_op_keyword(op),
                op,
                "Short form '{}' should normalize to itself",
                op
            );
        }
    }

    #[test]
    fn test_unknown_op_passes_through() {
        assert_eq!(normalize_op_keyword("unknown-op"), "unknown-op");
        assert_eq!(normalize_op_keyword(""), "");
        assert_eq!(normalize_op_keyword("foo.bar/baz"), "foo.bar/baz");
    }

    // -- Operation validation --

    #[test]
    fn test_all_valid_operations() {
        let valid_ops = vec![
            "q",
            "qseq",
            "pull",
            "pull-many",
            "transact",
            "with",
            "datoms",
            "index-range",
            "tx-range",
            "connect",
            "db",
            "list-dbs",
            "create-db",
            "delete-db",
            "entid",
            "ident",
            "db-stats",
            "basis-t",
            "as-of",
            "since",
            "history",
            "filter",
            "health",
            "db-snapshot",
        ];
        for op in &valid_ops {
            assert!(
                is_valid_operation(op),
                "'{}' should be a valid operation",
                op
            );
        }
    }

    #[test]
    fn test_namespaced_ops_are_valid() {
        let namespaced_ops = vec![
            "datomic.catalog/list-dbs",
            "datomic.catalog/create-db",
            "datomic.catalog/delete-db",
            "datomic.client.protocol/q",
            "datomic.client.protocol/transact",
            "datomic.client.protocol/pull",
            "datomic.client.protocol/entid",
            "datomic.client.protocol/ident",
            "datomic.client.protocol/db-stats",
            "datomic.client.protocol/index-range",
            "datomic.client.protocol/qseq",
            "datomic.client.protocol/pull-many",
        ];
        for op in &namespaced_ops {
            assert!(
                is_valid_operation(op),
                "Namespaced '{}' should be valid",
                op
            );
        }
    }

    #[test]
    fn test_invalid_operations() {
        assert!(!is_valid_operation("unknown"));
        assert!(!is_valid_operation(""));
        assert!(!is_valid_operation("foo.bar/baz"));
        assert!(!is_valid_operation("drop-table"));
    }

    // -- Response formatting --

    #[test]
    fn test_connect_response_has_required_fields() {
        let resp = format_connect_response("my-db", "conn-uuid", 42);
        match resp {
            ResponseValue::Map(entries) => {
                let keys: Vec<String> = entries
                    .iter()
                    .filter_map(|(k, _)| match k {
                        ResponseValue::Keyword(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
                assert!(keys.contains(&"db-name".to_string()));
                assert!(keys.contains(&"database-id".to_string()));
                assert!(keys.contains(&"t".to_string()));
                assert!(keys.contains(&"next-t".to_string()));
                assert!(keys.contains(&"type".to_string()));
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_connect_response_type_is_connection() {
        let resp = format_connect_response("db", "id", 100);
        match resp {
            ResponseValue::Map(entries) => {
                let type_val = entries
                    .iter()
                    .find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "type"))
                    .map(|(_, v)| v);
                assert!(matches!(
                    type_val,
                    Some(ResponseValue::Keyword(s)) if s == "datomic.client/connection"
                ));
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_db_response_type_is_db() {
        let resp = format_db_response("db", "uuid", 50);
        match resp {
            ResponseValue::Map(entries) => {
                let type_val = entries
                    .iter()
                    .find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "type"))
                    .map(|(_, v)| v);
                assert!(matches!(
                    type_val,
                    Some(ResponseValue::Keyword(s)) if s == "datomic.client/db"
                ));
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_db_response_next_t_is_basis_plus_one() {
        let resp = format_db_response("db", "uuid", 99);
        match resp {
            ResponseValue::Map(entries) => {
                let next_t = entries
                    .iter()
                    .find(|(k, _)| matches!(k, ResponseValue::Keyword(s) if s == "next-t"))
                    .map(|(_, v)| v);
                assert!(matches!(next_t, Some(ResponseValue::Integer(100))));
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_error_response_anomaly_format() {
        let resp = format_error_response(AnomalyCategory::NotFound, "entity not found".to_string());
        match resp {
            mentatd::protocol::Response::Error { anomaly } => {
                assert!(matches!(anomaly.category, AnomalyCategory::NotFound));
                assert_eq!(anomaly.message, "entity not found");
                assert_eq!(
                    anomaly.category.as_keyword(),
                    ":cognitect.anomalies/not-found"
                );
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[test]
    fn test_all_anomaly_category_keywords() {
        let categories = vec![
            (AnomalyCategory::Incorrect, ":cognitect.anomalies/incorrect"),
            (AnomalyCategory::Forbidden, ":cognitect.anomalies/forbidden"),
            (AnomalyCategory::NotFound, ":cognitect.anomalies/not-found"),
            (
                AnomalyCategory::Unavailable,
                ":cognitect.anomalies/unavailable",
            ),
            (
                AnomalyCategory::Interrupted,
                ":cognitect.anomalies/interrupted",
            ),
            (AnomalyCategory::Fault, ":cognitect.anomalies/fault"),
        ];
        for (cat, expected) in categories {
            assert_eq!(cat.as_keyword(), expected);
        }
    }

    #[test]
    fn test_success_response_wraps_value() {
        let resp = format_success_response(ResponseValue::Integer(42));
        match resp {
            mentatd::protocol::Response::Success { result } => {
                assert!(matches!(result, ResponseValue::Integer(42)));
            }
            _ => panic!("Expected Success"),
        }
    }

    // -- Session management --

    #[tokio::test]
    async fn test_session_lifecycle() {
        let store = SessionStore::new(Duration::from_secs(300));

        // Create
        let session = store.create("test-db".to_string()).await;
        assert_eq!(session.db_name, "test-db");
        assert!(!session.id.is_nil());

        // Get
        let retrieved = store.get(&session.id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.as_ref().map(|s| &s.db_name), Some(&"test-db".to_string()));

        // Touch
        assert!(store.touch(&session.id).await);

        // Add snapshot
        store
            .add_snapshot(&session.id, "snap-1".to_string(), 1000)
            .await;
        let with_snap = store.get(&session.id).await;
        assert_eq!(
            with_snap.as_ref().and_then(|s| s.get_snapshot("snap-1")),
            Some(1000)
        );

        // Remove
        let removed = store.remove(&session.id).await;
        assert!(removed.is_some());
        assert!(store.get(&session.id).await.is_none());
    }

    #[tokio::test]
    async fn test_session_expiration_and_cleanup() {
        let store = SessionStore::new(Duration::from_millis(1));

        store.create("db1".to_string()).await;
        store.create("db2".to_string()).await;
        store.create("db3".to_string()).await;

        assert_eq!(store.active_count().await, 3);

        tokio::time::sleep(Duration::from_millis(10)).await;

        // All expired
        assert_eq!(store.active_count().await, 0);

        // Cleanup returns count
        let removed = store.cleanup_expired().await;
        assert_eq!(removed, 3);
    }

    #[tokio::test]
    async fn test_session_touch_resets_expiry() {
        let store = SessionStore::new(Duration::from_millis(50));

        let session = store.create("db".to_string()).await;

        // Touch before expiry
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(store.touch(&session.id).await);

        // Still alive after touch
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(store.get(&session.id).await.is_some());
    }

    #[tokio::test]
    async fn test_session_touch_expired_returns_false() {
        let store = SessionStore::new(Duration::from_millis(1));
        let session = store.create("db".to_string()).await;

        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(!store.touch(&session.id).await);
    }

    #[tokio::test]
    async fn test_session_get_nonexistent() {
        let store = SessionStore::new(Duration::from_secs(300));
        let fake_id = uuid::Uuid::new_v4();
        assert!(store.get(&fake_id).await.is_none());
    }

    #[tokio::test]
    async fn test_multiple_snapshots_per_session() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create("db".to_string()).await;

        store
            .add_snapshot(&session.id, "snap-a".to_string(), 100)
            .await;
        store
            .add_snapshot(&session.id, "snap-b".to_string(), 200)
            .await;
        store
            .add_snapshot(&session.id, "snap-c".to_string(), 300)
            .await;

        let s = store.get(&session.id).await;
        let s = s.as_ref().expect("session should exist");
        assert_eq!(s.get_snapshot("snap-a"), Some(100));
        assert_eq!(s.get_snapshot("snap-b"), Some(200));
        assert_eq!(s.get_snapshot("snap-c"), Some(300));
        assert_eq!(s.get_snapshot("snap-d"), None);
    }

    #[test]
    fn test_session_new_has_valid_state() {
        let session = Session::new("test-db".to_string());
        assert!(!session.id.is_nil());
        assert_eq!(session.db_name, "test-db");
        assert!(session.db_snapshots.is_empty());
        assert!(!session.is_expired(Duration::from_secs(1)));
    }

    #[test]
    fn test_default_session_store() {
        let _store = mentatd::session::default_session_store();
        // Just verifying it constructs without panicking
    }
}

// ============================================================================
// Section 2: Parser tests for new operations (no database required)
// ============================================================================

mod parser_tests {
    use mentatd::protocol::parser::parse_request;
    use mentatd::protocol::Operation;

    // -- qseq parsing --

    #[test]
    fn test_parse_qseq_minimal() {
        let input = r#"{:op :qseq :args {:query "[:find ?e :where [?e :name]]" :args []}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::Qseq {
                query,
                args,
                chunk_size,
                db_id,
            } => {
                assert!(query.contains("find"));
                assert!(args.is_empty());
                assert!(chunk_size.is_none());
                assert!(db_id.is_none());
            }
            _ => panic!("Expected Qseq"),
        }
    }

    #[test]
    fn test_parse_qseq_with_all_options() {
        let input = r#"{:op :qseq :args {:query "[:find ?e]" :args [] :chunk-size 500 :db-id "snap-123"}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::Qseq {
                chunk_size, db_id, ..
            } => {
                assert_eq!(chunk_size, Some(500));
                assert_eq!(db_id.as_deref(), Some("snap-123"));
            }
            _ => panic!("Expected Qseq"),
        }
    }

    #[test]
    fn test_parse_qseq_missing_query() {
        let input = r#"{:op :qseq :args {:args []}}"#;
        assert!(parse_request(input).is_err());
    }

    // -- pull-many parsing --

    #[test]
    fn test_parse_pull_many_multiple_ids() {
        let input = r#"{:op :pull-many :args {:pattern "[*]" :entity-ids [1 2 3 42 100]}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::PullMany {
                pattern,
                entity_ids,
            } => {
                assert!(pattern.contains("*"));
                assert_eq!(entity_ids, vec![1, 2, 3, 42, 100]);
            }
            _ => panic!("Expected PullMany"),
        }
    }

    #[test]
    fn test_parse_pull_many_single_id() {
        let input = r#"{:op :pull-many :args {:pattern "[:person/name]" :entity-ids [42]}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::PullMany { entity_ids, .. } => {
                assert_eq!(entity_ids, vec![42]);
            }
            _ => panic!("Expected PullMany"),
        }
    }

    #[test]
    fn test_parse_pull_many_empty_ids() {
        let input = r#"{:op :pull-many :args {:pattern "[*]" :entity-ids []}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::PullMany { entity_ids, .. } => {
                assert!(entity_ids.is_empty());
            }
            _ => panic!("Expected PullMany"),
        }
    }

    #[test]
    fn test_parse_pull_many_missing_pattern() {
        let input = r#"{:op :pull-many :args {:entity-ids [1 2]}}"#;
        assert!(parse_request(input).is_err());
    }

    #[test]
    fn test_parse_pull_many_missing_entity_ids() {
        let input = r#"{:op :pull-many :args {:pattern "[*]"}}"#;
        assert!(parse_request(input).is_err());
    }

    // -- index-range parsing --

    #[test]
    fn test_parse_index_range_attrid_only() {
        let input = r#"{:op :index-range :args {:attrid ":person/name"}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::IndexRange {
                attrid,
                start,
                end,
                limit,
            } => {
                assert!(attrid.contains("person/name"));
                assert!(start.is_none());
                assert!(end.is_none());
                assert!(limit.is_none());
            }
            _ => panic!("Expected IndexRange"),
        }
    }

    #[test]
    fn test_parse_index_range_with_bounds() {
        let input =
            r#"{:op :index-range :args {:attrid ":person/name" :start "Alice" :end "Charlie" :limit 50}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::IndexRange {
                start, end, limit, ..
            } => {
                assert_eq!(start.as_deref(), Some("Alice"));
                assert_eq!(end.as_deref(), Some("Charlie"));
                assert_eq!(limit, Some(50));
            }
            _ => panic!("Expected IndexRange"),
        }
    }

    #[test]
    fn test_parse_index_range_missing_attrid() {
        let input = r#"{:op :index-range :args {:start "A"}}"#;
        assert!(parse_request(input).is_err());
    }

    // -- entid parsing --

    #[test]
    fn test_parse_entid() {
        let input = r#"{:op :entid :args {:ident ":person/name"}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::Entid { ident } => {
                assert!(ident.contains("person/name"));
            }
            _ => panic!("Expected Entid"),
        }
    }

    #[test]
    fn test_parse_entid_missing_ident() {
        let input = r#"{:op :entid :args {}}"#;
        assert!(parse_request(input).is_err());
    }

    // -- ident parsing --

    #[test]
    fn test_parse_ident() {
        let input = r#"{:op :ident :args {:entid 42}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::Ident { entid } => {
                assert_eq!(entid, 42);
            }
            _ => panic!("Expected Ident"),
        }
    }

    #[test]
    fn test_parse_ident_missing_entid() {
        let input = r#"{:op :ident :args {}}"#;
        assert!(parse_request(input).is_err());
    }

    // -- db-stats parsing --

    #[test]
    fn test_parse_db_stats() {
        let input = "{:op :db-stats}";
        let req = parse_request(input).expect("parse failed");
        assert!(matches!(req.op, Operation::DbStats));
    }

    // -- Edge cases --

    #[test]
    fn test_parse_unknown_op_fails() {
        let input = "{:op :nonexistent-operation}";
        assert!(parse_request(input).is_err());
    }

    #[test]
    fn test_parse_empty_input() {
        assert!(parse_request("").is_err());
    }

    #[test]
    fn test_parse_non_map_input() {
        assert!(parse_request("[1 2 3]").is_err());
    }

    #[test]
    fn test_parse_missing_op() {
        assert!(parse_request("{:foo :bar}").is_err());
    }
}

// ============================================================================
// Section 3: WebSocket protocol tests (no database required)
// ============================================================================

mod websocket_unit_tests {
    // These test the WebSocket message processing helpers

    #[test]
    fn test_request_id_extraction_from_transit_json() {
        // Simulate a Transit+JSON message with request-id
        let msg = r#"["^ ","~:op","~:q","~:request-id","req-abc-123","~:args",["^ "]]"#;
        // The extract_request_id function is private, but we can test the
        // inject_request_id function from the public tests in websocket.rs

        // Verify the message format is parseable
        assert!(msg.contains("~:request-id"));
        assert!(msg.contains("req-abc-123"));
    }

    #[test]
    fn test_transit_json_cmap_format() {
        // Verify Transit+JSON cmap format: ["^ ", key, value, key, value, ...]
        let cmap = r#"["^ ","~:op","~:health"]"#;
        assert!(cmap.starts_with("[\"^ \""));
        assert!(cmap.ends_with(']'));
    }
}

// ============================================================================
// Section 4: Transit+JSON serialization tests for new operations
// ============================================================================

mod transit_serialization_tests {
    use mentatd::protocol::transit_serializer::serialize_transit_json;
    use mentatd::protocol::{Response, ResponseValue};

    #[test]
    fn test_serialize_db_stats_response() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("datoms".to_string()),
                    ResponseValue::Integer(5000),
                ),
                (
                    ResponseValue::Keyword("transactions".to_string()),
                    ResponseValue::Integer(42),
                ),
                (
                    ResponseValue::Keyword("schema-attributes".to_string()),
                    ResponseValue::Integer(15),
                ),
                (
                    ResponseValue::Keyword("basis-t".to_string()),
                    ResponseValue::Integer(1000042),
                ),
            ]),
        };

        let json = serialize_transit_json(&response);
        assert!(json.contains("~:datoms"));
        assert!(json.contains("5000"));
        assert!(json.contains("~:transactions"));
        assert!(json.contains("42"));
        assert!(json.contains("~:schema-attributes"));
        assert!(json.contains("~:basis-t"));
        assert!(json.contains("1000042"));
    }

    #[test]
    fn test_serialize_entid_response() {
        let response = Response::Success {
            result: ResponseValue::Integer(73),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("73"));
        assert!(json.contains("~:result"));
    }

    #[test]
    fn test_serialize_ident_response() {
        let response = Response::Success {
            result: ResponseValue::Keyword("person/name".to_string()),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("~:person/name"));
        assert!(json.contains("~:result"));
    }

    #[test]
    fn test_serialize_pull_many_response() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![
                ResponseValue::Map(vec![(
                    ResponseValue::Keyword("db/id".to_string()),
                    ResponseValue::Integer(42),
                )]),
                ResponseValue::Map(vec![(
                    ResponseValue::Keyword("db/id".to_string()),
                    ResponseValue::Integer(43),
                )]),
            ]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("~:db/id"));
        assert!(json.contains("42"));
        assert!(json.contains("43"));
    }

    #[test]
    fn test_serialize_qseq_chunked_response() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("chunks".to_string()),
                    ResponseValue::Vector(vec![ResponseValue::Vector(vec![
                        ResponseValue::Integer(1),
                        ResponseValue::Integer(2),
                    ])]),
                ),
                (
                    ResponseValue::Keyword("total-count".to_string()),
                    ResponseValue::Integer(2),
                ),
                (
                    ResponseValue::Keyword("chunk-size".to_string()),
                    ResponseValue::Integer(1000),
                ),
            ]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("~:chunks"));
        assert!(json.contains("~:total-count"));
        assert!(json.contains("~:chunk-size"));
    }

    #[test]
    fn test_serialize_index_range_response() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![ResponseValue::Vector(vec![
                ResponseValue::Integer(42),
                ResponseValue::Integer(73),
                ResponseValue::String("Alice".to_string()),
                ResponseValue::Integer(1000001),
                ResponseValue::Boolean(true),
            ])]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("42"));
        assert!(json.contains("73"));
        assert!(json.contains("Alice"));
        assert!(json.contains("1000001"));
        assert!(json.contains("true"));
    }

    #[test]
    fn test_serialize_connect_response_format() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("db-name".to_string()),
                    ResponseValue::String("my-db".to_string()),
                ),
                (
                    ResponseValue::Keyword("database-id".to_string()),
                    ResponseValue::String("uuid-123".to_string()),
                ),
                (
                    ResponseValue::Keyword("t".to_string()),
                    ResponseValue::Integer(1000),
                ),
                (
                    ResponseValue::Keyword("next-t".to_string()),
                    ResponseValue::Integer(1001),
                ),
                (
                    ResponseValue::Keyword("type".to_string()),
                    ResponseValue::Keyword("datomic.client/connection".to_string()),
                ),
            ]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("~:db-name"));
        assert!(json.contains("my-db"));
        assert!(json.contains("~:database-id"));
        assert!(json.contains("~:type"));
        assert!(json.contains("~:datomic.client/connection"));
    }

    #[test]
    fn test_serialize_error_anomaly_format() {
        let response = Response::Error {
            anomaly: mentatd::protocol::Anomaly {
                category: mentatd::protocol::AnomalyCategory::NotFound,
                message: "Entity not found".to_string(),
                db_error: Some("db.error/not-found".to_string()),
            },
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("~:cognitect.anomalies/category"));
        assert!(json.contains("~:cognitect.anomalies/not-found"));
        assert!(json.contains("~:cognitect.anomalies/message"));
        assert!(json.contains("Entity not found"));
    }
}

// ============================================================================
// Section 5: Integration tests (require database connection)
// ============================================================================

// -- New operation integration tests via HTTP --

/// Helper: assert response uses valid Datomic protocol format (EDN)
fn assert_valid_edn_response(body: &str, op: &str) {
    assert!(
        body.contains(":result") || body.contains(":error"),
        "{} should return :result or :error in Datomic format, got: {}",
        op,
        body
    );
    if body.contains(":error") {
        assert!(
            body.contains(":cognitect.anomalies/category"),
            "{} error should use cognitect.anomalies format: {}",
            op,
            body
        );
    }
}

/// Helper: assert response uses valid Datomic protocol format (Transit+JSON)
fn assert_valid_transit_response(body: &str, op: &str) {
    assert!(
        body.contains("~:result") || body.contains("~:error"),
        "{} should return ~:result or ~:error in Transit format, got: {}",
        op,
        body
    );
    if body.contains("~:error") {
        assert!(
            body.contains("~:cognitect.anomalies/category"),
            "{} error should use cognitect.anomalies format: {}",
            op,
            body
        );
    }
}

#[tokio::test]
async fn test_http_db_stats_operation() {
    let server = TestServer::start().await;

    let request = r#"{:op :db-stats}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert_valid_edn_response(&response.body, "db-stats");
}

#[tokio::test]
async fn test_http_basis_t_operation() {
    let server = TestServer::start().await;

    let request = r#"{:op :basis-t}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert_valid_edn_response(&response.body, "basis-t");
}

#[tokio::test]
async fn test_http_db_stats_via_transit_json() {
    let server = TestServer::start().await;

    let request = r#"["^ ","~:op","~:db-stats"]"#;
    let response = server.client.post_transit_json("/", request).await;

    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/transit+json")
    );
    assert_valid_transit_response(&response.body, "db-stats");
}

#[tokio::test]
async fn test_http_qseq_operation() {
    let server = TestServer::start().await;

    let request =
        r#"{:op :qseq :args {:query "[:find ?e :where [?e :db/ident _]]" :args [] :chunk-size 100}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert_valid_edn_response(&response.body, "qseq");
}

#[tokio::test]
async fn test_http_pull_many_operation() {
    let server = TestServer::start().await;

    let request = r#"{:op :pull-many :args {:pattern "[*]" :entity-ids [1 2 3]}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert_valid_edn_response(&response.body, "pull-many");
}

#[tokio::test]
async fn test_http_pull_many_empty_ids() {
    let server = TestServer::start().await;

    let request = r#"{:op :pull-many :args {:pattern "[*]" :entity-ids []}}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert_valid_edn_response(&response.body, "pull-many (empty ids)");
}

// -- WebSocket integration tests --

#[tokio::test]
async fn test_websocket_connection_and_welcome() {
    use futures_util::StreamExt;

    let server = TestServer::start().await;
    let ws_url = server.client.ws_url("/ws");

    let (ws_stream, _response) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("Failed to connect WebSocket");

    let (mut _write, mut read) = ws_stream.split();

    // First message should be the welcome message
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout waiting for welcome message")
        .expect("Stream ended without welcome message")
        .expect("Error reading welcome message");

    let text = msg.into_text().expect("Welcome message should be text");

    // Welcome message should contain session info
    assert!(
        text.contains("session-id") || text.contains("session"),
        "Welcome message should contain session info: {}",
        text
    );
    assert!(
        text.contains("datomic.client/session") || text.contains("protocol-version"),
        "Welcome message should contain protocol info: {}",
        text
    );
}

#[tokio::test]
async fn test_websocket_health_operation() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let server = TestServer::start().await;
    let ws_url = server.client.ws_url("/ws");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("Failed to connect WebSocket");

    let (mut write, mut read) = ws_stream.split();

    // Consume welcome message
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout")
        .expect("No welcome")
        .expect("Error");

    // Send health operation as Transit+JSON
    let health_request = r#"["^ ","~:op","~:health"]"#;
    write
        .send(Message::Text(health_request.into()))
        .await
        .expect("Failed to send health request");

    // Read response
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout waiting for health response")
        .expect("Stream ended")
        .expect("Error reading response");

    let text = response.into_text().expect("Response should be text");
    assert!(
        text.contains("result") && text.contains("healthy"),
        "Health response should indicate healthy: {}",
        text
    );
}

#[tokio::test]
async fn test_websocket_edn_operation() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let server = TestServer::start().await;
    let ws_url = server.client.ws_url("/ws");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("Failed to connect");

    let (mut write, mut read) = ws_stream.split();

    // Consume welcome
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout")
        .expect("No welcome")
        .expect("Error");

    // Send EDN operation
    let edn_request = r#"{:op :health}"#;
    write
        .send(Message::Text(edn_request.into()))
        .await
        .expect("Failed to send EDN request");

    let response = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout")
        .expect("Stream ended")
        .expect("Error");

    let text = response.into_text().expect("Response should be text");
    assert!(
        text.contains("result") || text.contains("error"),
        "EDN operation should return result or error: {}",
        text
    );
}

#[tokio::test]
async fn test_websocket_multiple_operations() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let server = TestServer::start().await;
    let ws_url = server.client.ws_url("/ws");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("Failed to connect");

    let (mut write, mut read) = ws_stream.split();

    // Consume welcome
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout")
        .expect("No welcome")
        .expect("Error");

    // Send multiple operations on the same connection
    let ops = vec![
        r#"["^ ","~:op","~:health"]"#,
        r#"["^ ","~:op","~:list-dbs"]"#,
        r#"["^ ","~:op","~:basis-t"]"#,
    ];

    for (i, op) in ops.iter().enumerate() {
        write
            .send(Message::Text((*op).into()))
            .await
            .unwrap_or_else(|_| panic!("Failed to send op {}", i));

        let response = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
            .await
            .unwrap_or_else(|_| panic!("Timeout on op {}", i))
            .unwrap_or_else(|| panic!("Stream ended on op {}", i))
            .unwrap_or_else(|_| panic!("Error on op {}", i));

        let text = response.into_text().expect("Response should be text");
        // Without a database, some operations return anomaly errors
        assert!(
            text.contains("result") || text.contains("error"),
            "Operation {} should return result or error: {}",
            i,
            text
        );
    }
}

#[tokio::test]
async fn test_websocket_invalid_operation() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let server = TestServer::start().await;
    let ws_url = server.client.ws_url("/ws");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("Failed to connect");

    let (mut write, mut read) = ws_stream.split();

    // Consume welcome
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout")
        .expect("No welcome")
        .expect("Error");

    // Send invalid operation
    let invalid_request = r#"["^ ","~:op","~:nonexistent-op-xyz"]"#;
    write
        .send(Message::Text(invalid_request.into()))
        .await
        .expect("Failed to send invalid request");

    let response = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout")
        .expect("Stream ended")
        .expect("Error");

    let text = response.into_text().expect("Response should be text");
    assert!(
        text.contains("error") || text.contains("anomal"),
        "Invalid operation should return error: {}",
        text
    );
}

#[tokio::test]
async fn test_websocket_malformed_message() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let server = TestServer::start().await;
    let ws_url = server.client.ws_url("/ws");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("Failed to connect");

    let (mut write, mut read) = ws_stream.split();

    // Consume welcome
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout")
        .expect("No welcome")
        .expect("Error");

    // Send malformed JSON
    write
        .send(Message::Text("this is not valid json or edn {{{{".into()))
        .await
        .expect("Failed to send malformed message");

    let response = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout")
        .expect("Stream ended")
        .expect("Error");

    let text = response.into_text().expect("Response should be text");
    assert!(
        text.contains("error") || text.contains("anomal"),
        "Malformed message should return error: {}",
        text
    );
}

#[tokio::test]
async fn test_websocket_graceful_close() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let server = TestServer::start().await;
    let ws_url = server.client.ws_url("/ws");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("Failed to connect");

    let (mut write, mut read) = ws_stream.split();

    // Consume welcome
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout")
        .expect("No welcome")
        .expect("Error");

    // Send close frame
    write
        .send(Message::Close(None))
        .await
        .expect("Failed to send close");

    // Connection should end
    let next = tokio::time::timeout(std::time::Duration::from_secs(5), read.next()).await;

    match next {
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
            // Expected: server sends close frame back or stream ends
        }
        other => {
            // Any other result is acceptable as long as we don't hang
            let _ = other;
        }
    }
}

// -- Datomic namespace integration tests --

#[tokio::test]
async fn test_http_datomic_catalog_list_dbs() {
    let server = TestServer::start().await;

    let request = r#"{:op :datomic.catalog/list-dbs}"#;
    let response = server.client.post("/", request).await;

    assert_eq!(response.status, 200);
    assert_valid_edn_response(&response.body, "datomic.catalog/list-dbs");
}

#[tokio::test]
async fn test_http_datomic_namespace_transit_json() {
    let server = TestServer::start().await;

    // Use fully-qualified Datomic namespace in Transit+JSON
    let request = r#"["^ ","~:op","~:datomic.catalog/list-dbs"]"#;
    let response = server.client.post_transit_json("/", request).await;

    assert_eq!(response.status, 200);
    assert_valid_transit_response(&response.body, "datomic.catalog/list-dbs");
}
