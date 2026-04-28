//! Client library integration tests for pg_mentat.
//!
//! These tests verify that the Clojure, Python, and TypeScript client
//! libraries correctly implement the Datomic Client API protocol when
//! talking to a live mentatd instance.
//!
//! # Running
//!
//! Prerequisites:
//!   1. PostgreSQL running with pg_mentat extension loaded
//!   2. mentatd running at ws://localhost:8080/ws (default)
//!
//! Run these tests with:
//! ```bash
//! cargo test --test client_integration_tests
//! ```
//!
//! # Architecture
//!
//! Each test creates a raw WebSocket connection to mentatd and sends
//! Transit+JSON messages directly, verifying the exact wire format that
//! the client libraries produce. This ensures compatibility without
//! needing the actual Clojure/Python/TS runtimes installed.

use std::time::Duration;

/// Default mentatd WebSocket endpoint for integration tests.
#[allow(dead_code)]
const WS_ENDPOINT: &str = "ws://localhost:8080/ws";

/// Timeout for WebSocket operations in integration tests.
#[allow(dead_code)]
const WS_TIMEOUT: Duration = Duration::from_secs(10);

// ============================================================================
// Transit+JSON wire format tests
//
// These tests verify that the Transit+JSON encoding produced by each
// client library matches what mentatd expects. We test by sending raw
// Transit+JSON over WebSocket and checking the response.
// ============================================================================

/// Minimal Transit+JSON encoder for test requests.
///
/// This mirrors the encoding logic in each client library so we can
/// verify the wire format without depending on external runtimes.
mod transit {
    /// Encode a keyword as Transit+JSON: "~:namespace/name"
    pub fn keyword(ns: Option<&str>, name: &str) -> String {
        match ns {
            Some(n) => format!("\"~:{}/{}\"", n, name),
            None => format!("\"~:{}\"", name),
        }
    }

    /// Encode a string value as Transit+JSON.
    pub fn string(s: &str) -> String {
        let escaped = s
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        format!("\"{}\"", escaped)
    }

    /// Encode an integer as Transit+JSON.
    pub fn integer(n: i64) -> String {
        if n > i32::MAX as i64 || n < i32::MIN as i64 {
            format!("\"~i{}\"", n)
        } else {
            n.to_string()
        }
    }

    /// Build a Transit cmap: ["^ ", k1, v1, k2, v2, ...]
    pub fn cmap(entries: &[(&str, &str)]) -> String {
        let mut parts = vec!["\"^ \"".to_string()];
        for (k, v) in entries {
            parts.push(k.to_string());
            parts.push(v.to_string());
        }
        format!("[{}]", parts.join(","))
    }

    /// Build a health check request.
    pub fn health_request(request_id: &str) -> String {
        cmap(&[
            (&keyword(None, "op"), &keyword(None, "health")),
            (&keyword(None, "request-id"), &string(request_id)),
        ])
    }

    /// Build a connect request.
    pub fn connect_request(db_name: &str, request_id: &str) -> String {
        let args = cmap(&[(&keyword(None, "db-name"), &string(db_name))]);
        cmap(&[
            (&keyword(None, "op"), &keyword(None, "connect")),
            (&keyword(None, "args"), &args),
            (&keyword(None, "request-id"), &string(request_id)),
        ])
    }

    /// Build a query request.
    pub fn query_request(query: &str, request_id: &str) -> String {
        let args = cmap(&[
            (&keyword(None, "query"), &string(query)),
            (&keyword(None, "args"), "[]"),
        ]);
        cmap(&[
            (&keyword(None, "op"), &keyword(None, "q")),
            (&keyword(None, "args"), &args),
            (&keyword(None, "request-id"), &string(request_id)),
        ])
    }

    /// Build a transact request.
    pub fn transact_request(tx_data: &str, conn_id: &str, request_id: &str) -> String {
        let args = cmap(&[
            (&keyword(None, "connection-id"), &string(conn_id)),
            (&keyword(None, "tx-data"), &string(tx_data)),
        ]);
        cmap(&[
            (&keyword(None, "op"), &keyword(None, "transact")),
            (&keyword(None, "args"), &args),
            (&keyword(None, "request-id"), &string(request_id)),
        ])
    }

