# pg_mentat

[![CI](https://img.shields.io/github/actions/workflow/status/gburd/pg_mentat/ci.yml?branch=main&label=CI)](https://github.com/gburd/pg_mentat/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/gburd/pg_mentat/blob/main/LICENSE)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-13--18-336791.svg?logo=postgresql)](https://github.com/gburd/pg_mentat)
[![pgrx](https://img.shields.io/badge/pgrx-0.17-orange.svg)](https://github.com/pgcentralfoundation/pgrx)

**Datomic-compatible Datalog query engine running entirely inside PostgreSQL.**

pg_mentat is a PostgreSQL extension (built with [pgrx](https://github.com/pgcentralfoundation/pgrx) 0.17 in Rust) that implements Datomic's data model: immutable facts (datoms), schema-first attributes, a full Datalog query compiler, pull API, time travel, and ACID transactions — all accessible from any PostgreSQL client via SQL functions.

An optional HTTP daemon (`mentatd`) speaks the Datomic client wire protocol for applications that expect it.

## Features

| Category | Capabilities |
|----------|-------------|
| **Find specifications** | Relation, collection, tuple, scalar |
| **Where clauses** | Pattern matching, predicates, function expressions |
| **Predicates** | `=`, `!=`, `<`, `>`, `<=`, `>=` |
| **Arithmetic** | `+`, `-`, `*`, `/` |
| **Text search** | `LIKE`, `ILIKE`, full-text (BM25 via tsvector) |
| **Negation** | `not`, `not-join` |
| **Disjunction** | `or`, `or-join` |
| **Input bindings** | Scalar, collection (`[?x ...]`), tuple (`[?x ?y]`), relation (`[[?x ?y]]`) |
| **Built-in functions** | `get-else`, `missing?`, `ground` |
| **Aggregates** | `count`, `count-distinct`, `sum`, `avg`, `min`, `max`, `sample` |
| **Rules** | Named rules, recursive rules with cycle detection |
| **Pull API** | Wildcard, reverse refs, nested, recursive with cycle detection, defaults, rename, limit |
| **Time travel** | `as-of`, `since`, `history`, `tx-range` |
| **Excision** | GDPR-compliant permanent deletion of datoms |
| **Subscriptions** | Reactive queries via PostgreSQL `LISTEN`/`NOTIFY` |
| **Value types** | `ref`, `boolean`, `instant`, `long`, `double`, `string`, `keyword`, `uuid`, `bytes` |
| **Schema attributes** | `:db/valueType`, `:db/cardinality`, `:db/unique`, `:db/index`, `:db/fulltext`, `:db/isComponent`, `:db/noHistory` |
| **Storage** | 9 narrow per-type tables with covering indexes (EAVT, AEVT, AVET, VAET) |
| **mentatd** | HTTP daemon with Datomic wire protocol (EDN, Transit+JSON, Transit+MsgPack) |

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     Application                          │
└──────┬───────────────────────────────────┬──────────────┘
       │ SQL (any PG client)               │ HTTP/EDN
       ▼                                   ▼
┌──────────────┐                   ┌──────────────┐
│  PostgreSQL  │                   │   mentatd    │
│  ┌────────┐  │                   │  (optional)  │
│  │pg_mentat│ │◀──────────────────│              │
│  └────────┘  │  tokio-postgres   └──────────────┘
│              │
│  ┌────────────────────────────────────────────┐
│  │  mentat.datoms_ref     mentat.datoms_long  │
│  │  mentat.datoms_string  mentat.datoms_bool  │
│  │  mentat.datoms_double  mentat.datoms_inst  │
│  │  mentat.datoms_kw      mentat.datoms_uuid  │
│  │  mentat.datoms_bytes                       │
│  └────────────────────────────────────────────┘
└──────────────────────────────────────────────────────────┘
```

## Quick Start

### Docker

```bash
docker run -d --name pg_mentat \
  -e POSTGRES_PASSWORD=postgres \
  -p 5432:5432 \
  ghcr.io/gburd/pg_mentat:latest

psql -h localhost -U postgres -c "CREATE EXTENSION pg_mentat;"
```

### Nix

```bash
nix develop
cargo pgrx run pg16
```

### From Source

```bash
# Prerequisites: Rust 1.88+, PostgreSQL 15-17 dev headers
cargo install --locked cargo-pgrx --version 0.17.0
cargo pgrx init --pg16 $(which pg_config)

cd pg_mentat
cargo pgrx install --release
```

Then in `psql`:

```sql
CREATE EXTENSION pg_mentat;
```

## Usage

### Define a Schema

```sql
SELECT mentat.mentat_transact('[
  {:db/ident       :person/name
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique      :db.unique/identity}
  {:db/ident       :person/age
   :db/valueType   :db.type/long
   :db/cardinality :db.cardinality/one}
  {:db/ident       :person/friends
   :db/valueType   :db.type/ref
   :db/cardinality :db.cardinality/many}
]');
```

### Transact Data

```sql
SELECT mentat.mentat_transact('[
  {:person/name "Alice" :person/age 30}
  {:person/name "Bob"   :person/age 25}
]');
```

### Query (Datalog)

```sql
-- Find all people over 28
SELECT mentat.mentat_query('
  [:find ?name ?age
   :where [?e :person/name ?name]
          [?e :person/age ?age]
          [(> ?age 28)]]
');

-- With input bindings
SELECT mentat.mentat_q('default', '
  [:find ?name
   :in $ ?min-age
   :where [?e :person/name ?name]
          [?e :person/age ?age]
          [(>= ?age ?min-age)]]
', '[25]');
```

### Pull API

```sql
-- Pull all attributes for entity 10001
SELECT mentat.mentat_pull('[*]', 10001);

-- Nested pull with reverse refs and limits
SELECT mentat.mentat_pull('[
  :person/name
  {:person/friends [:person/name :person/age]}
  {(:person/_friends :as :admirers :limit 5) [:person/name]}
]', 10001);
```

### Time Travel

```sql
-- Query the database as it was at transaction 1000005
SELECT mentat.mentat_q('default', '
  [:find ?name
   :where [?e :person/name ?name]]
', '[]', 1000005, NULL);

-- View transaction log
SELECT mentat.log('default', 1000001, 1000010);
```

### Rules

```sql
SELECT mentat.mentat_query('
  [:find ?name
   :where (ancestor ?e 10001)
          [?e :person/name ?name]
   :rules [[(ancestor ?x ?y)
             [?x :person/friends ?y]]
            [(ancestor ?x ?y)
             [?x :person/friends ?z]
             (ancestor ?z ?y)]]]
');
```

### Excision (GDPR)

```sql
-- Permanently remove all datoms for entity 10042
SELECT mentat.mentat_excise('default', 10042, NULL);

-- Remove only a specific attribute
SELECT mentat.mentat_excise('default', 10042, ':person/email');
```

## SQL Functions

| Function | Description |
|----------|-------------|
| `mentat_transact(edn)` | Execute an EDN transaction against the default store |
| `mentat_query(datalog)` | Run a Datalog query against the default store |
| `mentat_q(store, datalog, inputs)` | Run a parameterized query with input bindings |
| `mentat_q_full(store, datalog, inputs, as_of_tx, since_tx)` | Query with time travel parameters |
| `mentat_pull(pattern, eid)` | Pull attributes for a single entity |
| `mentat_pull_many(pattern, eids)` | Pull attributes for multiple entities |
| `mentat_with(edn)` | Speculative transaction (returns result without committing) |
| `mentat_explain(datalog)` | Show the generated SQL for a Datalog query |
| `mentat_schema()` | Return the current schema as JSON |
| `mentat_entity(eid)` | Return all current datoms for an entity |
| `mentat_excise(store, eid, attr)` | Permanently delete datoms (GDPR excision) |
| `mentat_query_sql(datalog)` | Return the SQL that would be generated (no execution) |
| `mentat_query_view(name, datalog)` | Create a PostgreSQL VIEW from a Datalog query |
| `t(store, edn)` | Short alias for `mentat_transact` with store |
| `q(store, datalog, inputs)` | Short alias for `mentat_q` |
| `log(store, from_tx, to_tx)` | Transaction log for a tx range |
| `diff(store, from_tx, to_tx)` | Diff between two transactions |
| `subscribe(store, name, query)` | Register a reactive subscription |
| `create_store(name, desc)` | Create an isolated named store |
| `materialize(store, name, query)` | Create a materialized Datalog view |
| `recursive(store, name, query, depth)` | Register a recursive query |
| `mentat_backend_stats()` | Runtime statistics (cache hits, query counts) |
| `mentat_health_check()` | Extension health check |

## PostgreSQL Compatibility

pg_mentat supports PostgreSQL 13 through 18 via pgrx feature flags:

```toml
[features]
pg13 = ["pgrx/pg13"]
pg14 = ["pgrx/pg14"]
pg15 = ["pgrx/pg15"]
pg16 = ["pgrx/pg16"]  # default
pg17 = ["pgrx/pg17"]
pg18 = ["pgrx/pg18"]
```

CI currently tests against PostgreSQL 16. The extension compiles cleanly for all versions.

## Documentation

- [Getting Started](docs/GETTING_STARTED.md) — installation and first steps
- [Datalog Reference](docs/DATALOG_REFERENCE.md) — full query language documentation
- [Schema Reference](docs/SCHEMA_REFERENCE.md) — attribute types and constraints
- [Migration from Datomic](docs/MIGRATION_FROM_DATOMIC.md) — porting guide

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Ensure `cargo clippy -- -D warnings` passes with zero warnings
4. Run `cargo pgrx test pg16` and verify all tests pass
5. Submit a pull request

The project enforces strict Clippy lints including `unwrap_used = "deny"` and `panic = "deny"`. See `Cargo.toml` for the full lint configuration.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).

## History

pg_mentat is derived from Mozilla's [Mentat](https://github.com/mozilla/mentat) project (2016-2018), an embedded Datalog database written in Rust that used SQLite as its storage backend. Mentat was abandoned in September 2018 with 233 open issues.

This project rewrites Mentat as a PostgreSQL extension using pgrx, replacing the SQLite backend with PostgreSQL's storage engine, MVCC, and indexing infrastructure. The Datalog compiler, EDN parser, and query planner have been substantially rewritten to generate native PostgreSQL SQL instead of SQLite queries, and to take advantage of PostgreSQL features (GIN indexes for full-text search, advisory locks for concurrency, LISTEN/NOTIFY for subscriptions, and partitioned narrow tables for type-specific storage).

The fork lineage is: `mozilla/mentat` -> `qpdb/mentat` -> `gburd/pg_mentat`.
