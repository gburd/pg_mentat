//! Transit wire format input parsing for Datomic protocol compatibility.
//!
//! Parses Transit+JSON and Transit+MessagePack encoded request bodies into
//! [`Operation`] values. This complements the Transit serializer which handles
//! output encoding.
//!
//! ## Transit+JSON decoding
//!
//! Transit+JSON uses tagged strings to represent rich types:
//! - `"~:db/name"` -> keyword `:db/name`
//! - `"~$my-sym"` -> symbol `my-sym`
//! - `"~i12345678901234"` -> large integer
//! - `"~uUUID-STRING"` -> UUID
//! - `["^ ", k1, v1, k2, v2, ...]` -> map
//! - `["~#list", [items...]]` -> list
//!
//! ## Transit+MessagePack decoding
//!
//! Same tagging conventions but values are encoded in MessagePack binary format.

use super::{FilterPredicate, Operation, Request};
use crate::protocol::parser::ParseError;
use uuid::Uuid;

/// An intermediate Transit value before conversion to an `Operation`.
#[derive(Debug, Clone, PartialEq)]
enum TransitValue {
    Nil,
    Bool(bool),
    Integer(i64),
    String(String),
    Keyword(String),
    Symbol(String),
    Uuid(Uuid),
    Array(Vec<TransitValue>),
    Map(Vec<(TransitValue, TransitValue)>),
}

/// Maximum recursion depth for nested Transit structures.
///
/// Prevents stack overflow from deeply nested arrays/maps in crafted payloads.
/// Datomic protocol messages rarely exceed 10 levels of nesting; 64 provides
/// ample headroom while guarding against abuse.
const MAX_NESTING_DEPTH: usize = 64;

// ---------------------------------------------------------------------------
// Transit+JSON parser
// ---------------------------------------------------------------------------

/// Parse a Transit+JSON encoded request body into a `Request`.
pub fn parse_transit_json(input: &str) -> Result<Request, ParseError> {
    let json: serde_json::Value =
        serde_json::from_str(input).map_err(|e| ParseError::Edn(format!("Invalid Transit+JSON: {e}")))?;
    let transit_val = json_to_transit_bounded(&json, 0)?;
    transit_to_request(&transit_val)
}

/// Convert a `serde_json::Value` to a `TransitValue`, interpreting Transit tags.
///
/// Enforces a maximum nesting depth to prevent stack overflow from deeply nested
/// payloads. Returns a `ParseError` if the depth limit is exceeded.
fn json_to_transit_bounded(val: &serde_json::Value, depth: usize) -> Result<TransitValue, ParseError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(ParseError::Edn(format!(
            "Transit nesting depth exceeds maximum ({MAX_NESTING_DEPTH})"
        )));
    }
    match val {
        serde_json::Value::Null => Ok(TransitValue::Nil),
        serde_json::Value::Bool(b) => Ok(TransitValue::Bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(TransitValue::Integer(i))
            } else if let Some(u) = n.as_u64() {
                #[allow(clippy::cast_possible_wrap)]
                Ok(TransitValue::Integer(u as i64))
            } else {
                // Floating point -- represent as string
                Ok(TransitValue::String(n.to_string()))
            }
        }
        serde_json::Value::String(s) => Ok(decode_transit_tagged_string(s)),
        serde_json::Value::Array(arr) => decode_transit_array_bounded(arr, depth + 1),
        serde_json::Value::Object(obj) => {
            let mut entries = Vec::with_capacity(obj.len());
            for (k, v) in obj.iter() {
                let key = decode_transit_tagged_string(k);
                let value = json_to_transit_bounded(v, depth + 1)?;
                entries.push((key, value));
            }
            Ok(TransitValue::Map(entries))
        }
    }
}

/// Decode a Transit tagged string like `"~:keyword"`, `"~$symbol"`, `"~iNNN"`, etc.
fn decode_transit_tagged_string(s: &str) -> TransitValue {
    if let Some(rest) = s.strip_prefix("~:") {
        TransitValue::Keyword(rest.to_string())
    } else if let Some(rest) = s.strip_prefix("~$") {
        TransitValue::Symbol(rest.to_string())
    } else if let Some(rest) = s.strip_prefix("~i") {
        rest.parse::<i64>()
            .map(TransitValue::Integer)
            .unwrap_or_else(|_| TransitValue::String(s.to_string()))
    } else if let Some(rest) = s.strip_prefix("~u") {
        rest.parse::<Uuid>()
            .map(TransitValue::Uuid)
            .unwrap_or_else(|_| TransitValue::String(s.to_string()))
    } else if let Some(rest) = s.strip_prefix("~m") {
        // Instant as millis -- store as integer
        rest.parse::<i64>()
            .map(TransitValue::Integer)
            .unwrap_or_else(|_| TransitValue::String(s.to_string()))
    } else if let Some(rest) = s.strip_prefix("~~") {
        // Escaped tilde
        TransitValue::String(format!("~{rest}"))
    } else if let Some(rest) = s.strip_prefix("~^") {
        // Escaped caret
        TransitValue::String(format!("^{rest}"))
    } else {
        TransitValue::String(s.to_string())
    }
}

