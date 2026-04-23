use super::{Anomaly, Response, ResponseValue};
use std::fmt::Write;

pub fn serialize_response(response: &Response) -> String {
    match response {
        Response::Success { result } => {
            let mut output = String::from("{:result ");
            serialize_value(result, &mut output);
            output.push('}');
            output
        }
        Response::Error { anomaly } => serialize_anomaly(anomaly),
    }
}

fn serialize_value(value: &ResponseValue, output: &mut String) {
    match value {
        ResponseValue::Nil => {
            output.push_str("nil");
        }
        ResponseValue::String(s) => {
            write!(output, r#""{}""#, escape_string(s)).ok();
        }
        ResponseValue::Boolean(b) => {
            output.push_str(if *b { "true" } else { "false" });
        }
        ResponseValue::Integer(i) => {
            write!(output, "{}", i).ok();
        }
        ResponseValue::Keyword(k) => {
            output.push(':');
            output.push_str(k);
        }
        ResponseValue::List(items) => {
            output.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    output.push(' ');
                }
                serialize_value(item, output);
            }
            output.push(')');
        }
        ResponseValue::Vector(items) => {
            output.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    output.push(' ');
                }
                serialize_value(item, output);
            }
            output.push(']');
        }
        ResponseValue::Map(entries) => {
            output.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    output.push(' ');
                }
                serialize_value(k, output);
                output.push(' ');
                serialize_value(v, output);
            }
            output.push('}');
        }
    }
}

fn serialize_anomaly(anomaly: &Anomaly) -> String {
    let mut output = String::from("{:error {");

    write!(
        output,
        ":cognitect.anomalies/category {} ",
        anomaly.category.as_keyword()
    )
    .ok();

    write!(
        output,
        r#":cognitect.anomalies/message "{}""#,
        escape_string(&anomaly.message)
    )
    .ok();

    if let Some(db_error) = &anomaly.db_error {
        write!(output, " :db/error :{}", db_error).ok();
    }

    output.push_str("}}");
    output
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::AnomalyCategory;

    #[test]
    fn test_serialize_string() {
        let response = Response::Success {
            result: ResponseValue::String("hello".to_string()),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result "hello"}"#);
    }

    #[test]
    fn test_serialize_boolean() {
        let response = Response::Success {
            result: ResponseValue::Boolean(true),
        };
        let output = serialize_response(&response);
        assert_eq!(output, "{:result true}");
    }

    #[test]
    fn test_serialize_integer() {
        let response = Response::Success {
            result: ResponseValue::Integer(42),
        };
        let output = serialize_response(&response);
        assert_eq!(output, "{:result 42}");
    }

    #[test]
    fn test_serialize_nil() {
        let response = Response::Success {
            result: ResponseValue::Nil,
        };
        let output = serialize_response(&response);
        assert_eq!(output, "{:result nil}");
    }

    #[test]
    fn test_serialize_keyword() {
        let response = Response::Success {
            result: ResponseValue::Keyword("db/name".to_string()),
        };
        let output = serialize_response(&response);
        assert_eq!(output, "{:result :db/name}");
    }

    #[test]
    fn test_serialize_vector_of_strings() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![
                ResponseValue::String("db1".to_string()),
                ResponseValue::String("db2".to_string()),
            ]),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result ["db1" "db2"]}"#);
    }

    #[test]
    fn test_serialize_vector_of_mixed() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![
                ResponseValue::Integer(10001),
                ResponseValue::String("Alice".to_string()),
            ]),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result [10001 "Alice"]}"#);
    }

    #[test]
    fn test_serialize_nested_vectors() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![
                ResponseValue::Vector(vec![
                    ResponseValue::Integer(10001),
                    ResponseValue::String("Alice".to_string()),
                ]),
                ResponseValue::Vector(vec![
                    ResponseValue::Integer(10002),
                    ResponseValue::String("Bob".to_string()),
                ]),
            ]),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result [[10001 "Alice"] [10002 "Bob"]]}"#);
    }

    #[test]
    fn test_serialize_map() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![(
                ResponseValue::Keyword("connection-id".to_string()),
                ResponseValue::String("abc-123".to_string()),
            )]),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result {:connection-id "abc-123"}}"#);
    }

    #[test]
    fn test_serialize_map_with_mixed_values() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("tx-id".to_string()),
                    ResponseValue::Integer(12345),
                ),
                (
                    ResponseValue::Keyword("status".to_string()),
                    ResponseValue::String("committed".to_string()),
                ),
                (
                    ResponseValue::Keyword("tx-instant".to_string()),
                    ResponseValue::Nil,
                ),
            ]),
        };
        let output = serialize_response(&response);
        assert_eq!(
            output,
            r#"{:result {:tx-id 12345 :status "committed" :tx-instant nil}}"#
        );
    }

    #[test]
    fn test_serialize_error() {
        let response = Response::Error {
            anomaly: Anomaly {
                category: AnomalyCategory::NotFound,
                message: "Database not found".to_string(),
                db_error: Some("db.error/not-found".to_string()),
            },
        };
        let output = serialize_response(&response);
        assert!(output.contains(":cognitect.anomalies/not-found"));
        assert!(output.contains("Database not found"));
        assert!(output.contains(":db.error/not-found"));
    }

    #[test]
    fn test_escape_string() {
        let s = r#"hello "world" \n"#;
        let escaped = escape_string(s);
        assert!(escaped.contains(r#"\""#));
        assert!(escaped.contains(r#"\\"#));
    }
}
