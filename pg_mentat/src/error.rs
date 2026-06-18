/// Structured error types for pg_mentat.
///
/// Each variant carries contextual information (attribute names, expected types,
/// suggestions) so that error messages are actionable. The `Display`
/// implementation prefixes every message with a Datomic-compatible
/// `:db.error/...` error code, enabling programmatic error classification
/// without parsing free-form text.
use std::fmt;

// ---------------------------------------------------------------------------
// MentatError enum
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum MentatError {
    /// An attribute ident was referenced but does not exist in the schema.
    AttributeNotFound {
        attr: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },

    /// A value's type does not match the attribute's declared `:db/valueType`.
    TypeMismatch {
        attr: String,
        expected: String,
        got: String,
        expected_tag: i16,
        got_tag: i16,
    },

    /// A unique constraint would be violated by this assertion.
    UniqueConstraintViolation {
        attr: String,
        unique_type: String,
        existing_eid: i64,
        new_eid: i64,
    },

    /// A query or pull pattern could not be parsed / is structurally invalid.
    InvalidQuery {
        message: String,
        suggestion: Option<String>,
    },

    /// A pull pattern is structurally invalid.
    InvalidPullPattern { message: String },

    /// An EDN transaction document is structurally invalid.
    InvalidTransaction { message: String },

    /// An entity ID, tempid, or ident could not be resolved.
    EntityNotFound { ident: String, message: String },

    /// A lookup ref could not be resolved to an existing entity.
    LookupRefNotFound { attr: String, message: String },

    /// A lookup ref targets an attribute without a unique constraint.
    LookupRefRequiresUnique { attr: String },

    /// Entity ID / tempid allocation failed (partition exhausted or missing).
    AllocationFailed { partition: String },

    /// Cardinality-one attribute has multiple assertions in one transaction.
    CardinalityViolation {
        attr: String,
        entity: i64,
        count: usize,
    },

    /// `:db.fn/cas` compare-and-swap failed because the current value differs.
    CasFailed {
        entity: i64,
        attr: String,
        expected: String,
        actual: String,
    },

    /// Stored data is corrupt (wrong byte length for type tag, etc.).
    DataCorruption { message: String },

    /// An unsupported value type tag was encountered.
    UnsupportedType { type_tag: i16 },

    /// An invalid entity place (not an integer, string, keyword, or lookup ref).
    InvalidEntityPlace { got_type: String, got_value: String },

    /// An invalid attribute place (not an integer or keyword).
    InvalidAttribute { got_type: String, got_value: String },

    /// A value could not be encoded for storage.
    UnsupportedValueType { got_type: String, got_value: String },

    /// An unknown cardinality value was found in the schema.
    InvalidCardinality {
        cardinality: String,
        attr_entid: i64,
    },

    /// Nothing to retract for a given entity.
    NothingToRetract { entity: i64 },

    /// Transaction record creation failed.
    TransactionFailed { message: String },

    /// A batch operation type is unknown.
    UnknownBatchOp { op: String },

    /// A batch operation is missing required arguments.
    BatchMissingArg { op: String, message: String },

    /// Schema data integrity issue (missing column in schema row).
    DataIntegrity { message: String },

    /// Query exceeded the maximum allowed result row count.
    ResultLimitExceeded { limit: i32, message: String },

    /// Query exceeded the statement timeout.
    QueryTimeout { timeout_ms: i32 },

    /// A store with the given name already exists.
    StoreExists { store_name: String },

    /// A store with the given name was not found.
    StoreNotFound { store_name: String },

    /// A store name is invalid (bad characters, too long, reserved, etc.).
    InvalidStoreName { store_name: String, reason: String },

    /// Attempted to drop the default store, which is not allowed.
    CannotDropDefaultStore,

    /// A PostgreSQL serialization failure (SQLSTATE 40001) occurred.
    /// The caller should retry the transaction with backoff.
    SerializationFailure { message: String, attempt: u32 },

    /// Wraps an upstream error (SPI, EDN parse, etc.) with context.
    Internal {
        message: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

// ---------------------------------------------------------------------------
// Display -- every variant starts with :db.error/<code>
// ---------------------------------------------------------------------------

impl fmt::Display for MentatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AttributeNotFound {
                attr,
                available,
                suggestion,
            } => {
                write!(
                    f,
                    ":db.error/attribute-not-found Attribute '{}' not found in schema.",
                    attr
                )?;
                if !available.is_empty() {
                    if available.len() > 20 {
                        let shown: Vec<&str> =
                            available.iter().take(20).map(|s| s.as_str()).collect();
                        write!(f, " Available attributes (first 20): {}.", shown.join(", "))?;
                    } else {
                        let shown: Vec<&str> = available.iter().map(|s| s.as_str()).collect();
                        write!(f, " Available attributes: {}.", shown.join(", "))?;
                    }
                } else {
                    write!(f, " No schema attributes found. Did you forget to define schema with mentat_transact?")?;
                }
                if let Some(ref s) = suggestion {
                    write!(f, " Did you mean '{}'?", s)?;
                }
                Ok(())
            }

            Self::TypeMismatch {
                attr,
                expected,
                got,
                expected_tag,
                got_tag,
            } => {
                write!(
                    f,
                    ":db.error/wrong-type-for-attribute Type mismatch for attribute '{}': \
                     schema declares :db/valueType :db.type/{} (tag {}), but the asserted value \
                     has type {} (tag {}). Ensure the value matches the attribute's declared type.",
                    attr, expected, expected_tag, got, got_tag
                )
            }

            Self::UniqueConstraintViolation {
                attr,
                unique_type,
                existing_eid,
                new_eid,
            } => {
                write!(
                    f,
                    ":db.error/unique-conflict Unique constraint violation for attribute '{}' \
                     (unique type: :db.unique/{}). The asserted value already exists on entity {} \
                     but is being asserted for entity {}. \
                     To reassign the value, first retract it from entity {}.",
                    attr, unique_type, existing_eid, new_eid, existing_eid
                )
            }

            Self::InvalidQuery {
                message,
                suggestion,
            } => {
                write!(f, ":db.error/invalid-query {}", message)?;
                if let Some(ref s) = suggestion {
                    write!(f, " {}", s)?;
                }
                Ok(())
            }

            Self::InvalidPullPattern { message } => {
                write!(f, ":db.error/invalid-pull-pattern {}", message)
            }

            Self::InvalidTransaction { message } => {
                write!(f, ":db.error/invalid-transaction {}", message)
            }

            Self::EntityNotFound { ident, message } => {
                write!(
                    f,
                    ":db.error/ident-not-found Entity ident '{}' not found. {}",
                    ident, message
                )
            }

            Self::LookupRefNotFound { attr, message } => {
                write!(
                    f,
                    ":db.error/lookup-ref-not-found Lookup ref did not match any existing entity \
                     for attribute '{}'. {}",
                    attr, message
                )
            }

            Self::LookupRefRequiresUnique { attr } => {
                write!(
                    f,
                    ":db.error/lookup-ref-requires-unique Lookup ref attribute '{}' does not have \
                     a unique constraint. Only attributes with :db.unique/identity or :db.unique/value \
                     can be used in lookup refs. Add a unique constraint to the attribute definition, e.g.:\n  \
                     [:db/add \"attr\" :db/unique :db.unique/identity]",
                    attr
                )
            }

            Self::AllocationFailed { partition } => {
                write!(
                    f,
                    ":db.error/allocation-failed Failed to allocate entity ID. \
                     Check that the '{}' partition exists and has available IDs.",
                    partition
                )
            }

            Self::CardinalityViolation {
                attr,
                entity,
                count,
            } => {
                write!(
                    f,
                    ":db.error/cardinality-violation Attribute '{}' has :db/cardinality :db.cardinality/one \
                     but this transaction contains {} assertions for entity {}. \
                     Cardinality-one attributes can only have a single value per entity. \
                     Either remove duplicate assertions or change the attribute to :db.cardinality/many.",
                    attr, count, entity
                )
            }

            Self::CasFailed {
                entity,
                attr,
                expected,
                actual,
            } => {
                write!(
                    f,
                    ":db.fn/cas failed on entity {} attribute {}: expected {}, found {}",
                    entity, attr, expected, actual
                )
            }

            Self::DataCorruption { message } => {
                write!(f, ":db.error/data-corruption {}", message)
            }

            Self::UnsupportedType { type_tag } => {
                write!(
                    f,
                    ":db.error/unsupported-type Unsupported value type tag: {}. \
                     Known tags: 0=ref, 1=boolean, 2=long, 3=double, 4=instant, 7=string, \
                     8=keyword, 10=uuid, 11=bytes.",
                    type_tag
                )
            }

            Self::InvalidEntityPlace {
                got_type,
                got_value,
            } => {
                write!(
                    f,
                    ":db.error/invalid-entity-place Invalid entity place: got {} (value: {}). \
                     Entity position must be an integer (entity ID), string (tempid), \
                     keyword (ident), or 2-element vector (lookup ref like [:attr value]).",
                    got_type, got_value
                )
            }

            Self::InvalidAttribute {
                got_type,
                got_value,
            } => {
                write!(
                    f,
                    ":db.error/invalid-attribute Invalid attribute: got {} (value: {}). \
                     Attribute position must be an integer (entid) or keyword (e.g. :person/name).",
                    got_type, got_value
                )
            }

            Self::UnsupportedValueType {
                got_type,
                got_value,
            } => {
                write!(
                    f,
                    ":db.error/unsupported-value-type Cannot encode value of type {} (value: {}). \
                     Supported types: boolean, integer (long), double (float), instant, uuid, string, keyword. \
                     For ref values, use an entity ID, tempid string, or keyword ident.",
                    got_type, got_value
                )
            }

            Self::InvalidCardinality {
                cardinality,
                attr_entid,
            } => {
                write!(
                    f,
                    ":db.error/invalid-cardinality Unknown cardinality '{}' for attribute entid {}. \
                     Valid cardinalities are 'one' and 'many'. This may indicate schema corruption.",
                    cardinality, attr_entid
                )
            }

            Self::NothingToRetract { entity } => {
                write!(
                    f,
                    ":db.error/nothing-to-retract Entity {} has no current facts to retract. \
                     The entity may not exist or all its facts have already been retracted.",
                    entity
                )
            }

            Self::TransactionFailed { message } => {
                write!(f, ":db.error/tx-creation-failed {}", message)
            }

            Self::UnknownBatchOp { op } => {
                write!(
                    f,
                    ":db.error/unknown-batch-op Unknown batch operation type: {}. \
                     Valid operations: :query, :transact, :pull, :entity, :schema",
                    op
                )
            }

            Self::BatchMissingArg { op, message } => {
                write!(
                    f,
                    ":db.error/batch-missing-arg {} operation: {}",
                    op, message
                )
            }

            Self::DataIntegrity { message } => {
                write!(f, ":db.error/data-integrity {}", message)
            }

            Self::ResultLimitExceeded { limit, message } => {
                write!(
                    f,
                    ":db.error/result-limit-exceeded Query result exceeded the maximum of {} rows. \
                     {}. Adjust mentat.max_result_rows or add :limit to your query.",
                    limit, message
                )
            }

            Self::QueryTimeout { timeout_ms } => {
                write!(
                    f,
                    ":db.error/query-timeout Query exceeded the timeout of {}ms. \
                     Adjust mentat.query_timeout_ms or optimize the query.",
                    timeout_ms
                )
            }

            Self::StoreExists { store_name } => {
                write!(
                    f,
                    ":db.error/store-exists Store '{}' already exists.",
                    store_name
                )
            }

            Self::StoreNotFound { store_name } => {
                write!(
                    f,
                    ":db.error/store-not-found Store '{}' not found.",
                    store_name
                )
            }

            Self::InvalidStoreName { store_name, reason } => {
                write!(
                    f,
                    ":db.error/invalid-store-name Invalid store name '{}': {}",
                    store_name, reason
                )
            }

            Self::CannotDropDefaultStore => {
                write!(
                    f,
                    ":db.error/cannot-drop-default-store Cannot drop the default store. \
                     The default 'mentat' store is required for extension operation."
                )
            }

            Self::SerializationFailure { message, attempt } => {
                write!(
                    f,
                    ":db.error/serialization-failure Serialization failure on attempt {}: {}",
                    attempt, message
                )
            }

            Self::Internal { message, .. } => {
                write!(f, ":db.error/internal {}", message)
            }
        }
    }
}