/// Decode a Transit JSON array, handling special forms like cmap (`["^ ", ...]`)
/// and tagged values (`["~#list", ...]`, `["~#set", ...]`).
///
/// Enforces nesting depth to prevent stack overflow.
fn decode_transit_array_bounded(arr: &[serde_json::Value], depth: usize) -> Result<TransitValue, ParseError> {
    if arr.is_empty() {
        return Ok(TransitValue::Array(Vec::new()));
    }

    // Check for cmap marker: ["^ ", k1, v1, k2, v2, ...]
    if let Some(serde_json::Value::String(marker)) = arr.first() {
        if marker == "^ " {
            let mut entries = Vec::new();
            let mut i = 1;
            while i + 1 < arr.len() {
                let key = json_to_transit_bounded(&arr[i], depth)?;
                let value = json_to_transit_bounded(&arr[i + 1], depth)?;
                entries.push((key, value));
                i += 2;
            }
            return Ok(TransitValue::Map(entries));
        }

        // Check for tagged values like ["~#list", [items...]]
        if let Some(tag) = marker.strip_prefix("~#") {
            match tag {
                "list" => {
                    if let Some(items_val) = arr.get(1) {
                        if let serde_json::Value::Array(items) = items_val {
                            let converted: Result<Vec<_>, _> =
                                items.iter().map(|v| json_to_transit_bounded(v, depth)).collect();
                            return Ok(TransitValue::Array(converted?));
                        }
                    }
                    return Ok(TransitValue::Array(Vec::new()));
                }
                "set" => {
                    if let Some(items_val) = arr.get(1) {
                        if let serde_json::Value::Array(items) = items_val {
                            let converted: Result<Vec<_>, _> =
                                items.iter().map(|v| json_to_transit_bounded(v, depth)).collect();
                            return Ok(TransitValue::Array(converted?));
                        }
                    }
                    return Ok(TransitValue::Array(Vec::new()));
                }
                _ => {}
            }
        }
    }

    // Regular array
    let converted: Result<Vec<_>, _> =
        arr.iter().map(|v| json_to_transit_bounded(v, depth)).collect();
    Ok(TransitValue::Array(converted?))
}

// ---------------------------------------------------------------------------
// Transit+MessagePack parser
// ---------------------------------------------------------------------------

/// Parse a Transit+MessagePack encoded request body into a `Request`.
pub fn parse_transit_msgpack(input: &[u8]) -> Result<Request, ParseError> {
    let (transit_val, _remaining) = msgpack_read_value_bounded(input, 0)
        .map_err(|e| ParseError::Edn(format!("Invalid Transit+MessagePack: {e}")))?;
    transit_to_request(&transit_val)
}

/// Error type for MessagePack decoding.
#[derive(Debug)]
struct MsgpackError(String);

