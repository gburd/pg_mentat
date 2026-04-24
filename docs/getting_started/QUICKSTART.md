# pg_mentat Quickstart

Get from zero to your first Datalog query on PostgreSQL.

## What is pg_mentat?

pg_mentat is a PostgreSQL extension that brings Datomic-style Datalog queries and an immutable, time-aware data model to PostgreSQL. Data is stored as Entity-Attribute-Value-Transaction (EAVT) datoms, queried with Datalog, and managed through standard SQL function calls.

Key capabilities:
- **Datalog queries** via `mentat_query()` -- declarative, logic-based
- **Immutable history** -- every change recorded, nothing deleted
- **Time travel** -- query data as it existed at any past transaction
- **Schema flexibility** -- add attributes without ALTER TABLE
- **Pull API** -- retrieve nested entity data in one call
- **HTTP daemon** (`mentatd`) -- Datomic-compatible remote access

## Prerequisites

- PostgreSQL 13--18
- Rust 1.90+ (for building from source)

## Installation

### Option 1: Docker (fastest)

```bash
docker build -t pg_mentat .
docker run -d --name pg_mentat -p 5432:5432 pg_mentat
```

The container runs `demo.sql` on first start, which creates the extension, bootstraps the schema, and loads sample data. Connect immediately:

```bash
psql -h localhost -U postgres
```