impl std::error::Error for MentatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Internal {
                source: Some(ref e),
                ..
            } => Some(e.as_ref()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Conversions -- allow wrapping upstream errors into MentatError.
//
// `MentatError` implements `std::error::Error + Send + Sync`, so the blanket
// impl `From<E: Error + Send + Sync> for Box<dyn Error + Send + Sync>` in
// std already provides `.into()` conversion.
// ---------------------------------------------------------------------------

impl From<pgrx::spi::SpiError> for MentatError {
    fn from(e: pgrx::spi::SpiError) -> Self {
        Self::Internal {
            message: format!("SPI error: {}", e),
            source: Some(Box::new(e)),
        }
    }
}

impl From<std::string::FromUtf8Error> for MentatError {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Self::DataCorruption {
            message: format!("Invalid UTF-8 in stored value: {}", e),
        }
    }
}

// ---------------------------------------------------------------------------
// Error code accessor
// ---------------------------------------------------------------------------

impl MentatError {
    /// Return the Datomic-compatible `:db.error/...` code for this error.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::AttributeNotFound { .. } => ":db.error/attribute-not-found",
            Self::TypeMismatch { .. } => ":db.error/wrong-type-for-attribute",
            Self::UniqueConstraintViolation { .. } => ":db.error/unique-conflict",
            Self::InvalidQuery { .. } => ":db.error/invalid-query",
            Self::InvalidPullPattern { .. } => ":db.error/invalid-pull-pattern",
            Self::InvalidTransaction { .. } => ":db.error/invalid-transaction",
            Self::EntityNotFound { .. } => ":db.error/ident-not-found",
            Self::LookupRefNotFound { .. } => ":db.error/lookup-ref-not-found",
            Self::LookupRefRequiresUnique { .. } => ":db.error/lookup-ref-requires-unique",
            Self::AllocationFailed { .. } => ":db.error/allocation-failed",
            Self::CardinalityViolation { .. } => ":db.error/cardinality-violation",
            Self::CasFailed { .. } => ":db.fn/cas",
            Self::DataCorruption { .. } => ":db.error/data-corruption",
            Self::UnsupportedType { .. } => ":db.error/unsupported-type",
            Self::InvalidEntityPlace { .. } => ":db.error/invalid-entity-place",
            Self::InvalidAttribute { .. } => ":db.error/invalid-attribute",
            Self::UnsupportedValueType { .. } => ":db.error/unsupported-value-type",
            Self::InvalidCardinality { .. } => ":db.error/invalid-cardinality",
            Self::NothingToRetract { .. } => ":db.error/nothing-to-retract",
            Self::TransactionFailed { .. } => ":db.error/tx-creation-failed",
            Self::UnknownBatchOp { .. } => ":db.error/unknown-batch-op",
            Self::BatchMissingArg { .. } => ":db.error/batch-missing-arg",
            Self::DataIntegrity { .. } => ":db.error/data-integrity",
            Self::ResultLimitExceeded { .. } => ":db.error/result-limit-exceeded",
            Self::QueryTimeout { .. } => ":db.error/query-timeout",
            Self::StoreExists { .. } => ":db.error/store-exists",
            Self::StoreNotFound { .. } => ":db.error/store-not-found",
            Self::InvalidStoreName { .. } => ":db.error/invalid-store-name",
            Self::CannotDropDefaultStore => ":db.error/cannot-drop-default-store",
            Self::SerializationFailure { .. } => ":db.error/serialization-failure",
            Self::Internal { .. } => ":db.error/internal",
        }
    }
}

