//! Transit wire format serialization for Datomic protocol compatibility.
//!
//! Transit is a data format created by Cognitect (the creators of Datomic)
//! that extends JSON/MessagePack with tagged types for keywords, symbols, UUIDs,
//! dates, and other rich types that JSON cannot natively represent.
//!
//! There is no maintained Rust Transit library on crates.io, so this module
//! implements a minimal Transit+JSON encoder following the Transit
//! specification at <https://github.com/cognitect/transit-format>.
//!
//! ## Transit+JSON encoding conventions
//!
//! - Keywords: `"~:db/name"` (string with `~:` prefix)
//! - Symbols: `"~$my-sym"` (string with `~$` prefix)
//! - Integers (> i32): `"~i12345678901234"` (tagged string)
//! - Maps with non-string keys: `["^ ", key1, val1, key2, val2, ...]`
//! - UUIDs: `"~u550e8400-e29b-41d4-a716-446655440000"`
//! - Dates: `"~m1234567890"` (milliseconds since epoch)
//!
//! ## Transit+MessagePack
//!
//! Uses the same tagging conventions but encoded in MessagePack binary format
//! for better performance and smaller payloads. This module includes a minimal
//! MessagePack encoder (no external crate required) that covers the Transit
//! value types: nil, bool, integer, string, array, and map-as-array.

use super::{Anomaly, Response, ResponseValue};
use std::fmt::Write;

/// Supported Transit output encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitEncoding {
    /// Transit+JSON: tagged JSON values (text format, human-readable)
    Json,
    /// Transit+MessagePack: tagged values in msgpack binary
    Msgpack,
}

// ---------------------------------------------------------------------------
// Transit+JSON
// ---------------------------------------------------------------------------

/// Serialize a `Response` to Transit+JSON string.
pub fn serialize_transit_json(response: &Response) -> String {
    match response {
        Response::Success { result } => {
            let mut output = String::from("[\"^ \",\"~:result\",");
            value_to_transit_json(result, &mut output);
            output.push(']');
            output
        }
        Response::Error { anomaly } => anomaly_to_transit_json(anomaly),
    }
}

/// Convert a `ResponseValue` to Transit+JSON and append to the output string.
fn value_to_transit_json(value: &ResponseValue, output: &mut String) {
    match value {
        ResponseValue::Nil => output.push_str("null"),
        ResponseValue::String(s) => {
            // Strings starting with ~ or ^ need escaping in Transit
            if s.starts_with('~') || s.starts_with('^') {
                write!(output, "\"~{}\"", escape_transit_string(s)).ok();
            } else {
                write!(output, "\"{}\"", escape_transit_string(s)).ok();
            }
        }
        ResponseValue::Boolean(b) => {
            output.push_str(if *b { "true" } else { "false" });
        }
        ResponseValue::Integer(i) => {
            // Transit encodes integers that exceed JSON safe integer range as tagged strings
            if *i > 2_147_483_647 || *i < -2_147_483_648 {
                write!(output, "\"~i{}\"", i).ok();
            } else {
                write!(output, "{}", i).ok();
            }
        }
        ResponseValue::Keyword(k) => {
            // Keywords use the ~: prefix in Transit
            write!(output, "\"~:{}\"", k).ok();
        }
        ResponseValue::List(items) => {
            // Lists are tagged arrays: ["~#list", [items...]]
            output.push_str("[\"~#list\",[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    output.push(',');
                }
                value_to_transit_json(item, output);
            }
            output.push_str("]]");
        }
        ResponseValue::Vector(items) => {
            // Vectors are plain JSON arrays in Transit
            output.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    output.push(',');
                }
                value_to_transit_json(item, output);
            }
            output.push(']');
        }
        ResponseValue::Map(entries) => {
            // Maps use the cmap form: ["^ ", key1, val1, ...]
            output.push_str("[\"^ \"");
            for (k, v) in entries {
                output.push(',');
                value_to_transit_json(k, output);
                output.push(',');
                value_to_transit_json(v, output);
            }
            output.push(']');
        }
        ResponseValue::DbSnapshot { db_id, basis_t } => {
            // DbSnapshot as a tagged value: ["~#db", ["db_id", basis_t]]
            write!(output, "[\"~#db\",[\"{}\",{}]]", db_id, basis_t).ok();
        }
    }
}

