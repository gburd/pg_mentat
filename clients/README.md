# pg_mentat Client Libraries

Direct PostgreSQL client examples for pg_mentat. These connect to PostgreSQL
and call the extension's SQL functions directly -- **no mentatd daemon required**.

## Available Clients

| Language | File | PostgreSQL Driver |
|----------|------|-------------------|
| Python | `python/pg_mentat_client.py` | psycopg2 |
| Node.js | `nodejs/pg_mentat_client.js` | pg (node-postgres) |
| Go | `go/pg_mentat_client.go` | pgx/v5 |
| Rust | `rust/pg_mentat_client.rs` | tokio-postgres |

For Clojure/Datomic-compatible access, see [`../pg-mentat-client/`](../pg-mentat-client/).

## API Surface

All clients expose the same operations, mapping 1:1 to the pg_mentat SQL functions:

| Method | SQL Function | Description |
|--------|-------------|-------------|
| `transact(edn)` | `mentat_transact(edn)` | Process EDN transactions |
| `query(datalog, inputs)` | `mentat_query(query, inputs::jsonb)` | Execute Datalog queries |
| `pull(pattern, id)` | `mentat_pull(pattern, id)` | Pull entity attributes |
| `pull_many(pattern, ids)` | `mentat_pull_many(pattern, ids)` | Pull multiple entities |
| `entity(id)` | `mentat_entity(id)` | Get all entity attributes |
| `schema()` | `mentat_schema()` | Return current schema |
| `explain(datalog, inputs)` | `mentat_explain(query, inputs)` | Query execution plan |

## Quick Start

### Python

```bash
pip install psycopg2-binary
```

```python
from pg_mentat_client import MentatClient

with MentatClient("dbname=postgres") as m:
    m.transact('[{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]')
    m.transact('[{:person/name "Alice"}]')
    results = m.query('[:find ?name :where [?e :person/name ?name]]')
    print(results)
```

### Node.js

```bash
npm install pg
```

```javascript
const { MentatClient } = require('./pg_mentat_client');

const client = new MentatClient({ connectionString: 'postgresql://localhost/postgres' });
const results = await client.query('[:find ?name :where [?e :person/name ?name]]');
console.log(results);
await client.close();
```

### Go

```bash
go get github.com/jackc/pgx/v5
```

```go
client, _ := pgmentat.New(ctx, "postgresql://localhost/postgres")
defer client.Close()
results, _ := client.Query(ctx, `[:find ?name :where [?e :person/name ?name]]`, nil)
```

### Rust

```toml
[dependencies]
tokio-postgres = "0.7"
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

```rust
let client = MentatClient::connect("host=localhost dbname=postgres").await?;
let results = client.query("[:find ?name :where [?e :person/name ?name]]", None).await?;
```

## Direct PostgreSQL vs mentatd

The direct approach is the **recommended default**. Use mentatd only when you need
Datomic client protocol compatibility.

| | Direct PostgreSQL | Via mentatd |
|---|---|---|
| **Latency** | Lowest (no HTTP overhead) | +0.5-2ms per request |
| **Dependencies** | PostgreSQL driver only | mentatd daemon + PostgreSQL |
| **Wire format** | SQL protocol (binary) | EDN or Transit over HTTP |
| **Deployment** | Single service (PostgreSQL) | Two services |
| **Caching** | PostgreSQL built-in | mentatd LRU + PostgreSQL |
| **Connection pooling** | Driver-native (pgbouncer, etc.) | mentatd deadpool + driver |
| **Datomic compatibility** | No | Yes |
| **Best for** | All new projects | Migrating from Datomic |

## Running the Benchmark

```bash
# Direct PostgreSQL only:
python ../benchmarks/direct_vs_mentatd.py --direct-only

# Both paths (mentatd must be running):
python ../benchmarks/direct_vs_mentatd.py
```
