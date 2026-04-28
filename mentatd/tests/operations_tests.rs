// Datomic Client API operations compatibility tests.
//
// Tests all 6 new operations from Task #9 (qseq, pull-many, index-range,
// entid, ident, db-stats) plus existing operations, verifying:
//
// - Correct parsing of each operation's EDN request format
// - HTTP endpoint responses match expected Datomic format
// - Transit+JSON encoding of requests and responses
// - Error conditions and edge cases
// - Datomic Client API namespaced operation names
//
// Reference: tests/datomic_compatibility/README.md Section 3 (API Operations)

mod helpers;
use helpers::TestServer;

// ============================================================================
// Section 1: Parser tests for all 6 new operations (no database required)
// ============================================================================

mod parser_tests {
    use mentatd::protocol::parser::parse_request;
    use mentatd::protocol::Operation;

    // -- qseq ---------------------------------------------------------------

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
    fn test_parse_qseq_with_chunk_size() {
        let input =
            r#"{:op :qseq :args {:query "[:find ?e]" :args [] :chunk-size 500}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::Qseq { chunk_size, .. } => {
                assert_eq!(chunk_size, Some(500));
            }
            _ => panic!("Expected Qseq"),
        }
    }

    #[test]
    fn test_parse_qseq_with_db_id() {
        let input =
            r#"{:op :qseq :args {:query "[:find ?e]" :args [] :db-id "snap-123"}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::Qseq { db_id, .. } => {
                assert_eq!(db_id.as_deref(), Some("snap-123"));
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

    // -- pull-many ----------------------------------------------------------

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

    // -- index-range --------------------------------------------------------

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
    fn test_parse_index_range_with_bounds_and_limit() {
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
    fn test_parse_index_range_with_start_only() {
        let input = r#"{:op :index-range :args {:attrid ":person/age" :start "18"}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::IndexRange { start, end, .. } => {
                assert_eq!(start.as_deref(), Some("18"));
                assert!(end.is_none());
            }
            _ => panic!("Expected IndexRange"),
        }
    }

    #[test]
    fn test_parse_index_range_missing_attrid() {
        let input = r#"{:op :index-range :args {:start "A"}}"#;
        assert!(parse_request(input).is_err());
    }

    // -- entid --------------------------------------------------------------

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
    fn test_parse_entid_db_ident() {
        let input = r#"{:op :entid :args {:ident ":db/ident"}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::Entid { ident } => {
                assert!(ident.contains("db/ident"));
            }
            _ => panic!("Expected Entid"),
        }
    }

    #[test]
    fn test_parse_entid_missing_ident() {
        let input = r#"{:op :entid :args {}}"#;
        assert!(parse_request(input).is_err());
    }

    // -- ident --------------------------------------------------------------

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
    fn test_parse_ident_large_entid() {
        let input = r#"{:op :ident :args {:entid 1000001}}"#;
        let req = parse_request(input).expect("parse failed");
        match req.op {
            Operation::Ident { entid } => {
                assert_eq!(entid, 1000001);
            }
            _ => panic!("Expected Ident"),
        }
    }

    #[test]
    fn test_parse_ident_missing_entid() {
        let input = r#"{:op :ident :args {}}"#;
        assert!(parse_request(input).is_err());
    }

    // -- db-stats -----------------------------------------------------------

    #[test]
    fn test_parse_db_stats() {
        let input = "{:op :db-stats}";
        let req = parse_request(input).expect("parse failed");
        assert!(matches!(req.op, Operation::DbStats));
    }

    #[test]
    fn test_parse_db_stats_ignores_extra_fields() {
        let input = "{:op :db-stats :extra 42}";
        let req = parse_request(input).expect("parse failed");
        assert!(matches!(req.op, Operation::DbStats));
    }

    // -- Edge cases ---------------------------------------------------------

    #[test]
    fn test_parse_unknown_op() {
        assert!(parse_request("{:op :nonexistent}").is_err());
    }

    #[test]
    fn test_parse_empty_input() {
        assert!(parse_request("").is_err());
    }

    #[test]
    fn test_parse_non_map() {
        assert!(parse_request("[1 2 3]").is_err());
    }

    #[test]
    fn test_parse_missing_op() {
        assert!(parse_request("{:foo :bar}").is_err());
    }

    #[test]
    fn test_parse_op_not_keyword() {
        assert!(parse_request(r#"{:op "list-dbs"}"#).is_err());
    }
}

// ============================================================================
// Section 2: Protocol unit tests (no database required)
// ============================================================================

mod protocol_unit_tests {
    use mentatd::protocol::datomic_client::{
        format_connect_response, format_db_response, format_error_response,
        format_success_response, is_valid_operation, normalize_op_keyword,
    };
    use mentatd::protocol::{AnomalyCategory, ResponseValue};

    // -- Operation normalization --

    #[test]
    fn test_normalize_new_protocol_ops() {
        assert_eq!(normalize_op_keyword("datomic.client.protocol/qseq"), "qseq");
        assert_eq!(
            normalize_op_keyword("datomic.client.protocol/pull-many"),
            "pull-many"
        );
        assert_eq!(
            normalize_op_keyword("datomic.client.protocol/index-range"),
            "index-range"
        );
        assert_eq!(
            normalize_op_keyword("datomic.client.protocol/entid"),
            "entid"
        );
        assert_eq!(
            normalize_op_keyword("datomic.client.protocol/ident"),
            "ident"
        );
        assert_eq!(
            normalize_op_keyword("datomic.client.protocol/db-stats"),
            "db-stats"
        );
    }

    #[test]
    fn test_normalize_short_forms_pass_through() {
        let ops = [
            "q", "qseq", "pull", "pull-many", "transact", "with", "datoms",
            "index-range", "tx-range", "entid", "ident", "db-stats", "connect",
            "db", "list-dbs", "create-db", "delete-db", "as-of", "since",
            "history", "filter", "basis-t", "db-snapshot",
        ];
        for op in &ops {
            assert_eq!(
                normalize_op_keyword(op),
                *op,
                "Short form '{}' should normalize to itself",
                op
            );
        }
    }

    #[test]
    fn test_normalize_catalog_aliases() {
        assert_eq!(normalize_op_keyword("list-databases"), "list-dbs");
        assert_eq!(normalize_op_keyword("create-database"), "create-db");
        assert_eq!(normalize_op_keyword("delete-database"), "delete-db");
    }

    #[test]
    fn test_normalize_unknown_passes_through() {
        assert_eq!(normalize_op_keyword("unknown-op"), "unknown-op");
        assert_eq!(normalize_op_keyword(""), "");
    }

    // -- Operation validation --

    #[test]
    fn test_new_operations_are_valid() {
        for op in &["qseq", "pull-many", "index-range", "entid", "ident", "db-stats"] {
            assert!(
                is_valid_operation(op),
                "'{}' should be valid",
                op
            );
        }
    }

    #[test]
    fn test_namespaced_new_operations_are_valid() {
        for op in &[
            "datomic.client.protocol/qseq",
            "datomic.client.protocol/pull-many",
            "datomic.client.protocol/index-range",
            "datomic.client.protocol/entid",
            "datomic.client.protocol/ident",
            "datomic.client.protocol/db-stats",
        ] {
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
    }

    // -- Response formatting --

    #[test]
    fn test_connect_response_has_datomic_type() {
        let resp = format_connect_response("my-db", "conn-uuid", 42);
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
    fn test_connect_response_contains_all_fields() {
        let resp = format_connect_response("test-db", "uuid-123", 1000);
        match resp {
            ResponseValue::Map(entries) => {
                let keys: Vec<&str> = entries
                    .iter()
                    .filter_map(|(k, _)| match k {
                        ResponseValue::Keyword(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .collect();
                assert!(keys.contains(&"db-name"), "Missing db-name");
                assert!(keys.contains(&"database-id"), "Missing database-id");
                assert!(keys.contains(&"t"), "Missing t");
                assert!(keys.contains(&"next-t"), "Missing next-t");
                assert!(keys.contains(&"type"), "Missing type");
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_db_response_has_datomic_db_type() {
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
    fn test_next_t_is_basis_plus_one() {
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
        let resp = format_error_response(AnomalyCategory::NotFound, "not found".to_string());
        match resp {
            mentatd::protocol::Response::Error { anomaly } => {
                assert!(matches!(anomaly.category, AnomalyCategory::NotFound));
                assert_eq!(anomaly.message, "not found");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_all_anomaly_category_keywords() {
        let categories = [
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
        for (cat, expected) in &categories {
            assert_eq!(cat.as_keyword(), *expected);
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
}

// ============================================================================
// Section 3: HTTP integration tests for new operations
// These test protocol format compliance. Without a database, the server returns
// Datomic-compatible anomaly errors. We validate the response uses correct
// protocol format (either :result or :error with cognitect.anomalies).
// ============================================================================

/// Helper: assert response uses valid Datomic protocol format (EDN)
fn assert_valid_datomic_edn_response(body: &str, op: &str) {
    assert!(
        body.contains(":result") || body.contains(":error"),
        "{} should return :result or :error in Datomic format, got: {}",
        op,
        body
    );
    // If it's an error, verify it uses cognitect.anomalies format
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
fn assert_valid_datomic_transit_response(body: &str, op: &str) {
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

// -- db-stats ---------------------------------------------------------------

#[tokio::test]
async fn test_http_db_stats_edn() {
    let server = TestServer::start().await;
    let response = server.client.post("/", "{:op :db-stats}").await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_edn_response(&response.body, "db-stats");
}

#[tokio::test]
async fn test_http_db_stats_transit_json() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json("/", r#"["^ ","~:op","~:db-stats"]"#)
        .await;
    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/transit+json")
    );
    assert_valid_datomic_transit_response(&response.body, "db-stats");
}

// -- basis-t ----------------------------------------------------------------

#[tokio::test]
async fn test_http_basis_t() {
    let server = TestServer::start().await;
    let response = server.client.post("/", "{:op :basis-t}").await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_edn_response(&response.body, "basis-t");
}

#[tokio::test]
async fn test_http_basis_t_transit_json() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json("/", r#"["^ ","~:op","~:basis-t"]"#)
        .await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_transit_response(&response.body, "basis-t");
}

// -- qseq -------------------------------------------------------------------

#[tokio::test]
async fn test_http_qseq_edn() {
    let server = TestServer::start().await;
    let request =
        r#"{:op :qseq :args {:query "[:find ?e :where [?e :db/ident _]]" :args [] :chunk-size 100}}"#;
    let response = server.client.post("/", request).await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_edn_response(&response.body, "qseq");
}

#[tokio::test]
async fn test_http_qseq_default_chunk_size() {
    let server = TestServer::start().await;
    let request =
        r#"{:op :qseq :args {:query "[:find ?e :where [?e :db/ident _]]" :args []}}"#;
    let response = server.client.post("/", request).await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_edn_response(&response.body, "qseq (no chunk-size)");
}

// -- pull-many --------------------------------------------------------------

#[tokio::test]
async fn test_http_pull_many_with_ids() {
    let server = TestServer::start().await;
    let request = r#"{:op :pull-many :args {:pattern "[*]" :entity-ids [1 2 3]}}"#;
    let response = server.client.post("/", request).await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_edn_response(&response.body, "pull-many");
}

#[tokio::test]
async fn test_http_pull_many_empty_ids() {
    let server = TestServer::start().await;
    let request = r#"{:op :pull-many :args {:pattern "[*]" :entity-ids []}}"#;
    let response = server.client.post("/", request).await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_edn_response(&response.body, "pull-many (empty ids)");
}

#[tokio::test]
async fn test_http_pull_many_transit_json() {
    let server = TestServer::start().await;
    // Transit+JSON: {:op :pull-many :args {:pattern "[*]" :entity-ids [1 2]}}
    let request = r#"["^ ","~:op","~:pull-many","~:args",["^ ","~:pattern","[*]","~:entity-ids",[1,2]]]"#;
    let response = server.client.post_transit_json("/", request).await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_transit_response(&response.body, "pull-many");
}

// -- entid ------------------------------------------------------------------

#[tokio::test]
async fn test_http_entid_edn() {
    let server = TestServer::start().await;
    let request = r#"{:op :entid :args {:ident ":db/ident"}}"#;
    let response = server.client.post("/", request).await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_edn_response(&response.body, "entid");
}

// -- ident ------------------------------------------------------------------

#[tokio::test]
async fn test_http_ident_edn() {
    let server = TestServer::start().await;
    let request = r#"{:op :ident :args {:entid 1}}"#;
    let response = server.client.post("/", request).await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_edn_response(&response.body, "ident");
}

// -- Datomic namespaced operations via HTTP ---------------------------------

#[tokio::test]
async fn test_http_datomic_catalog_list_dbs() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post("/", r#"{:op :datomic.catalog/list-dbs}"#)
        .await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_edn_response(&response.body, "datomic.catalog/list-dbs");
}

#[tokio::test]
async fn test_http_datomic_namespace_transit_json() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json("/", r#"["^ ","~:op","~:datomic.catalog/list-dbs"]"#)
        .await;
    assert_eq!(response.status, 200);
    assert_valid_datomic_transit_response(&response.body, "datomic.catalog/list-dbs");
}

// -- Error conditions -------------------------------------------------------

#[tokio::test]
async fn test_http_invalid_operation() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post("/", r#"{:op :nonexistent-operation}"#)
        .await;
    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
    assert!(response.body.contains(":cognitect.anomalies/category"));
}

#[tokio::test]
async fn test_http_missing_op_field() {
    let server = TestServer::start().await;
    let response = server.client.post("/", r#"{:foo :bar}"#).await;
    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
}

#[tokio::test]
async fn test_http_invalid_edn() {
    let server = TestServer::start().await;
    let response = server.client.post("/", "not valid edn at all").await;
    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
}

#[tokio::test]
async fn test_http_empty_request() {
    let server = TestServer::start().await;
    let response = server.client.post("/", "").await;
    assert_eq!(response.status, 200);
    assert!(response.body.contains(":error"));
}

#[tokio::test]
async fn test_http_transit_json_invalid_operation() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json("/", r#"["^ ","~:op","~:nonexistent"]"#)
        .await;
    assert_eq!(response.status, 200);
    assert!(
        response.body.contains("error"),
        "Invalid transit op should error: {}",
        response.body
    );
}

#[tokio::test]
async fn test_http_transit_json_malformed() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json("/", "this is not valid transit json")
        .await;
    assert_eq!(response.status, 200);
    assert!(
        response.body.contains("error"),
        "Malformed transit should error: {}",
        response.body
    );
}

// -- Content-type negotiation -----------------------------------------------

#[tokio::test]
async fn test_edn_content_type_header() {
    let server = TestServer::start().await;
    let response = server.client.post("/", "{:op :health}").await;
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type.as_deref(), Some("application/edn"));
}

#[tokio::test]
async fn test_transit_json_content_type_header() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json("/", r#"["^ ","~:op","~:health"]"#)
        .await;
    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/transit+json")
    );
}

// -- Concurrent requests ----------------------------------------------------

#[tokio::test]
async fn test_concurrent_http_requests() {
    let server = TestServer::start().await;

    let mut handles = Vec::new();
    for _ in 0..10 {
        let client = server.client.clone();
        let handle = tokio::spawn(async move {
            client.post("/", "{:op :health}").await
        });
        handles.push(handle);
    }

    for handle in handles {
        let response = handle.await.unwrap();
        assert_eq!(response.status, 200);
        assert!(response.body.contains("healthy"));
    }
}