/// Serialize an anomaly (error) to Transit+JSON.
fn anomaly_to_transit_json(anomaly: &Anomaly) -> String {
    let mut output = String::from("[\"^ \",\"~:error\",[\"^ \"");

    write!(
        output,
        ",\"~:cognitect.anomalies/category\",\"~:{}\"",
        &anomaly.category.as_keyword()[1..] // strip leading :
    )
    .ok();

    write!(
        output,
        ",\"~:cognitect.anomalies/message\",\"{}\"",
        escape_transit_string(&anomaly.message)
    )
    .ok();

    if let Some(db_error) = &anomaly.db_error {
        write!(output, ",\"~:db/error\",\"~:{}\"", db_error).ok();
    }

    output.push_str("]]");
    output
}

fn escape_transit_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

// ---------------------------------------------------------------------------
// Transit+MessagePack (minimal encoder, no external crate)
// ---------------------------------------------------------------------------

/// Serialize a `Response` to Transit+MessagePack bytes.
///
/// Uses the same Transit tagging conventions as Transit+JSON but encoded
/// in the MessagePack binary format for smaller payloads and faster parsing.
pub fn serialize_transit_msgpack(response: &Response) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    match response {
        Response::Success { result } => {
            // Encode as array: ["^ ", "~:result", <value>]
            msgpack_write_array_len(&mut buf, 3);
            msgpack_write_str(&mut buf, "^ ");
            msgpack_write_str(&mut buf, "~:result");
            value_to_msgpack(result, &mut buf);
        }
        Response::Error { anomaly } => {
            anomaly_to_msgpack(anomaly, &mut buf);
        }
    }
    buf
}

/// Encode a `ResponseValue` as Transit-tagged MessagePack.
fn value_to_msgpack(value: &ResponseValue, buf: &mut Vec<u8>) {
    match value {
        ResponseValue::Nil => buf.push(0xc0),
        ResponseValue::Boolean(b) => buf.push(if *b { 0xc3 } else { 0xc2 }),
        ResponseValue::String(s) => {
            if s.starts_with('~') || s.starts_with('^') {
                let escaped = format!("~{s}");
                msgpack_write_str(buf, &escaped);
            } else {
                msgpack_write_str(buf, s);
            }
        }
        ResponseValue::Integer(i) => {
            // Transit: large ints are tagged strings; msgpack can handle
            // 64-bit ints natively, so we encode them directly.
            msgpack_write_i64(buf, *i);
        }
        ResponseValue::Keyword(k) => {
            let tagged = format!("~:{k}");
            msgpack_write_str(buf, &tagged);
        }
        ResponseValue::List(items) => {
            // ["~#list", [items...]]
            msgpack_write_array_len(buf, 2);
            msgpack_write_str(buf, "~#list");
            msgpack_write_array_len(buf, items.len());
            for item in items {
                value_to_msgpack(item, buf);
            }
        }
        ResponseValue::Vector(items) => {
            msgpack_write_array_len(buf, items.len());
            for item in items {
                value_to_msgpack(item, buf);
            }
        }
        ResponseValue::Map(entries) => {
            // Encode as array: ["^ ", k1, v1, k2, v2, ...]
            let len = 1 + entries.len() * 2;
            msgpack_write_array_len(buf, len);
            msgpack_write_str(buf, "^ ");
            for (k, v) in entries {
                value_to_msgpack(k, buf);
                value_to_msgpack(v, buf);
            }
        }
        ResponseValue::DbSnapshot { db_id, basis_t } => {
            // Encode as ["~#db", ["db_id", basis_t]]
            msgpack_write_array_len(buf, 2);
            msgpack_write_str(buf, "~#db");
            msgpack_write_array_len(buf, 2);
            msgpack_write_str(buf, db_id);
            msgpack_write_i64(buf, *basis_t);
        }
    }
}

