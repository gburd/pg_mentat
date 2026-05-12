# Getting Started

## Installation

### From Source (Nix)

The recommended development environment uses Nix with the provided flake:

```bash
git clone https://github.com/gburd/pg_mentat.git
cd pg_mentat
nix develop

# Build and install into pgrx-managed PostgreSQL
cargo pgrx install --features pg16
```

### From Source (Manual)

Prerequisites:
- Rust 1.88+ (with cargo)
- PostgreSQL 13-18 development headers
- LLVM/Clang (for pgrx bindgen)

```bash
git clone https://github.com/gburd/pg_mentat.git
cd pg_mentat

# Install cargo-pgrx if not already present
cargo install cargo-pgrx --version 0.17.0
cargo pgrx init --pg16 $(which pg_config)

# Build and install
cargo pgrx install --release --features pg16
```

### Docker

```bash
docker run -d \
  --name pg_mentat \
  -e POSTGRES_PASSWORD=secret \
  -p 5432:5432 \
  ghcr.io/gburd/pg_mentat:latest
```

Or use Docker Compose with the included configuration:

```bash
cd docker
docker compose up -d
```

This starts PostgreSQL with pg_mentat, the mentatd HTTP server, Prometheus, and Grafana.

## Quick Start

### 1. Create the Extension

```sql
CREATE EXTENSION pg_mentat;
```

This creates the `mentat` schema with all required tables, types, indexes, and functions.

### 2. Define Schema Attributes

Schema attributes describe the shape of your data. Every attribute needs an ident (namespaced keyword), a value type, and a cardinality.

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
   :db/cardinality :db.cardinality/many}

  {:db/ident       :person/friends
   :db/valueType   :db.type/ref
   :db/cardinality :db.cardinality/many}
]');
```

### 3. Transact Data

Use tempids (string identifiers) to create new entities. References between entities in the same transaction resolve automatically.

```sql
SELECT mentat.t('[
  {:db/id "alice"
   :person/name "Alice"
   :person/age 30
   :person/email "alice@example.com"}

  {:db/id "bob"
   :person/name "Bob"
   :person/age 25
   :person/friends "alice"}
]');
```

The return value is a JSON report containing the transaction ID, timestamp, and tempid resolution map.

### 4. Query with Datalog

```sql
-- Find all people over 25
SELECT mentat.q('
  [:find ?name ?age
   :where [?e :person/name ?name]
          [?e :person/age ?age]
          [(> ?age 25)]]
');

-- Find friends-of-friends (with input binding)
SELECT mentat.q('
  [:find ?friend-name
   :in $ ?name
   :where [?e :person/name ?name]
          [?e :person/friends ?f]
          [?f :person/name ?friend-name]]
', '["Alice"]');
```

### 5. Pull Entity Data

```sql
-- Pull all attributes for entity 10001
SELECT mentat.pull('[*]', 10001);

-- Pull specific attributes with nested navigation
SELECT mentat.pull(
  '[:person/name :person/age {:person/friends [:person/name]}]',
  10001
);
```

### 6. Time Travel

```sql
-- Query the database as it was at transaction 1000
SELECT mentat.q('[:find ?name :where [?e :person/name ?name]]', '[]', 1000, NULL);

-- View transaction log
SELECT mentat.log('default', 1000, 1010);

-- What changed between two transactions?
SELECT mentat.diff('default', 1000, 1005);
```

## Next Steps

- [Architecture](./architecture.md) -- understand how pg_mentat stores and queries data
- [Datalog Query Language](./datalog.md) -- complete query syntax reference
- [SQL Function Reference](./sql-functions.md) -- all available SQL functions
- [Schema Reference](./schema.md) -- attribute definitions and value types
