// Datomic Client API Transit encoding/decoding compatibility tests.
//
// Tests that Transit+JSON and Transit+MessagePack encoding matches the
// Datomic/Cognitect Transit specification. Verifies:
//
// - All Transit value type encodings (keywords, symbols, large ints, UUIDs, instants)
// - Map encoding as cmap: ["^ ", key, value, ...]
// - Vector, list, and nested structure encoding
// - Special float values (NaN, INF, -INF)
// - String escaping for tilde/caret prefixed strings
// - Anomaly/error encoding in cognitect.anomalies format
// - Content-type negotiation (Accept header parsing)
// - MessagePack binary format basics
//
// Reference: tests/datomic_compatibility/README.md Section 1 (Wire Protocol)

mod helpers;

// ============================================================================
// Section 1: Transit+JSON Serialization
// ============================================================================

mod transit_json_tests {
    use mentatd::protocol::transit_serializer::serialize_transit_json;
    use mentatd::protocol::{Anomaly, AnomalyCategory, Response, ResponseValue};

    // -- Primitive value types ----------------------------------------------

    /// Transit+JSON nil is JSON null.
    #[test]
    fn test_nil() {
        let response = Response::Success {
            result: ResponseValue::Nil,
        };
        assert_eq!(
            serialize_transit_json(&response),
            r#"["^ ","~:result",null]"#
        );
    }

    /// Transit+JSON booleans are JSON booleans.
    #[test]
    fn test_boolean_true() {
        let response = Response::Success {
            result: ResponseValue::Boolean(true),
        };
        assert_eq!(
            serialize_transit_json(&response),
            r#"["^ ","~:result",true]"#
        );
    }

    #[test]
    fn test_boolean_false() {
        let response = Response::Success {
            result: ResponseValue::Boolean(false),
        };
        assert_eq!(
            serialize_transit_json(&response),
            r#"["^ ","~:result",false]"#
        );
    }

    /// Strings are JSON strings. No tag prefix unless they start with ~ or ^.
    #[test]
    fn test_plain_string() {
        let response = Response::Success {
            result: ResponseValue::String("hello world".to_string()),
        };
        assert_eq!(
            serialize_transit_json(&response),
            r#"["^ ","~:result","hello world"]"#
        );
    }

