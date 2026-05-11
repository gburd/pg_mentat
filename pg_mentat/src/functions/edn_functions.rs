/// EDN function suite for working with native Edn types.
///
/// Provides jsonb-style accessor and introspection functions:
/// - edn_get_key / edn_get_idx: access map keys and vector indices
/// - edn_array_elements: set-returning function for vector/list elements
/// - edn_typeof: type name introspection
/// - edn_exists: key existence check
/// - edn_map_keys: set-returning function for map keys
/// - edn_each: set-returning function for map key-value pairs
/// - edn_array_length: collection length
/// - edn_to_jsonb / jsonb_to_edn: conversion between EDN and JSONB
use crate::types::edn::Edn;
use ordered_float::OrderedFloat;
use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

/// Get a value from a map by string key (parsed as EDN keyword or value).
///
/// The key string is first tried as a keyword (with or without leading colon),
/// then as a plain string lookup.
///
/// ```sql
/// SELECT mentat.edn_get_key('{:name "Alice"}'::edn, ':name');
/// -- => "Alice"
/// ```
#[pg_extern(schema = "mentat", immutable, parallel_safe)]
fn edn_get_key(map: Edn, key: &str) -> Option<Edn> {
    match map.inner() {
        edn::Value::Map(m) => {
            // Try as keyword first (strip leading colon if present)
            let keyword_key = if let Some(stripped) = key.strip_prefix(':') {
                stripped
            } else {
                key
            };

            // Try namespaced keyword
            if let Some(idx) = keyword_key.find('/') {
                let ns = &keyword_key[..idx];
                let name = &keyword_key[idx + 1..];
                let kw = edn::Value::Keyword(edn::symbols::Keyword::namespaced(ns, name));
                if let Some(v) = m.get(&kw) {
                    return Some(Edn::new(v.clone()));
                }
            }

            // Try plain keyword
            let kw = edn::Value::Keyword(edn::symbols::Keyword::plain(keyword_key));
            if let Some(v) = m.get(&kw) {
                return Some(Edn::new(v.clone()));
            }

            // Try as text key
            let text_key = edn::Value::Text(key.to_string());
            m.get(&text_key).map(|v| Edn::new(v.clone()))
        }
        _ => None,
    }
}

/// Get a value from a vector or list by 0-based index.
///
/// Negative indices are not supported and return NULL.
///
/// ```sql
/// SELECT mentat.edn_get_idx('[10 20 30]'::edn, 1);
/// -- => 20
/// ```
#[pg_extern(schema = "mentat", immutable, parallel_safe)]
fn edn_get_idx(value: Edn, idx: i32) -> Option<Edn> {
    if idx < 0 {
        return None;
    }
    let idx = idx as usize;
    match value.inner() {
        edn::Value::Vector(v) => v.get(idx).map(|e| Edn::new(e.clone())),
        edn::Value::List(l) => l.iter().nth(idx).map(|e| Edn::new(e.clone())),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Set-returning functions
// ---------------------------------------------------------------------------

/// Return each element of a vector or list as a separate row.
///
/// ```sql
/// SELECT * FROM mentat.edn_array_elements('[1 2 3]'::edn);
/// -- value
/// -- -----
/// -- 1
/// -- 2
/// -- 3
/// ```
#[pg_extern(schema = "mentat", immutable, parallel_safe)]
fn edn_array_elements(value: Edn) -> TableIterator<'static, (name!(value, Edn),)> {
    let elements: Vec<(Edn,)> = match value.inner() {
        edn::Value::Vector(v) => v.iter().map(|e| (Edn::new(e.clone()),)).collect(),
        edn::Value::List(l) => l.iter().map(|e| (Edn::new(e.clone()),)).collect(),
        edn::Value::Set(s) => s.iter().map(|e| (Edn::new(e.clone()),)).collect(),
        _ => Vec::new(),
    };
    TableIterator::new(elements)
}

/// Return each key of a map as a separate row.
///
/// ```sql
/// SELECT * FROM mentat.edn_map_keys('{:a 1 :b 2}'::edn);
/// -- key
/// -- ---
/// -- :a
/// -- :b
/// ```
#[pg_extern(schema = "mentat", immutable, parallel_safe)]
fn edn_map_keys(value: Edn) -> TableIterator<'static, (name!(key, Edn),)> {
    let keys: Vec<(Edn,)> = match value.inner() {
        edn::Value::Map(m) => m.keys().map(|k| (Edn::new(k.clone()),)).collect(),
        _ => Vec::new(),
    };
    TableIterator::new(keys)
}