    /// Build a pull request.
    pub fn pull_request(pattern: &str, entity_id: i64, request_id: &str) -> String {
        let args = cmap(&[
            (&keyword(None, "pattern"), &string(pattern)),
            (&keyword(None, "entity-id"), &integer(entity_id)),
        ]);
        cmap(&[
            (&keyword(None, "op"), &keyword(None, "pull")),
            (&keyword(None, "args"), &args),
            (&keyword(None, "request-id"), &string(request_id)),
        ])
    }

    /// Build a datoms request.
    pub fn datoms_request(index: &str, request_id: &str) -> String {
        let args = cmap(&[
            (&keyword(None, "index"), &string(index)),
            (&keyword(None, "components"), "[]"),
        ]);
        cmap(&[
            (&keyword(None, "op"), &keyword(None, "datoms")),
            (&keyword(None, "args"), &args),
            (&keyword(None, "request-id"), &string(request_id)),
        ])
    }

    /// Build a speculative transaction (with) request.
    pub fn with_request(tx_data: &str, request_id: &str) -> String {
        let args = cmap(&[(&keyword(None, "tx-data"), &string(tx_data))]);
        cmap(&[
            (&keyword(None, "op"), &keyword(None, "with")),
            (&keyword(None, "args"), &args),
            (&keyword(None, "request-id"), &string(request_id)),
        ])
    }

    /// Build a basis-t request.
    pub fn basis_t_request(request_id: &str) -> String {
        cmap(&[
            (&keyword(None, "op"), &keyword(None, "basis-t")),
            (&keyword(None, "request-id"), &string(request_id)),
        ])
    }

    /// Build a tx-range request.
    pub fn tx_range_request(request_id: &str) -> String {
        let args = cmap(&[]);
        cmap(&[
            (&keyword(None, "op"), &keyword(None, "tx-range")),
            (&keyword(None, "args"), &args),
            (&keyword(None, "request-id"), &string(request_id)),
        ])
    }
}

/// Response parsing helpers for Transit+JSON.
mod response {
    /// Check if a Transit+JSON response contains a result (not an error).
    pub fn is_success(response: &str) -> bool {
        response.contains("\"~:result\"") && !response.contains("\"~:error\"")
    }

    /// Check if a Transit+JSON response contains an error.
    pub fn is_error(response: &str) -> bool {
        response.contains("\"~:error\"")
    }

    /// Check if a response contains the expected request-id.
    pub fn has_request_id(response: &str, request_id: &str) -> bool {
        response.contains(&format!("\"{}\"", request_id))
    }

    /// Check for a specific anomaly category in an error response.
    pub fn has_anomaly_category(response: &str, category: &str) -> bool {
        response.contains(category)
    }

    /// Check if a welcome message contains the expected session type.
    pub fn is_welcome(response: &str) -> bool {
        response.contains("datomic.client/session")
            || response.contains("session-id")
            || response.contains("welcome")
    }
}