/// Encode an anomaly as Transit+MessagePack.
fn anomaly_to_msgpack(anomaly: &Anomaly, buf: &mut Vec<u8>) {
    // Outer: ["^ ", "~:error", <error-map>]
    msgpack_write_array_len(buf, 3);
    msgpack_write_str(buf, "^ ");
    msgpack_write_str(buf, "~:error");

    // Inner error map: ["^ ", k1, v1, ...]
    let mut inner_count: usize = 5; // "^ " + category key/val + message key/val
    if anomaly.db_error.is_some() {
        inner_count += 2;
    }
    msgpack_write_array_len(buf, inner_count);
    msgpack_write_str(buf, "^ ");

    // category
    msgpack_write_str(buf, "~:cognitect.anomalies/category");
    let cat_val = format!("~:{}", &anomaly.category.as_keyword()[1..]);
    msgpack_write_str(buf, &cat_val);

    // message
    msgpack_write_str(buf, "~:cognitect.anomalies/message");
    msgpack_write_str(buf, &anomaly.message);

    // db_error
    if let Some(db_error) = &anomaly.db_error {
        msgpack_write_str(buf, "~:db/error");
        let err_val = format!("~:{db_error}");
        msgpack_write_str(buf, &err_val);
    }
}

// ---------------------------------------------------------------------------
// Minimal MessagePack encoder
//
// Implements just enough of the msgpack spec for our Transit values:
// nil, bool, fixint, int8/16/32/64, str8/16/32, fixarray/array16/array32.
// See https://github.com/msgpack/msgpack/blob/master/spec.md
// ---------------------------------------------------------------------------

fn msgpack_write_array_len(buf: &mut Vec<u8>, len: usize) {
    if len <= 15 {
        #[allow(clippy::cast_possible_truncation)]
        buf.push(0x90 | (len as u8));
    } else if len <= 0xFFFF {
        buf.push(0xdc);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        buf.push(0xdd);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&(len as u32).to_be_bytes());
    }
}

fn msgpack_write_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len <= 31 {
        #[allow(clippy::cast_possible_truncation)]
        buf.push(0xa0 | (len as u8));
    } else if len <= 0xFF {
        buf.push(0xd9);
        #[allow(clippy::cast_possible_truncation)]
        buf.push(len as u8);
    } else if len <= 0xFFFF {
        buf.push(0xda);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        buf.push(0xdb);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&(len as u32).to_be_bytes());
    }
    buf.extend_from_slice(bytes);
}

fn msgpack_write_i64(buf: &mut Vec<u8>, val: i64) {
    if val >= 0 {
        #[allow(clippy::cast_sign_loss)]
        msgpack_write_u64(buf, val as u64);
    } else if val >= -32 {
        // negative fixint: 111XXXXX
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        buf.push(val as u8);
    } else if val >= i64::from(i8::MIN) {
        buf.push(0xd0);
        #[allow(clippy::cast_possible_truncation)]
        buf.push(val as u8);
    } else if val >= i64::from(i16::MIN) {
        buf.push(0xd1);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&(val as i16).to_be_bytes());
    } else if val >= i64::from(i32::MIN) {
        buf.push(0xd2);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&(val as i32).to_be_bytes());
    } else {
        buf.push(0xd3);
        buf.extend_from_slice(&val.to_be_bytes());
    }
}

fn msgpack_write_u64(buf: &mut Vec<u8>, val: u64) {
    if val <= 127 {
        #[allow(clippy::cast_possible_truncation)]
        buf.push(val as u8);
    } else if val <= u64::from(u8::MAX) {
        buf.push(0xcc);
        #[allow(clippy::cast_possible_truncation)]
        buf.push(val as u8);
    } else if val <= u64::from(u16::MAX) {
        buf.push(0xcd);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&(val as u16).to_be_bytes());
    } else if val <= u64::from(u32::MAX) {
        buf.push(0xce);
        #[allow(clippy::cast_possible_truncation)]
        buf.extend_from_slice(&(val as u32).to_be_bytes());
    } else {
        buf.push(0xcf);
        buf.extend_from_slice(&val.to_be_bytes());
    }
}

