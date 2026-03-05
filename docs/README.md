# Mentat PostgreSQL Documentation

Complete documentation for pg_mentat extension and mentatd server.

## Quick Links

**Getting Started:**
- [Quickstart Guide](guides/quickstart.md) - 5-minute setup and first query
- [Installation - pg_mentat](installation/pg_mentat.md) - Extension installation
- [Installation - mentatd](installation/mentatd.md) - Server installation

**API Reference:**
- [SQL Functions](api/sql_functions.md) - Complete function reference
- [Datomic Compatibility](api/datomic_compat.md) - What works, what doesn't

**Configuration:**
- [mentatd Configuration](configuration/mentatd_config.md) - Server configuration

**Guides:**
- [Migration Guide](guides/migration_guide.md) - Migrate from Datomic or SQLite

**Architecture:**
- [Datomic Protocol](architecture/datomic_protocol.md) - Wire protocol specification
- [pgrx Design](architecture/pgrx_design.md) - Extension architecture

## What is Mentat?

Mentat is a Datalog database implementation backed by PostgreSQL, providing:

- **Datalog Queries** - Powerful query language with pattern matching
- **Temporal Database** - Built-in time-travel and history tracking
- **EDN Data Type** - Native support for rich data structures
- **PostgreSQL Storage** - Leverage PostgreSQL's reliability and performance
- **Datomic Compatibility** - Protocol-compatible with Datomic clients

## Components

### pg_mentat Extension

PostgreSQL extension providing:
- Custom EDN type for Extensible Data Notation
- SQL functions for Datalog queries and transactions
- Storage schema optimized for temporal queries
- Integration with PostgreSQL query planner

### mentatd Server

HTTP server implementing the Datomic wire protocol:
- Accepts Datomic client connections
- Translates to PostgreSQL queries via pg_mentat
- EDN serialization support
- Compatible with existing Datomic clients

## Quick Setup

```bash
# Install pg_mentat extension
cargo install cargo-pgrx
cargo pgrx init --pg16=$(which pg_config)
cd pg_mentat && cargo pgrx install

# Create database
createdb mentat
psql mentat -c "CREATE EXTENSION pg_mentat;"
```

See [Quickstart Guide](guides/quickstart.md) for complete tutorial.

## Documentation Structure

```
docs/
├── README.md                    # This file
├── installation/
│   ├── pg_mentat.md            # Extension installation
│   ├── mentatd.md              # Server setup
│   └── migration.md            # Legacy migration docs
├── api/
│   ├── sql_functions.md        # SQL API reference
│   └── datomic_compat.md       # Datomic compatibility
├── configuration/
│   └── mentatd_config.md       # Server configuration
├── architecture/
│   ├── datomic_protocol.md     # Wire protocol
│   └── pgrx_design.md          # Extension design
└── guides/
    ├── quickstart.md           # Getting started
    └── migration_guide.md      # Migration guide
```

## Examples

### Schema and Data

```sql
-- Define schema
SELECT mentat.mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Add data
SELECT mentat.mentat_transact('[
  {:db/id "alice" :person/name "Alice Anderson"}
]');

-- Query
SELECT mentat.mentat_query('
  [:find ?name :where [?e :person/name ?name]]
', '{}'::jsonb);
```

See documentation for complete examples.

## Build Jekyll Site (Legacy)

For the legacy Jekyll documentation site:

1. Install [Jekyll](https://jekyllrb.com/docs/installation/)
2. `cd docs`
3. `bundle exec jekyll serve --incremental`
4. Open http://127.0.0.1:4000/

## License

Apache-2.0
