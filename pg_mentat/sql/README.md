# PostgreSQL Schema Files for pg_mentat

## Overview

This directory contains the complete SQL schema definition for the pg_mentat PostgreSQL extension. The schema implements Datomic-style immutable datom storage with full ACID compliance and time-travel query support.

## File Structure

The schema is organized into multiple SQL files loaded in sequence:

1. **01_types.sql** - Type definitions (enums for value_type, cardinality, uniqueness)
2. **02_tables.sql** - Core tables (datoms, schema, partitions, transactions, fulltext)
3. **03_indexes.sql** - EAVT, AEVT, AVET, VAET indexes
4. **04_constraints.sql** - Constraints, triggers, and validation functions
5. **05_functions.sql** - Helper functions (entity allocation, lookups, FTS)
6. **06_bootstrap_data.sql** - Bootstrap data (default partitions, core attributes)
7. **bootstrap.sql** - Main entry point that loads all other files

## Loading the Schema

### Via Extension Creation

When installing the pg_mentat extension:

```sql
CREATE EXTENSION pg_mentat CASCADE;
```

The bootstrap.sql file is automatically executed.

### Manual Installation

To manually install the schema:

```bash
cd pg_mentat/sql
psql -d mydb -f bootstrap.sql
```

Or load individual files:

```bash
psql -d mydb -f 01_types.sql
psql -d mydb -f 02_tables.sql
# ... etc
```

## Schema Components

### Types (01_types.sql)

Defines PostgreSQL enums:

- `mentat.value_type` - Value types (ref, boolean, long, string, etc.)
- `mentat.cardinality_type` - Cardinality (one, many)
- `mentat.unique_type` - Uniqueness constraints (value, identity)

### Tables (02_tables.sql)

Core tables:

- **`partitions`** - Entity ID allocation namespaces
- **`schema`** - Attribute definitions
- **`transactions`** - Transaction metadata
- **`datoms`** - The core fact table [e a v tx added]
- **`fulltext`** - Fulltext search support
- **`idents`** - Keyword-to-entid cache
- **`transaction_attrs`** - Transaction metadata linkage

### Indexes (03_indexes.sql)

Four primary indexes enable different query patterns:

- **EAVT** - Entity lookups
- **AEVT** - Attribute scans
- **AVET** - Value lookups and unique constraints
- **VAET** - Reverse reference lookups

Plus specialized indexes for fulltext search and temporal queries.

### Constraints (04_constraints.sql)

- Type validation trigger (`validate_datom_value_type`)
- Partition boundary validation
- Unique constraint enforcement
- Fulltext vector auto-update

### Functions (05_functions.sql)

Helper functions:

- `allocate_entid(partition_name)` - Allocate single entity ID
- `allocate_entids(partition_name, count)` - Allocate multiple IDs
- `current_tx()` - Get/create current transaction ID
- `resolve_ident(keyword)` - Keyword to entid resolution
- `lookup_ref(attr, value, type)` - Lookup entity by unique attribute
- `entity_datoms(entity_id)` - Get all facts about an entity
- `fulltext_search(query)` - Fulltext search
- `is_indexed(attr)` / `is_unique(attr)` - Schema introspection
- `attribute_value_type(attr)` - Get attribute's value type

### Bootstrap Data (06_bootstrap_data.sql)

Initializes:

- Default partitions (`db.part/db`, `db.part/user`, `db.part/tx`)
- Core schema attributes (`:db/ident`, `:db/valueType`, etc.)
- Value type references (`:db.type/ref`, `:db.type/string`, etc.)
- Cardinality references (`:db.cardinality/one`, `:db.cardinality/many`)
- Uniqueness references (`:db.unique/value`, `:db.unique/identity`)
- Idents cache for bootstrap attributes

## Usage Examples

### Define a New Attribute

```sql
-- Allocate an entid
SELECT mentat.allocate_entid('db.part/db'); -- Returns 100

-- Define the attribute
INSERT INTO mentat.schema (entid, ident, value_type, cardinality, unique_constraint, indexed)
VALUES (100, ':person/email', 'string', 'one', 'identity', TRUE);

-- Add to idents cache
INSERT INTO mentat.idents (ident, entid)
VALUES (':person/email', 100);
```

### Insert a Datom

```sql
-- Get current transaction
SELECT mentat.current_tx(); -- Returns tx_id, e.g., 1000000000

-- Insert a fact
INSERT INTO mentat.datoms (e, a, v, tx, added, value_type_tag)
VALUES (
    10000,                              -- entity
    100,                                -- attribute (:person/email)
    'alice@example.com'::bytea,         -- value
    1000000000,                         -- transaction
    TRUE,                               -- assertion
    10                                  -- value_type_tag for string
);
```

### Query Current State

```sql
-- Get all facts about entity 10000
SELECT a, v, value_type_tag
FROM mentat.datoms
WHERE e = 10000
  AND added = TRUE;
```