// ---------------------------------------------------------------------------
// Content-type negotiation
// ---------------------------------------------------------------------------

/// Parse the Accept header to determine the preferred response encoding.
///
/// Recognizes:
/// - `application/transit+json` -> `TransitEncoding::Json`
/// - `application/transit+msgpack` -> `TransitEncoding::Msgpack`
/// - `application/edn` or anything else -> `None` (use default EDN)
pub fn parse_accept_encoding(accept: &str) -> Option<TransitEncoding> {
    let mut best_encoding = None;
    let mut best_quality: f32 = -1.0;

    for part in accept.split(',') {
        let part = part.trim();
        let (media_type, quality) = parse_media_type_with_quality(part);

        let encoding = match media_type {
            "application/transit+json" => Some(TransitEncoding::Json),
            "application/transit+msgpack" => Some(TransitEncoding::Msgpack),
            _ => None,
        };

        if let Some(enc) = encoding {
            if quality > best_quality {
                best_quality = quality;
                best_encoding = Some(enc);
            }
        }
    }

    best_encoding
}

/// Parse a media type entry, returning (type, quality).
fn parse_media_type_with_quality(entry: &str) -> (&str, f32) {
    let parts: Vec<&str> = entry.split(';').collect();
    let media_type = parts[0].trim();
    let mut quality: f32 = 1.0;

    for param in parts.iter().skip(1) {
        let param = param.trim();
        if let Some(q_str) = param.strip_prefix("q=") {
            quality = q_str.parse().unwrap_or(1.0);
        }
    }

    (media_type, quality)
}