impl std::fmt::Display for MsgpackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Read a single MessagePack value from the input buffer, returning the decoded
/// `TransitValue` and the remaining unconsumed bytes.
///
/// Enforces a maximum nesting depth to prevent stack overflow from deeply nested payloads.
fn msgpack_read_value_bounded(buf: &[u8], depth: usize) -> Result<(TransitValue, &[u8]), MsgpackError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(MsgpackError(format!(
            "MessagePack nesting depth exceeds maximum ({MAX_NESTING_DEPTH})"
        )));
    }
    if buf.is_empty() {
        return Err(MsgpackError("Unexpected end of input".to_string()));
    }

    let first = buf[0];
    let rest = &buf[1..];

    match first {
        // nil
        0xc0 => Ok((TransitValue::Nil, rest)),
        // false
        0xc2 => Ok((TransitValue::Bool(false), rest)),
        // true
        0xc3 => Ok((TransitValue::Bool(true), rest)),

        // positive fixint: 0XXXXXXX (0x00 - 0x7f)
        0x00..=0x7f => Ok((TransitValue::Integer(i64::from(first)), rest)),

        // negative fixint: 111XXXXX (0xe0 - 0xff)
        0xe0..=0xff => {
            #[allow(clippy::cast_possible_wrap)]
            let val = first as i8;
            Ok((TransitValue::Integer(i64::from(val)), rest))
        }

        // uint8
        0xcc => {
            ensure_len(rest, 1)?;
            Ok((TransitValue::Integer(i64::from(rest[0])), &rest[1..]))
        }
        // uint16
        0xcd => {
            ensure_len(rest, 2)?;
            let val = u16::from_be_bytes([rest[0], rest[1]]);
            Ok((TransitValue::Integer(i64::from(val)), &rest[2..]))
        }
        // uint32
        0xce => {
            ensure_len(rest, 4)?;
            let val = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]);
            Ok((TransitValue::Integer(i64::from(val)), &rest[4..]))
        }
        // uint64
        0xcf => {
            ensure_len(rest, 8)?;
            let val = u64::from_be_bytes([
                rest[0], rest[1], rest[2], rest[3], rest[4], rest[5], rest[6], rest[7],
            ]);
            #[allow(clippy::cast_possible_wrap)]
            Ok((TransitValue::Integer(val as i64), &rest[8..]))
        }

        // int8
        0xd0 => {
            ensure_len(rest, 1)?;
            #[allow(clippy::cast_possible_wrap)]
            let val = rest[0] as i8;
            Ok((TransitValue::Integer(i64::from(val)), &rest[1..]))
        }
        // int16
        0xd1 => {
            ensure_len(rest, 2)?;
            let val = i16::from_be_bytes([rest[0], rest[1]]);
            Ok((TransitValue::Integer(i64::from(val)), &rest[2..]))
        }
        // int32
        0xd2 => {
            ensure_len(rest, 4)?;
            let val = i32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]);
            Ok((TransitValue::Integer(i64::from(val)), &rest[4..]))
        }
        // int64
        0xd3 => {
            ensure_len(rest, 8)?;
            let val = i64::from_be_bytes([
                rest[0], rest[1], rest[2], rest[3], rest[4], rest[5], rest[6], rest[7],
            ]);
            Ok((TransitValue::Integer(val), &rest[8..]))
        }

        // fixstr: 101XXXXX (0xa0 - 0xbf)
        b @ 0xa0..=0xbf => {
            let len = (b & 0x1f) as usize;
            ensure_len(rest, len)?;
            let s = std::str::from_utf8(&rest[..len])
                .map_err(|e| MsgpackError(format!("Invalid UTF-8 in fixstr: {e}")))?;
            let transit_val = decode_transit_tagged_string(s);
            Ok((transit_val, &rest[len..]))
        }
        // str8
        0xd9 => {
            ensure_len(rest, 1)?;
            let len = rest[0] as usize;
            ensure_len(&rest[1..], len)?;
            let s = std::str::from_utf8(&rest[1..1 + len])
                .map_err(|e| MsgpackError(format!("Invalid UTF-8 in str8: {e}")))?;
            let transit_val = decode_transit_tagged_string(s);
            Ok((transit_val, &rest[1 + len..]))
        }
        // str16
        0xda => {
            ensure_len(rest, 2)?;
            let len = u16::from_be_bytes([rest[0], rest[1]]) as usize;
            ensure_len(&rest[2..], len)?;
            let s = std::str::from_utf8(&rest[2..2 + len])
                .map_err(|e| MsgpackError(format!("Invalid UTF-8 in str16: {e}")))?;
            let transit_val = decode_transit_tagged_string(s);
            Ok((transit_val, &rest[2 + len..]))
        }
        // str32
        0xdb => {
            ensure_len(rest, 4)?;
            let len =
                u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            ensure_len(&rest[4..], len)?;
            let s = std::str::from_utf8(&rest[4..4 + len])
                .map_err(|e| MsgpackError(format!("Invalid UTF-8 in str32: {e}")))?;
            let transit_val = decode_transit_tagged_string(s);
            Ok((transit_val, &rest[4 + len..]))
        }

        // fixarray: 1001XXXX (0x90 - 0x9f)
        b @ 0x90..=0x9f => {
            let len = (b & 0x0f) as usize;
            msgpack_read_transit_array(rest, len, depth + 1)
        }
        // array16
        0xdc => {
            ensure_len(rest, 2)?;
            let len = u16::from_be_bytes([rest[0], rest[1]]) as usize;
            msgpack_read_transit_array(&rest[2..], len, depth + 1)
        }
        // array32
        0xdd => {
            ensure_len(rest, 4)?;
            let len =
                u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            msgpack_read_transit_array(&rest[4..], len, depth + 1)
        }

        // fixmap: 1000XXXX (0x80 - 0x8f)
        b @ 0x80..=0x8f => {
            let len = (b & 0x0f) as usize;
            msgpack_read_map(rest, len, depth + 1)
        }
        // map16
        0xde => {
            ensure_len(rest, 2)?;
            let len = u16::from_be_bytes([rest[0], rest[1]]) as usize;
            msgpack_read_map(&rest[2..], len, depth + 1)
        }
        // map32
        0xdf => {
            ensure_len(rest, 4)?;
            let len =
                u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            msgpack_read_map(&rest[4..], len, depth + 1)
        }

        // bin8
        0xc4 => {
            ensure_len(rest, 1)?;
            let len = rest[0] as usize;
            ensure_len(&rest[1..], len)?;
            let bytes = &rest[1..1 + len];
            Ok((
                TransitValue::String(format!("0x{}", hex::encode(bytes))),
                &rest[1 + len..],
            ))
        }
        // bin16
        0xc5 => {
            ensure_len(rest, 2)?;
            let len = u16::from_be_bytes([rest[0], rest[1]]) as usize;
            ensure_len(&rest[2..], len)?;
            let bytes = &rest[2..2 + len];
            Ok((
                TransitValue::String(format!("0x{}", hex::encode(bytes))),
                &rest[2 + len..],
            ))
        }
        // bin32
        0xc6 => {
            ensure_len(rest, 4)?;
            let len =
                u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            ensure_len(&rest[4..], len)?;
            let bytes = &rest[4..4 + len];
            Ok((
                TransitValue::String(format!("0x{}", hex::encode(bytes))),
                &rest[4 + len..],
            ))
        }

        // float32
        0xca => {
            ensure_len(rest, 4)?;
            let val = f32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]);
            Ok((TransitValue::String(val.to_string()), &rest[4..]))
        }
        // float64
        0xcb => {
            ensure_len(rest, 8)?;
            let val = f64::from_be_bytes([
                rest[0], rest[1], rest[2], rest[3], rest[4], rest[5], rest[6], rest[7],
            ]);
            Ok((TransitValue::String(val.to_string()), &rest[8..]))
        }

        other => Err(MsgpackError(format!(
            "Unsupported MessagePack type byte: 0x{other:02x}"
        ))),
    }
}

fn ensure_len(buf: &[u8], needed: usize) -> Result<(), MsgpackError> {
    if buf.len() < needed {
        Err(MsgpackError(format!(
            "Need {needed} bytes but only {} available",
            buf.len()
        )))
    } else {
        Ok(())
    }
}

/// Read `count` msgpack values as an array, then apply Transit array decoding
/// (cmap detection, tagged forms).
fn msgpack_read_transit_array(mut buf: &[u8], count: usize, depth: usize) -> Result<(TransitValue, &[u8]), MsgpackError> {
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        let (val, remaining) = msgpack_read_value_bounded(buf, depth)?;
        items.push(val);
        buf = remaining;
    }

    // Check for Transit cmap: first element is the string "^ "
    if let Some(TransitValue::String(marker)) = items.first() {
        if marker == "^ " {
            let mut entries = Vec::new();
            let mut i = 1;
            while i + 1 < items.len() {
                let key = items[i].clone();
                let value = items[i + 1].clone();
                entries.push((key, value));
                i += 2;
            }
            return Ok((TransitValue::Map(entries), buf));
        }
    }

    // Check for tagged forms: ["~#list", [...]] etc.
    if items.len() == 2 {
        if let TransitValue::String(ref tag) = items[0] {
            if tag == "~#list" || tag == "~#set" {
                if let TransitValue::Array(inner) = &items[1] {
                    return Ok((TransitValue::Array(inner.clone()), buf));
                }
            }
        }
    }

    Ok((TransitValue::Array(items), buf))
}

