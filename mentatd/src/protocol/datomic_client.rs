//! Datomic Client API protocol mapping.
//!
//! This module implements the exact Datomic Client API request/response protocol
//! used by Datomic's peer-server and client library. The Datomic Client API uses
//! Transit-encoded messages over HTTP or WebSocket connections.
//!
//! ## Protocol overview
//!
//! The Datomic Client API uses a request-response protocol where each request is
//! a Transit map containing:
//! - `:op` - The operation keyword (e.g., `:q`, `:transact`, `:pull`)
//! - `:args` - Operation-specific arguments as a Transit map
//!
//! Responses are Transit maps containing either:
//! - `:result` - The operation result on success
//! - `:cognitect.anomalies/category` + `:cognitect.anomalies/message` on error
//!
//! ## Datomic Client API operations
//!
//! The following operations are defined by the Datomic Client API:
//!
//! ### Catalog operations
//! - `datomic.catalog/list-dbs` - List available databases
//! - `datomic.catalog/create-db` - Create a new database
//! - `datomic.catalog/delete-db` - Delete a database
//!
//! ### Connection operations
//! - `datomic.client.protocol/connect` - Connect to a database
//! - `datomic.client.protocol/db` - Get current database value
//!
//! ### Query operations
//! - `datomic.client.protocol/q` - Execute a Datalog query
//! - `datomic.client.protocol/qseq` - Execute a query returning a lazy seq
//! - `datomic.client.protocol/pull` - Pull entity attributes
//! - `datomic.client.protocol/pull-many` - Pull attributes for multiple entities
//!
//! ### Transaction operations
//! - `datomic.client.protocol/transact` - Execute a transaction
//! - `datomic.client.protocol/with` - Speculative transaction
//!
//! ### Index operations
//! - `datomic.client.protocol/datoms` - Access raw datoms by index
//! - `datomic.client.protocol/index-range` - Range scan on AVET index
//!
//! ### Time operations
//! - `datomic.client.protocol/tx-range` - Query transaction log

use super::{Anomaly, AnomalyCategory, Operation, Response, ResponseValue};

/// A Datomic Client API protocol message.
///
/// This wraps the internal `Operation` with additional protocol-level metadata
/// that the Datomic Client API includes in its wire format.
#[derive(Debug, Clone)]
pub struct ClientMessage {
    /// The operation to execute.
    pub op: Operation,
    /// Optional session/connection context.
    pub session_id: Option<String>,
    /// Request identifier for correlation (WebSocket multiplexing).
    pub request_id: Option<String>,
}

/// Map a Datomic Client API fully-qualified operation name to the internal
/// operation keyword used by the existing parser.
///
/// The Datomic Client API uses namespaced keywords like
/// `datomic.client.protocol/q` while our internal protocol uses short forms
/// like `q`. This function normalizes both forms.
pub fn normalize_op_keyword(op: &str) -> &str {
    match op {
        // Catalog operations
        "datomic.catalog/list-dbs" | "list-dbs" | "list-databases" => "list-dbs",
        "datomic.catalog/create-db" | "create-db" | "create-database" => "create-db",
        "datomic.catalog/delete-db" | "delete-db" | "delete-database" => "delete-db",

        // Connection operations
        "datomic.client.protocol/connect" | "connect" => "connect",
        "datomic.client.protocol/db" | "db" => "db",

        // Query operations
        "datomic.client.protocol/q" | "q" => "q",
        "datomic.client.protocol/qseq" | "qseq" => "qseq",
        "datomic.client.protocol/pull" | "pull" => "pull",
        "datomic.client.protocol/pull-many" | "pull-many" => "pull-many",

        // Transaction operations
        "datomic.client.protocol/transact" | "transact" => "transact",
        "datomic.client.protocol/with" | "with" => "with",

        // Index operations
        "datomic.client.protocol/datoms" | "datoms" => "datoms",
        "datomic.client.protocol/index-range" | "index-range" => "index-range",

        // Entity resolution
        "datomic.client.protocol/entid" | "entid" => "entid",
        "datomic.client.protocol/ident" | "ident" => "ident",

        // Database statistics
        "datomic.client.protocol/db-stats" | "db-stats" => "db-stats",

        // Time operations
        "datomic.client.protocol/tx-range" | "tx-range" => "tx-range",

        // Database snapshot
        "datomic.client.protocol/db-snapshot" | "db-snapshot" => "db-snapshot",

        // Basis-t
        "datomic.client.protocol/basis-t" | "basis-t" => "basis-t",

        // Time-travel
        "datomic.client.protocol/as-of" | "as-of" => "as-of",
        "datomic.client.protocol/since" | "since" => "since",
        "datomic.client.protocol/history" | "history" => "history",

        // Filter
        "datomic.client.protocol/filter" | "filter" => "filter",

        // Pass through unknown operations
        other => other,
    }
}

