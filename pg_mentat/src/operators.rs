use crate::types::edn::EdnValue;
use pgrx::prelude::*;

/// Equality operator for EdnValue
#[pg_operator(immutable, parallel_safe)]
#[opname(=)]
fn edn_eq(left: EdnValue, right: EdnValue) -> bool {
    left == right
}

/// Inequality operator for EdnValue
#[pg_operator(immutable, parallel_safe)]
#[opname(<>)]
fn edn_ne(left: EdnValue, right: EdnValue) -> bool {
    left != right
}

/// Get value from map by key
/// Example: edn_get('{:name "Alice"}', ':name') -> '"Alice"'
#[pg_extern(immutable, parallel_safe)]
fn edn_get(map: EdnValue, key: EdnValue) -> Option<EdnValue> {
    match map.inner() {
        edn::Value::Map(m) => m.get(key.inner()).map(|v| EdnValue::new(v.clone())),
        _ => None,
    }
}

/// Get value from vector by index (0-based)
/// Example: edn_nth('[1 2 3]', 1) -> 2
#[pg_extern(immutable, parallel_safe)]
fn edn_nth(vec: EdnValue, index: i64) -> Option<EdnValue> {
    match vec.inner() {
        edn::Value::Vector(v) => {
            if index < 0 || index >= v.len() as i64 {
                None
            } else {
                Some(EdnValue::new(v[index as usize].clone()))
            }
        }
        _ => None,
    }
}

/// Get count of elements in a collection
/// Example: edn_count('[1 2 3]') -> 3
#[pg_extern(immutable, parallel_safe)]
fn edn_count(value: EdnValue) -> i64 {
    match value.inner() {
        edn::Value::Vector(v) => v.len() as i64,
        edn::Value::List(l) => l.len() as i64,
        edn::Value::Set(s) => s.len() as i64,
        edn::Value::Map(m) => m.len() as i64,
        _ => 0,
    }
}

/// Check if a value is nil
#[pg_extern(immutable, parallel_safe)]
fn edn_is_nil(value: EdnValue) -> bool {
    matches!(value.inner(), edn::Value::Nil)
}

/// Check if a value is a boolean
#[pg_extern(immutable, parallel_safe)]
fn edn_is_boolean(value: EdnValue) -> bool {
    matches!(value.inner(), edn::Value::Boolean(_))
}

/// Check if a value is an integer
#[pg_extern(immutable, parallel_safe)]
fn edn_is_integer(value: EdnValue) -> bool {
    matches!(value.inner(), edn::Value::Integer(_))
}

/// Check if a value is a float
#[pg_extern(immutable, parallel_safe)]
fn edn_is_float(value: EdnValue) -> bool {
    matches!(value.inner(), edn::Value::Float(_))
}

/// Check if a value is a string
#[pg_extern(immutable, parallel_safe)]
fn edn_is_text(value: EdnValue) -> bool {
    matches!(value.inner(), edn::Value::Text(_))
}

/// Check if a value is a keyword
#[pg_extern(immutable, parallel_safe)]
fn edn_is_keyword(value: EdnValue) -> bool {
    matches!(value.inner(), edn::Value::Keyword(_))
}

/// Check if a value is a vector
#[pg_extern(immutable, parallel_safe)]
fn edn_is_vector(value: EdnValue) -> bool {
    matches!(value.inner(), edn::Value::Vector(_))
}

/// Check if a value is a list
#[pg_extern(immutable, parallel_safe)]
fn edn_is_list(value: EdnValue) -> bool {
    matches!(value.inner(), edn::Value::List(_))
}

/// Check if a value is a set
#[pg_extern(immutable, parallel_safe)]
fn edn_is_set(value: EdnValue) -> bool {
    matches!(value.inner(), edn::Value::Set(_))
}

/// Check if a value is a map
#[pg_extern(immutable, parallel_safe)]
fn edn_is_map(value: EdnValue) -> bool {
    matches!(value.inner(), edn::Value::Map(_))
}

/// Check if a collection contains an element
#[pg_extern(immutable, parallel_safe)]
fn edn_contains(collection: EdnValue, element: EdnValue) -> bool {
    match collection.inner() {
        edn::Value::Vector(v) => v.contains(element.inner()),
        edn::Value::List(l) => l.iter().any(|item| item == element.inner()),
        edn::Value::Set(s) => s.contains(element.inner()),
        edn::Value::Map(m) => m.contains_key(element.inner()),
        _ => false,
    }
}

/// Extract keys from a map as a vector
#[pg_extern(immutable, parallel_safe)]
fn edn_keys(map: EdnValue) -> Option<EdnValue> {
    match map.inner() {
        edn::Value::Map(m) => {
            let keys: Vec<edn::Value> = m.keys().cloned().collect();
            Some(EdnValue::new(edn::Value::Vector(keys)))
        }
        _ => None,
    }
}

/// Extract values from a map as a vector
#[pg_extern(immutable, parallel_safe)]
fn edn_values(map: EdnValue) -> Option<EdnValue> {
    match map.inner() {
        edn::Value::Map(m) => {
            let values: Vec<edn::Value> = m.values().cloned().collect();
            Some(EdnValue::new(edn::Value::Vector(values)))
        }
        _ => None,
    }
}