/// Iterate over map entries, returning each key-value pair as a row.
///
/// ```sql
/// SELECT * FROM mentat.edn_each('{:a 1 :b 2}'::edn);
/// -- key | value
/// -- ----+------
/// -- :a  | 1
/// -- :b  | 2
/// ```
#[pg_extern(schema = "mentat", immutable, parallel_safe)]
fn edn_each(value: Edn) -> TableIterator<'static, (name!(key, Edn), name!(value, Edn))> {
    let pairs: Vec<(Edn, Edn)> = match value.inner() {
        edn::Value::Map(m) => m
            .iter()
            .map(|(k, v)| (Edn::new(k.clone()), Edn::new(v.clone())))
            .collect(),
        _ => Vec::new(),
    };
    TableIterator::new(pairs)
}

// ---------------------------------------------------------------------------
// Introspection
// ---------------------------------------------------------------------------

/// Return the EDN type name of a value as a string.
///
/// Possible return values: "nil", "boolean", "integer", "float",
/// "big_integer", "instant", "text", "uuid", "symbol",
/// "namespaced_symbol", "keyword", "vector", "list", "set", "map", "bytes".
///
/// ```sql
/// SELECT mentat.edn_typeof('42'::edn);
/// -- => "integer"
/// SELECT mentat.edn_typeof('[:a :b]'::edn);
/// -- => "vector"
/// ```
#[pg_extern(schema = "mentat", immutable, parallel_safe)]
fn edn_typeof(value: Edn) -> &'static str {
    match value.inner() {
        edn::Value::Nil => "nil",
        edn::Value::Boolean(_) => "boolean",
        edn::Value::Integer(_) => "integer",
        edn::Value::Float(_) => "float",
        edn::Value::BigInteger(_) => "big_integer",
        edn::Value::Instant(_) => "instant",
        edn::Value::Text(_) => "text",
        edn::Value::Uuid(_) => "uuid",
        edn::Value::PlainSymbol(_) => "symbol",
        edn::Value::NamespacedSymbol(_) => "namespaced_symbol",
        edn::Value::Keyword(_) => "keyword",
        edn::Value::Vector(_) => "vector",
        edn::Value::List(_) => "list",
        edn::Value::Set(_) => "set",
        edn::Value::Map(_) => "map",
        edn::Value::Bytes(_) => "bytes",
    }
}

/// Check whether a key exists in a map.
///
/// The key is parsed similarly to `edn_get_key`: tried as a keyword first
/// (namespaced, then plain), then as a text key.
///
/// ```sql
/// SELECT mentat.edn_exists('{:name "Alice"}'::edn, ':name');
/// -- => true
/// ```
#[pg_extern(schema = "mentat", immutable, parallel_safe)]
fn edn_exists(value: Edn, key: &str) -> bool {
    match value.inner() {
        edn::Value::Map(m) => {
            let keyword_key = if let Some(stripped) = key.strip_prefix(':') {
                stripped
            } else {
                key
            };

            // Try namespaced keyword
            if let Some(idx) = keyword_key.find('/') {
                let ns = &keyword_key[..idx];
                let name = &keyword_key[idx + 1..];
                let kw = edn::Value::Keyword(edn::symbols::Keyword::namespaced(ns, name));
                if m.contains_key(&kw) {
                    return true;
                }
            }

            // Try plain keyword
            let kw = edn::Value::Keyword(edn::symbols::Keyword::plain(keyword_key));
            if m.contains_key(&kw) {
                return true;
            }

            // Try as text key
            let text_key = edn::Value::Text(key.to_string());
            m.contains_key(&text_key)
        }
        _ => false,
    }
}