/// Returns the Content-Type string for the given encoding.
pub fn content_type_for_encoding(encoding: TransitEncoding) -> &'static str {
    match encoding {
        TransitEncoding::Json => "application/transit+json",
        TransitEncoding::Msgpack => "application/transit+msgpack",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::AnomalyCategory;

    // -- Transit+JSON tests ------------------------------------------------

    #[test]
    fn test_transit_json_string() {
        let response = Response::Success {
            result: ResponseValue::String("hello".to_string()),
        };
        let output = serialize_transit_json(&response);
        assert_eq!(output, r#"["^ ","~:result","hello"]"#);
    }

    #[test]
    fn test_transit_json_nil() {
        let response = Response::Success {
            result: ResponseValue::Nil,
        };
        let output = serialize_transit_json(&response);
        assert_eq!(output, r#"["^ ","~:result",null]"#);
    }

    #[test]
    fn test_transit_json_boolean() {
        let response = Response::Success {
            result: ResponseValue::Boolean(true),
        };
        let output = serialize_transit_json(&response);
        assert_eq!(output, r#"["^ ","~:result",true]"#);
    }

    #[test]
    fn test_transit_json_small_integer() {
        let response = Response::Success {
            result: ResponseValue::Integer(42),
        };
        let output = serialize_transit_json(&response);
        assert_eq!(output, r#"["^ ","~:result",42]"#);
    }

    #[test]
    fn test_transit_json_large_integer() {
        let response = Response::Success {
            result: ResponseValue::Integer(9_999_999_999),
        };
        let output = serialize_transit_json(&response);
        assert_eq!(output, r#"["^ ","~:result","~i9999999999"]"#);
    }

    #[test]
    fn test_transit_json_keyword() {
        let response = Response::Success {
            result: ResponseValue::Keyword("db/name".to_string()),
        };
        let output = serialize_transit_json(&response);
        assert_eq!(output, r#"["^ ","~:result","~:db/name"]"#);
    }

    #[test]
    fn test_transit_json_vector() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![
                ResponseValue::Integer(1),
                ResponseValue::String("two".to_string()),
            ]),
        };
        let output = serialize_transit_json(&response);
        assert_eq!(output, r#"["^ ","~:result",[1,"two"]]"#);
    }

    #[test]
    fn test_transit_json_list() {
        let response = Response::Success {
            result: ResponseValue::List(vec![
                ResponseValue::Integer(1),
                ResponseValue::Integer(2),
            ]),
        };
        let output = serialize_transit_json(&response);
        assert_eq!(output, r#"["^ ","~:result",["~#list",[1,2]]]"#);
    }

    #[test]
    fn test_transit_json_map() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![(
                ResponseValue::Keyword("connection-id".to_string()),
                ResponseValue::String("abc-123".to_string()),
            )]),
        };
        let output = serialize_transit_json(&response);
        assert_eq!(
            output,
            r#"["^ ","~:result",["^ ","~:connection-id","abc-123"]]"#
        );
    }

    #[test]
    fn test_transit_json_nested_vectors() {
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
        let output = serialize_transit_json(&response);
        assert_eq!(
            output,
            r#"["^ ","~:result",[[10001,"Alice"],[10002,"Bob"]]]"#
        );
    }

    #[test]
    fn test_transit_json_error() {
        let response = Response::Error {
            anomaly: Anomaly {
                category: AnomalyCategory::NotFound,
                message: "Database not found".to_string(),
                db_error: Some("db.error/not-found".to_string()),
            },
        };
        let output = serialize_transit_json(&response);
        assert!(output.contains("~:cognitect.anomalies/not-found"));
        assert!(output.contains("Database not found"));
        assert!(output.contains("~:db.error/not-found"));
    }

    #[test]
    fn test_transit_json_string_escape_tilde() {
        let response = Response::Success {
            result: ResponseValue::String("~special".to_string()),
        };
        let output = serialize_transit_json(&response);
        assert_eq!(output, r#"["^ ","~:result","~~special"]"#);
    }

    // -- Transit+MessagePack tests -----------------------------------------

    #[test]
    fn test_transit_msgpack_nonempty() {
        let response = Response::Success {
            result: ResponseValue::Integer(42),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(!bytes.is_empty());
        // First byte should be fixarray(3) = 0x93
        assert_eq!(bytes[0], 0x93);
    }

    #[test]
    fn test_transit_msgpack_nil() {
        let response = Response::Success {
            result: ResponseValue::Nil,
        };
        let bytes = serialize_transit_msgpack(&response);
        // fixarray(3), fixstr "^ ", fixstr "~:result", nil
        assert!(bytes.contains(&0xc0));
    }

    #[test]
    fn test_transit_msgpack_boolean() {
        let response_t = Response::Success {
            result: ResponseValue::Boolean(true),
        };
        let bytes_t = serialize_transit_msgpack(&response_t);
        assert!(bytes_t.contains(&0xc3)); // true

        let response_f = Response::Success {
            result: ResponseValue::Boolean(false),
        };
        let bytes_f = serialize_transit_msgpack(&response_f);
        assert!(bytes_f.contains(&0xc2)); // false
    }

    #[test]
    fn test_transit_msgpack_string_content() {
        let response = Response::Success {
            result: ResponseValue::String("hello".to_string()),
        };
        let bytes = serialize_transit_msgpack(&response);
        // "hello" should appear in the binary output
        let hello_bytes = b"hello";
        assert!(bytes
            .windows(hello_bytes.len())
            .any(|w| w == hello_bytes));
    }

    #[test]
    fn test_transit_msgpack_keyword_tagged() {
        let response = Response::Success {
            result: ResponseValue::Keyword("db/name".to_string()),
        };
        let bytes = serialize_transit_msgpack(&response);
        // "~:db/name" should appear in binary
        let kw_bytes = b"~:db/name";
        assert!(bytes.windows(kw_bytes.len()).any(|w| w == kw_bytes));
    }

    #[test]
    fn test_transit_msgpack_vector() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![
                ResponseValue::Integer(1),
                ResponseValue::Integer(2),
                ResponseValue::Integer(3),
            ]),
        };
        let bytes = serialize_transit_msgpack(&response);
        // The inner vector [1, 2, 3] should be fixarray(3) = 0x93
        // It appears somewhere in the byte stream
        assert!(bytes.contains(&0x93));
    }

    #[test]
    fn test_transit_msgpack_large_integer() {
        let response = Response::Success {
            result: ResponseValue::Integer(1_000_000),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(!bytes.is_empty());
        // 1_000_000 = 0x000F_4240, encoded as uint32 (0xce prefix)
        assert!(bytes.contains(&0xce));
    }

    #[test]
    fn test_transit_msgpack_negative_integer() {
        let response = Response::Success {
            result: ResponseValue::Integer(-100),
        };
        let bytes = serialize_transit_msgpack(&response);
        // -100 fits in int8 (0xd0 prefix)
        assert!(bytes.contains(&0xd0));
    }

    #[test]
    fn test_transit_msgpack_error() {
        let response = Response::Error {
            anomaly: Anomaly {
                category: AnomalyCategory::Fault,
                message: "test error".to_string(),
                db_error: None,
            },
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(!bytes.is_empty());
        // Should contain the category string
        let cat = b"cognitect.anomalies/fault";
        assert!(bytes.windows(cat.len()).any(|w| w == cat));
    }

    // -- Minimal msgpack encoder unit tests --------------------------------

    #[test]
    fn test_msgpack_fixint() {
        let mut buf = Vec::new();
        msgpack_write_i64(&mut buf, 0);
        assert_eq!(buf, vec![0x00]);

        buf.clear();
        msgpack_write_i64(&mut buf, 127);
        assert_eq!(buf, vec![0x7f]);

        buf.clear();
        msgpack_write_i64(&mut buf, -1);
        assert_eq!(buf, vec![0xff]); // negative fixint

        buf.clear();
        msgpack_write_i64(&mut buf, -32);
        assert_eq!(buf, vec![0xe0]); // negative fixint min
    }

    #[test]
    fn test_msgpack_int8() {
        let mut buf = Vec::new();
        msgpack_write_i64(&mut buf, -33);
        assert_eq!(buf, vec![0xd0, 0xdf]); // int8 format
    }

    #[test]
    fn test_msgpack_uint8() {
        let mut buf = Vec::new();
        msgpack_write_i64(&mut buf, 200);
        assert_eq!(buf, vec![0xcc, 200]); // uint8
    }

    #[test]
    fn test_msgpack_fixstr() {
        let mut buf = Vec::new();
        msgpack_write_str(&mut buf, "abc");
        assert_eq!(buf, vec![0xa3, b'a', b'b', b'c']);
    }

    #[test]
    fn test_msgpack_fixarray() {
        let mut buf = Vec::new();
        msgpack_write_array_len(&mut buf, 3);
        assert_eq!(buf, vec![0x93]);
    }

    // -- Content-type negotiation tests ------------------------------------

    #[test]
    fn test_parse_accept_transit_json() {
        assert_eq!(
            parse_accept_encoding("application/transit+json"),
            Some(TransitEncoding::Json)
        );
    }

    #[test]
    fn test_parse_accept_transit_msgpack() {
        assert_eq!(
            parse_accept_encoding("application/transit+msgpack"),
            Some(TransitEncoding::Msgpack)
        );
    }

    #[test]
    fn test_parse_accept_edn() {
        assert_eq!(parse_accept_encoding("application/edn"), None);
    }

    #[test]
    fn test_parse_accept_with_quality() {
        assert_eq!(
            parse_accept_encoding(
                "application/edn;q=0.8, application/transit+json;q=0.9, application/transit+msgpack;q=1.0"
            ),
            Some(TransitEncoding::Msgpack)
        );
    }

    #[test]
    fn test_parse_accept_transit_json_preferred_over_msgpack() {
        assert_eq!(
            parse_accept_encoding(
                "application/transit+json;q=1.0, application/transit+msgpack;q=0.5"
            ),
            Some(TransitEncoding::Json)
        );
    }

    #[test]
    fn test_parse_accept_wildcard() {
        assert_eq!(parse_accept_encoding("*/*"), None);
    }

    #[test]
    fn test_content_type_for_encoding() {
        assert_eq!(
            content_type_for_encoding(TransitEncoding::Json),
            "application/transit+json"
        );
        assert_eq!(
            content_type_for_encoding(TransitEncoding::Msgpack),
            "application/transit+msgpack"
        );
    }
}
