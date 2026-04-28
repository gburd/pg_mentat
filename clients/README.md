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

## Native SQL View Access (DocumentDB Pattern)

In addition to the Datalog/EDN function API above, all clients also expose
methods that query pg_mentat's SQL virtual tables directly. This enables a
**dual-interface pattern** (like MongoDB's DocumentDB compatibility): use
Datalog for complex graph queries and SQL views for simple lookups, analytics,
and integration with SQL tools.

### Native SQL Methods

| Method | SQL Source | Description |
|--------|-----------|-------------|
| `facts(entity_id?, attribute?)` | `{schema}.facts` view | Human-readable EAVT facts |
| `text_values(attribute?)` | `{schema}.text_values` view | Text attribute values |
| `numeric_values(attribute?)` | `{schema}.numeric_values` view | Numeric attribute values |
| `entity_references(source?, target?)` | `{schema}.entity_references` view | Relationship navigation |
| `entity_history(entity_id?)` | `{schema}.entity_history` view | Full change history |
| `tx_log(limit?)` | `{schema}.tx_log` view | Transaction log |
| `schema_summary()` | `{schema}.schema_summary` view | Attribute usage stats |
| `lookup_entity(attr, value)` | `{schema}.lookup_entity()` | Find entities by value |
| `entity_value(id, attr)` | `{schema}.entity_value()` | Get single attribute value |
| `find_text(query)` | `{schema}.find_text()` | Full-text search with ranking |

### Example: Python

```python
with MentatClient("dbname=postgres") as m:
    # Find entities by attribute value (no Datalog needed)
    alices = m.lookup_entity(':person/name', 'Alice')

    # Get a single value
    name = m.entity_value(alices[0]['entity_id'], ':person/name')

    # Browse all facts for an entity
    facts = m.facts(entity_id=alices[0]['entity_id'])

    # Full-text search
    results = m.find_text('engineer')

    # View entity change history
    history = m.entity_history(entity_id=alices[0]['entity_id'])

    # Navigate relationships
    refs = m.entity_references(source=alices[0]['entity_id'])
```

### Example: Node.js

```javascript
const client = new MentatClient({ connectionString: 'postgresql://localhost/postgres' });

const alices = await client.lookupEntity(':person/name', 'Alice');
const name = await client.entityValue(alices[0].entity_id, ':person/name');
const facts = await client.facts({ entityId: alices[0].entity_id });
const results = await client.findText('engineer');
const history = await client.entityHistory(alices[0].entity_id);

await client.close();
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