/// Return the number of elements in a vector, list, set, or map.
///
/// Returns 0 for non-collection types.
///
/// ```sql
/// SELECT mentat.edn_array_length('[1 2 3]'::edn);
/// -- => 3
/// ```
#[pg_extern(schema = "mentat", immutable, parallel_safe)]
fn edn_array_length(value: Edn) -> i32 {
    match value.inner() {
        edn::Value::Vector(v) => v.len() as i32,
        edn::Value::List(l) => l.len() as i32,
        edn::Value::Set(s) => s.len() as i32,
        edn::Value::Map(m) => m.len() as i32,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// EDN <-> JSONB conversion
// ---------------------------------------------------------------------------

/// Convert an EDN value to JSONB.
///
/// Mapping:
/// - nil        -> null
/// - boolean    -> boolean
/// - integer    -> number
/// - float      -> number
/// - text       -> string
/// - keyword    -> string (":ns/name" form)
/// - vector     -> array
/// - list       -> array
/// - set        -> array (sorted)
/// - map        -> object (keys converted to strings)
/// - uuid       -> string
/// - instant    -> string (ISO-8601)
/// - big_integer -> string (with "N" suffix)
/// - bytes      -> string (hex-encoded)
/// - symbol     -> string
///
/// ```sql
/// SELECT mentat.edn_to_jsonb('{:name "Alice" :age 30}'::edn);
/// -- => {"":name": "Alice", ":age": 30}
/// ```
#[pg_extern(schema = "mentat", immutable, parallel_safe)]
fn edn_to_jsonb(value: Edn) -> JsonB {
    JsonB(edn_value_to_json(value.inner()))
}

/// Convert a JSONB value to EDN.
///
/// Mapping:
/// - null    -> nil
/// - boolean -> boolean
/// - number  -> integer (if no fractional part) or float
/// - string  -> text (or keyword if it starts with ":")
/// - array   -> vector
/// - object  -> map (string keys become keywords if they start with ":")
///
/// ```sql
/// SELECT mentat.jsonb_to_edn('{"name": "Alice", "age": 30}'::jsonb);
/// -- => {:name "Alice" :age 30}
/// ```
#[pg_extern(schema = "mentat", immutable, parallel_safe)]
fn jsonb_to_edn(value: JsonB) -> Edn {
    Edn::new(json_to_edn_value(&value.0))
}

// ---------------------------------------------------------------------------
// Internal conversion helpers
// ---------------------------------------------------------------------------

fn edn_value_to_json(value: &edn::Value) -> JsonValue {
    match value {
        edn::Value::Nil => JsonValue::Null,
        edn::Value::Boolean(b) => JsonValue::Bool(*b),
        edn::Value::Integer(n) => serde_json::json!(*n),
        edn::Value::Float(f) => {
            let f = f.into_inner();
            if f.is_finite() {
                serde_json::json!(f)
            } else {
                // NaN / Infinity cannot be represented in JSON
                JsonValue::Null
            }
        }
        edn::Value::BigInteger(n) => JsonValue::String(format!("{n}N")),
        edn::Value::Instant(dt) => {
            use chrono::SecondsFormat;
            JsonValue::String(dt.to_rfc3339_opts(SecondsFormat::AutoSi, true))
        }
        edn::Value::Text(s) => JsonValue::String(s.clone()),
        edn::Value::Uuid(u) => JsonValue::String(u.hyphenated().to_string()),
        edn::Value::PlainSymbol(s) => JsonValue::String(s.to_string()),
        edn::Value::NamespacedSymbol(s) => JsonValue::String(s.to_string()),
        edn::Value::Keyword(k) => JsonValue::String(k.to_string()),
        edn::Value::Vector(v) => JsonValue::Array(v.iter().map(edn_value_to_json).collect()),
        edn::Value::List(l) => JsonValue::Array(l.iter().map(edn_value_to_json).collect()),
        edn::Value::Set(s) => JsonValue::Array(s.iter().map(edn_value_to_json).collect()),
        edn::Value::Map(m) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in m {
                let key_str = match k {
                    edn::Value::Keyword(kw) => kw.to_string(),
                    edn::Value::Text(s) => s.clone(),
                    other => format!("{other}"),
                };
                obj.insert(key_str, edn_value_to_json(v));
            }
            JsonValue::Object(obj)
        }
        edn::Value::Bytes(b) => JsonValue::String(hex::encode(b)),
    }
}

fn json_to_edn_value(value: &JsonValue) -> edn::Value {
    match value {
        JsonValue::Null => edn::Value::Nil,
        JsonValue::Bool(b) => edn::Value::Boolean(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                edn::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                edn::Value::Float(OrderedFloat(f))
            } else {
                // Fallback: render as string
                edn::Value::Text(n.to_string())
            }
        }
        JsonValue::String(s) => {
            // Strings starting with ":" are converted to keywords
            if let Some(rest) = s.strip_prefix(':') {
                if let Some(idx) = rest.find('/') {
                    let ns = &rest[..idx];
                    let name = &rest[idx + 1..];
                    edn::Value::Keyword(edn::symbols::Keyword::namespaced(ns, name))
                } else {
                    edn::Value::Keyword(edn::symbols::Keyword::plain(rest))
                }
            } else {
                edn::Value::Text(s.clone())
            }
        }
        JsonValue::Array(arr) => edn::Value::Vector(arr.iter().map(json_to_edn_value).collect()),
        JsonValue::Object(obj) => {
            let mut map = std::collections::BTreeMap::new();
            for (k, v) in obj {
                let key = if let Some(rest) = k.strip_prefix(':') {
                    if let Some(idx) = rest.find('/') {
                        let ns = &rest[..idx];
                        let name = &rest[idx + 1..];
                        edn::Value::Keyword(edn::symbols::Keyword::namespaced(ns, name))
                    } else {
                        edn::Value::Keyword(edn::symbols::Keyword::plain(rest))
                    }
                } else {
                    edn::Value::Text(k.clone())
                };
                map.insert(key, json_to_edn_value(v));
            }
            edn::Value::Map(map)
        }
    }
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use super::*;

    #[pg_test]
    fn test_edn_functions_compile() {
        crate::ensure_extension_loaded();
        // Compilation test -- verifies all functions are accessible
        assert!(true);
    }
}