Skip to [Verify Installation](#verify-installation) to confirm it works.

### Option 2: Nix

If you have Nix with flakes enabled:

```bash
git clone <repo-url> && cd pg_mentat
nix develop
cd pg_mentat
cargo pgrx run pg16
```

This drops you into a `psql` session with the extension loaded.

### Option 3: From source

```bash
# Install cargo-pgrx (must match version ~0.17 used by the project)
cargo install --locked cargo-pgrx --version '~0.17'

# Initialize pgrx with your system PostgreSQL
cargo pgrx init --pg16=$(which pg_config)

# Clone and build
git clone <repo-url> && cd pg_mentat/pg_mentat
cargo pgrx install --release
```

Then connect to PostgreSQL and create the extension:

```sql
CREATE EXTENSION pg_mentat;
```

### Option 4: Podman

```bash
podman build -t pg_mentat .
podman run -d --name pg_mentat -p 5432:5432 pg_mentat
psql -h localhost -U postgres
```

## Verify Installation

```sql
SELECT mentat_schema();
```

This returns a JSON object describing the bootstrap schema attributes (`:db/ident`, `:db/valueType`, `:db/cardinality`, etc.). If you see output, the extension is working.

## Step 1: Define a Schema

pg_mentat schemas are defined by transacting attribute definitions. Each attribute needs an ident (keyword name), a value type, and a cardinality.

```sql
SELECT mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/doc "A persons full name"}

  {:db/ident :person/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}

  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity
   :db/doc "Unique email address"}

  {:db/ident :person/hobbies
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/many
   :db/doc "A persons hobbies (multi-valued)"}

  {:db/ident :person/friend
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many
   :db/doc "References to other person entities"}
]');
```

**Schema attribute reference:**

| Attribute | Required | Values |
|-----------|----------|--------|
| `:db/ident` | Yes | Namespaced keyword (`:person/name`) |
| `:db/valueType` | Yes | `string`, `long`, `double`, `boolean`, `instant`, `ref`, `keyword`, `uuid`, `bytes` |
| `:db/cardinality` | Yes | `:db.cardinality/one` or `:db.cardinality/many` |
| `:db/unique` | No | `:db.unique/value` (unique) or `:db.unique/identity` (unique + upsert) |
| `:db/doc` | No | Documentation string |
| `:db/index` | No | `true` to create an AVET index for fast value lookups |
| `:db/fulltext` | No | `true` to enable full-text search on this attribute |

## Step 2: Transact Data

Insert entities using EDN map notation. String values like `"alice"` are temporary IDs that get resolved to permanent entity IDs.

```sql
SELECT mentat_transact('[
  {:db/id "alice"
   :person/name "Alice Johnson"
   :person/age 30
   :person/email "alice@example.com"
   :person/hobbies ["reading" "hiking" "photography"]}

  {:db/id "bob"
   :person/name "Bob Smith"
   :person/age 25
   :person/email "bob@example.com"
   :person/hobbies ["gaming" "cooking"]
   :person/friend "alice"}

  {:db/id "carol"
   :person/name "Carol Williams"
   :person/age 35
   :person/email "carol@example.com"
   :person/friend ["alice" "bob"]}
]');
```

The function returns a JSON transaction report:

```json
{
  "tx-id": 1000002,
  "tx-instant": null,
  "tempids": {"alice": 10000, "bob": 10001, "carol": 10002},
  "datoms-inserted": 12
}
```

The `tempids` map shows the permanent entity IDs assigned. Save these -- you will need them for pulls and direct updates.

**Transaction syntax:**
- `{:db/id "tempid" :attr value}` -- assert attributes on a new entity
- `[:db/add entity-id :attr value]` -- assert a single fact
- `[:db/retract entity-id :attr value]` -- retract a specific fact
- `[:db/retractEntity entity-id]` -- retract all facts about an entity

## Step 3: Query with Datalog

The `mentat_query` function takes a Datalog query string and a JSONB inputs object.

### Find all people

```sql
SELECT mentat_query(
  '[:find ?name ?age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]]',
  '{}'::jsonb
);
```

The `:where` clause contains patterns that match datoms. Variables (prefixed with `?`) bind to values. Using the same variable in multiple patterns creates implicit joins -- here `?e` joins name and age for the same entity.

### Filter with predicates

```sql
SELECT mentat_query(
  '[:find ?name ?age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    [(> ?age 28)]]',
  '{}'::jsonb
);
```

Returns only people older than 28. Supported predicates: `>`, `<`, `>=`, `<=`, `=`, `!=`.

### Query with input parameters

Use `:in` to pass parameters:

```sql
SELECT mentat_query(
  '[:find ?name
    :in ?min-age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    [(>= ?age ?min-age)]]',
  '{"inputs": [30]}'::jsonb
);
```

Parameters are passed positionally in the `"inputs"` array, matching the order of `:in` variables.

### Follow references (joins)

Find people and their friends' names:

```sql
SELECT mentat_query(
  '[:find ?name ?friend-name
    :where
    [?e :person/name ?name]
    [?e :person/friend ?friend]
    [?friend :person/name ?friend-name]]',
  '{}'::jsonb
);
```

### Find spec variants

Control the shape of results:

```sql
-- Relation (default): collection of tuples
[:find ?name ?age :where ...]
-- Returns: [["Alice", 30], ["Bob", 25]]

-- Scalar: single value
[:find ?name . :where ...]
-- Returns: "Alice"

-- Collection: single column
[:find [?name ...] :where ...]
-- Returns: ["Alice", "Bob", "Carol"]

-- Tuple: single row
[:find [?name ?age] :where ...]
-- Returns: ["Alice", 30]
```

### Aggregates

```sql
-- Count
SELECT mentat_query(
  '[:find (count ?e) .
    :where [?e :person/name]]',
  '{}'::jsonb
);

-- Average age
SELECT mentat_query(
  '[:find (avg ?age) .
    :where [?e :person/age ?age]]',
  '{}'::jsonb
);
```

Supported aggregates: `count`, `sum`, `avg`, `min`, `max`.

## Step 4: Pull Entity Data

The Pull API retrieves structured entity data by pattern. It is more natural than queries when you know which entity you want and need its attributes.

```sql
-- Pull specific attributes (use an entity ID from the tempids above)
SELECT mentat_pull('[:person/name :person/age :person/email]', 10000);
```

Result:

```json
{
  ":db/id": 10000,
  ":person/name": "Alice Johnson",
  ":person/age": 30,
  ":person/email": "alice@example.com"
}
```

### Pull with wildcard

```sql
SELECT mentat_pull('[*]', 10000);
```

Returns all attributes for the entity.

### Pull with nested references

```sql
SELECT mentat_pull(
  '[:person/name {:person/friend [:person/name :person/email]}]',
  10001
);
```

Result:

```json
{
  ":db/id": 10001,
  ":person/name": "Bob Smith",
  ":person/friend": {
    ":db/id": 10000,
    ":person/name": "Alice Johnson",
    ":person/email": "alice@example.com"
  }
}
```

### Reverse lookups

Find entities that reference a given entity:

```sql
SELECT mentat_pull('[:person/name :person/_friend]', 10000);
```

The `_` prefix on `:person/_friend` means "find all entities whose `:person/friend` points to this entity."

## Step 5: Time Travel

pg_mentat never deletes data. Every change is a new datom. This enables querying the database as it existed at any past transaction.

### Make a change

```sql
-- Update Alice's age
SELECT mentat_transact('[
  [:db/add 10000 :person/age 31]
]');
```

This automatically retracts the old age (30) and asserts the new one (31).

### As-of query

Query the database as it was before the update:

```sql
SELECT mentat_query(
  '[:find ?name ?age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]]',
  '{"asOf": 1000002}'::jsonb
);
```

Returns data from transaction 1000002 or earlier, showing Alice's age as 30.

### History query

See all assertions and retractions:

```sql
SELECT mentat_query(
  '[:find ?name ?age ?tx ?added
    :where
    [?e :person/name ?name]
    [?e :person/age ?age ?tx ?added]]',
  '{"history": true}'::jsonb
);
```

The `?added` variable is `true` for assertions and `false` for retractions, giving you a complete audit trail.

### Since query

Find changes since a specific transaction:

```sql
SELECT mentat_query(
  '[:find ?e ?name
    :where
    [?e :person/name ?name]]',
  '{"since": 1000002}'::jsonb
);
```

Returns only datoms with transaction IDs after 1000002.

## Step 6: Connect via mentatd (HTTP)

`mentatd` is an HTTP daemon that provides a Datomic-compatible API for remote clients. It connects to the same PostgreSQL database and calls the same extension functions.

### Start mentatd

```bash
cd mentatd
cargo build --release

# Configure via environment variables
export DATABASE_URL="postgresql://localhost/postgres"
export MENTATD_HOST="127.0.0.1"
export MENTATD_PORT="8080"

./target/release/mentatd
```

Or use a TOML config file:

```toml
# mentatd.toml
[server]
host = "127.0.0.1"
port = 8080

[database]
connection_string = "postgresql://localhost/postgres"
pool_size = 10

[logging]
level = "info"
format = "compact"

[cache]
enabled = true
capacity = 1000
ttl_secs = 300
```

```bash
MENTATD_CONFIG=mentatd.toml ./target/release/mentatd
```

### Query via HTTP

Requests use EDN format, sent as POST to the root endpoint:

```bash
# Health check
curl http://localhost:8080/health

# Query
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/edn" \
  -d '{:op :q
       :args {:query [:find ?name ?age
                       :where
                       [?e :person/name ?name]
                       [?e :person/age ?age]]}}'

# Transact
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/edn" \
  -d '{:op :transact
       :args {:connection-id "default"
              :tx-data [{:db/id "dave"
                         :person/name "Dave"
                         :person/age 28}]}}'

# Pull
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/edn" \
  -d '{:op :pull
       :args {:pattern [:person/name :person/age]
              :entity-id 10000}}'
```

### Response formats

mentatd supports three wire formats, selected via the `Accept` header:

| Accept Header | Format |
|---------------|--------|
| `application/edn` (default) | EDN |
| `application/transit+json` | Transit JSON |
| `application/transit+msgpack` | Transit MessagePack |

### Connect from Clojure

```clojure
(require '[datomic.api :as d])

(def conn (d/connect "datomic:sql://mentat?jdbc:postgresql://localhost:5432/postgres"))

;; Query
(d/q '[:find ?name
       :where [?e :person/name ?name]]
     (d/db conn))

;; Transact
(d/transact conn
  [{:db/id "new-person"
    :person/name "Eve"
    :person/age 22}])

;; Pull
(d/pull (d/db conn)
        [:person/name :person/age]
        [:person/email "eve@example.com"])
```

### Connect from Python

```python
import requests

MENTATD_URL = "http://localhost:8080"

def query(q, args=None):
    edn = '{:op :q :args {:query ' + q + '}}'
    resp = requests.post(MENTATD_URL, data=edn,
                         headers={"Content-Type": "application/edn",
                                  "Accept": "application/transit+json"})
    return resp.json()

result = query('[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]')
print(result)
```

### Connect from JavaScript

```javascript
async function query(q) {
  const resp = await fetch("http://localhost:8080", {
    method: "POST",
    headers: {
      "Content-Type": "application/edn",
      "Accept": "application/transit+json",
    },
    body: `{:op :q :args {:query ${q}}}`,
  });
  return resp.json();
}

const result = await query(
  "[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]"
);
console.log(result);
```

## SQL Function Reference

| Function | Signature | Description |
|----------|-----------|-------------|
| `mentat_transact` | `(edn TEXT) -> TEXT` | Process EDN transactions; returns JSON report with tx-id and tempids |
| `mentat_query` | `(query TEXT, inputs JSONB) -> JSONB` | Execute a Datalog query with optional inputs |
| `mentat_pull` | `(pattern TEXT, entity_id BIGINT) -> JSONB` | Pull entity attributes by pattern |
| `mentat_entity` | `(entity_id BIGINT) -> JSONB` | Get all attributes of an entity as JSON |
| `mentat_schema` | `() -> JSONB` | Return the current schema as JSON |

## Common Patterns

### Upsert (insert or update)

When an attribute has `:db.unique/identity`, transacting a value that already exists updates the existing entity instead of creating a new one:

```sql
SELECT mentat_transact('[
  {:person/email "alice@example.com"
   :person/name "Alice J. Updated"
   :person/age 31}
]');
```

### Retract data

```sql
-- Retract a specific fact
SELECT mentat_transact('[[:db/retract 10000 :person/age 31]]');

-- Retract all facts about an entity
SELECT mentat_transact('[[:db/retractEntity 10000]]');
```

### Rules (recursive queries)

```sql
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

Rules enable recursive traversals like transitive closure over graphs.

## Next Steps

- Read [CONCEPTS.md](./CONCEPTS.md) for deeper understanding of EAVT, Datalog, and schema evolution
- See [EXAMPLES.md](../../EXAMPLES.md) for real-world patterns (e-commerce, social networks, project management)
- Check the [API reference](../api/) for complete function documentation
- Read the [Docker guide](../../README_DOCKER.md) for container deployment
