//! Shared type-tag constants for EAVT value encoding.
//!
//! These tags identify which narrow datom table a value lives in and
//! how to decode it back to a typed Rust/JSON/EDN value. Every module
//! that reads or writes datoms must agree on these values.

/// Value type tags matching the `value_type_tag` column in the
/// compatibility view and the narrow table routing logic.
pub mod type_tag {
    pub const REF: i16 = 0;
    pub const BOOLEAN: i16 = 1;
    pub const LONG: i16 = 2;
    pub const DOUBLE: i16 = 3;
    pub const INSTANT: i16 = 4;
    // 5 = BigInteger (rejected with :db.error/unsupported-constant)
    // 6 = unused
    pub const STRING: i16 = 7;
    pub const KEYWORD: i16 = 8;
    // 9 = unused
    pub const UUID: i16 = 10;
    pub const BYTES: i16 = 11;
}
