# pg_mentat

A Datomic-compatible Datalog query engine as a PostgreSQL extension.

pg_mentat brings the power of [Datomic](https://docs.datomic.com/)'s immutable, time-aware, Datalog-based data model to PostgreSQL. Store data as Entity-Attribute-Value-Transaction (EAVT) tuples and query it with Datalog -- all through standard SQL function calls.

Based on Mozilla's [Mentat](https://github.com/mozilla/mentat) project, pg_mentat reimplements the core as a [pgrx](https://github.com/pgcentralfoundation/pgrx) PostgreSQL extension with a companion HTTP daemon (`mentatd`) for remote access.

## Features

| Feature | Status | Notes |
|---------|--------|-------|
| Schema definition (value types, cardinality, uniqueness) | Complete | All Datomic schema attributes supported |
| Transactions (assert, retract, retractEntity) | Complete | EDN transaction format |
| Lookup refs in transactions and queries | Complete | `[:person/email "alice@example.com"]` |
| Datalog queries with `:find`, `:where`, `:in` | Complete | Relations, tuples, collections, scalars |
| Rules engine (recursive) | Complete | Transitive closure, graph traversal |
| Aggregates | Complete | `count`, `sum`, `avg`, `min`, `max` |
| Predicates and functions | Complete | `>`, `<`, `>=`, `<=`, `=`, `!=`, `ground`, `get-else` |
| OR / NOT clauses | Complete | Composable query constraints |
| Full-text search with scoring | Complete | PostgreSQL `tsvector` + BM25 ranking |
| Pull API | Complete | Recursive pulls, reverse lookups, limits, defaults, wildcards |
| Time travel (as-of, since, history) | Complete | Immutable audit trail |
| Cardinality many | Complete | Multi-valued attributes with set semantics |
| Entity and schema introspection | Complete | `mentat_entity()`, `mentat_schema()` |
| mentatd HTTP daemon | Complete | EDN + Transit wire formats, connection pooling, caching |
| Value types | Complete | string, long, double, boolean, instant, keyword, ref, uuid, bytes |

## Quick Start

### Docker (fastest)

```bash
docker build -t pg_mentat .
docker run -d --name pg_mentat -p 5432:5432 pg_mentat
psql -h localhost -U postgres
```

### With Nix

```bash
nix develop
cd pg_mentat
cargo pgrx run pg16
```

### From source

Requires Rust 1.88+, PostgreSQL 13-18, and [cargo-pgrx](https://github.com/pgcentralfoundation/pgrx):

```bash
cargo install --locked cargo-pgrx --version '~0.17'
cargo pgrx init --pg16=$(which pg_config)
cd pg_mentat
cargo pgrx install --release
```

Then in PostgreSQL:

```sql
CREATE EXTENSION pg_mentat;
```

## Usage

### Define a schema

```sql
SELECT mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
  {:db/ident :person/friends
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many}
]');
```

### Transact data

```sql
SELECT mentat_transact('[
  {:db/id "alice"
   :person/name "Alice"
   :person/email "alice@example.com"}
  {:db/id "bob"
   :person/name "Bob"
   :person/email "bob@example.com"
   :person/friends "alice"}
]');
```

### Query with Datalog

```sql
SELECT mentat_query('
  [:find ?name ?email
   :where
   [?e :person/name ?name]
   [?e :person/email ?email]]
', '{}');
```

### Pull entities

```sql
-- Pull specific attributes
SELECT mentat_pull('[:person/name :person/email]', 42);

-- Pull with nested refs
SELECT mentat_pull('[* {:person/friends [:person/name]}]', 42);

-- Reverse lookups: who lists this entity as a friend?
SELECT mentat_pull('[:person/name :person/_friends]', 42);

-- With limits and defaults
SELECT mentat_pull('[(:person/friends :limit 5) (:person/bio :default "N/A")]', 42);
```

### Time travel

```sql
-- See the database as of transaction 100
SELECT mentat_query('
  [:find ?name :where [?e :person/name ?name]]
', '{"asOf": 100}');

-- Full history with assertion/retraction flags
SELECT mentat_query('
  [:find ?e ?name ?tx ?added
   :where [?e :person/name ?name ?tx ?added]]
', '{"history": true}');
```

### Rules

```sql
-- Recursive graph traversal
SELECT mentat_query('
  [:find ?boss-name
   :in $ ?employee-name
   :where
   [?e :person/name ?employee-name]
   (reports-to ?e ?boss)
   [?boss :person/name ?boss-name]]
  :rules [
   [(reports-to ?e ?boss) [?e :employee/manager ?boss]]
   [(reports-to ?e ?boss)
    [?e :employee/manager ?mid]
    (reports-to ?mid ?boss)]]
', '{"employee-name": "Dave"}');
```

See [EXAMPLES.md](EXAMPLES.md) for comprehensive usage examples including e-commerce catalogs, social networks, and project management patterns.

## Architecture

pg_mentat consists of two components:

```
Datomic Client --> mentatd (HTTP/EDN) --> PostgreSQL (pg_mentat extension) --> Datoms
```

**pg_mentat** (PostgreSQL extension) -- Implements the core Datalog engine as SQL functions (`mentat_transact`, `mentat_query`, `mentat_pull`, `mentat_entity`, `mentat_schema`). Data is stored in PostgreSQL tables (`mentat.datoms`, `mentat.schema`, `mentat.transactions`) with four covering indexes (EAVT, AEVT, AVET, VAET), full-text search via tsvector/GIN, and serializable isolation for consistency. Built with [pgrx](https://github.com/pgcentralfoundation/pgrx).

**mentatd** (HTTP daemon) -- Provides a Datomic-compatible HTTP API for remote clients. Connects to PostgreSQL via `tokio-postgres`, supports EDN and Transit wire formats, connection pooling via `deadpool`, LRU query caching, and Prometheus metrics. Built with [Axum](https://github.com/tokio-rs/axum).

### Data model

All data is stored as immutable EAVT (Entity-Attribute-Value-Transaction) datoms:

- **Entity** (E): 64-bit integer identifier
- **Attribute** (A): Schema-defined keyword (`:person/name`, `:order/total`)
- **Value** (V): Typed value (string, long, ref, boolean, double, instant, keyword, uuid, bytes)
- **Transaction** (Tx): Transaction ID when the datom was asserted
- **Added**: Boolean flag (true = assertion, false = retraction)

Retractions never delete data -- they record that a fact is no longer current. This provides a complete audit trail and enables time-travel queries.

## SQL Function Reference

| Function | Description |
|----------|-------------|
| `mentat_transact(edn TEXT)` | Process EDN transactions (assert, retract, retractEntity) |
| `mentat_query(query TEXT, inputs JSONB)` | Execute Datalog queries |
| `mentat_pull(pattern TEXT, entity_id BIGINT)` | Pull entity attributes by pattern |
| `mentat_entity(entity_id BIGINT)` | Get all attributes of an entity as JSON |
| `mentat_schema()` | Return current schema as JSON |

## PostgreSQL Compatibility

pg_mentat supports PostgreSQL 13 through 18 via pgrx feature flags:

```bash
cargo pgrx install --release --features pg16  # default
cargo pgrx install --release --features pg17
```

## Development

### Running tests

```bash
cd pg_mentat
cargo pgrx test pg16
```

### Project structure

```
pg_mentat/              PostgreSQL extension (pgrx)
mentatd/                HTTP daemon (Axum)
edn/                    EDN parser (rust-peg)
core/ + core-traits/    Fundamental types (ValueType, TypedValue)
db/ + db-traits/        Core storage logic
query-algebrizer/       Datalog to algebraic query compilation
query-projector/        Query result projection
query-pull/             Pull API implementation
query-sql/              SQL generation
sql/ + sql-traits/      SQL text generation and abstraction
transaction/            Transaction processing
tools/cli/              Command-line interface
tools/pg_mentat_cli/    PostgreSQL-specific CLI
```

## History

pg_mentat is derived from [Mozilla Mentat](https://github.com/mozilla/mentat), an embedded Datalog database originally backed by SQLite. This project was started by Mozilla but is [no longer maintained by them](https://mail.mozilla.org/pipermail/firefox-dev/2018-September/006780.html). This fork replaces the storage layer with PostgreSQL, adds a Datomic-compatible HTTP daemon, and reimplements the query engine as a PostgreSQL extension for production use.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on environment setup, coding standards, testing requirements, and pull request process.

## License

Apache-2.0. See [LICENSE](LICENSE) for details.
