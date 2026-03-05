# PostgreSQL Storage Schema for Mentat

## Overview

This document describes the PostgreSQL storage schema for mentat datoms, designed to provide ACID-compliant, scalable storage with full support for Datomic-style queries and time-travel.

## Design Principles

1. **Immutability**: Facts are never deleted, only marked as retracted (added=FALSE)
2. **Time-aware**: All facts are timestamped with the transaction that created them
3. **Index-oriented**: Multiple indexes (EAVT, AEVT, AVET, VAET) enable efficient query patterns
4. **Schema-aware**: Attributes have strongly-typed schemas that constrain values
5. **ACID compliance**: PostgreSQL's SERIALIZABLE isolation level ensures transactional consistency

## Core Tables

### `partitions`

Manages entity ID allocation across different namespaces.

```sql
CREATE TABLE mentat.partitions (
    name TEXT PRIMARY KEY,
    start_entid BIGINT NOT NULL,
    end_entid BIGINT NOT NULL,
    next_entid BIGINT NOT NULL,
    allow_excision BOOLEAN NOT NULL DEFAULT FALSE
);
```

Default partitions:
- `db.part/db` (0-9999): Built-in schema attributes
- `db.part/user` (10000-999999999): User entities
- `db.part/tx` (1000000000-1999999999): Transaction entities

### `schema`

Defines attribute schemas with their types and constraints.

```sql
CREATE TABLE mentat.schema (
    entid BIGINT PRIMARY KEY,
    ident TEXT UNIQUE NOT NULL,
    value_type mentat.value_type NOT NULL,
    cardinality mentat.cardinality_type NOT NULL DEFAULT 'one',
    unique_constraint mentat.unique_type,
    indexed BOOLEAN NOT NULL DEFAULT FALSE,
    fulltext BOOLEAN NOT NULL DEFAULT FALSE,
    component BOOLEAN NOT NULL DEFAULT FALSE,
    no_history BOOLEAN NOT NULL DEFAULT FALSE
);
```

Attributes define:
- **value_type**: What type of value can be stored (ref, string, long, etc.)
- **cardinality**: Single-valued (one) or multi-valued (many)
- **unique_constraint**: Optional uniqueness (value or identity)
- **indexed**: Whether AVET index applies
- **fulltext**: Whether fulltext search is enabled
- **component**: Whether this is a component relationship
- **no_history**: Whether to skip history tracking

### `transactions`

Transaction metadata table.

```sql
CREATE TABLE mentat.transactions (
    tx_id BIGINT PRIMARY KEY,
    instant TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

Each transaction is an entity with its own ID and timestamp.

### `datoms`

The core fact table - stores all assertions and retractions.

```sql
CREATE TABLE mentat.datoms (
    e BIGINT NOT NULL,              -- Entity ID
    a BIGINT NOT NULL,              -- Attribute ID (entid)
    v BYTEA NOT NULL,               -- Value (encoded as bytes)
    tx BIGINT NOT NULL,             -- Transaction ID
    added BOOLEAN NOT NULL,         -- TRUE=assertion, FALSE=retraction
    value_type_tag SMALLINT NOT NULL -- Type tag for decoding
);
```

Structure: `[entity, attribute, value, transaction, added]`

### `fulltext`

Fulltext search support table.

```sql
CREATE TABLE mentat.fulltext (
    rowid BIGSERIAL PRIMARY KEY,
    text_value TEXT NOT NULL,
    search_vector TSVECTOR
);
```

For attributes marked `:db/fulltext true`, the datom's value field contains the rowid from this table.

### `idents`

Cached keyword-to-entid mappings for fast resolution.

```sql
CREATE TABLE mentat.idents (
    ident TEXT PRIMARY KEY,
    entid BIGINT NOT NULL UNIQUE
);
```

### `transaction_attrs`

Links transactions to their metadata attributes.

```sql
CREATE TABLE mentat.transaction_attrs (
    tx_id BIGINT NOT NULL,
    attr_entid BIGINT NOT NULL,
    value BYTEA NOT NULL,
    value_type_tag SMALLINT NOT NULL,
    PRIMARY KEY (tx_id, attr_entid)
);
```

## Index Design

### EAVT Index

```sql
CREATE INDEX idx_datoms_eavt ON mentat.datoms
    USING BTREE (e, a, value_type_tag, v, tx)
    WHERE added = TRUE;
```

Optimized for: "What are all the facts about entity E?"

### AEVT Index

```sql
CREATE INDEX idx_datoms_aevt ON mentat.datoms
    USING BTREE (a, e, value_type_tag, v, tx)
    WHERE added = TRUE;
```

Optimized for: "What are all entities with attribute A?"

### AVET Index

```sql
CREATE INDEX idx_datoms_avet ON mentat.datoms
    USING BTREE (a, value_type_tag, v, e, tx)
    WHERE added = TRUE;
```

Optimized for: "What entities have attribute A with value V?"

Used for:
- Unique constraint enforcement
- Lookup-refs `[:person/email "alice@example.com"]`
- Value-based queries

### VAET Index

```sql
CREATE INDEX idx_datoms_vaet ON mentat.datoms
    USING BTREE (v, a, e, tx)
    WHERE added = TRUE AND value_type_tag = 0;
```

Optimized for: "What entities reference entity E?" (reverse lookup)

Partial index: only for ref types (value_type_tag = 0).

### Fulltext Index

```sql
CREATE INDEX idx_fulltext_search ON mentat.fulltext
    USING GIN (search_vector);