    /// Small integers (within i32 range) are JSON numbers.
    #[test]
    fn test_small_integer() {
        let response = Response::Success {
            result: ResponseValue::Integer(42),
        };
        assert_eq!(serialize_transit_json(&response), r#"["^ ","~:result",42]"#);
    }

    #[test]
    fn test_zero() {
        let response = Response::Success {
            result: ResponseValue::Integer(0),
        };
        assert_eq!(serialize_transit_json(&response), r#"["^ ","~:result",0]"#);
    }

    #[test]
    fn test_negative_integer() {
        let response = Response::Success {
            result: ResponseValue::Integer(-100),
        };
        assert_eq!(
            serialize_transit_json(&response),
            r#"["^ ","~:result",-100]"#
        );
    }

    /// Large integers (outside i32 range) are tagged: "~i<number>".
    #[test]
    fn test_large_integer_positive() {
        let response = Response::Success {
            result: ResponseValue::Integer(9_999_999_999),
        };
        assert_eq!(
            serialize_transit_json(&response),
            r#"["^ ","~:result","~i9999999999"]"#
        );
    }

    #[test]
    fn test_large_integer_negative() {
        let response = Response::Success {
            result: ResponseValue::Integer(-3_000_000_000),
        };
        assert_eq!(
            serialize_transit_json(&response),
            r#"["^ ","~:result","~i-3000000000"]"#
        );
    }

    #[test]
    fn test_integer_at_i32_boundary() {
        // i32::MAX = 2_147_483_647
        let response = Response::Success {
            result: ResponseValue::Integer(2_147_483_647),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result",2147483647]"#);

        // One beyond i32 max
        let response = Response::Success {
            result: ResponseValue::Integer(2_147_483_648),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result","~i2147483648"]"#);
    }

    // -- Keywords -----------------------------------------------------------

    /// Keywords use the ~: prefix: "~:db/name".
    #[test]
    fn test_keyword_simple() {
        let response = Response::Success {
            result: ResponseValue::Keyword("name".to_string()),
        };
        assert_eq!(
            serialize_transit_json(&response),
            r#"["^ ","~:result","~:name"]"#
        );
    }

    #[test]
    fn test_keyword_namespaced() {
        let response = Response::Success {
            result: ResponseValue::Keyword("db/ident".to_string()),
        };
        assert_eq!(
            serialize_transit_json(&response),
            r#"["^ ","~:result","~:db/ident"]"#
        );
    }

    #[test]
    fn test_keyword_with_dashes() {
        let response = Response::Success {
            result: ResponseValue::Keyword("person/first-name".to_string()),
        };
        assert_eq!(
            serialize_transit_json(&response),
            r#"["^ ","~:result","~:person/first-name"]"#
        );
    }

    // -- UUIDs --------------------------------------------------------------

    /// UUIDs use the ~u prefix: "~u550e8400-...".
    #[test]
    fn test_uuid() {
        let response = Response::Success {
            result: ResponseValue::Uuid("550e8400-e29b-41d4-a716-446655440000".to_string()),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(
            json,
            r#"["^ ","~:result","~u550e8400-e29b-41d4-a716-446655440000"]"#
        );
    }

    // -- Instants -----------------------------------------------------------

    /// Instants use the ~m prefix with milliseconds: "~m1234567890".
    #[test]
    fn test_instant() {
        // 1_714_000_000_000 microseconds = 1_714_000_000 milliseconds
        let response = Response::Success {
            result: ResponseValue::Instant(1_714_000_000_000),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result","~m1714000000"]"#);
    }

    // -- Floats -------------------------------------------------------------

    #[test]
    fn test_float_with_fraction() {
        let response = Response::Success {
            result: ResponseValue::Float(3.14),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("3.14"));
    }

    #[test]
    fn test_float_whole_number() {
        let response = Response::Success {
            result: ResponseValue::Float(42.0),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("42.0"));
    }

    /// Special floats use tagged strings: "~zNaN", "~zINF", "~z-INF".
    #[test]
    fn test_float_nan() {
        let response = Response::Success {
            result: ResponseValue::Float(f64::NAN),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~zNaN\""));
    }

    #[test]
    fn test_float_positive_infinity() {
        let response = Response::Success {
            result: ResponseValue::Float(f64::INFINITY),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~zINF\""));
    }

    #[test]
    fn test_float_negative_infinity() {
        let response = Response::Success {
            result: ResponseValue::Float(f64::NEG_INFINITY),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~z-INF\""));
    }

    // -- String escaping ----------------------------------------------------

    /// Strings starting with ~ must be escaped with an extra ~ prefix.
    #[test]
    fn test_string_tilde_escape() {
        let response = Response::Success {
            result: ResponseValue::String("~special".to_string()),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result","~~special"]"#);
    }

    /// Strings starting with ^ must also be escaped.
    #[test]
    fn test_string_caret_escape() {
        let response = Response::Success {
            result: ResponseValue::String("^caret".to_string()),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result","~^caret"]"#);
    }

    /// Strings with embedded quotes are JSON-escaped.
    #[test]
    fn test_string_with_quotes() {
        let response = Response::Success {
            result: ResponseValue::String(r#"say "hello""#.to_string()),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains(r#"say \"hello\""#));
    }

    /// Strings with newlines are escaped.
    #[test]
    fn test_string_with_newline() {
        let response = Response::Success {
            result: ResponseValue::String("line1\nline2".to_string()),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("line1\\nline2"));
    }

    // -- Collections --------------------------------------------------------

    /// Vectors are plain JSON arrays.
    #[test]
    fn test_vector() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![
                ResponseValue::Integer(1),
                ResponseValue::String("two".to_string()),
                ResponseValue::Boolean(true),
            ]),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result",[1,"two",true]]"#);
    }

    /// Empty vector is [].
    #[test]
    fn test_empty_vector() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![]),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result",[]]"#);
    }

    /// Lists are tagged arrays: ["~#list", [items...]].
    #[test]
    fn test_list() {
        let response = Response::Success {
            result: ResponseValue::List(vec![ResponseValue::Integer(1), ResponseValue::Integer(2)]),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result",["~#list",[1,2]]]"#);
    }

    /// Maps use cmap form: ["^ ", key, val, key, val, ...].
    #[test]
    fn test_map() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![(
                ResponseValue::Keyword("name".to_string()),
                ResponseValue::String("Alice".to_string()),
            )]),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result",["^ ","~:name","Alice"]]"#);
    }

    /// Empty map.
    #[test]
    fn test_empty_map() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![]),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result",["^ "]]"#);
    }

