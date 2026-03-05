# TypedValue to PostgreSQL Type Mapping

## Overview

This document details how mentat's `TypedValue` enum values are encoded for storage in PostgreSQL and decoded back to their original types.

## Value Type Tags

Each `TypedValue` variant has a corresponding tag used in the `value_type_tag` column:

| Tag | TypedValue Variant | PostgreSQL Native Type | Storage Format (BYTEA) |
|-----|-------------------|----------------------|----------------------|
| 0 | `Ref(i64)` | BIGINT | 8 bytes, big-endian |
| 1 | `Boolean(bool)` | BOOLEAN | 1 byte (0x00 or 0x01) |
| 3 | `Double(f64)` | DOUBLE PRECISION | 8 bytes, IEEE 754 |
| 4 | `Long(i64)` | BIGINT | 8 bytes, big-endian |
| 5 | `Instant(DateTime)` | TIMESTAMPTZ | 8 bytes, microseconds since Unix epoch |
| 10 | `String(String)` | TEXT | UTF-8 encoded bytes |
| 11 | `Uuid(Uuid)` | UUID | 16 bytes, RFC 4122 format |
| 12 | `Bytes(Vec<u8>)` | BYTEA | Raw bytes |
| 13 | `Keyword(Keyword)` | EdnValue (custom) | EDN-encoded format |

## Encoding Details

### Ref (Tag 0)

References are entity IDs stored as 64-bit integers.

**Rust:**
```rust
TypedValue::Ref(123456)
```

**PostgreSQL:**
```sql
-- Stored in datoms.v as 8-byte big-endian
-- value_type_tag = 0
SELECT '\x000000000001e240'::bytea; -- 123456 in big-endian
```

**Encoding:**
```rust
fn encode_ref(entid: i64) -> Vec<u8> {
    entid.to_be_bytes().to_vec()
}
```

### Boolean (Tag 1)

Boolean values are stored as a single byte.

**Rust:**
```rust
TypedValue::Boolean(true)
TypedValue::Boolean(false)
```

**PostgreSQL:**
```sql
-- value_type_tag = 1
SELECT '\x01'::bytea; -- true
SELECT '\x00'::bytea; -- false
```

**Encoding:**
```rust
fn encode_boolean(b: bool) -> Vec<u8> {
    vec![if b { 1 } else { 0 }]
}
```

### Long (Tag 4)

64-bit signed integers.

**Rust:**
```rust
TypedValue::Long(42)
```

**PostgreSQL:**
```sql
-- value_type_tag = 4
SELECT '\x000000000000002a'::bytea; -- 42 in big-endian
```

**Encoding:**
```rust
fn encode_long(n: i64) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}
```

### Double (Tag 3)

64-bit floating point numbers using IEEE 754 format.

**Rust:**
```rust
TypedValue::Double(OrderedFloat(3.14159))
```

**PostgreSQL:**
```sql
-- value_type_tag = 3
-- IEEE 754 double precision format
SELECT '\x400921fb54442d18'::bytea; -- 3.14159
```

**Encoding:**
```rust
fn encode_double(d: f64) -> Vec<u8> {
    d.to_be_bytes().to_vec()
}
```

### Instant (Tag 5)

Timestamps are stored as microseconds since Unix epoch (1970-01-01 00:00:00 UTC).

**Rust:**
```rust
use chrono::{DateTime, Utc};
TypedValue::Instant(DateTime::from_timestamp(1234567890, 0).unwrap())
```

**PostgreSQL:**
```sql
-- value_type_tag = 5
-- Microseconds since Unix epoch
SELECT '\x000000499602d200'::bytea; -- 1234567890000000 microseconds
```

**Encoding:**
```rust
fn encode_instant(dt: DateTime<Utc>) -> Vec<u8> {
    let micros = dt.timestamp() * 1_000_000 + dt.timestamp_subsec_micros() as i64;
    micros.to_be_bytes().to_vec()
}
```

### String (Tag 10)

UTF-8 encoded text strings.

**Rust:**
```rust
TypedValue::String(ValueRc::new("hello".to_string()))
```

**PostgreSQL:**
```sql
-- value_type_tag = 10
SELECT 'hello'::bytea; -- UTF-8 encoded
```

**Encoding:**
```rust
fn encode_string(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}
```

**Note:** For fulltext-indexed strings, the value is a rowid reference (encoded as Ref/Long) to the `fulltext` table.

### Keyword (Tag 13)

EDN keywords using the custom `EdnValue` type.

**Rust:**
```rust
TypedValue::Keyword(ValueRc::new(Keyword::namespaced("db", "ident")))
```

**PostgreSQL:**
```sql
-- value_type_tag = 13
-- Stored using EdnValue custom type
SELECT mentat.edn_in(':db/ident');
```

**Encoding:**
Uses the custom `EdnValue` PostgreSQL type provided by pgrx. The keyword is serialized to EDN format and stored as bytea.

### Uuid (Tag 11)

128-bit UUIDs in RFC 4122 format.

**Rust:**
```rust
use uuid::Uuid;
TypedValue::Uuid(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap())
```

**PostgreSQL:**
```sql
-- value_type_tag = 11
SELECT '\x550e8400e29b41d4a716446655440000'::bytea;
-- Or use PostgreSQL's UUID type
SELECT '550e8400-e29b-41d4-a716-446655440000'::uuid;
```