```

GIN index on tsvector for fast fulltext queries using PostgreSQL's FTS.

## TypedValue Mapping

Mentat's `TypedValue` enum maps to PostgreSQL types as follows:

| Mentat TypedValue | PostgreSQL Type | value_type_tag | Notes |
|-------------------|-----------------|----------------|-------|
| `Ref(i64)` | BIGINT | 0 | Entity reference |
| `Boolean(bool)` | BOOLEAN | 1 | True/false |
| `Long(i64)` | BIGINT | 4 | 64-bit integer |
| `Double(f64)` | DOUBLE PRECISION | 3 | 64-bit float |
| `Instant(DateTime)` | TIMESTAMPTZ | 5 | Timestamp with timezone |
| `String(String)` | TEXT | 10 | UTF-8 text |
| `Keyword(Keyword)` | EdnValue | 13 | Custom EDN type |
| `Uuid(Uuid)` | UUID | 11 | 128-bit UUID |
| `Bytes(Vec<u8>)` | BYTEA | 12 | Binary data |

All values are encoded to BYTEA for storage in the `v` column, with `value_type_tag` indicating how to decode them.

### Encoding Rules

- **Ref, Long**: Stored as 8-byte big-endian BIGINT
- **Boolean**: 1 byte (0x00 or 0x01)
- **Double**: 8-byte IEEE 754 double
- **Instant**: 8-byte Unix timestamp (microseconds since epoch)
- **String**: UTF-8 encoded bytes (or rowid for fulltext)
- **Keyword**: EDN-encoded using custom type
- **Uuid**: 16-byte binary representation
- **Bytes**: Raw binary data

## Transaction Semantics

### ACID Guarantees

- **Atomicity**: All datoms in a transaction succeed or fail together
- **Consistency**: Schema constraints are enforced via triggers
- **Isolation**: SERIALIZABLE isolation level prevents anomalies
- **Durability**: PostgreSQL's WAL ensures durability

### Transaction Lifecycle

1. Allocate transaction ID from `db.part/tx` partition
2. Create transaction record in `transactions` table
3. Insert datoms with the transaction ID
4. Commit atomically

### Recommended Settings

```sql
SET default_transaction_isolation = 'serializable';
SET synchronous_commit = on;
SET wal_level = replica;
```

## Time-Travel Queries

### Current State

Query with `added = TRUE` to see current facts:

```sql
SELECT e, a, v
FROM mentat.datoms
WHERE e = :entity_id
  AND added = TRUE;
```

### Historical State (as-of)

Query up to a specific transaction:

```sql
SELECT DISTINCT ON (e, a) e, a, v
FROM mentat.datoms
WHERE e = :entity_id
  AND tx <= :as_of_tx
ORDER BY e, a, tx DESC;
```

### History (all time)

Query all assertions and retractions:

```sql
SELECT e, a, v, tx, added
FROM mentat.datoms
WHERE e = :entity_id
ORDER BY tx;
```

## Fulltext Search

Attributes marked `:db/fulltext true` store text in the `fulltext` table:

1. Insert text into `fulltext` table
2. Get rowid
3. Store rowid as the datom value (encoded as BIGINT)

Searching:

```sql
SELECT d.e, f.text_value, ts_rank(f.search_vector, query) as rank
FROM mentat.datoms d
JOIN mentat.fulltext f ON decode(d.v, 'escape')::bigint = f.rowid
CROSS JOIN websearch_to_tsquery('english', 'search terms') query
WHERE d.a = :fulltext_attr
  AND d.added = TRUE
  AND f.search_vector @@ query
ORDER BY rank DESC;
```

## Schema Evolution

### Adding New Attributes

1. Allocate entid from `db.part/db`
2. Insert into `schema` table
3. Insert into `idents` cache
4. Datoms using the new attribute are immediately valid

### Modifying Attributes

Limited modifications are allowed:
- Change cardinality (requires data migration)
- Add/remove indexes
- Add/remove fulltext

Prohibited:
- Change value_type (type safety)
- Change unique constraint (data consistency)

### Schema Validation

The `validate_datom_value_type()` trigger enforces type safety:
- Checks that value_type_tag matches schema
- Prevents inserting wrong types
- Raises exception on mismatch

## Performance Considerations

### Partitioning Strategy

For very large datasets, consider table partitioning:

```sql
-- Partition by transaction ID ranges
CREATE TABLE mentat.datoms_p1 PARTITION OF mentat.datoms
    FOR VALUES FROM (0) TO (1000000000);

CREATE TABLE mentat.datoms_p2 PARTITION OF mentat.datoms
    FOR VALUES FROM (1000000000) TO (2000000000);
```

### Index Maintenance

- EAVT, AEVT, AVET are always used - keep them
- VAET is partial (ref types only) - minimal overhead
- Consider FILLFACTOR=90 for datoms table to reduce page splits

### Query Optimization

- Use prepared statements with parameterized queries
- Leverage index-only scans where possible
- For large scans, consider BRIN indexes on tx column
- Use PostgreSQL's query planner with proper statistics

### Vacuum and Maintenance

```sql
-- Regular vacuuming
VACUUM ANALYZE mentat.datoms;

-- Autovacuum tuning
ALTER TABLE mentat.datoms SET (
    autovacuum_vacuum_scale_factor = 0.01,
    autovacuum_analyze_scale_factor = 0.005
);
```

## Compatibility

- **PostgreSQL Version**: 14+
- **Extensions Required**: None (uses built-in FTS)
- **Isolation Level**: SERIALIZABLE recommended
- **Character Encoding**: UTF-8

## Future Enhancements

1. **Excision Support**: Implement physical deletion for retracted facts
2. **Compression**: TOAST compression for large values
3. **Replication**: Logical replication for multi-node setups
4. **Analytics**: Materialized views for common query patterns
5. **Time-series Optimization**: BRIN indexes for temporal queries
