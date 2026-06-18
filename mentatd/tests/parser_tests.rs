// Comprehensive parser tests for mentatd protocol parser.
//
// Tests cover:
// 1. All operation types parsing
// 2. Missing required fields
// 3. Invalid operation names
// 4. Invalid EDN input
// 5. Optional field handling
// 6. Edge cases in field values

use mentatd::protocol::parser::{parse_request, ParseError};
use mentatd::protocol::Operation;

// ============================================================================
// 1. List Databases
// ============================================================================

#[test]
fn test_parse_list_dbs() {
    let req = parse_request("{:op :list-dbs}").expect("parse failed");
    assert!(matches!(req.op, Operation::ListDatabases));
}

#[test]
fn test_parse_list_dbs_catalog_form() {
    let req = parse_request("{:op :datomic.catalog/list-dbs}").expect("parse failed");
    assert!(matches!(req.op, Operation::ListDatabases));
}

// ============================================================================
// 2. Create Database
// ============================================================================

#[test]
fn test_parse_create_db() {
    let req = parse_request("{:op :create-db :args {:db-name \"mydb\"}}").expect("parse failed");
    match req.op {
        Operation::CreateDatabase { db_name } => assert_eq!(db_name, "mydb"),
        _ => panic!("Expected CreateDatabase"),
    }
}

#[test]
fn test_parse_create_db_catalog_form() {
    let req = parse_request("{:op :datomic.catalog/create-db :args {:db-name \"testdb\"}}")
        .expect("parse failed");
    match req.op {
        Operation::CreateDatabase { db_name } => assert_eq!(db_name, "testdb"),
        _ => panic!("Expected CreateDatabase"),
    }
}

#[test]
fn test_parse_create_db_missing_name() {
    let result = parse_request("{:op :create-db}");
    assert!(result.is_err());
}

// ============================================================================
// 3. Delete Database
// ============================================================================

#[test]
fn test_parse_delete_db() {
    let req = parse_request("{:op :delete-db :args {:db-name \"olddb\"}}").expect("parse failed");
    match req.op {
        Operation::DeleteDatabase { db_name } => assert_eq!(db_name, "olddb"),
        _ => panic!("Expected DeleteDatabase"),
    }
}

#[test]
fn test_parse_delete_db_missing_name() {
    let result = parse_request("{:op :delete-db}");
    assert!(result.is_err());
}

// ============================================================================
// 4. Connect
// ============================================================================

#[test]
fn test_parse_connect() {
    let req = parse_request("{:op :connect :args {:db-name \"mydb\"}}").expect("parse failed");
    match req.op {
        Operation::Connect { db_name } => assert_eq!(db_name, "mydb"),
        _ => panic!("Expected Connect"),
    }
}

// ============================================================================
// 5. Db
// ============================================================================

#[test]
fn test_parse_db() {
    let req =
        parse_request("{:op :db :args {:connection-id \"550e8400-e29b-41d4-a716-446655440000\"}}")
            .expect("parse failed");
    match req.op {
        Operation::Db { connection_id } => {
            assert_eq!(
                connection_id.to_string(),
                "550e8400-e29b-41d4-a716-446655440000"
            );
        }
        _ => panic!("Expected Db"),
    }
}

#[test]
fn test_parse_db_invalid_uuid() {
    let result = parse_request("{:op :db :connection-id \"not-a-uuid\"}");
    assert!(result.is_err());
}

// ============================================================================
// 6. Query
// ============================================================================

#[test]
fn test_parse_query() {
    let req = parse_request("{:op :q :args {:query [:find ?e :where [?e :db/ident _]]}}")
        .expect("parse failed");
    match req.op {
        Operation::Query {
            query,
            args,
            timeout,
            limit,
            offset,
            db_id,
        } => {
            assert!(query.contains(":find"));
            assert!(args.is_empty());
            assert!(timeout.is_none());
            assert!(limit.is_none());
            assert!(offset.is_none());
            assert!(db_id.is_none());
        }
        _ => panic!("Expected Query"),
    }
}