    /// Map with multiple entries.
    #[test]
    fn test_map_multiple_entries() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("db/id".to_string()),
                    ResponseValue::Integer(42),
                ),
                (
                    ResponseValue::Keyword("person/name".to_string()),
                    ResponseValue::String("Alice".to_string()),
                ),
            ]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~:db/id\",42"));
        assert!(json.contains("\"~:person/name\",\"Alice\""));
    }

    // -- Nested structures --------------------------------------------------

    /// Nested vectors (query result rows).
    #[test]
    fn test_nested_vectors() {
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
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result",[[10001,"Alice"],[10002,"Bob"]]]"#);
    }

    /// Map containing vectors (typical datoms response).
    #[test]
    fn test_map_with_vector_values() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("chunks".to_string()),
                    ResponseValue::Vector(vec![ResponseValue::Vector(vec![
                        ResponseValue::Integer(1),
                        ResponseValue::Integer(2),
                    ])]),
                ),
                (
                    ResponseValue::Keyword("total-count".to_string()),
                    ResponseValue::Integer(2),
                ),
            ]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~:chunks\",[[1,2]]"));
        assert!(json.contains("\"~:total-count\",2"));
    }

    // -- DbSnapshot ---------------------------------------------------------

    #[test]
    fn test_db_snapshot() {
        let response = Response::Success {
            result: ResponseValue::DbSnapshot {
                db_id: "snap-123".to_string(),
                basis_t: 1000042,
            },
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("~#db"));
        assert!(json.contains("snap-123"));
        assert!(json.contains("1000042"));
    }

    // -- Error/Anomaly encoding ---------------------------------------------

    /// Error responses follow cognitect.anomalies format.
    #[test]
    fn test_error_not_found() {
        let response = Response::Error {
            anomaly: Anomaly {
                category: AnomalyCategory::NotFound,
                message: "Entity not found".to_string(),
                db_error: Some("db.error/not-found".to_string()),
            },
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~:cognitect.anomalies/category\""));
        assert!(json.contains("\"~:cognitect.anomalies/not-found\""));
        assert!(json.contains("\"~:cognitect.anomalies/message\""));
        assert!(json.contains("Entity not found"));
        assert!(json.contains("\"~:db/error\""));
        assert!(json.contains("\"~:db.error/not-found\""));
    }

    /// Error without db_error field.
    #[test]
    fn test_error_without_db_error() {
        let response = Response::Error {
            anomaly: Anomaly {
                category: AnomalyCategory::Incorrect,
                message: "Bad request".to_string(),
                db_error: None,
            },
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~:cognitect.anomalies/incorrect\""));
        assert!(json.contains("Bad request"));
        assert!(!json.contains("~:db/error"));
    }

    /// All anomaly categories produce correct Transit keywords.
    #[test]
    fn test_all_anomaly_categories_transit() {
        let categories = [
            (AnomalyCategory::Incorrect, "incorrect"),
            (AnomalyCategory::Forbidden, "forbidden"),
            (AnomalyCategory::NotFound, "not-found"),
            (AnomalyCategory::Unavailable, "unavailable"),
            (AnomalyCategory::Interrupted, "interrupted"),
            (AnomalyCategory::Fault, "fault"),
        ];
        for (cat, name) in &categories {
            let response = Response::Error {
                anomaly: Anomaly {
                    category: *cat,
                    message: "test".to_string(),
                    db_error: None,
                },
            };
            let json = serialize_transit_json(&response);
            let expected = format!("\"~:cognitect.anomalies/{}\"", name);
            assert!(
                json.contains(&expected),
                "Category {} should produce {}: {}",
                name,
                expected,
                json
            );
        }
    }

    // -- Response format for new operations ---------------------------------

    #[test]
    fn test_serialize_db_stats_response() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("datoms".to_string()),
                    ResponseValue::Integer(5000),
                ),
                (
                    ResponseValue::Keyword("transactions".to_string()),
                    ResponseValue::Integer(42),
                ),
            ]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~:datoms\",5000"));
        assert!(json.contains("\"~:transactions\",42"));
    }

    #[test]
    fn test_serialize_entid_response() {
        let response = Response::Success {
            result: ResponseValue::Integer(73),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result",73]"#);
    }

    #[test]
    fn test_serialize_ident_response() {
        let response = Response::Success {
            result: ResponseValue::Keyword("person/name".to_string()),
        };
        let json = serialize_transit_json(&response);
        assert_eq!(json, r#"["^ ","~:result","~:person/name"]"#);
    }

    #[test]
    fn test_serialize_pull_many_response() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![
                ResponseValue::Map(vec![(
                    ResponseValue::Keyword("db/id".to_string()),
                    ResponseValue::Integer(42),
                )]),
                ResponseValue::Map(vec![(
                    ResponseValue::Keyword("db/id".to_string()),
                    ResponseValue::Integer(43),
                )]),
            ]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~:db/id\",42"));
        assert!(json.contains("\"~:db/id\",43"));
    }

    #[test]
    fn test_serialize_qseq_chunked_response() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("chunks".to_string()),
                    ResponseValue::Vector(vec![ResponseValue::Vector(vec![
                        ResponseValue::Integer(1),
                        ResponseValue::Integer(2),
                    ])]),
                ),
                (
                    ResponseValue::Keyword("total-count".to_string()),
                    ResponseValue::Integer(2),
                ),
                (
                    ResponseValue::Keyword("chunk-size".to_string()),
                    ResponseValue::Integer(1000),
                ),
            ]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~:chunks\""));
        assert!(json.contains("\"~:total-count\",2"));
        assert!(json.contains("\"~:chunk-size\",1000"));
    }

    #[test]
    fn test_serialize_index_range_response() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![ResponseValue::Vector(vec![
                ResponseValue::Integer(42),
                ResponseValue::Integer(73),
                ResponseValue::String("Alice".to_string()),
                ResponseValue::Integer(1000001),
                ResponseValue::Boolean(true),
            ])]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("42"));
        assert!(json.contains("73"));
        assert!(json.contains("\"Alice\""));
        assert!(json.contains("1000001"));
        assert!(json.contains("true"));
    }

    #[test]
    fn test_serialize_connect_response() {
        let response = Response::Success {
            result: ResponseValue::Map(vec![
                (
                    ResponseValue::Keyword("db-name".to_string()),
                    ResponseValue::String("my-db".to_string()),
                ),
                (
                    ResponseValue::Keyword("type".to_string()),
                    ResponseValue::Keyword("datomic.client/connection".to_string()),
                ),
            ]),
        };
        let json = serialize_transit_json(&response);
        assert!(json.contains("\"~:db-name\",\"my-db\""));
        assert!(json.contains("\"~:type\",\"~:datomic.client/connection\""));
    }
}