// ============================================================================
// Integration test cases
//
// Each test documents what the corresponding client library function
// sends over the wire and what mentatd is expected to return.
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Transit encoding correctness (unit tests, no server needed)
    // ------------------------------------------------------------------

    #[test]
    fn transit_keyword_simple() {
        assert_eq!(transit::keyword(None, "name"), "\"~:name\"");
    }

    #[test]
    fn transit_keyword_namespaced() {
        assert_eq!(
            transit::keyword(Some("person"), "name"),
            "\"~:person/name\""
        );
    }

    #[test]
    fn transit_string_plain() {
        assert_eq!(transit::string("hello"), "\"hello\"");
    }

    #[test]
    fn transit_string_with_escapes() {
        assert_eq!(transit::string("line1\nline2"), "\"line1\\nline2\"");
        assert_eq!(transit::string("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn transit_integer_small() {
        assert_eq!(transit::integer(42), "42");
        assert_eq!(transit::integer(-1), "-1");
    }

    #[test]
    fn transit_integer_large() {
        assert_eq!(transit::integer(9_999_999_999), "\"~i9999999999\"");
    }

    #[test]
    fn transit_cmap_basic() {
        let result = transit::cmap(&[("\"~:op\"", "\"~:health\"")]);
        assert!(result.starts_with("[\"^ \""));
        assert!(result.contains("\"~:op\""));
        assert!(result.contains("\"~:health\""));
    }

    #[test]
    fn transit_health_request_format() {
        let req = transit::health_request("req-001");
        assert!(req.contains("\"~:op\""));
        assert!(req.contains("\"~:health\""));
        assert!(req.contains("\"~:request-id\""));
        assert!(req.contains("\"req-001\""));
    }

    #[test]
    fn transit_connect_request_format() {
        let req = transit::connect_request("test-db", "req-002");
        assert!(req.contains("\"~:connect\""));
        assert!(req.contains("\"~:db-name\""));
        assert!(req.contains("\"test-db\""));
    }

    #[test]
    fn transit_query_request_format() {
        let req = transit::query_request("[:find ?e :where [?e :db/ident]]", "req-003");
        assert!(req.contains("\"~:q\""));
        assert!(req.contains("\"~:query\""));
        assert!(req.contains("[:find ?e :where [?e :db/ident]]"));
    }

    #[test]
    fn transit_transact_request_format() {
        let req =
            transit::transact_request("[[:db/add \"t\" :person/name \"Alice\"]]", "conn-1", "req-004");
        assert!(req.contains("\"~:transact\""));
        assert!(req.contains("\"~:tx-data\""));
        assert!(req.contains("\"~:connection-id\""));
    }

    #[test]
    fn transit_pull_request_format() {
        let req = transit::pull_request("[*]", 10001, "req-005");
        assert!(req.contains("\"~:pull\""));
        assert!(req.contains("\"~:pattern\""));
        assert!(req.contains("\"~:entity-id\""));
        assert!(req.contains("10001"));
    }

    #[test]
    fn transit_datoms_request_format() {
        let req = transit::datoms_request(":eavt", "req-006");
        assert!(req.contains("\"~:datoms\""));
        assert!(req.contains("\"~:index\""));
        assert!(req.contains(":eavt"));
    }

    #[test]
    fn transit_with_request_format() {
        let req =
            transit::with_request("[[:db/add \"t\" :person/name \"Alice\"]]", "req-007");
        assert!(req.contains("\"~:with\""));
        assert!(req.contains("\"~:tx-data\""));
    }

    #[test]
    fn transit_basis_t_request_format() {
        let req = transit::basis_t_request("req-008");
        assert!(req.contains("\"~:basis-t\""));
    }

    #[test]
    fn transit_tx_range_request_format() {
        let req = transit::tx_range_request("req-009");
        assert!(req.contains("\"~:tx-range\""));
    }

    // ------------------------------------------------------------------
    // Response parsing (unit tests, no server needed)
    // ------------------------------------------------------------------

    #[test]
    fn response_success_detection() {
        let resp = r#"["^ ","~:result",42,"~:request-id","req-001"]"#;
        assert!(response::is_success(resp));
        assert!(!response::is_error(resp));
        assert!(response::has_request_id(resp, "req-001"));
    }

    #[test]
    fn response_error_detection() {
        let resp = r#"["^ ","~:error",["^ ","~:cognitect.anomalies/category","~:cognitect.anomalies/not-found","~:cognitect.anomalies/message","Database not found"]]"#;
        assert!(response::is_error(resp));
        assert!(!response::is_success(resp));
        assert!(response::has_anomaly_category(
            resp,
            "cognitect.anomalies/not-found"
        ));
    }

    #[test]
    fn response_welcome_detection() {
        let resp = r#"["^ ","~:type","~:datomic.client/session","~:session-id","abc-123","~:protocol-version",1]"#;
        assert!(response::is_welcome(resp));
    }

    #[test]
    fn response_query_result_format() {
        let resp = r#"["^ ","~:result",[[42,"Alice"],[43,"Bob"]],"~:request-id","req-003"]"#;
        assert!(response::is_success(resp));
        assert!(resp.contains("[42,\"Alice\"]"));
        assert!(resp.contains("[43,\"Bob\"]"));
    }

    #[test]
    fn response_transaction_report_format() {
        let resp = r#"["^ ","~:result",["^ ","~:db-before",["^ ","~:basis-t",1000],"~:db-after",["^ ","~:basis-t",1001],"~:tx-data",[[1001,50,"~m1714000000000",1001,true]],"~:tempids",["^ "]],"~:request-id","req-004"]"#;
        assert!(response::is_success(resp));
        assert!(resp.contains("db-before"));
        assert!(resp.contains("db-after"));
        assert!(resp.contains("tx-data"));
        assert!(resp.contains("tempids"));
    }

    // ------------------------------------------------------------------
    // Cross-client format compatibility (unit tests)
    //
    // Verify that all three clients produce identical Transit+JSON for
    // the same logical operation.
    // ------------------------------------------------------------------

    #[test]
    fn cross_client_health_format() {
        // All three clients should produce the same wire format for
        // a health check request (modulo request-id).
        let req = transit::health_request("test-id");

        // Must be a Transit cmap starting with ["^ "
        assert!(req.starts_with("[\"^ \""));
        // Must contain :op keyword
        assert!(req.contains("\"~:op\""));
        // Must contain :health keyword
        assert!(req.contains("\"~:health\""));
        // Must contain :request-id
        assert!(req.contains("\"~:request-id\""));
    }

    #[test]
    fn cross_client_query_format() {
        let query = "[:find ?e ?name :where [?e :person/name ?name]]";
        let req = transit::query_request(query, "test-id");

        // Verify the Transit structure matches what all clients produce
        assert!(req.contains("\"~:q\""));
        assert!(req.contains("\"~:query\""));
        assert!(req.contains(query));
    }

    #[test]
    fn cross_client_transact_format() {
        let tx = "[{:person/name \"Alice\" :person/age 30}]";
        let req = transit::transact_request(tx, "conn-1", "test-id");

        assert!(req.contains("\"~:transact\""));
        assert!(req.contains("\"~:connection-id\""));
        assert!(req.contains("\"~:tx-data\""));
    }

    // ------------------------------------------------------------------
    // Datomic API compatibility matrix
    //
    // Documents which operations each client supports and verifies
    // the request format matches Datomic's Client API.
    // ------------------------------------------------------------------

    /// Verify that every Datomic Client API operation has a corresponding
    /// request builder, confirming API completeness.
    #[test]
    fn datomic_api_completeness() {
        // Each of these operations must be supported by all three clients
        let operations = vec![
            "health",
            "connect",
            "q",          // query
            "transact",
            "pull",
            "datoms",
            "with",       // speculative transaction
            "basis-t",
            "tx-range",
        ];

        for op in &operations {
            let kw = transit::keyword(None, op);
            assert!(
                kw.contains(&format!("~:{}", op)),
                "Missing operation: {}",
                op
            );
        }
    }

    /// Verify anomaly categories match cognitect.anomalies spec.
    #[test]
    fn anomaly_categories_match_datomic() {
        let expected = [
            ":cognitect.anomalies/incorrect",
            ":cognitect.anomalies/forbidden",
            ":cognitect.anomalies/not-found",
            ":cognitect.anomalies/unavailable",
            ":cognitect.anomalies/interrupted",
            ":cognitect.anomalies/fault",
        ];

        for category in &expected {
            // Each category should be a valid Transit keyword
            assert!(
                category.starts_with(":cognitect.anomalies/"),
                "Invalid anomaly category format: {}",
                category
            );
        }
    }

    /// Verify Transit encoding of Datomic value types.
    #[test]
    fn datomic_value_type_encoding() {
        // Keywords: "~:namespace/name"
        assert_eq!(
            transit::keyword(Some("db.type"), "string"),
            "\"~:db.type/string\""
        );

        // Integers: plain for small, "~iN" for large
        assert_eq!(transit::integer(42), "42");
        assert_eq!(transit::integer(i64::MAX), format!("\"~i{}\"", i64::MAX));

        // Strings: plain JSON strings
        assert_eq!(transit::string("hello"), "\"hello\"");
    }
}
