# PostgreSQL Storage Backend - Phase 1 Implementation

## Overview

This document describes the Phase 1 implementation of the PostgreSQL storage backend for Mentat, located in `/pg_mentat/src/storage.rs`.

## Architecture Decision

**Location**: Implemented in the `/pg_mentat` extension (not `/db` crate)

**Rationale**:
- pgrx requires PostgreSQL installed at build time ($PGRX_HOME)
- `/pg_mentat` extension already has pgrx configured
- Avoids circular dependencies between crates
- Clean separation: SQLite in `/db`, PostgreSQL in `/pg_mentat`
- Storage operations exposed as SQL functions callable from extension

## Phase 1 Scope

Phase 1 provides core storage operations:

### Entity ID Allocation
- `mentat.alloc_entid(partition_name TEXT) → BIGINT`
- Wraps schema helper `mentat.allocate_entid()`
- Uses PostgreSQL sequences for ID generation

### Entity Resolution
- `mentat.resolve_ident_to_entid(ident TEXT) → BIGINT`
- Resolves keyword idents to entity IDs
- Wraps schema helper `mentat.resolve_ident()`

### Entity Lookup
- `mentat.lookup_entity_by_attr(attr_ident TEXT, value_str TEXT) → BIGINT`
- Looks up entity by unique attribute value
- Phase 1: String values only
- Phase 2: Full TypedValue support

### Entity Retrieval
- `mentat.get_entity_datoms(entity_id BIGINT) → TABLE`
- Returns all current datoms for an entity
- Columns: attribute, value, value_type, transaction

### Transaction Lifecycle
- `mentat.begin_transaction() → VOID`
  - Creates temporary staging tables
  - Tables: temp_exact_searches, temp_inexact_searches, temp_search_results

- `mentat.commit_transaction(tx_id BIGINT) → VOID`
  - Materializes staged datoms to datoms table
  - Records transaction in transactions table
  - Phase 1: Simplified without full conflict resolution

## Implementation Status

### ✅ Completed
- Core function implementations
- SQL function interfaces
- Schema helper integration
- Transaction staging table creation
- Basic datom insertion
- Transaction recording

### ⏳ Deferred to Later Phases
- **Phase 2**: Full transaction processing
  - Complete conflict detection
  - Cardinality enforcement
  - Schema validation
  - Full TypedValue encoding

- **Phase 3**: Complex queries
  - Join operations
  - Query optimization
  - Index usage

- **Phase 4**: Fulltext search
  - to_tsvector/to_tsquery integration
  - FTS index management

- **Phase 5**: Testing & optimization
  - Comprehensive test suite
  - Performance benchmarking
  - Concurrent transaction handling

## SQL API Usage

```sql
-- Allocate entity IDs
SELECT mentat.alloc_entid('db.part/user');
-- Returns: 65536 (example)

-- Resolve idents
SELECT mentat.resolve_ident_to_entid(':db/ident');
-- Returns: 10 (example)

-- Lookup by attribute
SELECT mentat.lookup_entity_by_attr(':user/email', 'alice@example.com');
-- Returns: 100001 (example entity ID)

-- Begin transaction
SELECT mentat.begin_transaction();

-- Stage datoms (Phase 2 will add helpers for this)
INSERT INTO temp_exact_searches (e0, a0, v0, value_type_tag0, added0, flags0)
VALUES (100001, 10, decode('616c696365', 'hex'), 5, true, 0);

-- Commit transaction
SELECT mentat.commit_transaction(268435456);

-- Retrieve entity
SELECT * FROM mentat.get_entity_datoms(100001);
```

## Dependencies

### Mentat Crates
- `core_traits` - Core type definitions
- `mentat_core` - Schema and core types
- `db_traits` - Error types
- `mentat_db` - Database traits (future integration)
- `edn` - EDN value handling

### External
- `pgrx` 0.17.0 - PostgreSQL extension framework
- `serde` - Serialization (for EDN types)
- `ciborium` - CBOR encoding (for value storage)

## Build Requirements

### Prerequisites
1. PostgreSQL 16 installed
2. pgrx CLI tool: `cargo install --locked cargo-pgrx`
3. Initialize pgrx: `cargo pgrx init`
4. Set environment: `export PGRX_HOME=~/.pgrx`

### Build Commands
```bash
cd pg_mentat

# Check compilation
cargo pgrx check

# Run tests
cargo pgrx test pg16

# Install extension
cargo pgrx install --release

# Package extension
cargo pgrx package
```

### Install Schema
Before using storage functions, install the schema from Task #14:
```bash
psql -d your_database -f sql/01_types.sql
psql -d your_database -f sql/02_tables.sql
psql -d your_database -f sql/03_indexes.sql
psql -d your_database -f sql/04_constraints.sql
psql -d your_database -f sql/05_functions.sql
psql -d your_database -f sql/06_bootstrap_data.sql
```

## Testing

### Unit Tests
Located in `src/storage.rs`:
```rust
#[pg_test]
fn test_alloc_entid_basic() {
    let result = alloc_entid("db.part/user");
    assert!(result.is_ok());
}
```

Run with: `cargo pgrx test pg16`

### Integration Tests
Phase 2 will add comprehensive integration tests covering:
- Transaction ACID properties
- Concurrent transactions
- Schema enforcement
- Index usage
- Performance benchmarks

## Future Enhancements

### Phase 2 (Next)
- Complete TypedValue encoding/decoding
- Full transaction conflict resolution
- Schema validation during transactions
- Cardinality enforcement
- Retraction handling
- Metadata extraction

### Phase 3
- Complex query support
- Join optimization
- Query planner integration
- Index strategy selection

### Phase 4
- Fulltext search via to_tsvector
- FTS index management
- Search ranking
- Multi-language support

### Phase 5
- Comprehensive test suite
- Performance optimization
- Concurrent transaction testing
- Load testing
- Documentation completion

## Known Limitations

### Phase 1 Limitations
1. **String values only** in lookups - Full TypedValue support in Phase 2
2. **No conflict detection** - Added in Phase 2
3. **No cardinality enforcement** - Added in Phase 2
4. **No fulltext search** - Added in Phase 4
5. **Simplified commit** - No retraction handling yet

### Build Environment
- Requires PostgreSQL development environment
- Cannot build/test without $PGRX_HOME configured
- Workspace profile warnings (non-critical)

## Contributing

When extending this implementation:
1. Follow incremental phases
2. Add SQL function documentation
3. Include pg_test unit tests
4. Update this document
5. Test against schema from Task #14

## References

- Task #14: PostgreSQL Schema Design
- Task #15: Storage Backend Implementation (this task)
- Task #16: SQL Function API (uses this implementation)
- pgrx documentation: https://github.com/pgcentralfoundation/pgrx
- PostgreSQL SPI: https://www.postgresql.org/docs/current/spi.html