#[test]
fn test_parse_query_with_args() {
    let req = parse_request(
        "{:op :q :args {:query [:find ?e :in ?name :where [?e :person/name ?name]] :args [\"Alice\"]}}",
    )
    .expect("parse failed");
    match req.op {
        Operation::Query { args, .. } => {
            assert_eq!(args.len(), 1);
        }
        _ => panic!("Expected Query"),
    }
}

#[test]
fn test_parse_query_with_limit_offset() {
    let req = parse_request(
        "{:op :q :args {:query [:find ?e :where [?e :db/ident _]] :limit 10 :offset 5}}",
    )
    .expect("parse failed");
    match req.op {
        Operation::Query { limit, offset, .. } => {
            assert_eq!(limit, Some(10));
            assert_eq!(offset, Some(5));
        }
        _ => panic!("Expected Query"),
    }
}

#[test]
fn test_parse_query_with_timeout() {
    let req =
        parse_request("{:op :q :args {:query [:find ?e :where [?e :db/ident _]] :timeout 5000}}")
            .expect("parse failed");
    match req.op {
        Operation::Query { timeout, .. } => {
            assert_eq!(timeout, Some(5000));
        }
        _ => panic!("Expected Query"),
    }
}

#[test]
fn test_parse_query_missing_query() {
    let result = parse_request("{:op :q :args {}}");
    assert!(result.is_err());
}

// ============================================================================
// 7. Transact
// ============================================================================

#[test]
fn test_parse_transact() {
    let req = parse_request(
        "{:op :transact :args {:connection-id \"abc\" :tx-data [[:db/add \"e\" :db/ident :test]]}}",
    )
    .expect("parse failed");
    match req.op {
        Operation::Transact {
            connection_id,
            tx_data,
        } => {
            assert_eq!(connection_id, "abc");
            assert!(tx_data.contains(":db/add"));
        }
        _ => panic!("Expected Transact"),
    }
}

#[test]
fn test_parse_transact_missing_connection_id() {
    let result =
        parse_request("{:op :transact :args {:tx-data [[:db/add \"e\" :db/ident :test]]}}");
    assert!(result.is_err());
}

#[test]
fn test_parse_transact_missing_tx_data() {
    let result = parse_request("{:op :transact :args {:connection-id \"abc\"}}");
    assert!(result.is_err());
}

// ============================================================================
// 8. Pull
// ============================================================================

#[test]
fn test_parse_pull() {
    let req =
        parse_request("{:op :pull :args {:pattern [:person/name :person/age] :entity-id 10001}}")
            .expect("parse failed");
    match req.op {
        Operation::Pull { pattern, entity_id } => {
            assert!(pattern.contains(":person/name"));
            assert_eq!(entity_id, 10001);
        }
        _ => panic!("Expected Pull"),
    }
}

// ============================================================================
// 9. Time Travel Operations
// ============================================================================

#[test]
fn test_parse_as_of() {
    let req =
        parse_request("{:op :as-of :args {:query [:find ?e :where [?e :db/ident _]] :t 1000001}}")
            .expect("parse failed");
    match req.op {
        Operation::AsOf { t, .. } => assert_eq!(t, 1000001),
        _ => panic!("Expected AsOf"),
    }
}

#[test]
fn test_parse_since() {
    let req =
        parse_request("{:op :since :args {:query [:find ?e :where [?e :db/ident _]] :t 1000001}}")
            .expect("parse failed");
    match req.op {
        Operation::Since { t, .. } => assert_eq!(t, 1000001),
        _ => panic!("Expected Since"),
    }
}

#[test]
fn test_parse_history() {
    let req = parse_request("{:op :history :args {:query [:find ?e ?a ?v :where [?e ?a ?v]]}}")
        .expect("parse failed");
    assert!(matches!(req.op, Operation::History { .. }));
}

// ============================================================================
// 10. Datoms
// ============================================================================

#[test]
fn test_parse_datoms_eavt() {
    let req =
        parse_request("{:op :datoms :args {:index :eavt :components []}}").expect("parse failed");
    match req.op {
        Operation::Datoms { index, components } => {
            assert!(matches!(index, mentatd::protocol::DatomsIndex::EAVT));
            assert!(components.is_empty());
        }
        _ => panic!("Expected Datoms"),
    }
}

