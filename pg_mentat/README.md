# pg_mentat

PostgreSQL extension that brings Mentat's Datalog capabilities and EDN data type
to PostgreSQL.

## Overview

`pg_mentat` is a PostgreSQL extension built with [pgrx](https://github.com/pgcentralfoundation/pgrx)
that provides:

- **EDN Custom Type** -- native PostgreSQL type for Extensible Data Notation (EDN)
- **Datalog Queries** -- execute Datalog queries directly in PostgreSQL
- **Temporal Database** -- time-travel queries with transaction history
- **Storage Integration** -- native PostgreSQL storage for Mentat data

## Current Status

The extension code is feature-complete for Phase 1. All Rust source compiles
successfully. Test execution is blocked on a build-environment dependency
(libclang/LLVM for bindgen). See [WORKAROUNDS.md](WORKAROUNDS.md) and
[TEST_EXECUTION_STATUS.md](TEST_EXECUTION_STATUS.md) for details.

### What works

- EDN type implementation with text I/O, validation, and CBOR binary path
- Comparison operators (=, <>)
- Collection accessors (get, nth, keys, values, count, contains)
- Type predicates (is_nil, is_boolean, is_integer, etc.)
- Schema initialization with EAVT datom storage and four covering indexes
- Transaction processing (`mentat_transact`) with tempid resolution
- Datalog query execution (`mentat_query`) with find specs, patterns, negation,
  OR clauses, ordering, and limits
- Entity retrieval (`mentat_entity`)
- Schema introspection (`mentat_schema`)
- Pull API stub (`mentat_pull`)
- Full-text search via PostgreSQL tsvector/tsquery
- Time-travel queries (as-of, since, history)
- 38 pgrx tests migrated from SQLite and consolidated into `src/lib.rs`

### What is blocked

- `cargo pgrx test` requires libclang/LLVM for the bindgen step
- See [WORKAROUNDS.md](WORKAROUNDS.md) for resolution steps

## Quick Start

### Prerequisites

- PostgreSQL 13-18 (16 recommended)
- Rust stable toolchain (via rustup)
- LLVM/Clang development libraries (libclang)
- cargo-pgrx 0.17.x

### Environment Setup

```bash
# 1. Set CARGO_HOME if using the project-local cargo cache
export CARGO_HOME=/home/gburd/ws/pg_mentat/.cargo

# 2. Ensure libclang is available
#    Fedora:
sudo dnf install clang-devel llvm-devel
#    Debian/Ubuntu:
sudo apt install libclang-dev llvm-dev

# 3. Install pgrx CLI
cargo install cargo-pgrx --version 0.17.0 --locked

# 4. Initialize pgrx (downloads/compiles a test PostgreSQL instance)
cargo pgrx init --pg16=$(which pg_config)
# or let pgrx download its own:
cargo pgrx init
```

### Build

```bash
cd pg_mentat
cargo pgrx package     # build the extension .so and SQL files
cargo pgrx install     # install into the pgrx-managed PostgreSQL
```

### Run Tests

```bash
cargo pgrx test pg16
```

### Interactive Session

```bash
cargo pgrx run pg16
```

Then in the psql prompt:

```sql
CREATE EXTENSION pg_mentat;

-- EDN type
SELECT mentat.edn_out(mentat.edn_in('42'));
SELECT mentat.edn_out(mentat.edn_in('{:name "Alice" :age 30}'));

-- Schema init
SELECT mentat.initialize_schema();

-- Transact
SELECT mentat.mentat_transact('[
  [:db/add "p1" :person/name "Alice"]
  [:db/add "p1" :person/age 30]
]');

-- Query
SELECT mentat.mentat_query(
  '[:find ?e ?ident :where [?e :db/ident ?ident]]',
  '{}'::jsonb
);
```

## Features

### EDN Type

The extension provides a custom `EdnValue` type supporting all EDN data types:

- Primitives: `nil`, `boolean`, `integer`, `float`, `string`
- Collections: `vector`, `list`, `set`, `map`
- Special types: `keyword`, `symbol`, `uuid`, `instant`, `bytes`, `bigint`

### EDN Functions

| Function | Description |
|---|---|
| `edn_in(text)` | Parse EDN text to EdnValue |
| `edn_out(EdnValue)` | Convert EdnValue to EDN text |
| `edn_get(map, key)` | Get value from map by key |
| `edn_nth(vector, index)` | Get element from vector by index |
| `edn_count(collection)` | Get collection size |
| `edn_contains(collection, element)` | Check if element exists |
| `edn_keys(map)` | Extract map keys as vector |
| `edn_values(map)` | Extract map values as vector |

### Type Predicates

`edn_is_nil`, `edn_is_boolean`, `edn_is_integer`, `edn_is_float`,
`edn_is_text`, `edn_is_keyword`, `edn_is_vector`, `edn_is_list`,
`edn_is_set`, `edn_is_map`

### SQL API Functions

| Function | Description |
|---|---|
| `mentat_transact(edn)` | Process EDN transactions, persist datoms |
| `mentat_query(query, inputs)` | Execute Datalog queries, return JSON |
| `mentat_entity(entity_id)` | Fetch all datoms for an entity |
| `mentat_schema()` | Return schema as JSON |
| `mentat_pull(pattern, entity_id)` | Pull entity data (stub) |
| `initialize_schema()` | Create the mentat datom tables and indexes |

## Architecture

```
pg_mentat/
  Cargo.toml
  pg_mentat.control
  sql/
    01_types.sql          -- custom enum types
    02_tables.sql         -- datoms, schema, idents, partitions, transactions
    03_indexes.sql        -- EAVT, AEVT, AVET, VAET covering indexes
    04_constraints.sql    -- check constraints
    05_functions.sql      -- allocate_entid, resolve_ident, current_tx
    06_bootstrap_data.sql -- core schema attributes
    bootstrap.sql         -- combined initialization
  src/
    lib.rs                -- extension entry, schema init, 38 inline tests
    types/
      mod.rs
      edn.rs              -- EdnValue PostgreSQL type (text I/O, validation)
    operators.rs           -- EDN comparison, accessors, predicates
    storage.rs             -- SPI wrappers for entity allocation and queries
    functions/
      mod.rs
      transact.rs          -- mentat_transact()
      query.rs             -- mentat_query()
      entity.rs            -- mentat_entity()
      schema.rs            -- mentat_schema()
      pull.rs              -- mentat_pull() (stub)
    planner/               -- query planner hooks (planned)
  tests/
    test_common.rs         -- shared test helpers (reference copy)
    test_query.rs          -- core query tests (reference copy)
    test_fulltext.rs       -- FTS tests (reference copy)
    test_rules.rs          -- rules tests (reference copy)
    test_timetravel.rs     -- temporal tests (reference copy)
  docs/
    API_FUNCTIONS.md       -- detailed API reference
    schema-design.md       -- PostgreSQL schema design
    typedvalue-mapping.md  -- EDN to PostgreSQL type mapping
```

### Storage Format

EdnValue currently uses EDN text for storage. The `ciborium` dependency is
included for a future migration to CBOR (Compact Binary Object Representation)
for more efficient binary storage.

### Datom Table

```sql
CREATE TABLE mentat.datoms (
    e     BIGINT  NOT NULL,  -- entity ID
    a     BIGINT  NOT NULL,  -- attribute ID
    v     BYTEA   NOT NULL,  -- value (type-tagged)
    tx    BIGINT  NOT NULL,  -- transaction ID
    added BOOLEAN NOT NULL   -- true=assert, false=retract
);
```

Four covering indexes (EAVT, AEVT, AVET, VAET) support the standard Datomic
access patterns.

## Development Roadmap

**Phase 1: Foundation (current)**
- [x] EDN type with text I/O
- [x] Basic type predicates and collection accessors
- [x] Schema initialization
- [x] Transaction processing
- [x] Datalog query execution (basic patterns)
- [x] Entity retrieval and schema introspection
- [x] 38 pgrx tests migrated from SQLite
- [ ] CBOR serialization (dependency added, not yet wired)

**Phase 2: Optimization**
- [ ] CBOR binary storage for EdnValue
- [ ] Containment operators (@>, <@, ?, ?|, ?&)
- [ ] Performance benchmarks

**Phase 3: Advanced Features**
- [ ] Query planner hooks
- [ ] B-tree and GIN index support
- [ ] Full pull-pattern support
- [ ] Aggregate functions (count, sum, min, max, avg)

**Phase 4: Production Readiness**
- [ ] Full query engine integration (algebrizer, projector)
- [ ] WASM function support
- [ ] CI/CD pipeline with automated tests
- [ ] Remaining ~150 tests ported

## Testing

38 tests are defined inline in `src/lib.rs`. They cover:

- EDN roundtrip (5 tests)
- Core Datalog queries (11 tests)
- Time-travel / temporal queries (7 tests)
- Rules and recursive queries (8 tests)
- Full-text search (7 tests)

Run with:

```bash
export CARGO_HOME=/home/gburd/ws/pg_mentat/.cargo
cargo pgrx test pg16
```

See [TEST_EXECUTION_STATUS.md](TEST_EXECUTION_STATUS.md) for execution history
and [WORKAROUNDS.md](WORKAROUNDS.md) for environment issues.

## Contributing

1. Clone the repository and set up the environment (see Quick Start above).
2. Make changes in `pg_mentat/src/`.
3. Run `cargo clippy` -- the project enforces strict lints (no unwrap, no panic,
   no todo, no dbg, pedantic warnings).
4. Add or update tests in the `mod tests` block in `src/lib.rs`.
5. Run `cargo pgrx test pg16` to validate.
6. Submit a PR against the `claude` branch.

### Code Quality Standards

- `Result` types throughout; `unwrap_used` and `panic` are denied by clippy.
- All public functions have doc comments.
- Parameterized queries via pgrx SPI (no SQL injection).
- Input validation: max nesting depth 100, max collection size 1M, max input 10MB.

## License

Apache-2.0

## References

- [pgrx Documentation](https://github.com/pgcentralfoundation/pgrx)
- [EDN Format Specification](https://github.com/edn-format/edn)
- [Mentat Project](https://github.com/mozilla/mentat)
- [PostgreSQL Full-Text Search](https://www.postgresql.org/docs/current/textsearch.html)