/// Format a success response in the Datomic Client API protocol format.
///
/// Datomic Client API success responses are Transit maps:
/// ```edn
/// {:result <value>}
/// ```
pub fn format_success_response(result: ResponseValue) -> Response {
    Response::Success { result }
}

/// Format an error response in the Datomic Client API protocol format.
///
/// Datomic Client API error responses use the cognitect.anomalies format:
/// ```edn
/// {:cognitect.anomalies/category :cognitect.anomalies/<category>
///  :cognitect.anomalies/message "<message>"
///  :db/error :<error-code>}
/// ```
pub fn format_error_response(category: AnomalyCategory, message: String) -> Response {
    Response::Error {
        anomaly: Anomaly {
            category,
            message,
            db_error: None,
        },
    }
}

/// Format a connection response matching Datomic's connect result.
///
/// Datomic returns:
/// ```edn
/// {:db-name "my-db"
///  :database-id "<uuid>"
///  :t <basis-t>
///  :next-t <basis-t+1>
///  :type :datomic.client/connection}
/// ```
pub fn format_connect_response(
    db_name: &str,
    connection_id: &str,
    basis_t: i64,
) -> ResponseValue {
    ResponseValue::Map(vec![
        (
            ResponseValue::Keyword("db-name".to_string()),
            ResponseValue::String(db_name.to_string()),
        ),
        (
            ResponseValue::Keyword("database-id".to_string()),
            ResponseValue::String(connection_id.to_string()),
        ),
        (
            ResponseValue::Keyword("t".to_string()),
            ResponseValue::Integer(basis_t),
        ),
        (
            ResponseValue::Keyword("next-t".to_string()),
            ResponseValue::Integer(basis_t + 1),
        ),
        (
            ResponseValue::Keyword("type".to_string()),
            ResponseValue::Keyword("datomic.client/connection".to_string()),
        ),
    ])
}

/// Format a database value response matching Datomic's db result.
///
/// Datomic returns:
/// ```edn
/// {:db-name "my-db"
///  :database-id "<uuid>"
///  :t <basis-t>
///  :next-t <basis-t+1>
///  :as-of-t <t>
///  :type :datomic.client/db}
/// ```
pub fn format_db_response(db_name: &str, database_id: &str, basis_t: i64) -> ResponseValue {
    ResponseValue::Map(vec![
        (
            ResponseValue::Keyword("db-name".to_string()),
            ResponseValue::String(db_name.to_string()),
        ),
        (
            ResponseValue::Keyword("database-id".to_string()),
            ResponseValue::String(database_id.to_string()),
        ),
        (
            ResponseValue::Keyword("t".to_string()),
            ResponseValue::Integer(basis_t),
        ),
        (
            ResponseValue::Keyword("next-t".to_string()),
            ResponseValue::Integer(basis_t + 1),
        ),
        (
            ResponseValue::Keyword("type".to_string()),
            ResponseValue::Keyword("datomic.client/db".to_string()),
        ),
    ])
}