/// Read `count` msgpack key-value pairs as a native msgpack map.
fn msgpack_read_map(mut buf: &[u8], count: usize, depth: usize) -> Result<(TransitValue, &[u8]), MsgpackError> {
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let (key, remaining) = msgpack_read_value_bounded(buf, depth)?;
        let (value, remaining2) = msgpack_read_value_bounded(remaining, depth)?;
        entries.push((key, value));
        buf = remaining2;
    }
    Ok((TransitValue::Map(entries), buf))
}

// ---------------------------------------------------------------------------
// Transit -> Operation conversion
// ---------------------------------------------------------------------------

/// Convert a parsed `TransitValue` (which should be a map) into a `Request`.
fn transit_to_request(val: &TransitValue) -> Result<Request, ParseError> {
    let map = match val {
        TransitValue::Map(entries) => entries,
        _ => return Err(ParseError::InvalidType("request must be a map".to_string())),
    };

    let op_val = map_get(map, "op")
        .ok_or_else(|| ParseError::MissingField("op".to_string()))?;

    let op_keyword = match op_val {
        TransitValue::Keyword(k) => k.as_str(),
        _ => return Err(ParseError::InvalidType("op must be a keyword".to_string())),
    };

    let op = parse_transit_operation(op_keyword, map)?;
    Ok(Request { op })
}

fn parse_transit_operation(
    op_keyword: &str,
    map: &[(TransitValue, TransitValue)],
) -> Result<Operation, ParseError> {
    match op_keyword {
        "list-dbs" | "datomic.catalog/list-dbs" => Ok(Operation::ListDatabases),
        "health" => Ok(Operation::Health),

        "create-db" | "datomic.catalog/create-db" => {
            let args = get_args_map(map)?;
            let db_name = get_string_from_map(&args, "db-name")?;
            Ok(Operation::CreateDatabase { db_name })
        }

        "delete-db" | "datomic.catalog/delete-db" => {
            let args = get_args_map(map)?;
            let db_name = get_string_from_map(&args, "db-name")?;
            Ok(Operation::DeleteDatabase { db_name })
        }

        "connect" => {
            let args = get_args_map(map)?;
            let db_name = get_string_from_map(&args, "db-name")?;
            Ok(Operation::Connect { db_name })
        }

        "db" => {
            let args = get_args_map(map)?;
            let conn_id_str = get_string_from_map(&args, "connection-id")?;
            let uuid = conn_id_str.parse().map_err(|_| {
                ParseError::InvalidType("connection-id must be valid UUID".to_string())
            })?;
            Ok(Operation::Db {
                connection_id: uuid,
            })
        }

        "q" => {
            let args = get_args_map(map)?;
            let query = get_value_as_string(&args, "query")?;
            let query_args = get_optional_string_vector(&args, "args");
            let timeout = get_optional_int(&args, "timeout").map(|i| i as u64);
            let limit = get_optional_int(&args, "limit").map(|i| i as usize);
            let offset = get_optional_int(&args, "offset").map(|i| i as usize);
            let db_id = get_optional_string(&args, "db-id");

            Ok(Operation::Query {
                query,
                args: query_args,
                timeout,
                limit,
                offset,
                db_id,
            })
        }

        "transact" => {
            let args = get_args_map(map)?;
            let connection_id = get_string_from_map(&args, "connection-id")?;
            let tx_data = get_value_as_string(&args, "tx-data")?;
            Ok(Operation::Transact {
                connection_id,
                tx_data,
            })
        }

        "pull" => {
            let args = get_args_map(map)?;
            let pattern = get_value_as_string(&args, "pattern")?;
            let entity_id = get_required_int(&args, "entity-id")?;
            Ok(Operation::Pull { pattern, entity_id })
        }

        "datoms" => {
            let args = get_args_map(map)?;
            let index_str = get_value_as_string(&args, "index")?;
            let index = parse_datoms_index(&index_str)?;
            let components = get_optional_string_vector(&args, "components");
            Ok(Operation::Datoms { index, components })
        }

        "as-of" => {
            let args = get_args_map(map)?;
            let query = get_value_as_string(&args, "query")?;
            let query_args = get_optional_string_vector(&args, "args");
            let t = get_required_int(&args, "t")?;
            Ok(Operation::AsOf {
                query,
                args: query_args,
                t,
            })
        }

        "since" => {
            let args = get_args_map(map)?;
            let query = get_value_as_string(&args, "query")?;
            let query_args = get_optional_string_vector(&args, "args");
            let t = get_required_int(&args, "t")?;
            Ok(Operation::Since {
                query,
                args: query_args,
                t,
            })
        }

        "history" => {
            let args = get_args_map(map)?;
            let query = get_value_as_string(&args, "query")?;
            let query_args = get_optional_string_vector(&args, "args");
            Ok(Operation::History {
                query,
                args: query_args,
            })
        }

        "tx-range" => {
            let args = get_args_map(map)?;
            let start = get_optional_int(&args, "start");
            let end = get_optional_int(&args, "end");
            Ok(Operation::TxRange { start, end })
        }

        "with" => {
            let args = get_args_map(map)?;
            let tx_data = get_value_as_string(&args, "tx-data")?;
            Ok(Operation::With { tx_data })
        }

        "filter" => {
            let args = get_args_map(map)?;
            let predicate = parse_filter_predicate(&args)?;
            let query = get_value_as_string(&args, "query")?;
            let query_args = get_optional_string_vector(&args, "args");
            Ok(Operation::Filter {
                predicate,
                query,
                args: query_args,
            })
        }

        "basis-t" => Ok(Operation::BasisT),

        _ => Err(ParseError::InvalidOperation(op_keyword.to_string())),
    }
}