// ============================================================================
// Section 2: Transit+MessagePack Serialization
// ============================================================================

mod transit_msgpack_tests {
    use mentatd::protocol::transit_serializer::serialize_transit_msgpack;
    use mentatd::protocol::{Anomaly, AnomalyCategory, Response, ResponseValue};

    /// MessagePack output starts with fixarray(3) = 0x93 for success responses.
    #[test]
    fn test_success_starts_with_fixarray3() {
        let response = Response::Success {
            result: ResponseValue::Integer(42),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert_eq!(bytes[0], 0x93, "Should start with fixarray(3)");
    }

    /// Contains the "^ " cmap marker.
    #[test]
    fn test_contains_cmap_marker() {
        let response = Response::Success {
            result: ResponseValue::Nil,
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(
            bytes.windows(2).any(|w| w == b"^ "),
            "Should contain cmap marker '^ '"
        );
    }

    /// Contains "~:result" keyword.
    #[test]
    fn test_contains_result_keyword() {
        let response = Response::Success {
            result: ResponseValue::Nil,
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(bytes.windows(8).any(|w| w == b"~:result"));
    }

    /// Nil is encoded as 0xC0.
    #[test]
    fn test_nil_byte() {
        let response = Response::Success {
            result: ResponseValue::Nil,
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(bytes.contains(&0xC0));
    }

    /// Boolean true is 0xC3, false is 0xC2.
    #[test]
    fn test_boolean_bytes() {
        let true_resp = Response::Success {
            result: ResponseValue::Boolean(true),
        };
        let false_resp = Response::Success {
            result: ResponseValue::Boolean(false),
        };
        assert!(serialize_transit_msgpack(&true_resp).contains(&0xC3));
        assert!(serialize_transit_msgpack(&false_resp).contains(&0xC2));
    }

    /// Strings appear as byte content in the msgpack output.
    #[test]
    fn test_string_content() {
        let response = Response::Success {
            result: ResponseValue::String("hello".to_string()),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(bytes.windows(5).any(|w| w == b"hello"));
    }

    /// Keywords are tagged with ~: prefix.
    #[test]
    fn test_keyword_tagged() {
        let response = Response::Success {
            result: ResponseValue::Keyword("db/name".to_string()),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(bytes.windows(9).any(|w| w == b"~:db/name"));
    }

    /// Large integers use uint32 encoding (0xCE prefix).
    #[test]
    fn test_large_integer() {
        let response = Response::Success {
            result: ResponseValue::Integer(1_000_000),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(bytes.contains(&0xCE));
    }

    /// Negative integers use int8 format (0xD0 prefix) for small negatives.
    #[test]
    fn test_negative_integer() {
        let response = Response::Success {
            result: ResponseValue::Integer(-100),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(bytes.contains(&0xD0));
    }

    /// Vectors produce a fixarray header in the output.
    #[test]
    fn test_vector_array_header() {
        let response = Response::Success {
            result: ResponseValue::Vector(vec![
                ResponseValue::Integer(1),
                ResponseValue::Integer(2),
                ResponseValue::Integer(3),
            ]),
        };
        let bytes = serialize_transit_msgpack(&response);
        // Inner vector [1, 2, 3] should have fixarray(3) = 0x93
        assert!(bytes.contains(&0x93));
    }

    /// Error responses contain the anomaly category string.
    #[test]
    fn test_error_contains_category() {
        let response = Response::Error {
            anomaly: Anomaly {
                category: AnomalyCategory::Fault,
                message: "test error".to_string(),
                db_error: None,
            },
        };
        let bytes = serialize_transit_msgpack(&response);
        let cat = b"cognitect.anomalies/fault";
        assert!(bytes.windows(cat.len()).any(|w| w == cat));
    }

    /// Error responses contain the message.
    #[test]
    fn test_error_contains_message() {
        let response = Response::Error {
            anomaly: Anomaly {
                category: AnomalyCategory::NotFound,
                message: "entity not found".to_string(),
                db_error: None,
            },
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(bytes.windows(16).any(|w| w == b"entity not found"));
    }

    /// UUIDs appear with ~u prefix in msgpack.
    #[test]
    fn test_uuid_tagged() {
        let response = Response::Success {
            result: ResponseValue::Uuid("550e8400-e29b-41d4-a716-446655440000".to_string()),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(bytes.windows(3).any(|w| w == b"~u5"));
    }

    /// Instants appear with ~m prefix in msgpack.
    #[test]
    fn test_instant_tagged() {
        let response = Response::Success {
            result: ResponseValue::Instant(1_714_000_000_000),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(bytes.windows(2).any(|w| w == b"~m"));
    }

    /// Lists are tagged ["~#list", [items...]].
    #[test]
    fn test_list_tagged() {
        let response = Response::Success {
            result: ResponseValue::List(vec![ResponseValue::Integer(1), ResponseValue::Integer(2)]),
        };
        let bytes = serialize_transit_msgpack(&response);
        assert!(bytes.windows(6).any(|w| w == b"~#list"));
    }
}

// ============================================================================
// Section 3: Content-Type Negotiation
// ============================================================================

mod content_type_tests {
    use mentatd::protocol::transit_serializer::{
        content_type_for_encoding, parse_accept_encoding, TransitEncoding,
    };

    #[test]
    fn test_accept_transit_json() {
        assert_eq!(
            parse_accept_encoding("application/transit+json"),
            Some(TransitEncoding::Json)
        );
    }

    #[test]
    fn test_accept_transit_msgpack() {
        assert_eq!(
            parse_accept_encoding("application/transit+msgpack"),
            Some(TransitEncoding::Msgpack)
        );
    }

    #[test]
    fn test_accept_edn_returns_none() {
        assert_eq!(parse_accept_encoding("application/edn"), None);
    }

    #[test]
    fn test_accept_wildcard_returns_none() {
        assert_eq!(parse_accept_encoding("*/*"), None);
    }

    #[test]
    fn test_accept_quality_negotiation() {
        assert_eq!(
            parse_accept_encoding(
                "application/transit+json;q=0.8, application/transit+msgpack;q=1.0"
            ),
            Some(TransitEncoding::Msgpack)
        );
    }

    #[test]
    fn test_accept_json_preferred_by_quality() {
        assert_eq!(
            parse_accept_encoding(
                "application/transit+json;q=1.0, application/transit+msgpack;q=0.5"
            ),
            Some(TransitEncoding::Json)
        );
    }

    #[test]
    fn test_accept_multiple_with_edn() {
        // EDN is not a transit encoding, so the best transit one wins
        assert_eq!(
            parse_accept_encoding("application/edn;q=1.0, application/transit+json;q=0.9"),
            Some(TransitEncoding::Json)
        );
    }

    #[test]
    fn test_content_type_for_json() {
        assert_eq!(
            content_type_for_encoding(TransitEncoding::Json),
            "application/transit+json"
        );
    }

    #[test]
    fn test_content_type_for_msgpack() {
        assert_eq!(
            content_type_for_encoding(TransitEncoding::Msgpack),
            "application/transit+msgpack"
        );
    }
}

// ============================================================================
// Section 4: Transit+MessagePack over HTTP Integration
// ============================================================================

use helpers::TestServer;

#[tokio::test]
async fn test_transit_msgpack_health_request() {
    let server = TestServer::start().await;

    // Build msgpack-encoded Transit request: ["^ ", "~:op", "~:health"]
    let mut buf = Vec::new();
    buf.push(0x93); // fixarray(3)
    buf.push(0xa2); // fixstr(2)
    buf.extend_from_slice(b"^ ");
    buf.push(0xa4); // fixstr(4)
    buf.extend_from_slice(b"~:op");
    buf.push(0xa8); // fixstr(8)
    buf.extend_from_slice(b"~:health");

    let response = server.client.post_transit_msgpack("/", buf).await;
    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/transit+msgpack")
    );
    assert!(
        !response.body.is_empty(),
        "MessagePack response should not be empty"
    );
}

#[tokio::test]
async fn test_transit_msgpack_list_dbs_request() {
    let server = TestServer::start().await;

    // ["^ ", "~:op", "~:list-dbs"]
    let mut buf = Vec::new();
    buf.push(0x93);
    buf.push(0xa2);
    buf.extend_from_slice(b"^ ");
    buf.push(0xa4);
    buf.extend_from_slice(b"~:op");
    buf.push(0xa9); // fixstr(9)
    buf.extend_from_slice(b"~:list-dbs");

    let response = server.client.post_transit_msgpack("/", buf).await;
    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/transit+msgpack")
    );
    assert!(!response.body.is_empty());
}

// ============================================================================
// Section 5: Transit+JSON over HTTP Integration
// ============================================================================

#[tokio::test]
async fn test_transit_json_health() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json("/", r#"["^ ","~:op","~:health"]"#)
        .await;
    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/transit+json")
    );
    assert!(response.body.contains("result"));
}

#[tokio::test]
async fn test_transit_json_list_dbs() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json("/", r#"["^ ","~:op","~:list-dbs"]"#)
        .await;
    assert_eq!(response.status, 200);
    // Without a database, server returns anomaly error in Datomic format
    assert!(
        response.body.contains("~:result") || response.body.contains("~:error"),
        "list-dbs should return ~:result or ~:error in Transit format: {}",
        response.body
    );
}

#[tokio::test]
async fn test_transit_json_connect() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json(
            "/",
            r#"["^ ","~:op","~:connect","~:args",["^ ","~:db-name","postgres"]]"#,
        )
        .await;
    assert_eq!(response.status, 200);
    // Without a database, server returns anomaly error in Datomic format
    assert!(
        response.body.contains("~:result") || response.body.contains("~:error"),
        "Connect should return ~:result or ~:error in Transit format: {}",
        response.body
    );
}

#[tokio::test]
async fn test_transit_json_invalid_operation() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json("/", r#"["^ ","~:op","~:nonexistent-xyz"]"#)
        .await;
    assert_eq!(response.status, 200);
    assert!(response.body.contains("error"));
}

#[tokio::test]
async fn test_transit_json_malformed() {
    let server = TestServer::start().await;
    let response = server
        .client
        .post_transit_json("/", "this is not valid transit json")
        .await;
    assert_eq!(response.status, 200);
    assert!(response.body.contains("error"));
}
