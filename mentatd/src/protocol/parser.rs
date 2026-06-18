use super::{Anomaly, AnomalyCategory, Operation, Request};
use edn::parse;
use edn::symbols::Keyword;
use edn::types::{SpannedValue, ValueAndSpan};
use lazy_static::lazy_static;
use thiserror::Error;

// Phase 0 Optimization: Pre-allocate commonly used keyword keys
// These are created once at startup and reused for all requests,
// avoiding repeated allocations during parsing.
lazy_static! {
    static ref KEY_OP: ValueAndSpan = make_keyword_key("op");
    static ref KEY_ARGS: ValueAndSpan = make_keyword_key("args");
    static ref KEY_QUERY: ValueAndSpan = make_keyword_key("query");
    static ref KEY_PATTERN: ValueAndSpan = make_keyword_key("pattern");
    static ref KEY_ENTITY_ID: ValueAndSpan = make_keyword_key("entity-id");
    static ref KEY_DB_NAME: ValueAndSpan = make_keyword_key("db-name");
    static ref KEY_CONNECTION_ID: ValueAndSpan = make_keyword_key("connection-id");
    static ref KEY_TX_DATA: ValueAndSpan = make_keyword_key("tx-data");
    static ref KEY_INDEX: ValueAndSpan = make_keyword_key("index");
    static ref KEY_COMPONENTS: ValueAndSpan = make_keyword_key("components");
    static ref KEY_TIMEOUT: ValueAndSpan = make_keyword_key("timeout");
    static ref KEY_LIMIT: ValueAndSpan = make_keyword_key("limit");
    static ref KEY_OFFSET: ValueAndSpan = make_keyword_key("offset");
    static ref KEY_DB_ID: ValueAndSpan = make_keyword_key("db-id");
    static ref KEY_T: ValueAndSpan = make_keyword_key("t");
    static ref KEY_START: ValueAndSpan = make_keyword_key("start");
    static ref KEY_END: ValueAndSpan = make_keyword_key("end");
    static ref KEY_PREDICATE: ValueAndSpan = make_keyword_key("predicate");
    static ref KEY_TYPE: ValueAndSpan = make_keyword_key("type");
    static ref KEY_VALUE: ValueAndSpan = make_keyword_key("value");
}

