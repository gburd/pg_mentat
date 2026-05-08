# pg_mentat

A PostgreSQL extension that parses EDN transactions, stores them as
Entity-Attribute-Value-Transaction (EAVT) datoms, and answers a subset
of Datalog queries. Derived from Mozilla's [Mentat](https://github.com/mozilla/mentat)
project, rewritten on top of [pgrx](https://github.com/pgcentralfoundation/pgrx)
so that all functionality is reachable from any PostgreSQL client via
`mentat_transact`, `mentat_query`, `mentat_pull`, and friends. An
optional HTTP daemon (`mentatd`) speaks the Datomic-style EDN wire
protocol for clients that need it.

pg_mentat is under active development. `docs/STATUS.md` lists what
works, what partially works, and what is missing; `docs/ROADMAP.md`
lays out the six-phase plan for closing the gaps.

## Features

Legend: **Works** — exercised by tests and usable today.
**Partial** — present with a stated gap.
**Not implemented** — absent; use the workaround if there is one.

| Feature | Status | Notes |
|---------|--------|-------|
| Schema definition (value types, cardinality, uniqueness) | Works | All Datomic schema attributes listed in `docs/STATUS.md` are accepted. |
| Transactions (`assert`, `retract`, `:db.fn/retractEntity`) | Works | EDN transaction format; tempids and lookup refs resolve during the same transaction. |
| Lookup refs in transactions and queries | Works | `[:person/email "alice@example.com"]` resolves to an entity ID. |
| Datalog queries with `:find` and `:where` | Works | Scalar / tuple / collection / relation find-specs. |
| `:in` bindings | Partial | Scalar inputs work; collection (`[?x ...]`), tuple (`[?x ?y]`), and relation (`[[?x ?y]]`) bindings are not yet executed end-to-end. |
| Rules | Partial | Recursive rules work with patterns and the six comparison operators in bodies; non-arithmetic where-functions inside rule bodies return `:db.error/unsupported-rule-*`. |
| Aggregates | Works | `count`, `sum`, `avg`, `min`, `max`. Other aggregates return `:db.error/unsupported-aggregate`. |
| Predicates (`<`, `>`, `<=`, `>=`, `=`, `!=`) | Works | Supported at top level and in rule bodies. |
| Arithmetic where-functions (`*`, `+`, `-`, `/`) | Works | Supported at top level and in rule bodies. |
| `ground`, `get-else`, `tuple` | Not implemented | Roadmap Phase 3. No workaround today; the older feature table listing these as done was wrong. |
| Attribute predicates (`[(attribute ?a :db/unique)]`) | Not implemented | Roadmap Phase 3. Read the schema via `mentat_schema()` from SQL as a stopgap. |
| `or` / `or-join` | Partial | One top-level `or-join` per query; patterns and supported predicates work inside branches; `not` and rule invocations inside branches are rejected. |
| `not` / `not-join` | Partial | Pattern clauses work with a groundedness check; predicates and function calls inside `not` are rejected. |
| Full-text search | Works | Backed by PostgreSQL `tsvector` / GIN. Score exposed via `fulltext`. |
| Pull API | Works | Attribute lists, wildcards, recursive nested pulls, reverse lookups, `:limit`, `:default`. |
| Time travel (`asOf`, `since`, `history`) | Works | Passed via the `inputs` JSONB argument to `mentat_query`. |
| Cardinality many | Works | Set semantics on storage and queries. |
| Entity / schema introspection | Partial | `mentat_entity()` returns a JSON snapshot; `mentat_schema()` returns the current schema. There is no lazy `d/entity`-style navigator. |
| Excision (`:db/excise`) | Not implemented | No roadmap item; retractions are preserved as history. |
| Clojure peer library (`d/connect`, `d/db`, `d/entity`, …) | Not implemented | Roadmap Phase 5. `pg-mentat-client/` has a stub HTTP client for `mentatd`, not a peer. |
| `mentatd` HTTP daemon | Partial | EDN wire format works; Transit is stubbed; connection pooling and LRU caching are wired. |
| Value types | Works | `string`, `long`, `double`, `boolean`, `instant`, `keyword`, `ref`, `uuid`, `bytes`. `bigint` is rejected with `:db.error/unsupported-constant`. |

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

pg_mentat supports two access paths. Direct PostgreSQL access is the recommended default.

```
Recommended:   App (any language) --> PostgreSQL (pg_mentat extension) --> Datoms

Optional:      Datomic Client --> mentatd (HTTP/EDN) --> PostgreSQL (pg_mentat extension) --> Datoms
```

**pg_mentat** (PostgreSQL extension) -- The core component. Implements the Datalog engine as SQL functions (`mentat_transact`, `mentat_query`, `mentat_pull`, `mentat_entity`, `mentat_schema`). Data is stored in PostgreSQL tables (`mentat.datoms`, `mentat.schema`, `mentat.transactions`) with four covering indexes (EAVT, AEVT, AVET, VAET), full-text search via tsvector/GIN, and serializable isolation for consistency. Built with [pgrx](https://github.com/pgcentralfoundation/pgrx). All functionality is available through standard SQL function calls from any PostgreSQL client.

**mentatd** (optional HTTP daemon) -- A Datomic-compatible HTTP gateway. Only needed if you have existing Datomic clients or require the Datomic wire protocol (EDN/Transit). Connects to PostgreSQL via `tokio-postgres`, supports EDN and Transit wire formats, connection pooling via `deadpool`, LRU query caching, and Prometheus metrics. Built with [Axum](https://github.com/tokio-rs/axum).

### When to use each approach

| | Direct PostgreSQL | Via mentatd |
|---|---|---|
| **Latency** | Lowest (no HTTP overhead) | +0.5-2ms per request |
| **Dependencies** | PostgreSQL + pg_mentat extension | + mentatd daemon |
| **Deployment** | Single service | Two services |
| **Best for** | All new projects | Migrating from Datomic |
| **Datomic compatibility** | No | Yes (EDN + Transit) |
| **Connection pooling** | Driver-native (pgbouncer, etc.) | mentatd deadpool + driver |
| **Caching** | PostgreSQL built-in | mentatd LRU + PostgreSQL |

### Data model

All data is stored as immutable EAVT (Entity-Attribute-Value-Transaction) datoms:

- **Entity** (E): 64-bit integer identifier
- **Attribute** (A): Schema-defined keyword (`:person/name`, `:order/total`)
- **Value** (V): Typed value (string, long, ref, boolean, double, instant, keyword, uuid, bytes)
- **Transaction** (Tx): Transaction ID when the datom was asserted
- **Added**: Boolean flag (true = assertion, false = retraction)

Retractions never delete data -- they record that a fact is no longer current. This provides a complete audit trail and enables time-travel queries.

## SQL Function Reference

These functions are the primary API. Call them from any PostgreSQL client.

| Function | Description |
|----------|-------------|
| `mentat_transact(edn TEXT)` | Process EDN transactions (assert, retract, retractEntity) |
| `mentat_query(query TEXT, inputs JSONB)` | Execute Datalog queries |
| `mentat_pull(pattern TEXT, entity_id BIGINT)` | Pull entity attributes by pattern |
| `mentat_pull_many(pattern TEXT, entity_ids BIGINT[])` | Pull attributes for multiple entities |
| `mentat_entity(entity_id BIGINT)` | Get all attributes of an entity as JSON |
| `mentat_schema()` | Return current schema as JSON |
| `mentat_explain(query TEXT, inputs JSONB)` | Show query execution plan |
| `mentat_query_stats()` | Query performance statistics |
| `mentat_storage_stats()` | Storage usage statistics |

## PostgreSQL Compatibility

pg_mentat supports PostgreSQL 13 through 18 via pgrx feature flags:

```bash
cargo pgrx install --release --features pg16  # default
cargo pgrx install --release --features pg17
```

## Client Libraries

pg_mentat works with any PostgreSQL client in any language. The `clients/` directory contains thin wrapper examples for common languages. See [clients/README.md](clients/README.md) for details.

### Python (direct PostgreSQL)

```python
from pg_mentat_client import MentatClient

with MentatClient("dbname=postgres") as m:
    m.transact('[{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]')
    m.transact('[{:person/name "Alice"}]')
    results = m.query('[:find ?name :where [?e :person/name ?name]]')
```

### Node.js (direct PostgreSQL)

```javascript
const { MentatClient } = require('./pg_mentat_client');

const client = new MentatClient({ connectionString: 'postgresql://localhost/postgres' });
await client.transact('[{:person/name "Alice"}]');
const results = await client.query('[:find ?name :where [?e :person/name ?name]]');
await client.close();
```

### Go (direct PostgreSQL)

```go
client, _ := pgmentat.New(ctx, "postgresql://localhost/postgres")
defer client.Close()
results, _ := client.Query(ctx, `[:find ?name :where [?e :person/name ?name]]`, nil)
```

### Rust (direct PostgreSQL)

```rust
let client = MentatClient::connect("host=localhost dbname=postgres").await?;
let results = client.query("[:find ?name :where [?e :person/name ?name]]", None).await?;
```

### Clojure (via mentatd -- Datomic compatibility)

For existing Datomic clients, a Datomic-compatible Clojure client library is available in `pg-mentat-client/`. This requires the mentatd daemon.

```clojure
(require '[pg-mentat.client :as mentat])

(def conn (mentat/connect "http://localhost:8080"))
(def db (mentat/db conn))

;; Query
(mentat/q '[:find ?e ?name :where [?e :person/name ?name]] db)

;; Transact
(mentat/transact conn [{:db/id "tempid1" :person/name "Charlie"}])

;; Pull
(mentat/pull db [:person/name :person/email] 10001)
```

See [pg-mentat-client/README.md](pg-mentat-client/README.md) for full documentation.

### Raw SQL (no client library needed)

You do not need any client library. Any PostgreSQL connection works:

```sql
-- psql, pgAdmin, DBeaver, or any PostgreSQL client
SELECT mentat_transact('[{:person/name "Alice"}]');
SELECT mentat_query('[:find ?name :where [?e :person/name ?name]]', '{}');
SELECT mentat_pull('[*]', 10001);
SELECT mentat_entity(10001);
SELECT mentat_schema();
```

## Development

### Running tests

```bash
cd pg_mentat
cargo pgrx test pg16
```

### Project structure

```
pg_mentat/              PostgreSQL extension (pgrx) -- the core component
clients/                Direct PostgreSQL client examples (Python, Node.js, Go, Rust)
mentatd/                HTTP daemon (Axum) -- optional, for Datomic compatibility
pg-mentat-client/       Clojure client library (uses mentatd)
edn/                    EDN parser (rust-peg)
core/ + core-traits/    Fundamental types (ValueType, TypedValue)
db/ + db-traits/        Core storage logic
query-algebrizer/       Datalog to algebraic query compilation
query-projector/        Query result projection
query-pull/             Pull API implementation
query-sql/              SQL generation
sql/ + sql-traits/      SQL text generation and abstraction
transaction/            Transaction processing
benchmarks/             Performance benchmarks (including direct vs mentatd)
tools/cli/              Command-line interface
tools/pg_mentat_cli/    PostgreSQL-specific CLI
```

## History

pg_mentat is derived from [Mozilla Mentat](https://github.com/mozilla/mentat), an embedded Datalog database originally backed by SQLite. This project was started by Mozilla but is [no longer maintained by them](https://mail.mozilla.org/pipermail/firefox-dev/2018-September/006780.html). This fork replaces the storage layer with PostgreSQL, adds a Datomic-compatible HTTP daemon, and reimplements the query engine as a PostgreSQL extension for production use.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on environment setup, coding standards, testing requirements, and pull request process.

## License

Apache-2.0. See [LICENSE](LICENSE) for details.