// ---------------------------------------------------------------------------
// TransitValue map helpers
// ---------------------------------------------------------------------------

/// Parse a filter predicate from a Transit map.
fn parse_filter_predicate(
    args: &[(TransitValue, TransitValue)],
) -> Result<FilterPredicate, ParseError> {
    let pred_val = map_get(args, "predicate")
        .ok_or_else(|| ParseError::MissingField("predicate".to_string()))?;

    let pred_map = match pred_val {
        TransitValue::Map(entries) => entries,
        _ => {
            return Err(ParseError::InvalidType(
                "predicate must be a map with :type and :value".to_string(),
            ))
        }
    };

    let pred_type = match map_get(pred_map, "type") {
        Some(TransitValue::Keyword(k)) => k.clone(),
        _ => {
            return Err(ParseError::MissingField(
                "predicate :type".to_string(),
            ))
        }
    };

    let pred_value = map_get(pred_map, "value")
        .ok_or_else(|| ParseError::MissingField("predicate :value".to_string()))?;

    match pred_type.as_str() {
        "attr-equals" => {
            let attr = transit_value_to_edn_string(pred_value);
            Ok(FilterPredicate::AttrEquals(attr))
        }
        "entity-equals" => match pred_value {
            TransitValue::Integer(i) => Ok(FilterPredicate::EntityEquals(*i)),
            _ => Err(ParseError::InvalidType(
                "entity-equals :value must be an integer".to_string(),
            )),
        },
        "since" => match pred_value {
            TransitValue::Integer(i) => Ok(FilterPredicate::Since(*i)),
            _ => Err(ParseError::InvalidType(
                "since :value must be an integer".to_string(),
            )),
        },
        "custom" => match pred_value {
            TransitValue::String(s) => Ok(FilterPredicate::Custom(s.clone())),
            _ => Err(ParseError::InvalidType(
                "custom :value must be a string".to_string(),
            )),
        },
        other => Err(ParseError::InvalidOperation(format!(
            "Unknown filter predicate type: {other}"
        ))),
    }
}

/// Look up a key by name in a Transit map. Checks both keyword and string keys.
fn map_get<'a>(
    entries: &'a [(TransitValue, TransitValue)],
    key: &str,
) -> Option<&'a TransitValue> {
    entries.iter().find_map(|(k, v)| match k {
        TransitValue::Keyword(kw) if kw == key => Some(v),
        TransitValue::String(s) if s == key => Some(v),
        _ => None,
    })
}

/// Extract the `args` sub-map from a Transit request map.
fn get_args_map(
    map: &[(TransitValue, TransitValue)],
) -> Result<Vec<(TransitValue, TransitValue)>, ParseError> {
    match map_get(map, "args") {
        Some(TransitValue::Map(entries)) => Ok(entries.clone()),
        _ => Err(ParseError::MissingField("args".to_string())),
    }
}

/// Extract a string value from a Transit map by keyword key.
fn get_string_from_map(
    entries: &[(TransitValue, TransitValue)],
    key: &str,
) -> Result<String, ParseError> {
    match map_get(entries, key) {
        Some(TransitValue::String(s)) => Ok(s.clone()),
        Some(_) => Err(ParseError::InvalidType(key.to_string())),
        None => Err(ParseError::MissingField(key.to_string())),
    }
}

/// Get an integer value from a Transit map.
fn get_optional_int(
    entries: &[(TransitValue, TransitValue)],
    key: &str,
) -> Option<i64> {
    match map_get(entries, key) {
        Some(TransitValue::Integer(i)) => Some(*i),
        _ => None,
    }
}

fn get_required_int(
    entries: &[(TransitValue, TransitValue)],
    key: &str,
) -> Result<i64, ParseError> {
    get_optional_int(entries, key)
        .ok_or_else(|| ParseError::MissingField(key.to_string()))
}

/// Extract a value as a string representation (for queries, patterns, etc.).
fn get_value_as_string(
    entries: &[(TransitValue, TransitValue)],
    key: &str,
) -> Result<String, ParseError> {
    match map_get(entries, key) {
        Some(val) => Ok(transit_value_to_edn_string(val)),
        None => Err(ParseError::MissingField(key.to_string())),
    }
}

/// Convert a Transit value into an EDN-like string representation,
/// suitable for passing to the EDN-based query engine.
fn transit_value_to_edn_string(val: &TransitValue) -> String {
    match val {
        TransitValue::Nil => "nil".to_string(),
        TransitValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        TransitValue::Integer(i) => i.to_string(),
        TransitValue::String(s) => format!("\"{s}\""),
        TransitValue::Keyword(k) => format!(":{k}"),
        TransitValue::Symbol(s) => s.clone(),
        TransitValue::Uuid(u) => format!("#uuid \"{u}\""),
        TransitValue::Array(items) => {
            let inner: Vec<String> = items.iter().map(transit_value_to_edn_string).collect();
            format!("[{}]", inner.join(" "))
        }
        TransitValue::Map(entries) => {
            let inner: Vec<String> = entries
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{} {}",
                        transit_value_to_edn_string(k),
                        transit_value_to_edn_string(v)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(" "))
        }
    }
}