**Encoding:**
```rust
fn encode_uuid(uuid: &Uuid) -> Vec<u8> {
    uuid.as_bytes().to_vec()
}
```

### Bytes (Tag 12)

Raw binary data stored as-is.

**Rust:**
```rust
TypedValue::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF])
```

**PostgreSQL:**
```sql
-- value_type_tag = 12
SELECT '\xdeadbeef'::bytea;
```

**Encoding:**
```rust
fn encode_bytes(bytes: &[u8]) -> Vec<u8> {
    bytes.to_vec()
}
```

## Decoding from PostgreSQL

To decode a datom value, read both `value_type_tag` and `v`:

```rust
fn decode_typed_value(value_type_tag: i16, v: &[u8]) -> Result<TypedValue> {
    match value_type_tag {
        0 => Ok(TypedValue::Ref(i64::from_be_bytes(v.try_into()?))),
        1 => Ok(TypedValue::Boolean(v[0] != 0)),
        3 => Ok(TypedValue::Double(OrderedFloat(f64::from_be_bytes(v.try_into()?)))),
        4 => Ok(TypedValue::Long(i64::from_be_bytes(v.try_into()?))),
        5 => {
            let micros = i64::from_be_bytes(v.try_into()?);
            let dt = DateTime::from_timestamp(micros / 1_000_000, ((micros % 1_000_000) * 1000) as u32)?;
            Ok(TypedValue::Instant(dt))
        }
        10 => Ok(TypedValue::String(ValueRc::new(String::from_utf8(v.to_vec())?))),
        11 => Ok(TypedValue::Uuid(Uuid::from_bytes(v.try_into()?))),
        12 => Ok(TypedValue::Bytes(v.to_vec())),
        13 => {
            // Decode EDN keyword
            let edn_str = String::from_utf8(v.to_vec())?;
            let keyword = parse_edn_keyword(&edn_str)?;
            Ok(TypedValue::Keyword(ValueRc::new(keyword)))
        }
        _ => Err(format!("Unknown value type tag: {}", value_type_tag).into()),
    }
}
```

## SQL Helper Functions

### Encoding in SQL

```sql
-- Encode a long value
CREATE FUNCTION mentat.encode_long(n BIGINT) RETURNS BYTEA AS $$
    SELECT int8send(n);
$$ LANGUAGE sql IMMUTABLE;

-- Encode a boolean
CREATE FUNCTION mentat.encode_boolean(b BOOLEAN) RETURNS BYTEA AS $$
    SELECT CASE WHEN b THEN '\x01'::bytea ELSE '\x00'::bytea END;
$$ LANGUAGE sql IMMUTABLE;

-- Encode a double
CREATE FUNCTION mentat.encode_double(d DOUBLE PRECISION) RETURNS BYTEA AS $$
    SELECT float8send(d);
$$ LANGUAGE sql IMMUTABLE;

-- Encode a UUID
CREATE FUNCTION mentat.encode_uuid(u UUID) RETURNS BYTEA AS $$
    SELECT uuid_send(u);
$$ LANGUAGE sql IMMUTABLE;

-- Encode a string
CREATE FUNCTION mentat.encode_string(s TEXT) RETURNS BYTEA AS $$
    SELECT s::bytea;
$$ LANGUAGE sql IMMUTABLE;
```

### Decoding in SQL

```sql
-- Decode based on value type tag
CREATE FUNCTION mentat.decode_value(vtype_tag SMALLINT, v BYTEA)
RETURNS TEXT AS $$
BEGIN
    RETURN CASE vtype_tag
        WHEN 0 THEN int8recv(v)::text              -- Ref
        WHEN 1 THEN (v = '\x01'::bytea)::text      -- Boolean
        WHEN 3 THEN float8recv(v)::text            -- Double
        WHEN 4 THEN int8recv(v)::text              -- Long
        WHEN 5 THEN to_timestamp(int8recv(v) / 1000000.0)::text  -- Instant
        WHEN 10 THEN convert_from(v, 'UTF8')       -- String
        WHEN 11 THEN uuid_recv(v)::text            -- UUID
        WHEN 12 THEN v::text                       -- Bytes (hex)
        WHEN 13 THEN convert_from(v, 'UTF8')       -- Keyword (EDN)
        ELSE 'unknown'
    END;
END;
$$ LANGUAGE plpgsql IMMUTABLE;
```

## Performance Considerations

1. **Fixed-width types** (Ref, Boolean, Long, Double, Instant, UUID) are more efficient to encode/decode
2. **Variable-width types** (String, Bytes, Keyword) may have overhead
3. **Fulltext strings** store only a rowid reference, making datom storage more compact
4. **Indexing**: BTREE indexes on BYTEA columns use byte-order comparison, which works well for big-endian encoded integers

## Validation

The `validate_datom_value_type()` trigger ensures that:
- The `value_type_tag` matches the attribute's declared `value_type`
- Type safety is enforced at insertion time
- Prevents storing incorrect types

Example:

```sql
-- This would fail if attribute 42 has value_type='string'
INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag)
VALUES (10000, 42, '\x0000000000000001'::bytea, 1000000000, TRUE, 4);
-- ERROR: Value type mismatch for attribute 42: expected string, got tag 4
```

## Compatibility with SQLite

The encoding scheme is designed to be compatible with mentat's original SQLite storage:

- Same value_type_tag values
- Same byte encoding for fixed-width types
- EDN encoding for complex types

This allows for potential migration paths between SQLite and PostgreSQL backends.