fn make_keyword_key(name: &str) -> ValueAndSpan {
    ValueAndSpan {
        inner: SpannedValue::Keyword(Keyword::plain(name)),
        span: edn::types::Span::new(0, 0),
    }
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Failed to parse EDN: {0}")]
    Edn(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
    #[error("Invalid argument type for field {0}")]
    InvalidType(String),
}

impl From<ParseError> for Anomaly {
    fn from(err: ParseError) -> Self {
        Self {
            category: AnomalyCategory::Incorrect,
            message: err.to_string(),
            db_error: Some(":db.error/invalid-request".to_string()),
        }
    }
}

pub fn parse_request(input: &str) -> Result<Request, ParseError> {
    let value_and_span = parse::value(input).map_err(|e| ParseError::Edn(e.to_string()))?;

    let map = match &value_and_span.inner {
        SpannedValue::Map(m) => m,
        _ => return Err(ParseError::InvalidType("request must be a map".to_string())),
    };

    // Phase 0 Optimization: Use pre-allocated key instead of creating new one
    let op_value = map
        .get(&*KEY_OP)
        .ok_or_else(|| ParseError::MissingField("op".to_string()))?;

    let op = parse_operation(op_value, map, input)?;

    Ok(Request { op })
}

/// Extract the original EDN text for a `ValueAndSpan` using its span offsets.
/// Falls back to Debug formatting if the span is invalid.
fn extract_edn_text(input: &str, v: &ValueAndSpan) -> String {
    let start = v.span.0 as usize;
    let end = v.span.1 as usize;
    if start < end && end <= input.len() {
        input[start..end].to_string()
    } else {
        // Fallback: this shouldn't happen with valid spans
        format!("{:?}", v.inner)
    }
}

fn parse_operation(
    op_value: &ValueAndSpan,
    map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>,
    input: &str,
) -> Result<Operation, ParseError> {
    let op_keyword = match &op_value.inner {
        SpannedValue::Keyword(k) => k,
        _ => return Err(ParseError::InvalidType("op must be a keyword".to_string())),
    };

    match op_keyword.name() {
        "list-dbs" | "datomic.catalog/list-dbs" => Ok(Operation::ListDatabases),

        "create-db" | "datomic.catalog/create-db" => {
            let db_name = extract_string_arg(map, "db-name")?;
            Ok(Operation::CreateDatabase { db_name })
        }

        "delete-db" | "datomic.catalog/delete-db" => {
            let db_name = extract_string_arg(map, "db-name")?;
            Ok(Operation::DeleteDatabase { db_name })
        }

        "connect" => {
            let db_name = extract_string_arg(map, "db-name")?;
            Ok(Operation::Connect { db_name })
        }

        "db" => {
            let conn_id = extract_string_arg(map, "connection-id")?;
            let uuid = conn_id.parse().map_err(|_| {
                ParseError::InvalidType("connection-id must be valid UUID".to_string())
            })?;
            Ok(Operation::Db {
                connection_id: uuid,
            })
        }

        "db-snapshot" => Ok(Operation::DbSnapshot),

        "q" => {
            let args_map = extract_args_map(map)?;

            // Phase 0 Optimization: Use pre-allocated keys
            let query = match args_map.get(&*KEY_QUERY) {
                Some(v) => extract_edn_text(input, v),
                None => return Err(ParseError::MissingField("query".to_string())),
            };

            let args = match args_map.get(&*KEY_ARGS) {
                Some(v) => match &v.inner {
                    SpannedValue::Vector(vec) => {
                        vec.iter().map(|arg| extract_edn_text(input, arg)).collect()
                    }
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            };

            let timeout = extract_optional_int(args_map, "timeout").map(|i| i as u64);
            let limit = extract_optional_int(args_map, "limit").map(|i| i as usize);
            let offset = extract_optional_int(args_map, "offset").map(|i| i as usize);
            let db_id = extract_optional_string(args_map, "db-id");

            Ok(Operation::Query {
                query,
                args,
                timeout,
                limit,
                offset,
                db_id,
            })
        }

        "transact" => {
            let args_map = extract_args_map(map)?;

            // Phase 0 Optimization: Use pre-allocated keys
            let connection_id = match args_map.get(&*KEY_CONNECTION_ID) {
                Some(v) => match &v.inner {
                    SpannedValue::Text(s) => s.to_string(),
                    _ => return Err(ParseError::MissingField("connection-id".to_string())),
                },
                _ => return Err(ParseError::MissingField("connection-id".to_string())),
            };

            let tx_data = match args_map.get(&*KEY_TX_DATA) {
                Some(v) => extract_edn_text(input, v),
                None => return Err(ParseError::MissingField("tx-data".to_string())),
            };

            Ok(Operation::Transact {
                connection_id,
                tx_data,
            })
        }

        "pull" => {
            let args_map = extract_args_map(map)?;
            let pattern = extract_value_as_string(&args_map, "pattern", input)?;
            let entity_id = extract_required_int(&args_map, "entity-id")?;

            Ok(Operation::Pull { pattern, entity_id })
        }

        "datoms" => {
            let args_map = extract_args_map(map)?;
            let index_str = extract_value_as_string(&args_map, "index", input)?;
            let index = parse_datoms_index(&index_str)?;
            let components =
                extract_optional_vector(&args_map, "components", input).unwrap_or_default();

            Ok(Operation::Datoms { index, components })
        }

        "as-of" => {
            let args_map = extract_args_map(map)?;
            let query = extract_value_as_string(&args_map, "query", input)?;
            let args = extract_optional_vector(&args_map, "args", input).unwrap_or_default();
            let t = extract_required_int(&args_map, "t")?;

            Ok(Operation::AsOf { query, args, t })
        }

        "since" => {
            let args_map = extract_args_map(map)?;
            let query = extract_value_as_string(&args_map, "query", input)?;
            let args = extract_optional_vector(&args_map, "args", input).unwrap_or_default();
            let t = extract_required_int(&args_map, "t")?;

            Ok(Operation::Since { query, args, t })
        }

        "history" => {
            let args_map = extract_args_map(map)?;
            let query = extract_value_as_string(&args_map, "query", input)?;
            let args = extract_optional_vector(&args_map, "args", input).unwrap_or_default();

            Ok(Operation::History { query, args })
        }

        "tx-range" => {
            let args_map = extract_args_map(map)?;
            let start = extract_optional_int(&args_map, "start");
            let end = extract_optional_int(&args_map, "end");

            Ok(Operation::TxRange { start, end })
        }

        "with" => {
            let args_map = extract_args_map(map)?;

            // Phase 0 Optimization: Use pre-allocated key
            let tx_data = match args_map.get(&*KEY_TX_DATA) {
                Some(v) => extract_edn_text(input, v),
                None => return Err(ParseError::MissingField("tx-data".to_string())),
            };

            Ok(Operation::With { tx_data })
        }

        "filter" => {
            let args_map = extract_args_map(map)?;
            let predicate = parse_filter_predicate(&args_map)?;
            let query = extract_value_as_string(&args_map, "query", input)?;
            let args = extract_optional_vector(&args_map, "args", input).unwrap_or_default();

            Ok(Operation::Filter {
                predicate,
                query,
                args,
            })
        }

        "basis-t" => Ok(Operation::BasisT),

        "qseq" => {
            let args_map = extract_args_map(map)?;
            let query = extract_value_as_string(args_map, "query", input)?;
            let args = extract_optional_vector(args_map, "args", input).unwrap_or_default();
            let chunk_size = extract_optional_int(args_map, "chunk-size").map(|i| i as usize);
            let db_id = extract_optional_string(args_map, "db-id");

            Ok(Operation::Qseq {
                query,
                args,
                chunk_size,
                db_id,
            })
        }

        "pull-many" => {
            let args_map = extract_args_map(map)?;
            let pattern = extract_value_as_string(args_map, "pattern", input)?;

            let entity_ids_key = get_key_for("entity-ids");
            let entity_ids = match args_map.get(entity_ids_key.as_ref()) {
                Some(v) => match &v.inner {
                    SpannedValue::Vector(vec) => {
                        let mut ids = Vec::with_capacity(vec.len());
                        for item in vec {
                            match &item.inner {
                                SpannedValue::Integer(i) => ids.push(*i),
                                _ => {
                                    return Err(ParseError::InvalidType(
                                        "entity-ids elements must be integers".to_string(),
                                    ))
                                }
                            }
                        }
                        ids
                    }
                    _ => {
                        return Err(ParseError::InvalidType(
                            "entity-ids must be a vector".to_string(),
                        ))
                    }
                },
                None => return Err(ParseError::MissingField("entity-ids".to_string())),
            };

            Ok(Operation::PullMany {
                pattern,
                entity_ids,
            })
        }

        "index-range" => {
            let args_map = extract_args_map(map)?;
            let attrid = extract_value_as_string(args_map, "attrid", input)?;
            let start = extract_optional_string(args_map, "start");
            let end = extract_optional_string(args_map, "end");
            let limit = extract_optional_int(args_map, "limit").map(|i| i as usize);

            Ok(Operation::IndexRange {
                attrid,
                start,
                end,
                limit,
            })
        }

        "entid" => {
            let args_map = extract_args_map(map)?;
            let ident = extract_value_as_string(args_map, "ident", input)?;
            Ok(Operation::Entid { ident })
        }

        "ident" => {
            let args_map = extract_args_map(map)?;
            let entid = extract_required_int(args_map, "entid")?;
            Ok(Operation::Ident { entid })
        }

        "db-stats" => Ok(Operation::DbStats),

        "health" => Ok(Operation::Health),

        _ => Err(ParseError::InvalidOperation(op_keyword.name().to_string())),
    }
}

fn parse_filter_predicate(
    map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>,
) -> Result<super::FilterPredicate, ParseError> {
    // Phase 0 Optimization: Use pre-allocated key
    let pred_value = map
        .get(&*KEY_PREDICATE)
        .ok_or_else(|| ParseError::MissingField("predicate".to_string()))?;

    // Predicate can be a map like {:type :attr-equals :value ":person/name"}
    // or a keyword for simple predicates
    match &pred_value.inner {
        SpannedValue::Map(pred_map) => {
            // Phase 0 Optimization: Use pre-allocated keys
            let pred_type = pred_map
                .get(&*KEY_TYPE)
                .ok_or_else(|| ParseError::MissingField("predicate :type".to_string()))?;

            let pred_type_str = match &pred_type.inner {
                SpannedValue::Keyword(k) => k.name().to_string(),
                _ => {
                    return Err(ParseError::InvalidType(
                        "predicate :type must be a keyword".to_string(),
                    ))
                }
            };

            let pred_val = pred_map
                .get(&*KEY_VALUE)
                .ok_or_else(|| ParseError::MissingField("predicate :value".to_string()))?;

            match pred_type_str.as_str() {
                "attr-equals" => {
                    let attr = format!("{:?}", pred_val.inner);
                    Ok(super::FilterPredicate::AttrEquals(attr))
                }
                "entity-equals" => match &pred_val.inner {
                    SpannedValue::Integer(i) => Ok(super::FilterPredicate::EntityEquals(*i)),
                    _ => Err(ParseError::InvalidType(
                        "entity-equals :value must be an integer".to_string(),
                    )),
                },
                "since" => match &pred_val.inner {
                    SpannedValue::Integer(i) => Ok(super::FilterPredicate::Since(*i)),
                    _ => Err(ParseError::InvalidType(
                        "since :value must be an integer".to_string(),
                    )),
                },
                "custom" => match &pred_val.inner {
                    SpannedValue::Text(s) => Ok(super::FilterPredicate::Custom(s.clone())),
                    _ => Err(ParseError::InvalidType(
                        "custom :value must be a string".to_string(),
                    )),
                },
                other => Err(ParseError::InvalidOperation(format!(
                    "Unknown filter predicate type: {}",
                    other
                ))),
            }
        }
        _ => Err(ParseError::InvalidType(
            "predicate must be a map with :type and :value".to_string(),
        )),
    }
}

// Phase 0 Optimization: Return reference instead of cloning the entire BTreeMap
fn extract_args_map(
    map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>,
) -> Result<&std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>, ParseError> {
    // Use pre-allocated key
    match map.get(&*KEY_ARGS) {
        Some(v) => match &v.inner {
            SpannedValue::Map(m) => Ok(m), // Return reference, not clone
            _ => Err(ParseError::MissingField("args".to_string())),
        },
        _ => Err(ParseError::MissingField("args".to_string())),
    }
}

fn extract_string_arg(
    map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>,
    key: &str,
) -> Result<String, ParseError> {
    let args_map = extract_args_map(map)?;
    let key_value = ValueAndSpan {
        inner: SpannedValue::Keyword(Keyword::plain(key)),
        span: edn::types::Span::new(0, 0),
    };
    match args_map.get(&key_value) {
        Some(v) => match &v.inner {
            SpannedValue::Text(s) => Ok(s.to_string()),
            _ => Err(ParseError::InvalidType(key.to_string())),
        },
        None => Err(ParseError::MissingField(key.to_string())),
    }
}

// Phase 0 Optimization: Helper to get ValueAndSpan key for a string
// Uses pre-allocated static keys for common fields, creates on-demand for others
fn get_key_for(key_str: &str) -> std::borrow::Cow<'static, ValueAndSpan> {
    match key_str {
        "op" => std::borrow::Cow::Borrowed(&*KEY_OP),
        "args" => std::borrow::Cow::Borrowed(&*KEY_ARGS),
        "query" => std::borrow::Cow::Borrowed(&*KEY_QUERY),
        "pattern" => std::borrow::Cow::Borrowed(&*KEY_PATTERN),
        "entity-id" => std::borrow::Cow::Borrowed(&*KEY_ENTITY_ID),
        "db-name" => std::borrow::Cow::Borrowed(&*KEY_DB_NAME),
        "connection-id" => std::borrow::Cow::Borrowed(&*KEY_CONNECTION_ID),
        "tx-data" => std::borrow::Cow::Borrowed(&*KEY_TX_DATA),
        "index" => std::borrow::Cow::Borrowed(&*KEY_INDEX),
        "components" => std::borrow::Cow::Borrowed(&*KEY_COMPONENTS),
        "timeout" => std::borrow::Cow::Borrowed(&*KEY_TIMEOUT),
        "limit" => std::borrow::Cow::Borrowed(&*KEY_LIMIT),
        "offset" => std::borrow::Cow::Borrowed(&*KEY_OFFSET),
        "db-id" => std::borrow::Cow::Borrowed(&*KEY_DB_ID),
        "t" => std::borrow::Cow::Borrowed(&*KEY_T),
        "start" => std::borrow::Cow::Borrowed(&*KEY_START),
        "end" => std::borrow::Cow::Borrowed(&*KEY_END),
        "predicate" => std::borrow::Cow::Borrowed(&*KEY_PREDICATE),
        "type" => std::borrow::Cow::Borrowed(&*KEY_TYPE),
        "value" => std::borrow::Cow::Borrowed(&*KEY_VALUE),
        // For uncommon keys, create on-demand (still better than before due to reduced allocations)
        _ => std::borrow::Cow::Owned(make_keyword_key(key_str)),
    }
}

fn extract_optional_int(
    map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>,
    key: &str,
) -> Option<i64> {
    let key_value = get_key_for(key);
    map.get(key_value.as_ref()).and_then(|v| match &v.inner {
        SpannedValue::Integer(i) => Some(*i),
        _ => None,
    })
}

fn extract_required_int(
    map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>,
    key: &str,
) -> Result<i64, ParseError> {
    extract_optional_int(map, key).ok_or_else(|| ParseError::MissingField(key.to_string()))
}

fn extract_value_as_string(
    map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>,
    key: &str,
    input: &str,
) -> Result<String, ParseError> {
    let key_value = get_key_for(key);
    match map.get(key_value.as_ref()) {
        Some(v) => Ok(extract_edn_text(input, v)),
        None => Err(ParseError::MissingField(key.to_string())),
    }
}

fn extract_optional_vector(
    map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>,
    key: &str,
    input: &str,
) -> Option<Vec<String>> {
    let key_value = get_key_for(key);
    map.get(key_value.as_ref()).and_then(|v| match &v.inner {
        SpannedValue::Vector(vec) => {
            Some(vec.iter().map(|arg| extract_edn_text(input, arg)).collect())
        }
        _ => None,
    })
}

fn extract_optional_string(
    map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>,
    key: &str,
) -> Option<String> {
    let key_value = get_key_for(key);
    map.get(key_value.as_ref()).and_then(|v| match &v.inner {
        SpannedValue::Text(s) => Some(s.to_string()),
        _ => None,
    })
}

fn parse_datoms_index(s: &str) -> Result<super::DatomsIndex, ParseError> {
    use super::DatomsIndex;
    let s_clean = s.trim_matches(|c| c == ':' || c == '"');
    match s_clean {
        "eavt" | "EAVT" => Ok(DatomsIndex::EAVT),
        "aevt" | "AEVT" => Ok(DatomsIndex::AEVT),
        "avet" | "AVET" => Ok(DatomsIndex::AVET),
        "vaet" | "VAET" => Ok(DatomsIndex::VAET),
        _ => Err(ParseError::InvalidType(format!(
            "Invalid datoms index: {}. Expected :eavt, :aevt, :avet, or :vaet",
            s
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_health() {
        let input = "{:op :health}";
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Health => {}
            _ => panic!("Expected Health operation"),
        }
    }

    #[test]
    fn test_parse_list_dbs() {
        let input = "{:op :list-dbs}";
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::ListDatabases => {}
            _ => panic!("Expected ListDatabases operation"),
        }
    }

    #[test]
    fn test_parse_connect() {
        let input = r#"{:op :connect :args {:db-name "test-db"}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Connect { db_name } => assert_eq!(db_name, "test-db"),
            _ => panic!("Expected Connect operation"),
        }
    }

    #[test]
    fn test_parse_basis_t() {
        let input = "{:op :basis-t}";
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::BasisT => {}
            _ => panic!("Expected BasisT operation"),
        }
    }

    #[test]
    fn test_parse_with() {
        let input = r#"{:op :with :args {:tx-data [[:db/add "t1" :person/name "Alice"]]}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::With { tx_data } => {
                // The EDN debug format uses Rust Debug for SpannedValue,
                // so keywords appear as Keyword(...) and strings as Text(...)
                assert!(
                    !tx_data.is_empty(),
                    "tx_data should not be empty: {}",
                    tx_data
                );
            }
            _ => panic!("Expected With operation"),
        }
    }

    #[test]
    fn test_parse_filter() {
        let input = r#"{:op :filter :args {:predicate {:type :attr-equals :value :person/name} :query "[:find ?e :where [?e :person/name]]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Filter {
                predicate, query, ..
            } => {
                match predicate {
                    super::super::FilterPredicate::AttrEquals(attr) => {
                        // The EDN debug format wraps keywords in Keyword(...)
                        assert!(
                            !attr.is_empty(),
                            "attr predicate should not be empty: {}",
                            attr
                        );
                    }
                    _ => panic!("Expected AttrEquals predicate"),
                }
                // Query is formatted via Debug, so check it's non-empty
                assert!(!query.is_empty(), "query should not be empty: {}", query);
            }
            _ => panic!("Expected Filter operation"),
        }
    }

    #[test]
    fn test_parse_filter_since() {
        let input = r#"{:op :filter :args {:predicate {:type :since :value 1000} :query "[:find ?e :where [?e :person/name]]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Filter { predicate, .. } => match predicate {
                super::super::FilterPredicate::Since(t) => assert_eq!(t, 1000),
                _ => panic!("Expected Since predicate"),
            },
            _ => panic!("Expected Filter operation"),
        }
    }

    #[test]
    fn test_parse_filter_entity_equals() {
        let input = r#"{:op :filter :args {:predicate {:type :entity-equals :value 42} :query "[:find ?a ?v :where [42 ?a ?v]]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Filter { predicate, .. } => match predicate {
                super::super::FilterPredicate::EntityEquals(eid) => assert_eq!(eid, 42),
                _ => panic!("Expected EntityEquals predicate"),
            },
            _ => panic!("Expected Filter operation"),
        }
    }

    #[test]
    fn test_parse_with_missing_tx_data() {
        let input = r#"{:op :with :args {}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_filter_missing_predicate() {
        let input = r#"{:op :filter :args {:query "[:find ?e :where [?e _ _]]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    // ---- Query operation parsing ----

    #[test]
    fn test_parse_query_basic() {
        let input = r#"{:op :q :args {:query "[:find ?e :where [?e :name]]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Query {
                query,
                args,
                timeout,
                limit,
                offset,
                db_id,
            } => {
                assert!(query.contains("find"));
                assert!(args.is_empty());
                assert!(timeout.is_none());
                assert!(limit.is_none());
                assert!(offset.is_none());
                assert!(db_id.is_none());
            }
            _ => panic!("Expected Query operation"),
        }
    }

    #[test]
    fn test_parse_query_with_args() {
        let input = r#"{:op :q :args {:query "[:find ?e :in $ ?name :where [?e :name ?name]]" :args ["Alice"]}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Query { args, .. } => {
                assert_eq!(args.len(), 1);
            }
            _ => panic!("Expected Query operation"),
        }
    }

    #[test]
    fn test_parse_query_with_timeout() {
        let input = r#"{:op :q :args {:query "[:find ?e]" :args [] :timeout 5000}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Query { timeout, .. } => {
                assert_eq!(timeout, Some(5000));
            }
            _ => panic!("Expected Query operation"),
        }
    }

    #[test]
    fn test_parse_query_with_limit_and_offset() {
        let input = r#"{:op :q :args {:query "[:find ?e]" :args [] :limit 10 :offset 5}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Query { limit, offset, .. } => {
                assert_eq!(limit, Some(10));
                assert_eq!(offset, Some(5));
            }
            _ => panic!("Expected Query operation"),
        }
    }

    #[test]
    fn test_parse_query_missing_query_field() {
        let input = r#"{:op :q :args {:args []}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    // ---- Transact operation parsing ----

    #[test]
    fn test_parse_transact() {
        let input = r#"{:op :transact :args {:connection-id "abc-123" :tx-data "[{:db/id -1 :name \"Bob\"}]"}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Transact {
                connection_id,
                tx_data,
            } => {
                assert_eq!(connection_id, "abc-123");
                assert!(tx_data.contains("db/id"));
            }
            _ => panic!("Expected Transact operation"),
        }
    }

    #[test]
    fn test_parse_transact_missing_connection_id() {
        let input = r#"{:op :transact :args {:tx-data "[{:db/id -1}]"}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_transact_missing_tx_data() {
        let input = r#"{:op :transact :args {:connection-id "abc"}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    // ---- Pull operation parsing ----

    #[test]
    fn test_parse_pull() {
        let input = r#"{:op :pull :args {:pattern "[*]" :entity-id 42}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Pull { entity_id, .. } => {
                assert_eq!(entity_id, 42);
            }
            _ => panic!("Expected Pull operation"),
        }
    }

    #[test]
    fn test_parse_pull_missing_entity_id() {
        let input = r#"{:op :pull :args {:pattern "[*]"}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    // ---- Datoms operation parsing ----

    // The datoms parser uses `extract_value_as_string` which extracts the
    // original EDN text from the input using span offsets. Keywords like
    // `:eavt` are returned as ":eavt" and strings as "\"eavt\"".

    #[test]
    fn test_parse_datoms_index_direct_eavt() {
        assert!(matches!(
            parse_datoms_index("eavt"),
            Ok(super::super::DatomsIndex::EAVT)
        ));
        assert!(matches!(
            parse_datoms_index("EAVT"),
            Ok(super::super::DatomsIndex::EAVT)
        ));
        assert!(matches!(
            parse_datoms_index(":eavt"),
            Ok(super::super::DatomsIndex::EAVT)
        ));
    }

    #[test]
    fn test_parse_datoms_index_direct_aevt() {
        assert!(matches!(
            parse_datoms_index("aevt"),
            Ok(super::super::DatomsIndex::AEVT)
        ));
        assert!(matches!(
            parse_datoms_index("AEVT"),
            Ok(super::super::DatomsIndex::AEVT)
        ));
    }

    #[test]
    fn test_parse_datoms_index_direct_avet() {
        assert!(matches!(
            parse_datoms_index("avet"),
            Ok(super::super::DatomsIndex::AVET)
        ));
        assert!(matches!(
            parse_datoms_index("AVET"),
            Ok(super::super::DatomsIndex::AVET)
        ));
    }

    #[test]
    fn test_parse_datoms_index_direct_vaet() {
        assert!(matches!(
            parse_datoms_index("vaet"),
            Ok(super::super::DatomsIndex::VAET)
        ));
        assert!(matches!(
            parse_datoms_index("VAET"),
            Ok(super::super::DatomsIndex::VAET)
        ));
    }

    #[test]
    fn test_parse_datoms_index_direct_invalid() {
        assert!(parse_datoms_index("invalid").is_err());
        assert!(parse_datoms_index("").is_err());
        assert!(parse_datoms_index("xyz").is_err());
    }

    #[test]
    fn test_parse_datoms_keyword_index() {
        // extract_value_as_string now uses span-based extraction, returning
        // the original EDN text (e.g., ":eavt" or "\"eavt\"") instead of
        // Debug-formatted Rust types.
        let keyword_input = r#"{:op :datoms :args {:index :eavt :components []}}"#;
        assert!(parse_request(keyword_input).is_ok());

        let string_input = r#"{:op :datoms :args {:index "eavt" :components []}}"#;
        assert!(parse_request(string_input).is_ok());
    }

    // ---- Time-travel operation parsing ----

    #[test]
    fn test_parse_as_of() {
        let input = r#"{:op :as-of :args {:query "[:find ?e]" :args [] :t 1000}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::AsOf { t, .. } => assert_eq!(t, 1000),
            _ => panic!("Expected AsOf operation"),
        }
    }

    #[test]
    fn test_parse_as_of_missing_t() {
        let input = r#"{:op :as-of :args {:query "[:find ?e]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_since() {
        let input = r#"{:op :since :args {:query "[:find ?e]" :args [] :t 500}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Since { t, .. } => assert_eq!(t, 500),
            _ => panic!("Expected Since operation"),
        }
    }

    #[test]
    fn test_parse_history() {
        let input = r#"{:op :history :args {:query "[:find ?e]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::History { .. } => {}
            _ => panic!("Expected History operation"),
        }
    }

    // ---- Tx-range operation parsing ----

    #[test]
    fn test_parse_tx_range_both() {
        let input = r#"{:op :tx-range :args {:start 100 :end 200}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::TxRange { start, end } => {
                assert_eq!(start, Some(100));
                assert_eq!(end, Some(200));
            }
            _ => panic!("Expected TxRange operation"),
        }
    }

    #[test]
    fn test_parse_tx_range_start_only() {
        let input = r#"{:op :tx-range :args {:start 100}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::TxRange { start, end } => {
                assert_eq!(start, Some(100));
                assert!(end.is_none());
            }
            _ => panic!("Expected TxRange operation"),
        }
    }

    #[test]
    fn test_parse_tx_range_end_only() {
        let input = r#"{:op :tx-range :args {:end 200}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::TxRange { start, end } => {
                assert!(start.is_none());
                assert_eq!(end, Some(200));
            }
            _ => panic!("Expected TxRange operation"),
        }
    }

    #[test]
    fn test_parse_tx_range_empty() {
        let input = r#"{:op :tx-range :args {}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::TxRange { start, end } => {
                assert!(start.is_none());
                assert!(end.is_none());
            }
            _ => panic!("Expected TxRange operation"),
        }
    }

    // ---- DB operation parsing ----

    #[test]
    fn test_parse_db() {
        let input = r#"{:op :db :args {:connection-id "550e8400-e29b-41d4-a716-446655440000"}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Db { connection_id } => {
                assert_eq!(
                    connection_id.to_string(),
                    "550e8400-e29b-41d4-a716-446655440000"
                );
            }
            _ => panic!("Expected Db operation"),
        }
    }

    #[test]
    fn test_parse_db_invalid_uuid() {
        let input = r#"{:op :db :args {:connection-id "not-a-uuid"}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    // ---- Create/Delete database parsing ----

    #[test]
    fn test_parse_create_database() {
        let input = r#"{:op :create-db :args {:db-name "my_db"}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::CreateDatabase { db_name } => assert_eq!(db_name, "my_db"),
            _ => panic!("Expected CreateDatabase operation"),
        }
    }

    #[test]
    fn test_parse_delete_database() {
        let input = r#"{:op :delete-db :args {:db-name "old_db"}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::DeleteDatabase { db_name } => assert_eq!(db_name, "old_db"),
            _ => panic!("Expected DeleteDatabase operation"),
        }
    }

    // ---- Datomic catalog namespace aliases ----

    #[test]
    fn test_parse_datomic_catalog_list_dbs() {
        let input = "{:op :datomic.catalog/list-dbs}";
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::ListDatabases => {}
            _ => panic!("Expected ListDatabases operation"),
        }
    }

    #[test]
    fn test_parse_datomic_catalog_create_db() {
        let input = r#"{:op :datomic.catalog/create-db :args {:db-name "new_db"}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::CreateDatabase { db_name } => assert_eq!(db_name, "new_db"),
            _ => panic!("Expected CreateDatabase operation"),
        }
    }

    #[test]
    fn test_parse_datomic_catalog_delete_db() {
        let input = r#"{:op :datomic.catalog/delete-db :args {:db-name "old_db"}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::DeleteDatabase { db_name } => assert_eq!(db_name, "old_db"),
            _ => panic!("Expected DeleteDatabase operation"),
        }
    }

    // ---- Error cases ----

    #[test]
    fn test_parse_invalid_edn() {
        let result = parse_request("not valid edn at all {{{");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_non_map_input() {
        let result = parse_request("[1 2 3]");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_op() {
        let result = parse_request("{:foo :bar}");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::MissingField(f) => assert_eq!(f, "op"),
            other => panic!("Expected MissingField, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_unknown_operation() {
        let result = parse_request("{:op :unknown-op-xyz}");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::InvalidOperation(op) => assert_eq!(op, "unknown-op-xyz"),
            other => panic!("Expected InvalidOperation, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_op_not_keyword() {
        let result = parse_request(r#"{:op "not-a-keyword"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_string() {
        let result = parse_request("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_whitespace_only() {
        let result = parse_request("   \n\t  ");
        assert!(result.is_err());
    }

    // ---- Datoms index parsing ----

    #[test]
    fn test_parse_datoms_index_case_insensitive() {
        assert!(parse_datoms_index(":eavt").is_ok());
        assert!(parse_datoms_index(":EAVT").is_ok());
        assert!(parse_datoms_index(":aevt").is_ok());
        assert!(parse_datoms_index(":AEVT").is_ok());
        assert!(parse_datoms_index(":avet").is_ok());
        assert!(parse_datoms_index(":AVET").is_ok());
        assert!(parse_datoms_index(":vaet").is_ok());
        assert!(parse_datoms_index(":VAET").is_ok());
    }

    #[test]
    fn test_parse_datoms_index_invalid() {
        assert!(parse_datoms_index(":invalid").is_err());
        assert!(parse_datoms_index("").is_err());
    }

    // ---- ParseError to Anomaly conversion ----

    #[test]
    fn test_parse_error_to_anomaly() {
        let err = ParseError::MissingField("test".to_string());
        let anomaly: Anomaly = err.into();
        assert!(matches!(anomaly.category, AnomalyCategory::Incorrect));
        assert!(anomaly.message.contains("test"));
        assert_eq!(
            anomaly.db_error.as_deref(),
            Some(":db.error/invalid-request")
        );
    }

    // ---- Filter predicate edge cases ----

    #[test]
    fn test_parse_filter_custom_predicate() {
        let input = r#"{:op :filter :args {:predicate {:type :custom :value "e > 100"} :query "[:find ?e]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Filter { predicate, .. } => match predicate {
                super::super::FilterPredicate::Custom(expr) => {
                    assert_eq!(expr, "e > 100");
                }
                _ => panic!("Expected Custom predicate"),
            },
            _ => panic!("Expected Filter operation"),
        }
    }

    #[test]
    fn test_parse_filter_unknown_predicate_type() {
        let input = r#"{:op :filter :args {:predicate {:type :unknown-type :value 1} :query "[:find ?e]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_filter_predicate_not_a_map() {
        let input = r#"{:op :filter :args {:predicate "not-a-map" :query "[:find ?e]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_filter_predicate_missing_type() {
        let input = r#"{:op :filter :args {:predicate {:value 42} :query "[:find ?e]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_filter_predicate_missing_value() {
        let input = r#"{:op :filter :args {:predicate {:type :entity-equals} :query "[:find ?e]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_connect_missing_db_name() {
        let input = r#"{:op :connect :args {}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_create_db_missing_name() {
        let input = r#"{:op :create-db :args {}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_pull_missing_pattern() {
        let input = r#"{:op :pull :args {:entity-id 42}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_args_not_a_map() {
        let input = r#"{:op :q :args "not-a-map"}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_args_missing_entirely() {
        let input = r#"{:op :q}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    // ---- New Datomic operations ----

    #[test]
    fn test_parse_qseq_basic() {
        let input = r#"{:op :qseq :args {:query "[:find ?e :where [?e :name]]" :args []}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
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
            _ => panic!("Expected Qseq operation"),
        }
    }

    #[test]
    fn test_parse_qseq_with_chunk_size() {
        let input = r#"{:op :qseq :args {:query "[:find ?e]" :args [] :chunk-size 500}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Qseq { chunk_size, .. } => {
                assert_eq!(chunk_size, Some(500));
            }
            _ => panic!("Expected Qseq operation"),
        }
    }

    #[test]
    fn test_parse_pull_many() {
        let input = r#"{:op :pull-many :args {:pattern "[*]" :entity-ids [42 43 44]}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::PullMany { entity_ids, .. } => {
                assert_eq!(entity_ids, vec![42, 43, 44]);
            }
            _ => panic!("Expected PullMany operation"),
        }
    }

    #[test]
    fn test_parse_pull_many_empty_ids() {
        let input = r#"{:op :pull-many :args {:pattern "[*]" :entity-ids []}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::PullMany { entity_ids, .. } => {
                assert!(entity_ids.is_empty());
            }
            _ => panic!("Expected PullMany operation"),
        }
    }

    #[test]
    fn test_parse_pull_many_missing_ids() {
        let input = r#"{:op :pull-many :args {:pattern "[*]"}}"#;
        let req = parse_request(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_index_range_basic() {
        let input = r#"{:op :index-range :args {:attrid ":person/name"}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
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
            _ => panic!("Expected IndexRange operation"),
        }
    }

    #[test]
    fn test_parse_index_range_with_bounds() {
        let input =
            r#"{:op :index-range :args {:attrid ":person/name" :start "A" :end "M" :limit 100}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::IndexRange {
                start, end, limit, ..
            } => {
                assert_eq!(start.as_deref(), Some("A"));
                assert_eq!(end.as_deref(), Some("M"));
                assert_eq!(limit, Some(100));
            }
            _ => panic!("Expected IndexRange operation"),
        }
    }

    #[test]
    fn test_parse_entid() {
        let input = r#"{:op :entid :args {:ident ":person/name"}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Entid { ident } => {
                assert!(ident.contains("person/name"));
            }
            _ => panic!("Expected Entid operation"),
        }
    }

    #[test]
    fn test_parse_ident() {
        let input = r#"{:op :ident :args {:entid 42}}"#;
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::Ident { entid } => {
                assert_eq!(entid, 42);
            }
            _ => panic!("Expected Ident operation"),
        }
    }

    #[test]
    fn test_parse_db_stats() {
        let input = "{:op :db-stats}";
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::DbStats => {}
            _ => panic!("Expected DbStats operation"),
        }
    }
}
