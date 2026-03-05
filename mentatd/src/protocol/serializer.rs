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
        ResponseValue::String(s) => {
            write!(output, r#""{}""#, escape_string(s)).ok();
        }
        ResponseValue::Boolean(b) => {
            output.push_str(if *b { "true" } else { "false" });
        }
        ResponseValue::Integer(i) => {
            write!(output, "{}", i).ok();
        }
        ResponseValue::List(items) => {
            output.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    output.push(' ');
                }
                write!(output, r#""{}""#, escape_string(item)).ok();
            }
            output.push(']');
        }
        ResponseValue::Map(map) => {
            output.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    output.push(' ');
                }
                write!(output, ":{} ", k).ok();
                write!(output, r#""{}""#, escape_string(v)).ok();
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
    ).ok();

    write!(
        output,
        r#":cognitect.anomalies/message "{}""#,
        escape_string(&anomaly.message)
    ).ok();

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
    use std::collections::BTreeMap;

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
    fn test_serialize_list() {
        let response = Response::Success {
            result: ResponseValue::List(vec![
                "db1".to_string(),
                "db2".to_string(),
            ]),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result ["db1" "db2"]}"#);
    }

    #[test]
    fn test_serialize_map() {
        let mut map = BTreeMap::new();
        map.insert("connection-id".to_string(), "abc-123".to_string());
        let response = Response::Success {
            result: ResponseValue::Map(map),
        };
        let output = serialize_response(&response);
        assert_eq!(output, r#"{:result {:connection-id "abc-123"}}"#);
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