#[test]
fn test_parse_datoms_avet() {
    let req = parse_request("{:op :datoms :args {:index :avet}}").expect("parse failed");
    match req.op {
        Operation::Datoms { index, .. } => {
            assert!(matches!(index, mentatd::protocol::DatomsIndex::AVET));
        }
        _ => panic!("Expected Datoms"),
    }
}

// ============================================================================
// 11. TX Range
// ============================================================================

#[test]
fn test_parse_tx_range() {
    let req =
        parse_request("{:op :tx-range :args {:start 1000001 :end 1000010}}").expect("parse failed");
    match req.op {
        Operation::TxRange { start, end } => {
            assert_eq!(start, Some(1000001));
            assert_eq!(end, Some(1000010));
        }
        _ => panic!("Expected TxRange"),
    }
}

#[test]
fn test_parse_tx_range_no_bounds() {
    let req = parse_request("{:op :tx-range :args {}}").expect("parse failed");
    match req.op {
        Operation::TxRange { start, end } => {
            assert!(start.is_none());
            assert!(end.is_none());
        }
        _ => panic!("Expected TxRange"),
    }
}

// ============================================================================
// 12. With (speculative transaction)
// ============================================================================

#[test]
fn test_parse_with() {
    let req =
        parse_request("{:op :with :args {:tx-data [[:db/add \"e\" :person/name \"Alice\"]]}}")
            .expect("parse failed");
    match req.op {
        Operation::With { tx_data } => {
            assert!(tx_data.contains(":person/name"));
        }
        _ => panic!("Expected With"),
    }
}

// ============================================================================
// 13. BasisT and Health
// ============================================================================

#[test]
fn test_parse_basis_t() {
    let req = parse_request("{:op :basis-t}").expect("parse failed");
    assert!(matches!(req.op, Operation::BasisT));
}

#[test]
fn test_parse_health() {
    let req = parse_request("{:op :health}").expect("parse failed");
    assert!(matches!(req.op, Operation::Health));
}

#[test]
fn test_parse_db_snapshot() {
    let req = parse_request("{:op :db-snapshot}").expect("parse failed");
    assert!(matches!(req.op, Operation::DbSnapshot));
}

// ============================================================================
// 14. Error Cases
// ============================================================================

#[test]
fn test_parse_invalid_edn() {
    let result = parse_request("not valid edn");
    assert!(result.is_err());
}

#[test]
fn test_parse_not_a_map() {
    let result = parse_request("[1 2 3]");
    assert!(result.is_err());
}

#[test]
fn test_parse_missing_op() {
    let result = parse_request("{:db-name \"test\"}");
    assert!(result.is_err());
}

#[test]
fn test_parse_op_not_keyword() {
    let result = parse_request("{:op \"list-dbs\"}");
    assert!(result.is_err());
}

#[test]
fn test_parse_unknown_op() {
    let result = parse_request("{:op :nonexistent-operation}");
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_map() {
    let result = parse_request("{}");
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_string() {
    let result = parse_request("");
    assert!(result.is_err());
}

// ============================================================================
// 15. Filter Operation
// ============================================================================

#[test]
fn test_parse_filter_attr_equals() {
    let req = parse_request(
        "{:op :filter :args {:predicate {:type :attr-equals :value \":person/name\"} :query [:find ?e :where [?e _ _]]}}",
    )
    .expect("parse failed");
    assert!(matches!(req.op, Operation::Filter { .. }));
}

// ============================================================================
// 16. Edge Cases
// ============================================================================

#[test]
fn test_parse_whitespace_tolerance() {
    let req = parse_request("  {   :op   :list-dbs   }  ").expect("whitespace should be tolerated");
    assert!(matches!(req.op, Operation::ListDatabases));
}

#[test]
fn test_parse_extra_fields_ignored() {
    let req = parse_request("{:op :list-dbs :extra-field 42 :another \"test\"}")
        .expect("extra fields should be ignored");
    assert!(matches!(req.op, Operation::ListDatabases));
}

#[test]
fn test_parse_unicode_in_db_name() {
    let req =
        parse_request("{:op :create-db :args {:db-name \"日本語db\"}}").expect("parse failed");
    match req.op {
        Operation::CreateDatabase { db_name } => assert_eq!(db_name, "日本語db"),
        _ => panic!("Expected CreateDatabase"),
    }
}
