use super::{Anomaly, AnomalyCategory, Operation, Request};
use edn::parse;
use edn::symbols::Keyword;
use edn::types::{SpannedValue, ValueAndSpan};
use thiserror::Error;

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

    let op_keyword = Keyword::plain("op");
    let op_key = ValueAndSpan {
        inner: SpannedValue::Keyword(op_keyword.clone()),
        span: edn::types::Span::new(0, 0),
    };

    let op_value = map
        .get(&op_key)
        .ok_or_else(|| ParseError::MissingField("op".to_string()))?;

    let op = parse_operation(op_value, map)?;

    Ok(Request { op })
}

fn parse_operation(op_value: &ValueAndSpan, map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>) -> Result<Operation, ParseError> {
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
            let uuid = conn_id
                .parse()
                .map_err(|_| ParseError::InvalidType("connection-id must be valid UUID".to_string()))?;
            Ok(Operation::Db { connection_id: uuid })
        }

        "q" => {
            let args_map = extract_args_map(map)?;

            let query_key = ValueAndSpan {
                inner: SpannedValue::Keyword(Keyword::plain("query")),
                span: edn::types::Span::new(0, 0),
            };

            let query = match args_map.get(&query_key) {
                Some(v) => format!("{:?}", v.inner),
                None => return Err(ParseError::MissingField("query".to_string())),
            };

            let args_key = ValueAndSpan {
                inner: SpannedValue::Keyword(Keyword::plain("args")),
                span: edn::types::Span::new(0, 0),
            };

            let args = match args_map.get(&args_key) {
                Some(v) => match &v.inner {
                    SpannedValue::Vector(vec) => vec.iter().map(|arg| format!("{:?}", arg.inner)).collect(),
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            };

            let timeout = extract_optional_int(&args_map, "timeout").map(|i| i as u64);
            let limit = extract_optional_int(&args_map, "limit").map(|i| i as usize);
            let offset = extract_optional_int(&args_map, "offset").map(|i| i as usize);

            Ok(Operation::Query { query, args, timeout, limit, offset })
        }

        "transact" => {
            let args_map = extract_args_map(map)?;

            let conn_key = ValueAndSpan {
                inner: SpannedValue::Keyword(Keyword::plain("connection-id")),
                span: edn::types::Span::new(0, 0),
            };

            let connection_id = match args_map.get(&conn_key) {
                Some(v) => match &v.inner {
                    SpannedValue::Text(s) => s.to_string(),
                    _ => return Err(ParseError::MissingField("connection-id".to_string())),
                },
                _ => return Err(ParseError::MissingField("connection-id".to_string())),
            };

            let tx_key = ValueAndSpan {
                inner: SpannedValue::Keyword(Keyword::plain("tx-data")),
                span: edn::types::Span::new(0, 0),
            };

            let tx_data = match args_map.get(&tx_key) {
                Some(v) => format!("{:?}", v.inner),
                None => return Err(ParseError::MissingField("tx-data".to_string())),
            };

            Ok(Operation::Transact { connection_id, tx_data })
        }

        "health" => Ok(Operation::Health),

        _ => Err(ParseError::InvalidOperation(op_keyword.name().to_string())),
    }
}

fn extract_args_map(map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>) -> Result<std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>, ParseError> {
    let args_key = ValueAndSpan {
        inner: SpannedValue::Keyword(Keyword::plain("args")),
        span: edn::types::Span::new(0, 0),
    };
    match map.get(&args_key) {
        Some(v) => match &v.inner {
            SpannedValue::Map(m) => Ok(m.clone()),
            _ => Err(ParseError::MissingField("args".to_string())),
        },
        _ => Err(ParseError::MissingField("args".to_string())),
    }
}

fn extract_string_arg(map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>, key: &str) -> Result<String, ParseError> {
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

fn extract_optional_int(map: &std::collections::BTreeMap<ValueAndSpan, ValueAndSpan>, key: &str) -> Option<i64> {
    let key_value = ValueAndSpan {
        inner: SpannedValue::Keyword(Keyword::plain(key)),
        span: edn::types::Span::new(0, 0),
    };
    map.get(&key_value)
        .and_then(|v| match &v.inner {
            SpannedValue::Integer(i) => Some(*i),
            _ => None,
        })
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
            Operation::Health => {},
            _ => panic!("Expected Health operation"),
        }
    }

    #[test]
    fn test_parse_list_dbs() {
        let input = "{:op :list-dbs}";
        let req = parse_request(input);
        assert!(req.is_ok());
        match req.unwrap().op {
            Operation::ListDatabases => {},
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
}
