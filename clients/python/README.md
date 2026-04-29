# pg_mentat Python Client

Idiomatic Python client for pg_mentat. Connects directly to PostgreSQL and
calls extension functions via SQL -- no mentatd daemon required.

## Installation

```bash
pip install psycopg2-binary
```

Then copy or install the `pg_mentat` package.

## Quick Start

```python
from pg_mentat import Connection

# Connect to PostgreSQL
conn = Connection("dbname=postgres host=localhost")

# Define schema
conn.transact("""[
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
]""")

# Transact data
tx_report = conn.transact("""[
  {:person/name "Alice"
   :person/email "alice@example.com"
   :person/age 30}
  {:person/name "Bob"
   :person/email "bob@example.com"
   :person/age 25}
]""")

# Get a database snapshot and query
db = conn.db()
results = db.q('[:find ?name ?email :where [?e :person/name ?name] [?e :person/email ?email]]')
print(results)
# => [["Alice", "alice@example.com"], ["Bob", "bob@example.com"]]

conn.close()
```

## API Reference

### Connection

```python
Connection(dsn=None, connection=None, **kwargs)
```

Create a connection to a pg_mentat-enabled PostgreSQL database.

- **dsn**: PostgreSQL connection string (e.g. `"dbname=postgres host=localhost"`)
- **connection**: An existing psycopg2 connection to reuse
- **\*\*kwargs**: Additional psycopg2.connect() arguments

#### Methods

| Method | Description |
|--------|-------------|
| `conn.db()` | Get current database snapshot |
| `conn.transact(tx_data)` | Execute EDN transaction |
| `conn.close()` | Close the connection |
| `conn.closed` | Whether connection is closed (property) |

### Database

Obtained via `conn.db()`. Immutable snapshot of the database state.

#### Methods

| Method | Description |
|--------|-------------|
| `db.q(query, *inputs)` | Execute Datalog query |
| `db.pull(pattern, eid)` | Pull entity attributes |
| `db.pull_many(pattern, eids)` | Pull attributes for multiple entities |
| `db.entity(eid)` | Get all entity attributes as dict |
| `db.as_of(tx_or_instant)` | Time-travel to a point in time |

## Usage Examples

### Context Manager

```python
from pg_mentat import Connection

with Connection("dbname=postgres") as conn:
    conn.transact('[{:person/name "Alice"}]')
    db = conn.db()
    results = db.q('[:find ?name :where [?e :person/name ?name]]')
    print(results)
# Connection is automatically closed
```

### Pull API

```python
db = conn.db()

# Pull all attributes
entity = db.pull('[*]', 42)

# Pull specific attributes
entity = db.pull('[:person/name :person/email]', 42)

# Pull multiple entities
entities = db.pull_many('[*]', [42, 43, 44])
```

### Time Travel

```python
db = conn.db()

# Query as of a specific transaction ID
old_db = db.as_of(1000005)
old_results = old_db.q('[:find ?name :where [?e :person/name ?name]]')

# Query as of a specific timestamp
from datetime import datetime
past_db = db.as_of(datetime(2024, 1, 1))
past_results = past_db.q('[:find ?name :where [?e :person/name ?name]]')
```

### Transacting with Python Dicts

```python
# Instead of raw EDN, pass a list of dicts
conn.transact([
    {":db/ident": ":person/name",
     ":db/valueType": ":db.type/string",
     ":db/cardinality": ":db.cardinality/one"}
])
```

### Reuse an Existing psycopg2 Connection

```python
import psycopg2

pg_conn = psycopg2.connect("dbname=postgres")
pg_conn.autocommit = True

conn = Connection(connection=pg_conn)
db = conn.db()
results = db.q('[:find ?e :where [?e :person/name]]')

# conn.close() will NOT close pg_conn since it was passed in externally
conn.close()
pg_conn.close()
```

## Compatibility

- Python 3.7+
- psycopg2-binary >= 2.9.0 (or psycopg2 for production builds)
- PostgreSQL with pg_mentat extension installed

## SQL Functions Used

| Client Method | SQL Function |
|--------------|-------------|
| `conn.transact()` | `mentat_transact(edn)` |
| `db.q()` | `mentat_query(query, inputs::jsonb)` |
| `db.pull()` | `mentat_pull(pattern, entity_id)` |
| `db.pull_many()` | `mentat_pull_many(pattern, entity_ids)` |
| `db.entity()` | `mentat_entity(entity_id)` |
