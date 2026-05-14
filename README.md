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

All examples use the **convenience aliases** (`mentat.t`, `mentat.q`, `mentat.pull`, etc.) which are the recommended way to interact with the extension. See [Function Naming](#function-naming) for details.

### Define a Schema

```sql
SELECT mentat.t('[
  {:db/ident       :person/name
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique      :db.unique/identity}
  {:db/ident       :person/age
   :db/valueType   :db.type/long
   :db/cardinality :db.cardinality/one}
  {:db/ident       :person/email
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique      :db.unique/value}
  {:db/ident       :person/friends
   :db/valueType   :db.type/ref
   :db/cardinality :db.cardinality/many}
]');
```

### Transact Data

```sql
SELECT mentat.t('[
  {:person/name "Alice" :person/age 30 :person/email "alice@example.com"}
  {:person/name "Bob"   :person/age 25 :person/email "bob@example.com"}
  {:person/name "Carol" :person/age 35 :person/email "carol@example.com"}
]');
```

### Query (Datalog)

```sql
-- Find all people over 28
SELECT mentat.q('
  [:find ?name ?age
   :where [?e :person/name ?name]
          [?e :person/age ?age]
          [(> ?age 28)]]
');

-- With input bindings
SELECT mentat.q('
  [:find ?name
   :in $ ?min-age
   :where [?e :person/name ?name]
          [?e :person/age ?age]
          [(>= ?age ?min-age)]]
', '[25]');
```

### Pull API

```sql
-- Pull all attributes for an entity
SELECT mentat.pull('[*]', 10001);

-- Nested pull with reverse refs and limits
SELECT mentat.pull('[
  :person/name
  {:person/friends [:person/name :person/age]}
  {(:person/_friends :as :admirers :limit 5) [:person/name]}
]', 10001);
```

### Time Travel

```sql
-- Query the database as it was at transaction 1000005
SELECT mentat.q('
  [:find ?name
   :where [?e :person/name ?name]]
', '[]', 1000005, NULL);

-- View transaction log
SELECT mentat.log('default', 1000001, 1000010);
```

### Excision (GDPR)

```sql
-- Permanently remove all datoms for entity 10042
SELECT mentat.mentat_excise('default', 10042, NULL);

-- Remove only a specific attribute
SELECT mentat.mentat_excise('default', 10042, ':person/email');
```

---

## Examples That Will Make SQL Programmers Rethink Everything

### Graph traversal without JOINs

Find all friends-of-friends using recursive rules — no self-joins, no CTEs, no depth limits baked into the query:

```sql
-- "Who can Alice reach through any chain of friendships?"
SELECT mentat.q('
  [:find ?name
   :in $ ?start
   :where [?start :person/name ?start-name]
          (reachable ?start ?friend)
          [?friend :person/name ?name]
   :rules [[(reachable ?a ?b)
              [?a :person/friends ?b]]
           [(reachable ?a ?b)
              [?a :person/friends ?c]
              (reachable ?c ?b)]]]
', '["Alice"]');
```

The SQL equivalent would require a recursive CTE with cycle detection, explicit join conditions, and careful termination logic — easily 20+ lines. The Datalog version is declarative: *state the relationship, not the algorithm*.

### Schema-as-data: query your own schema

In pg_mentat, schema IS data — it lives in the same datom store as your application data:

```sql
-- "What attributes does the system know about?"
SELECT mentat.q('
  [:find ?ident ?type ?cardinality
   :where [?a :db/ident ?ident]
          [?a :db/valueType ?vt]
          [?vt :db/ident ?type]
          [?a :db/cardinality ?card]
          [?card :db/ident ?cardinality]]
');
```

No `information_schema`. No `pg_catalog`. Your schema and your data are queried with the same language.

### Temporal diff: what changed between two transactions?

```sql
-- Show exactly what changed between tx 1000003 and 1000007
SELECT mentat.diff('default', 1000003, 1000007);
```

Returns every datom that was added or retracted between those two points in time. In traditional SQL, you'd need audit tables, triggers, temporal extensions, or CDC infrastructure. Here it's built into the storage model.

### Speculative transactions: "what if?" without committing

```sql
-- Try a transaction without persisting it
SELECT mentat.mentat_with('[
  {:person/name "Alice" :person/age 99}
]');
```

Returns the full transaction report (tempid resolution, new datoms) but writes nothing. Use this for validation, conflict detection, or previewing the effect of a batch import.

### Upsert by identity: no INSERT ON CONFLICT gymnastics

```sql
-- If "Alice" already exists (by :db.unique/identity), update her age.
-- If she doesn't exist, create her. One transaction, no conditional logic.
SELECT mentat.t('[
  {:person/name "Alice" :person/age 31}
]');
```

Because `:person/name` has `:db.unique/identity`, this is automatically an upsert. No `ON CONFLICT`, no `MERGE`, no `WHERE EXISTS` subquery.

### Pull API: reshape query results without application code

```sql
-- Get a nested JSON document in one round-trip
SELECT mentat.pull('[
  :person/name
  :person/email
  {:person/friends [
    :person/name
    :person/age
    {:person/friends [:person/name]}
  ]}
]', 10001);
```

Returns:

```json
{
  "person/name": "Alice",
  "person/email": "alice@example.com",
  "person/friends": [
    {
      "person/name": "Bob",
      "person/age": 25,
      "person/friends": [{"person/name": "Carol"}]
    }
  ]
}
```

No GraphQL server. No ORM. No N+1 queries. One SQL call, arbitrarily nested results.

### Combine Datalog with SQL: the best of both worlds

Create a PostgreSQL VIEW backed by a Datalog query, then join it with relational tables:

```sql
-- Create a view powered by Datalog
SELECT mentat.mentat_query_view('people_over_30', '
  [:find ?name ?age ?email
   :where [?e :person/name ?name]
          [?e :person/age ?age]
          [?e :person/email ?email]
          [(> ?age 30)]]
');

-- Now use it like any SQL view — join with relational data
SELECT v.name, v.email, o.total
FROM mentat.people_over_30 v
JOIN orders o ON o.customer_email = v.email
WHERE o.total > 100.00
ORDER BY o.total DESC;
```

Your Datalog knowledge graph and your relational tables, queryable together in a single SQL statement.

---

## Function Naming

pg_mentat installs into the `mentat` schema by default. Functions are available in two forms:

### Convenience aliases (recommended)

For the default `mentat` schema, short aliases avoid redundancy:

| Function | Description |
|----------|-------------|
| `mentat.t(edn)` | Transact EDN data |
| `mentat.q(query, inputs)` | Run a Datalog query |
| `mentat.pull(pattern, eid)` | Pull attributes for an entity |
| `mentat.pull_many(pattern, eids)` | Pull for multiple entities |
| `mentat.entity(eid)` | All datoms for an entity as JSON |
| `mentat.schema()` | Current schema as JSON |
| `mentat.explain(query)` | Show generated SQL for a query |
| `mentat.stats()` | Query execution statistics |
| `mentat.storage()` | Storage and index statistics |
| `mentat.cache_stats()` | Prepared statement cache info |
| `mentat.cache_clear()` | Clear the statement cache |
| `mentat.log(store, from_tx, to_tx)` | Transaction log |
| `mentat.diff(store, from_tx, to_tx)` | Diff between transactions |
| `mentat.subscribe(store, name, query)` | Reactive subscription |
| `mentat.create_store(name, desc)` | Create an isolated store |

### Full-name functions

The underlying pgrx-exported functions use a `mentat_` prefix. These are always available and are what you'd use if you install the extension into a custom schema:

```sql
-- Default schema: use convenience aliases
SELECT mentat.q('[:find ?e :where [?e :person/name "Alice"]]');

-- Custom schema: full names are natural
CREATE EXTENSION pg_mentat SCHEMA myapp;
SELECT myapp.mentat_query('[:find ?e :where [?e :person/name "Alice"]]');
```

| Full function | Convenience alias |
|---------------|-------------------|
| `mentat_transact(edn)` | `t(edn)` |
| `mentat_query(query, inputs)` | `q(query, inputs)` |
| `mentat_pull(pattern, eid)` | `pull(pattern, eid)` |
| `mentat_pull_many(pattern, eids)` | `pull_many(pattern, eids)` |
| `mentat_entity(eid)` | `entity(eid)` |
| `mentat_schema()` | `schema()` |
| `mentat_explain(query)` | `explain(query)` |
| `mentat_with(edn)` | *(speculative transaction)* |
| `mentat_excise(store, eid, attr)` | *(GDPR excision)* |
| `mentat_query_view(name, query)` | *(create Datalog-backed VIEW)* |
| `mentat_query_sql(query)` | *(show generated SQL)* |
| `mentat_health_check()` | *(extension health)* |

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

Full documentation is available as an [mdBook](https://gburd.github.io/pg_mentat/):

- [Getting Started](docs/src/getting-started.md)
- [Datalog Query Language](docs/src/datalog.md)
- [Pull API](docs/src/pull-api.md)
- [Time Travel](docs/src/time-travel.md)
- [Schema Reference](docs/src/schema.md)
- [SQL Function Reference](docs/src/sql-functions.md)
- [Configuration](docs/src/configuration.md)
- [Datomic Compatibility](docs/src/datomic-compat.md)

### Performance

- [Phase 2 benchmark](docs/benchmarks/phase2.md) — 100K / 300K / 1M
  datom workload comparing pg_mentat to a hand-written EAV baseline,
  with raw CSVs, EXPLAIN plans, and a CPU flamegraph. Reproducible
  via `bash benchmarks/phase2/run.sh`.

### Extension integrations

pg_mentat composes with other PostgreSQL extensions for capabilities
beyond the core Datalog engine. Each integration is opt-in and
soft-dependency: pg_mentat works without the optional extension; if
the extension is installed, a Datalog where-fn or value type becomes
available.

- **pg_tre** — [approximate-regex search](docs/src/fuzzy-search.md)
  via `(fuzzy-match $ :attr "pattern" k)`.

Further integrations (pgvector, PostGIS, TimescaleDB, fuzzystrmatch,
pg_partman, pg_trgm, pg_jsonschema, pg_cron, postgres_fdw, ...) are
planned. See [docs/INTEGRATIONS.md](docs/INTEGRATIONS.md) for the full
list with integration shape, effort, and priority for each.

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

The fork lineage is: `mozilla/mentat` → `qpdb/mentat` → `gburd/pg_mentat`.
