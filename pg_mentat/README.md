# pg_mentat

PostgreSQL extension providing a Datomic-compatible Datalog query engine with a native EDN data type.

## Overview

`pg_mentat` is a PostgreSQL extension built with [pgrx](https://github.com/pgcentralfoundation/pgrx)
that provides:

- **Datalog Query Engine** -- execute Datalog queries with find specs, rules, aggregates, predicates, OR/NOT clauses, and full-text search
- **Transaction Processing** -- assert and retract facts using EDN transaction format with tempid resolution, lookup refs, and upsert semantics
- **Pull API** -- retrieve entity attributes with pattern matching, nested traversal, reverse lookups, limits, defaults, and recursion
- **Temporal Database** -- time-travel queries with as-of, since, and full history modes
- **Native EDN Type** -- first-class PostgreSQL type for Extensible Data Notation with collection access, type predicates, and equality operators
- **SQL Integration** -- all operations exposed as SQL functions, composable with CTEs, window functions, JOINs, and any PostgreSQL feature
- **EDN Helper Functions** -- batch operations, import/export, entity helpers

## Quick Start

### Prerequisites

- PostgreSQL 13-18 (16 recommended)
- Rust stable toolchain (1.88+)
- LLVM/Clang development libraries
- cargo-pgrx 0.17.x

### Build and Install

```bash
cd pg_mentat
cargo pgrx install --release
```

### Interactive Session

```bash
cargo pgrx run pg16
```

```sql
CREATE EXTENSION pg_mentat;

-- Define schema
SELECT mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
  {:db/ident :person/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
]');

-- Transact data
SELECT mentat_transact('[
  {:db/id "alice" :person/name "Alice" :person/email "alice@example.com" :person/age 30}
  {:db/id "bob"   :person/name "Bob"   :person/email "bob@example.com"   :person/age 25}
]');

-- Query
SELECT mentat_query('
  [:find ?name ?email
   :where [?e :person/name ?name] [?e :person/email ?email]]
', '{}');

-- Pull
SELECT mentat_pull('[*]', 10000);

-- Entity
SELECT mentat_entity(10000);
```

## SQL Function Reference

### Core API

| Function | Description |
|----------|-------------|
| `mentat_transact(edn TEXT)` | Process EDN transactions (assert, retract, retractEntity) |
| `mentat_query(query TEXT, inputs JSONB)` | Execute Datalog queries with temporal and pagination options |
| `mentat_pull(pattern TEXT, entity_id BIGINT)` | Pull entity attributes by pattern |
| `mentat_pull_many(pattern TEXT, entity_ids BIGINT[])` | Pull attributes for multiple entities |
| `mentat_entity(entity_id BIGINT)` | Get all attributes of an entity as JSONB |
| `mentat_schema()` | Return current schema as JSONB |
| `mentat_explain(query TEXT, inputs JSONB)` | Show query execution plan and generated SQL |

### EDN Helper Functions (mentat schema)

| Function | Description |
|----------|-------------|
| `mentat.batch(edn TEXT)` | Execute multiple operations in a single batch |
| `mentat.export_edn(entity_ids BIGINT[])` | Export entities to EDN format |
| `mentat.import_edn(edn TEXT)` | Import entities from EDN format |
| `mentat.query_export_edn(query TEXT, inputs JSONB)` | Query and export matching entities |
| `mentat.export_all_edn()` | Export entire database to EDN |

### Entity Helper Functions (mentat schema)

| Function | Description |
|----------|-------------|
| `mentat.lookup_by_ident(attr TEXT, value TEXT)` | Look up entity by attribute value |
| `mentat.entity_attrs(entity_id BIGINT)` | List attribute idents for an entity |
| `mentat.attribute_values(attr TEXT)` | Get all values for an attribute |
| `mentat.retract_entity(entity_id BIGINT)` | Retract all facts about an entity |

### Operational Functions

| Function | Description |
|----------|-------------|
| `mentat_query_stats()` | Query performance and database statistics |
| `mentat_storage_stats()` | Table and index size information |
| `mentat_slow_queries(threshold_ms)` | Find slow functions and heavy transactions |
| `mentat_stmt_cache_stats()` | Prepared statement cache statistics |
| `mentat_stmt_cache_clear()` | Clear prepared statement cache |

### EDN Type Functions

| Function | Description |
|----------|-------------|
| `edn_get(map, key)` | Get value from map by key |
| `edn_nth(vector, index)` | Get element by 0-based index |
| `edn_count(collection)` | Collection size |
| `edn_contains(collection, element)` | Membership test |
| `edn_keys(map)` | Extract map keys |
| `edn_values(map)` | Extract map values |
| `edn_is_nil`, `edn_is_boolean`, `edn_is_integer`, `edn_is_float`, `edn_is_text`, `edn_is_keyword`, `edn_is_vector`, `edn_is_list`, `edn_is_set`, `edn_is_map` | Type predicates |

## SQL Integration

pg_mentat functions return standard PostgreSQL types (JSONB, TEXT, BIGINT), making them composable with all PostgreSQL features:

```sql
-- CTEs with Datalog
WITH engineers AS (
  SELECT elem->>0 AS eid, elem->>1 AS name
  FROM mentat_query('[:find ?e ?name :where
    [?e :person/department ?d] [?d :dept/name "Engineering"]
    [?e :person/name ?name]]', '{}') AS q,
  jsonb_array_elements(q->'results') AS elem
)
SELECT * FROM engineers;

-- Window functions
WITH salaries AS (
  SELECT (elem->>1)::text AS name, (elem->>2)::int AS salary
  FROM mentat_query('[:find ?e ?name ?salary :where
    [?e :person/name ?name] [?e :person/salary ?salary]]', '{}') AS q,
  jsonb_array_elements(q->'results') AS elem
)
SELECT name, salary, RANK() OVER (ORDER BY salary DESC) FROM salaries;

-- Join with relational tables
WITH people AS (
  SELECT elem->>0 AS name, elem->>1 AS email
  FROM mentat_query('[:find ?name ?email :where
    [?e :person/name ?name] [?e :person/email ?email]]', '{}') AS q,
  jsonb_array_elements(q->'results') AS elem
)
SELECT p.name, t.project_name, SUM(t.hours)
FROM people p JOIN time_entries t ON t.person_email = p.email
GROUP BY p.name, t.project_name;
```

See [docs/SQL_INTEGRATION.md](docs/SQL_INTEGRATION.md) for the complete SQL integration guide and [docs/EDN_TYPE.md](docs/EDN_TYPE.md) for the EDN type reference.

## Architecture

```
src/
  lib.rs                -- Extension entry point, Edn type definition, schema bootstrap SQL
  types/
    edn.rs              -- Edn impl, edn_in/edn_out/edn_send/edn_recv
  operators.rs          -- EDN operators and accessor functions
  functions/
    transact.rs         -- mentat_transact() - EDN transaction processing
    query.rs            -- mentat_query(), mentat_explain() - Datalog to SQL compilation
    pull.rs             -- mentat_pull(), mentat_pull_many() - Pull API
    entity.rs           -- mentat_entity() - Entity retrieval
    schema.rs           -- mentat_schema() - Schema introspection
    stats.rs            -- mentat_query_stats(), mentat_storage_stats(), mentat_slow_queries()
    helpers.rs          -- lookup_by_ident, entity_attrs, attribute_values, retract_entity
    edn_helpers.rs      -- batch, export_edn, import_edn, query_export_edn, export_all_edn
    bootstrap.rs        -- bootstrap_schema() - Core schema initialization
  storage.rs            -- SPI wrappers for entity allocation and lookups
  cache.rs              -- Schema cache (attribute ident resolution)
  error.rs              -- Error types with contextual messages
  planner/
    hooks.rs            -- GUC parameters, optimizer hints, index suggestions
    mod.rs              -- Planner module
```

### Storage Model

All data is stored in the `mentat` schema as EAVT datoms in a partitioned table:

- **mentat.datoms** -- partitioned by `value_type_tag` (LIST partitioning) with type-specific value columns (`v_ref`, `v_long`, `v_text`, `v_bool`, `v_double`, `v_instant`, `v_keyword`, `v_uuid`, `v_bytes`)
- **mentat.schema** -- attribute definitions
- **mentat.idents** -- keyword to entity ID mappings
- **mentat.transactions** -- transaction metadata
- **mentat.fulltext** -- full-text search with tsvector/GIN

Eight indexes cover the four Datomic access patterns (EAVT, AEVT, AVET, VAET) with type-specific partial indexes for correct native comparisons.

## Testing

```bash
cargo pgrx test pg16
```

The test suite includes 1,900+ tests covering transactions, queries, pulls, temporal operations, rules, aggregates, predicates, upserts, retractions, concurrency, and edge cases.

## Configuration

GUC parameters for tuning:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `mentat.enable_optimizer_hints` | `true` | Enable SET LOCAL optimizer hints |
| `mentat.default_work_mem` | `64MB` | work_mem for complex queries |
| `mentat.max_result_rows` | `0` | Maximum result rows (0 = unlimited) |

## Contributing

1. Clone the repository and set up the environment (see Quick Start above).
2. Make changes in `pg_mentat/src/`.
3. Run `cargo clippy` -- the project enforces strict lints (no unwrap, no panic,
   no todo, no dbg, pedantic warnings).
4. Add or update tests.
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
- [Datomic Documentation](https://docs.datomic.com/)
