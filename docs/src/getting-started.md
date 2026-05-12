# Getting Started

## Installation

### From Source

Prerequisites:
- Rust 1.88+ (install via [rustup](https://rustup.rs/))
- PostgreSQL 15-17 with development headers
- LLVM/Clang development libraries (for pgrx bindgen)
- `pkg-config`, `build-essential` (or equivalent)

On Debian/Ubuntu:

```bash
sudo apt-get install -y postgresql-16 postgresql-server-dev-16 \
    libpq-dev build-essential pkg-config libclang-dev clang
```

On Fedora/RHEL:

```bash
sudo dnf install -y postgresql16-server postgresql16-devel \
    clang-devel llvm-devel pkg-config
```

On macOS (Homebrew):

```bash
brew install postgresql@16 llvm pkg-config
```

Then build and install:

```bash
git clone https://github.com/gburd/pg_mentat.git
cd pg_mentat

# Install cargo-pgrx (the PostgreSQL extension build tool)
cargo install --locked cargo-pgrx --version 0.17.0

# Tell pgrx where your PostgreSQL is installed
cargo pgrx init --pg16 $(which pg_config)

# Build and install the extension into your PostgreSQL
cd pg_mentat
cargo pgrx install --release --no-default-features --features pg16
```

### Docker

```bash
docker build -t pg_mentat .
docker run -d --name pg_mentat \
  -e POSTGRES_PASSWORD=secret \
  -p 5432:5432 \
  pg_mentat
```

Or use Docker Compose for the full stack (pg_mentat + mentatd + Prometheus + Grafana):

```bash
docker compose -f docker/docker-compose.yml up -d
```

### From Source (Nix)

If you use Nix, the provided flake handles all dependencies:

```bash
git clone https://github.com/gburd/pg_mentat.git
cd pg_mentat
nix develop

# Everything is pre-configured — just build
cd pg_mentat
cargo pgrx install --release --no-default-features --features pg16
```

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
