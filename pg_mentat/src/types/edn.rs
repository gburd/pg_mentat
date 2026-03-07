use pgrx::prelude::*;

// EdnValue is now defined in the mentat schema module in lib.rs
// Import it here for use in impl blocks and functions
pub use crate::mentat::EdnValue;

/// Maximum nesting depth for EDN structures to prevent stack overflow
const MAX_EDN_NESTING: usize = 100;

/// Maximum collection size to prevent memory exhaustion
const MAX_COLLECTION_SIZE: usize = 1_000_000;

/// Maximum input size (10MB)
const MAX_INPUT_SIZE: usize = 10 * 1024 * 1024;

impl EdnValue {
    /// Create a new EdnValue from an EDN Value
    pub fn new(value: edn::Value) -> Self {
        EdnValue { inner: value }
    }

    /// Get a reference to the inner EDN Value
    pub fn inner(&self) -> &edn::Value {
        &self.inner
    }

    /// Take ownership of the inner EDN Value
    pub fn into_inner(self) -> edn::Value {
        self.inner
    }

    /// Validate EDN value constraints
    fn validate(&self) -> Result<(), String> {
        self.validate_depth(0)?;
        self.validate_size(0)?;
        Ok(())
    }

    /// Recursively validate nesting depth
    fn validate_depth(&self, depth: usize) -> Result<(), String> {
        if depth > MAX_EDN_NESTING {
            return Err(format!(
                "EDN nesting depth exceeds maximum of {MAX_EDN_NESTING}"
            ));
        }

        match &self.inner {
            edn::Value::Vector(v) => {
                for item in v {
                    EdnValue::new(item.clone()).validate_depth(depth + 1)?;
                }
            }
            edn::Value::List(l) => {
                for item in l {
                    EdnValue::new(item.clone()).validate_depth(depth + 1)?;
                }
            }
            edn::Value::Set(s) => {
                for item in s {
                    EdnValue::new(item.clone()).validate_depth(depth + 1)?;
                }
            }
            edn::Value::Map(m) => {
                for (k, v) in m {
                    EdnValue::new(k.clone()).validate_depth(depth + 1)?;
                    EdnValue::new(v.clone()).validate_depth(depth + 1)?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Recursively validate collection sizes
    fn validate_size(&self, count: usize) -> Result<(), String> {
        if count > MAX_COLLECTION_SIZE {
            return Err(format!(
                "EDN collection size exceeds maximum of {MAX_COLLECTION_SIZE}"
            ));
        }

        match &self.inner {
            edn::Value::Vector(v) => {
                let new_count = count + v.len();
                if new_count > MAX_COLLECTION_SIZE {
                    return Err(format!(
                        "EDN collection size exceeds maximum of {MAX_COLLECTION_SIZE}"
                    ));
                }
                for item in v {
                    EdnValue::new(item.clone()).validate_size(new_count)?;
                }
            }
            edn::Value::List(l) => {
                let new_count = count + l.len();
                if new_count > MAX_COLLECTION_SIZE {
                    return Err(format!(
                        "EDN collection size exceeds maximum of {MAX_COLLECTION_SIZE}"
                    ));
                }
                for item in l {
                    EdnValue::new(item.clone()).validate_size(new_count)?;
                }
            }
            edn::Value::Set(s) => {
                let new_count = count + s.len();
                if new_count > MAX_COLLECTION_SIZE {
                    return Err(format!(
                        "EDN collection size exceeds maximum of {MAX_COLLECTION_SIZE}"
                    ));
                }
                for item in s {
                    EdnValue::new(item.clone()).validate_size(new_count)?;
                }
            }
            edn::Value::Map(m) => {
                let new_count = count + m.len();
                if new_count > MAX_COLLECTION_SIZE {
                    return Err(format!(
                        "EDN collection size exceeds maximum of {MAX_COLLECTION_SIZE}"
                    ));
                }
                for (k, v) in m {
                    EdnValue::new(k.clone()).validate_size(new_count)?;
                    EdnValue::new(v.clone()).validate_size(new_count)?;
                }
            }
            _ => {}
        }

        Ok(())
    }
}

/// Input function: Parse EDN text into EdnValue
#[pg_extern(immutable, parallel_safe)]
fn edn_in(input: &str) -> Result<EdnValue, Box<dyn std::error::Error>> {
    // Validate input size
    if input.len() > MAX_INPUT_SIZE {
        return Err("EDN input too large (max 10MB)".into());
    }

    // Parse EDN text using mentat's parser
    let value_and_span = edn::parse::value(input)?;

    // Extract the value (discard span information)
    let value = value_and_span.without_spans();

    // Create EdnValue and validate
    let edn_value = EdnValue::new(value);
    edn_value
        .validate()
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    Ok(edn_value)
}

/// Output function: Convert EdnValue to EDN text
#[pg_extern(immutable, parallel_safe)]
fn edn_out(value: EdnValue) -> String {
    format!("{}", value.inner)
}

/// Binary send function: Serialize EdnValue for binary transmission
/// Currently uses EDN text format. TODO: Implement CBOR serialization
#[pg_extern(immutable, parallel_safe)]
fn edn_send(value: EdnValue) -> Vec<u8> {
    let text = format!("{}", value.inner);
    text.into_bytes()
}

/// Binary receive function: Deserialize EdnValue from binary transmission
/// Currently uses EDN text format. TODO: Implement CBOR deserialization
#[pg_extern(immutable, parallel_safe)]
fn edn_recv(data: Vec<u8>) -> Result<EdnValue, Box<dyn std::error::Error>> {
    let text = String::from_utf8(data)?;
    edn_in(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edn_value_validation() {
        // Test nesting depth
        let mut deep_nested = String::from("[");
        for _ in 0..MAX_EDN_NESTING + 1 {
            deep_nested.push_str("[");
        }
        for _ in 0..MAX_EDN_NESTING + 1 {
            deep_nested.push_str("]");
        }
        deep_nested.push_str("]");

        let result = edn_in(&deep_nested);
        assert!(result.is_err());
    }

    #[test]
    fn test_edn_value_size() {
        // Test input size limit
        let large_input = "a".repeat(MAX_INPUT_SIZE + 1);
        let result = edn_in(&large_input);
        assert!(result.is_err());
    }
}