/// Check whether an operation keyword is a valid Datomic Client API operation.
pub fn is_valid_operation(op: &str) -> bool {
    matches!(
        normalize_op_keyword(op),
        "list-dbs"
            | "create-db"
            | "delete-db"
            | "connect"
            | "db"
            | "q"
            | "qseq"
            | "pull"
            | "pull-many"
            | "transact"
            | "with"
            | "datoms"
            | "index-range"
            | "tx-range"
            | "db-snapshot"
            | "basis-t"
            | "as-of"
            | "since"
            | "history"
            | "filter"
            | "entid"
            | "ident"
            | "db-stats"
            | "health"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_catalog_ops() {
        assert_eq!(normalize_op_keyword("datomic.catalog/list-dbs"), "list-dbs");
        assert_eq!(normalize_op_keyword("list-dbs"), "list-dbs");
        assert_eq!(
            normalize_op_keyword("datomic.catalog/create-db"),
            "create-db"
        );
        assert_eq!(
            normalize_op_keyword("datomic.catalog/delete-db"),
            "delete-db"
        );
    }

    #[test]
    fn test_normalize_protocol_ops() {
        assert_eq!(normalize_op_keyword("datomic.client.protocol/q"), "q");
        assert_eq!(normalize_op_keyword("q"), "q");
        assert_eq!(
            normalize_op_keyword("datomic.client.protocol/transact"),
            "transact"
        );
        assert_eq!(
            normalize_op_keyword("datomic.client.protocol/pull"),
            "pull"
        );
        assert_eq!(
            normalize_op_keyword("datomic.client.protocol/datoms"),
            "datoms"
        );
    }

    #[test]
    fn test_normalize_unknown_passes_through() {
        assert_eq!(normalize_op_keyword("unknown-op"), "unknown-op");
    }

    #[test]
    fn test_is_valid_operation() {
        assert!(is_valid_operation("q"));
        assert!(is_valid_operation("datomic.client.protocol/q"));
        assert!(is_valid_operation("transact"));
        assert!(is_valid_operation("pull"));
        assert!(is_valid_operation("datoms"));
        assert!(is_valid_operation("list-dbs"));
        assert!(!is_valid_operation("unknown-op"));
    }

    #[test]
    fn test_format_connect_response() {
        let resp = format_connect_response("test-db", "conn-123", 1000);
        match resp {
            ResponseValue::Map(entries) => {
                assert!(entries.iter().any(|(k, v)| matches!(
                    (k, v),
                    (ResponseValue::Keyword(k), ResponseValue::String(v))
                    if k == "db-name" && v == "test-db"
                )));
                assert!(entries.iter().any(|(k, v)| matches!(
                    (k, v),
                    (ResponseValue::Keyword(k), ResponseValue::Integer(1000))
                    if k == "t"
                )));
                assert!(entries.iter().any(|(k, v)| matches!(
                    (k, v),
                    (ResponseValue::Keyword(k), ResponseValue::Keyword(v))
                    if k == "type" && v == "datomic.client/connection"
                )));
            }
            other => panic!("Expected Map, got {other:?}"),
        }
    }

    #[test]
    fn test_format_db_response() {
        let resp = format_db_response("test-db", "db-uuid-123", 500);
        match resp {
            ResponseValue::Map(entries) => {
                assert!(entries.iter().any(|(k, v)| matches!(
                    (k, v),
                    (ResponseValue::Keyword(k), ResponseValue::Keyword(v))
                    if k == "type" && v == "datomic.client/db"
                )));
            }
            other => panic!("Expected Map, got {other:?}"),
        }
    }

    #[test]
    fn test_format_success_response() {
        let resp = format_success_response(ResponseValue::Integer(42));
        match resp {
            Response::Success {
                result: ResponseValue::Integer(42),
            } => {}
            other => panic!("Expected Success(42), got {other:?}"),
        }
    }

    #[test]
    fn test_format_error_response() {
        let resp = format_error_response(AnomalyCategory::NotFound, "not found".to_string());
        match resp {
            Response::Error { anomaly } => {
                assert!(matches!(anomaly.category, AnomalyCategory::NotFound));
                assert_eq!(anomaly.message, "not found");
            }
            other => panic!("Expected Error, got {other:?}"),
        }
    }
}
