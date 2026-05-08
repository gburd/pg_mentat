//! EDN value validation helpers.
//!
//! The `Edn` struct is defined in `lib.rs` with `#[derive(PostgresType)]`.
//! pgrx auto-generates `edn_in` / `edn_out` / `edn_send` / `edn_recv` from
//! the struct's `Serialize` / `Deserialize` impls — do NOT declare them
//! manually here (duplicate-symbol link error).
//!
//! Validation (size, depth) runs inside the serde `deserialize_edn` hook
//! in `lib.rs`, which calls [`Edn::validate`] on every inbound value.

// Edn is defined in the mentat schema module in lib.rs
pub use crate::mentat::Edn;

/// Maximum nesting depth for EDN structures to prevent stack overflow
pub(crate) const MAX_EDN_NESTING: usize = 100;

/// Maximum collection size to prevent memory exhaustion
pub(crate) const MAX_COLLECTION_SIZE: usize = 1_000_000;

/// Maximum input size (10MB)
pub(crate) const MAX_INPUT_SIZE: usize = 10 * 1024 * 1024;

impl Edn {
    /// Create a new Edn from an EDN Value
    pub fn new(value: edn::Value) -> Self {
        Edn { inner: value }
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
                    Edn::new(item.clone()).validate_depth(depth + 1)?;
                }
            }
            edn::Value::List(l) => {
                for item in l {
                    Edn::new(item.clone()).validate_depth(depth + 1)?;
                }
            }
            edn::Value::Set(s) => {
                for item in s {
                    Edn::new(item.clone()).validate_depth(depth + 1)?;
                }
            }
            edn::Value::Map(m) => {
                for (k, v) in m {
                    Edn::new(k.clone()).validate_depth(depth + 1)?;
                    Edn::new(v.clone()).validate_depth(depth + 1)?;
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
                    Edn::new(item.clone()).validate_size(new_count)?;
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
                    Edn::new(item.clone()).validate_size(new_count)?;
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
                    Edn::new(item.clone()).validate_size(new_count)?;
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
                    Edn::new(k.clone()).validate_size(new_count)?;
                    Edn::new(v.clone()).validate_size(new_count)?;
                }
            }
            _ => {}
        }

        Ok(())
    }
}

/// Parse + validate an EDN text blob.
///
/// Used by the serde `deserialize_edn` hook in `lib.rs`. Keep this as the
/// single entry point for parse-with-validation so the same limits apply
/// to text input, binary input, and serde-deserialized input.
pub(crate) fn parse_and_validate(input: &str) -> Result<Edn, String> {
    if input.len() > MAX_INPUT_SIZE {
        return Err(format!(
            "EDN input too large ({} bytes, max {})",
            input.len(),
            MAX_INPUT_SIZE
        ));
    }
    let value_and_span = edn::parse::value(input).map_err(|e| format!("EDN parse error: {e}"))?;
    let value = value_and_span.without_spans();
    let edn_value = Edn::new(value);
    edn_value.validate()?;
    Ok(edn_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edn_value_validation_nesting() {
        let mut deep_nested = String::from("[");
        for _ in 0..MAX_EDN_NESTING + 1 {
            deep_nested.push_str("[");
        }
        for _ in 0..MAX_EDN_NESTING + 1 {
            deep_nested.push_str("]");
        }
        deep_nested.push_str("]");

        let result = parse_and_validate(&deep_nested);
        assert!(result.is_err(), "expected nesting-depth error");
    }

    #[test]
    fn test_edn_value_size_limit() {
        let large_input = "a".repeat(MAX_INPUT_SIZE + 1);
        let result = parse_and_validate(&large_input);
        assert!(result.is_err(), "expected size-limit error");
    }

    #[test]
    fn test_edn_value_accepts_small_input() {
        let result = parse_and_validate("{:a 1 :b [2 3 4]}");
        assert!(result.is_ok());
    }
}