// ---------------------------------------------------------------------------
// "Did you mean?" helper -- Levenshtein distance
// ---------------------------------------------------------------------------

/// Compute the Levenshtein edit distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    // Use a single-row buffer (O(min(m,n)) space).
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Given a target string and a list of candidates, return the closest match
/// if its edit distance is within a reasonable threshold.
///
/// The threshold is max(2, target.len() / 3), which keeps suggestions useful
/// without being too loose for short strings.
pub fn suggest_closest(target: &str, candidates: &[String]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }

    let threshold = 2.max(target.len() / 3);

    let mut best: Option<(usize, &String)> = None;

    for candidate in candidates {
        let dist = levenshtein(target, candidate);
        if dist <= threshold {
            match best {
                None => best = Some((dist, candidate)),
                Some((best_dist, _)) if dist < best_dist => {
                    best = Some((dist, candidate));
                }
                _ => {}
            }
        }
    }

    best.map(|(_, s)| s.clone())
}

/// Retrieve available attribute idents from the schema cache for use in error
/// messages and "did you mean?" suggestions.
pub fn get_available_attributes() -> Vec<String> {
    let result: Result<Vec<String>, _> = pgrx::spi::Spi::connect(|client| {
        let mut idents = Vec::new();
        let rows = client.select(
            "SELECT ident FROM mentat.schema ORDER BY ident LIMIT 50",
            None,
            &[],
        )?;
        for row in rows {
            if let Ok(Some(ident)) = row.get::<String>(1) {
                idents.push(ident);
            }
        }
        Ok::<_, pgrx::spi::SpiError>(idents)
    });
    result.unwrap_or_default()
}