### Lookup by Unique Attribute

```sql
-- Find entity with email alice@example.com
SELECT mentat.lookup_ref(':person/email', 'alice@example.com'::bytea, 10);
```

### Fulltext Search

```sql
-- Search for text
SELECT * FROM mentat.fulltext_search('postgresql database');
```

### Time-Travel Query (as-of)

```sql
-- State of entity 10000 at transaction 1000000050
SELECT DISTINCT ON (e, a) e, a, v
FROM mentat.datoms
WHERE e = 10000
  AND tx <= 1000000050
ORDER BY e, a, tx DESC;
```

## Configuration

### Recommended PostgreSQL Settings

```sql
-- Transaction isolation for consistency
SET default_transaction_isolation = 'serializable';

-- Durability
SET synchronous_commit = on;
SET wal_level = replica;

-- Performance
SET shared_buffers = '256MB';
SET effective_cache_size = '1GB';
SET random_page_cost = 1.1;

-- Autovacuum tuning for high-write workloads
ALTER TABLE mentat.datoms SET (
    autovacuum_vacuum_scale_factor = 0.01,
    autovacuum_analyze_scale_factor = 0.005
);
```

### Fulltext Search Configuration

Default configuration uses English text search:

```sql
-- Change to another language
ALTER TABLE mentat.fulltext
    ALTER COLUMN search_vector TYPE tsvector USING to_tsvector('spanish', text_value);

-- Or use simple (language-agnostic) tokenization
ALTER TABLE mentat.fulltext
    ALTER COLUMN search_vector TYPE tsvector USING to_tsvector('simple', text_value);
```

## Schema Validation

Run basic validation:

```sql
-- Check all tables exist
SELECT tablename FROM pg_tables WHERE schemaname = 'mentat';

-- Check all indexes exist
SELECT indexname FROM pg_indexes WHERE schemaname = 'mentat';

-- Verify bootstrap data loaded
SELECT COUNT(*) FROM mentat.partitions; -- Should be 3
SELECT COUNT(*) FROM mentat.schema WHERE entid < 100; -- Should be ~40

-- Validate partition integrity
SELECT * FROM mentat.partitions WHERE next_entid < start_entid OR next_entid > end_entid;
-- Should return no rows
```

## Migration from SQLite

To migrate from mentat's SQLite backend:

1. Export datoms from SQLite using mentat's API
2. Map SQLite value types to PostgreSQL encoding
3. Insert into PostgreSQL tables maintaining transaction order
4. Rebuild indexes
5. Validate entity counts and sample queries

See `docs/typedvalue-mapping.md` for encoding details.

## Performance Tuning

### Index Usage

Check if queries are using the right indexes:

```sql
EXPLAIN (ANALYZE, BUFFERS)
SELECT * FROM mentat.datoms WHERE e = 10000 AND added = TRUE;
```

Should use `idx_datoms_eavt`.

### Statistics

Update statistics after bulk loads:

```sql
ANALYZE mentat.datoms;
ANALYZE mentat.schema;
ANALYZE mentat.fulltext;
```

### Partitioning

For very large datasets (billions of datoms), consider table partitioning:

```sql
-- Partition by transaction ID ranges
ALTER TABLE mentat.datoms PARTITION BY RANGE (tx);

CREATE TABLE mentat.datoms_p1 PARTITION OF mentat.datoms
    FOR VALUES FROM (0) TO (1000000000);

CREATE TABLE mentat.datoms_p2 PARTITION OF mentat.datoms
    FOR VALUES FROM (1000000000) TO (2000000000);
```

## Troubleshooting

### Type Mismatch Errors

```
ERROR: Value type mismatch for attribute X: expected Y, got tag Z
```

**Cause:** Trying to insert a value with wrong type tag for the attribute.

**Fix:** Ensure `value_type_tag` matches the attribute's declared `value_type` in the schema table.

### Unique Constraint Violations

```
ERROR: duplicate key value violates unique constraint "idx_datoms_unique_value"
```

**Cause:** Trying to insert a duplicate value for a unique attribute.

**Fix:** Check existing datoms or use `lookup_ref` to find existing entity.

### Partition Boundary Errors

```
ERROR: Partition X next_entid (Y) must be between start (A) and end (B)
```

**Cause:** Partition exhausted or invalid allocation.

**Fix:** Check partition bounds and consider expanding or adding new partitions.

## Further Reading

- **[schema-design.md](../docs/schema-design.md)** - Detailed schema design documentation
- **[typedvalue-mapping.md](../docs/typedvalue-mapping.md)** - TypedValue encoding/decoding reference
- **[Datomic Architecture](https://docs.datomic.com/architecture.html)** - Original inspiration
- **[PostgreSQL FTS](https://www.postgresql.org/docs/current/textsearch.html)** - Full-text search documentation

## License

Apache License 2.0 - See LICENSE file in repository root.
