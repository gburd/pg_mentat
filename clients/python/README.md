# pg-mentat-client (Python)

Datomic-compatible Python client for pg_mentat. Connects to mentatd via
WebSocket using the Transit+JSON protocol.

## Installation

```bash
pip install pg-mentat-client
```

For async support:

```bash
pip install pg-mentat-client[async]
```

## Usage

```python
import pg_mentat

# Create a client
c = pg_mentat.client(endpoint="ws://localhost:8080/ws")

# Connect to a database
conn = pg_mentat.connect(c, db_name="my-db")

# Get a database value
database = pg_mentat.db(conn)

# Query
results = pg_mentat.q('[:find ?e ?name :where [?e :person/name ?name]]', database)

# Transact
pg_mentat.transact(conn, tx_data='[{:person/name "Alice"}]')

# Pull entity attributes
entity = pg_mentat.pull(database, "[*]", 10001)

# Time travel
old_db = pg_mentat.as_of(database, 1000)
results = pg_mentat.q('[:find ?e :where [?e :person/name]]', old_db)

# Close
conn.close()
```