/// Build an `AttributeNotFound` error with automatic suggestion.
pub fn attribute_not_found(attr: &str) -> MentatError {
    let available = get_available_attributes();
    let suggestion = suggest_closest(attr, &available);
    MentatError::AttributeNotFound {
        attr: attr.to_string(),
        available,
        suggestion,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_identical() {
        assert_eq!(levenshtein("abc", "abc"), 0);
    }

    #[test]
    fn test_levenshtein_empty() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn test_levenshtein_one_edit() {
        assert_eq!(levenshtein(":person/name", ":person/namee"), 1);
        assert_eq!(levenshtein(":person/name", ":person/nme"), 1);
    }

    #[test]
    fn test_suggest_closest_finds_typo() {
        let candidates = vec![
            ":person/name".to_string(),
            ":person/age".to_string(),
            ":person/email".to_string(),
        ];
        assert_eq!(
            suggest_closest(":person/namee", &candidates),
            Some(":person/name".to_string())
        );
    }

    #[test]
    fn test_suggest_closest_no_match() {
        let candidates = vec![":person/name".to_string(), ":person/age".to_string()];
        // Too different -- should return None
        assert_eq!(suggest_closest(":zzz/yyy", &candidates), None);
    }

    #[test]
    fn test_suggest_closest_empty_candidates() {
        assert_eq!(suggest_closest("anything", &[]), None);
    }

    #[test]
    fn test_error_code() {
        let e = MentatError::AttributeNotFound {
            attr: ":test".into(),
            available: vec![],
            suggestion: None,
        };
        assert_eq!(e.error_code(), ":db.error/attribute-not-found");
    }

    #[test]
    fn test_display_attribute_not_found_with_suggestion() {
        let e = MentatError::AttributeNotFound {
            attr: ":person/namee".into(),
            available: vec![
                ":person/name".into(),
                ":person/age".into(),
                ":person/email".into(),
            ],
            suggestion: Some(":person/name".into()),
        };
        let msg = e.to_string();
        assert!(msg.starts_with(":db.error/attribute-not-found"));
        assert!(msg.contains(":person/namee"));
        assert!(msg.contains("Available attributes:"));
        assert!(msg.contains("Did you mean ':person/name'?"));
    }

    #[test]
    fn test_display_type_mismatch() {
        let e = MentatError::TypeMismatch {
            attr: ":person/age".into(),
            expected: "long".into(),
            got: "string".into(),
            expected_tag: 2,
            got_tag: 7,
        };
        let msg = e.to_string();
        assert!(msg.starts_with(":db.error/wrong-type-for-attribute"));
        assert!(msg.contains(":person/age"));
        assert!(msg.contains("long"));
        assert!(msg.contains("string"));
    }

    #[test]
    fn test_display_unique_violation() {
        let e = MentatError::UniqueConstraintViolation {
            attr: ":person/email".into(),
            unique_type: "identity".into(),
            existing_eid: 100,
            new_eid: 200,
        };
        let msg = e.to_string();
        assert!(msg.starts_with(":db.error/unique-conflict"));
        assert!(msg.contains("100"));
        assert!(msg.contains("200"));
    }

    #[test]
    fn test_into_box_dyn_error() {
        let e = MentatError::DataCorruption {
            message: "test".into(),
        };
        let boxed: Box<dyn std::error::Error + Send + Sync> = e.into();
        assert!(boxed.to_string().contains(":db.error/data-corruption"));
    }
}