/// Get an optional string from a Transit map.
fn get_optional_string(
    entries: &[(TransitValue, TransitValue)],
    key: &str,
) -> Option<String> {
    match map_get(entries, key) {
        Some(TransitValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Get an optional vector of strings from a Transit map.
fn get_optional_string_vector(
    entries: &[(TransitValue, TransitValue)],
    key: &str,
) -> Vec<String> {
    match map_get(entries, key) {
        Some(TransitValue::Array(items)) => items
            .iter()
            .map(transit_value_to_edn_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_datoms_index(s: &str) -> Result<super::DatomsIndex, ParseError> {
    use super::DatomsIndex;
    let s_clean = s.trim_matches(|c: char| c == ':' || c == '"');
    match s_clean {
        "eavt" | "EAVT" => Ok(DatomsIndex::EAVT),
        "aevt" | "AEVT" => Ok(DatomsIndex::AEVT),
        "avet" | "AVET" => Ok(DatomsIndex::AVET),
        "vaet" | "VAET" => Ok(DatomsIndex::VAET),
        _ => Err(ParseError::InvalidType(format!(
            "Invalid datoms index: {s}. Expected :eavt, :aevt, :avet, or :vaet"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Content-Type detection
// ---------------------------------------------------------------------------

/// Determine the input format from a Content-Type header value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Edn,
    TransitJson,
    TransitMsgpack,
}

/// Parse a Content-Type header to determine the input format.
pub fn detect_input_format(content_type: &str) -> InputFormat {
    let ct = content_type.split(';').next().unwrap_or("").trim();
    match ct {
        "application/transit+json" => InputFormat::TransitJson,
        "application/transit+msgpack" => InputFormat::TransitMsgpack,
        _ => InputFormat::Edn,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Transit+JSON parsing tests ----------------------------------------

    #[test]
    fn test_parse_transit_json_health() {
        let input = r#"["^ ","~:op","~:health"]"#;
        let req = parse_transit_json(input);
        assert!(req.is_ok());
        match req.expect("should parse").op {
            Operation::Health => {}
            other => panic!("Expected Health, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_transit_json_list_dbs() {
        let input = r#"["^ ","~:op","~:list-dbs"]"#;
        let req = parse_transit_json(input);
        assert!(req.is_ok());
        match req.expect("should parse").op {
            Operation::ListDatabases => {}
            other => panic!("Expected ListDatabases, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_transit_json_connect() {
        let input =
            r#"["^ ","~:op","~:connect","~:args",["^ ","~:db-name","test-db"]]"#;
        let req = parse_transit_json(input);
        assert!(req.is_ok());
        match req.expect("should parse").op {
            Operation::Connect { db_name } => assert_eq!(db_name, "test-db"),
            other => panic!("Expected Connect, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_transit_json_query() {
        let input = r#"["^ ","~:op","~:q","~:args",["^ ","~:query","[:find ?e :where [?e :name]]","~:args",[]]]"#;
        let req = parse_transit_json(input);
        assert!(req.is_ok());
        match req.expect("should parse").op {
            Operation::Query { query, args, .. } => {
                assert!(query.contains("find"));
                assert!(args.is_empty());
            }
            other => panic!("Expected Query, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_transit_json_create_db() {
        let input = r#"["^ ","~:op","~:create-db","~:args",["^ ","~:db-name","mydb"]]"#;
        let req = parse_transit_json(input);
        assert!(req.is_ok());
        match req.expect("should parse").op {
            Operation::CreateDatabase { db_name } => assert_eq!(db_name, "mydb"),
            other => panic!("Expected CreateDatabase, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_transit_json_invalid() {
        let input = "not json at all";
        let req = parse_transit_json(input);
        assert!(req.is_err());
    }

    #[test]
    fn test_parse_transit_json_missing_op() {
        let input = r#"["^ ","~:foo","~:bar"]"#;
        let req = parse_transit_json(input);
        assert!(req.is_err());
    }

    // -- Transit tagged string decoding ------------------------------------

    #[test]
    fn test_decode_keyword() {
        assert_eq!(
            decode_transit_tagged_string("~:db/name"),
            TransitValue::Keyword("db/name".to_string())
        );
    }

    #[test]
    fn test_decode_symbol() {
        assert_eq!(
            decode_transit_tagged_string("~$my-sym"),
            TransitValue::Symbol("my-sym".to_string())
        );
    }

    #[test]
    fn test_decode_large_int() {
        assert_eq!(
            decode_transit_tagged_string("~i9999999999"),
            TransitValue::Integer(9_999_999_999)
        );
    }

    #[test]
    fn test_decode_uuid() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let tagged = format!("~u{uuid_str}");
        match decode_transit_tagged_string(&tagged) {
            TransitValue::Uuid(u) => assert_eq!(u.to_string(), uuid_str),
            other => panic!("Expected Uuid, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_escaped_tilde() {
        assert_eq!(
            decode_transit_tagged_string("~~hello"),
            TransitValue::String("~hello".to_string())
        );
    }

    #[test]
    fn test_decode_plain_string() {
        assert_eq!(
            decode_transit_tagged_string("hello"),
            TransitValue::String("hello".to_string())
        );
    }

    // -- Transit+MessagePack parsing tests --------------------------------

    #[test]
    fn test_parse_transit_msgpack_health() {
        // Build a msgpack-encoded Transit request: ["^ ", "~:op", "~:health"]
        let mut buf = Vec::new();
        // fixarray(3)
        buf.push(0x93);
        // fixstr "^ " (2 bytes)
        buf.push(0xa2);
        buf.extend_from_slice(b"^ ");
        // fixstr "~:op" (4 bytes)
        buf.push(0xa4);
        buf.extend_from_slice(b"~:op");
        // fixstr "~:health" (8 bytes)
        buf.push(0xa8);
        buf.extend_from_slice(b"~:health");

        let req = parse_transit_msgpack(&buf);
        assert!(req.is_ok(), "Failed: {req:?}");
        match req.expect("should parse").op {
            Operation::Health => {}
            other => panic!("Expected Health, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_transit_msgpack_connect() {
        // Build: ["^ ", "~:op", "~:connect", "~:args", ["^ ", "~:db-name", "test-db"]]
        let mut buf = Vec::new();
        // fixarray(5) for outer map
        buf.push(0x95);
        // "^ "
        buf.push(0xa2);
        buf.extend_from_slice(b"^ ");
        // "~:op"
        buf.push(0xa4);
        buf.extend_from_slice(b"~:op");
        // "~:connect"
        buf.push(0xa9);
        buf.extend_from_slice(b"~:connect");
        // "~:args"
        buf.push(0xa6);
        buf.extend_from_slice(b"~:args");
        // Inner map: fixarray(3) = ["^ ", "~:db-name", "test-db"]
        buf.push(0x93);
        buf.push(0xa2);
        buf.extend_from_slice(b"^ ");
        buf.push(0xa9);
        buf.extend_from_slice(b"~:db-name");
        buf.push(0xa7);
        buf.extend_from_slice(b"test-db");

        let req = parse_transit_msgpack(&buf);
        assert!(req.is_ok(), "Failed: {req:?}");
        match req.expect("should parse").op {
            Operation::Connect { db_name } => assert_eq!(db_name, "test-db"),
            other => panic!("Expected Connect, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_transit_msgpack_integers() {
        // Test that integer decoding works for various ranges
        let test_cases: Vec<(i64, Vec<u8>)> = vec![
            (0, vec![0x00]),
            (42, vec![42]),
            (127, vec![0x7f]),
            (200, vec![0xcc, 200]),
            (-1, vec![0xff]),
            (-32, vec![0xe0]),
            (-33, vec![0xd0, 0xdf]),
        ];

        for (expected, bytes) in test_cases {
            let (val, _) =
                msgpack_read_value_bounded(&bytes, 0).unwrap_or_else(|e| panic!("Failed for {expected}: {e}"));
            match val {
                TransitValue::Integer(i) => assert_eq!(i, expected, "Mismatch for input {bytes:?}"),
                other => panic!("Expected Integer({expected}), got {other:?}"),
            }
        }
    }

    #[test]
    fn test_parse_transit_msgpack_empty_input() {
        let result = parse_transit_msgpack(&[]);
        assert!(result.is_err());
    }

    // -- Content-Type detection tests --------------------------------------

    #[test]
    fn test_detect_edn() {
        assert_eq!(detect_input_format("application/edn"), InputFormat::Edn);
    }

    #[test]
    fn test_detect_transit_json() {
        assert_eq!(
            detect_input_format("application/transit+json"),
            InputFormat::TransitJson
        );
    }

    #[test]
    fn test_detect_transit_msgpack() {
        assert_eq!(
            detect_input_format("application/transit+msgpack"),
            InputFormat::TransitMsgpack
        );
    }

    #[test]
    fn test_detect_with_charset() {
        assert_eq!(
            detect_input_format("application/transit+json; charset=utf-8"),
            InputFormat::TransitJson
        );
    }

    #[test]
    fn test_detect_unknown_defaults_to_edn() {
        assert_eq!(detect_input_format("text/plain"), InputFormat::Edn);
    }

    // -- Round-trip tests --------------------------------------------------

    #[test]
    fn test_roundtrip_transit_json_health() {
        // Encode as Transit+JSON cmap, then parse it back
        let json_str = r#"["^ ","~:op","~:health"]"#;
        let req = parse_transit_json(json_str);
        assert!(req.is_ok());
        match req.expect("should parse").op {
            Operation::Health => {}
            other => panic!("Expected Health, got {other:?}"),
        }
    }

    #[test]
    fn test_roundtrip_transit_msgpack_list_dbs() {
        use crate::protocol::transit_serializer::{
            serialize_transit_msgpack,
        };
        use crate::protocol::{Response, ResponseValue};

        // Test that we can serialize a response and that the serializer output
        // is valid msgpack by reading it back
        let response = Response::Success {
            result: ResponseValue::Vector(vec![
                ResponseValue::String("db1".to_string()),
                ResponseValue::String("db2".to_string()),
            ]),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(!bytes.is_empty());

        // Verify the msgpack bytes can be decoded
        let (val, remaining) =
            msgpack_read_value_bounded(&bytes, 0).unwrap_or_else(|e| panic!("Failed to decode: {e}"));
        assert!(remaining.is_empty(), "Leftover bytes: {remaining:?}");

        // The top-level should be a map with a "result" key
        match val {
            TransitValue::Map(entries) => {
                assert!(!entries.is_empty());
                // Find the result key
                let result_entry = entries.iter().find(|(k, _)| {
                    matches!(k, TransitValue::Keyword(kw) if kw == "result")
                });
                assert!(result_entry.is_some(), "No :result key found");
                match &result_entry.expect("checked above").1 {
                    TransitValue::Array(items) => {
                        assert_eq!(items.len(), 2);
                        assert_eq!(
                            items[0],
                            TransitValue::String("db1".to_string())
                        );
                        assert_eq!(
                            items[1],
                            TransitValue::String("db2".to_string())
                        );
                    }
                    other => panic!("Expected Array, got {other:?}"),
                }
            }
            other => panic!("Expected Map, got {other:?}"),
        }
    }

    #[test]
    fn test_roundtrip_all_value_types_msgpack() {
        use crate::protocol::transit_serializer::serialize_transit_msgpack;
        use crate::protocol::{Response, ResponseValue};

        let response = Response::Success {
            result: ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("nil-val".to_string()),
                    ResponseValue::Nil,
                ),
                (
                    ResponseValue::Keyword("bool-val".to_string()),
                    ResponseValue::Boolean(true),
                ),
                (
                    ResponseValue::Keyword("int-val".to_string()),
                    ResponseValue::Integer(42),
                ),
                (
                    ResponseValue::Keyword("big-int-val".to_string()),
                    ResponseValue::Integer(9_999_999_999),
                ),
                (
                    ResponseValue::Keyword("neg-int-val".to_string()),
                    ResponseValue::Integer(-100),
                ),
                (
                    ResponseValue::Keyword("str-val".to_string()),
                    ResponseValue::String("hello world".to_string()),
                ),
                (
                    ResponseValue::Keyword("kw-val".to_string()),
                    ResponseValue::Keyword("db/name".to_string()),
                ),
                (
                    ResponseValue::Keyword("vec-val".to_string()),
                    ResponseValue::Vector(vec![
                        ResponseValue::Integer(1),
                        ResponseValue::Integer(2),
                    ]),
                ),
                (
                    ResponseValue::Keyword("list-val".to_string()),
                    ResponseValue::List(vec![
                        ResponseValue::Integer(3),
                        ResponseValue::Integer(4),
                    ]),
                ),
            ]),
        };

        let bytes = serialize_transit_msgpack(&response);
        assert!(!bytes.is_empty());

        // Decode and verify structure
        let (val, remaining) =
            msgpack_read_value_bounded(&bytes, 0).unwrap_or_else(|e| panic!("Failed: {e}"));
        assert!(remaining.is_empty());

        match val {
            TransitValue::Map(entries) => {
                // Find the result entry
                let result = entries
                    .iter()
                    .find(|(k, _)| matches!(k, TransitValue::Keyword(kw) if kw == "result"));
                assert!(result.is_some(), "Missing :result key");
                match &result.expect("checked").1 {
                    TransitValue::Map(inner) => {
                        // Verify each entry
                        let nil_val = inner
                            .iter()
                            .find(|(k, _)| matches!(k, TransitValue::Keyword(kw) if kw == "nil-val"));
                        assert!(
                            matches!(nil_val, Some((_, TransitValue::Nil))),
                            "nil-val mismatch: {nil_val:?}"
                        );

                        let bool_val = inner
                            .iter()
                            .find(|(k, _)| matches!(k, TransitValue::Keyword(kw) if kw == "bool-val"));
                        assert!(
                            matches!(bool_val, Some((_, TransitValue::Bool(true)))),
                            "bool-val mismatch: {bool_val:?}"
                        );

                        let int_val = inner
                            .iter()
                            .find(|(k, _)| matches!(k, TransitValue::Keyword(kw) if kw == "int-val"));
                        assert!(
                            matches!(int_val, Some((_, TransitValue::Integer(42)))),
                            "int-val mismatch: {int_val:?}"
                        );

                        let kw_val = inner
                            .iter()
                            .find(|(k, _)| matches!(k, TransitValue::Keyword(kw) if kw == "kw-val"));
                        match kw_val {
                            Some((_, TransitValue::Keyword(k))) => {
                                assert_eq!(k, "db/name");
                            }
                            other => panic!("kw-val mismatch: {other:?}"),
                        }
                    }
                    other => panic!("Expected inner Map, got {other:?}"),
                }
            }
            other => panic!("Expected Map, got {other:?}"),
        }
    }

    // -- transit_value_to_edn_string tests ---------------------------------

    #[test]
    fn test_transit_value_to_edn_string_keyword() {
        let val = TransitValue::Keyword("db/name".to_string());
        assert_eq!(transit_value_to_edn_string(&val), ":db/name");
    }

    #[test]
    fn test_transit_value_to_edn_string_vector() {
        let val = TransitValue::Array(vec![
            TransitValue::Keyword("find".to_string()),
            TransitValue::Symbol("?e".to_string()),
        ]);
        assert_eq!(transit_value_to_edn_string(&val), "[:find ?e]");
    }

    // -- Security: nesting depth tests -------------------------------------

    #[test]
    fn test_transit_json_depth_limit_rejects_deep_nesting() {
        // Build a JSON string with nesting deeper than MAX_NESTING_DEPTH.
        // Each level is an array wrapping the next: [[[...[42]...]]]
        let mut json = "42".to_string();
        for _ in 0..=MAX_NESTING_DEPTH + 1 {
            json = format!("[{}]", json);
        }
        let result = parse_transit_json(&json);
        assert!(result.is_err(), "Should reject deeply nested JSON");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("nesting depth"),
            "Error should mention nesting depth: {err_msg}"
        );
    }

    #[test]
    fn test_transit_json_moderate_nesting_allowed() {
        // Build a JSON string with nesting well within limits (10 levels).
        let mut json = r#"["^ ","~:op","~:health"]"#.to_string();
        for _ in 0..10 {
            json = format!("[{}]", json);
        }
        // This should parse without error (the inner cmap becomes a nested array).
        // The parse may fail at the operation level (not a map at top level),
        // but it should NOT fail due to depth.
        let result = parse_transit_json(&json);
        // The error, if any, should be about type mismatch, not depth.
        if let Err(e) = &result {
            let msg = format!("{e}");
            assert!(
                !msg.contains("nesting depth"),
                "Should not fail due to depth at 10 levels: {msg}"
            );
        }
    }

    #[test]
    fn test_transit_msgpack_depth_limit_rejects_deep_nesting() {
        // Build a msgpack payload with deeply nested fixarrays.
        // Each level: 0x91 (fixarray of length 1) wrapping the next.
        let mut buf = vec![0x00]; // inner value: integer 0
        for _ in 0..=MAX_NESTING_DEPTH + 1 {
            let mut wrapper = vec![0x91]; // fixarray(1)
            wrapper.extend_from_slice(&buf);
            buf = wrapper;
        }
        let result = parse_transit_msgpack(&buf);
        assert!(result.is_err(), "Should reject deeply nested MessagePack");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("nesting depth"),
            "Error should mention nesting depth: {err_msg}"
        );
    }
}
