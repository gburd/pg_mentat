# Introduction

**pg_mentat** is a PostgreSQL extension that implements Datomic's data model and query language entirely within PostgreSQL. It provides an entity-attribute-value store with immutable history, Datalog query execution, a pull API, temporal queries, and ACID transaction processing -- all accessible through standard SQL function calls.

## What pg_mentat Provides

- **Datomic-compatible schema** -- define attributes with value types, cardinalities, uniqueness constraints, and component semantics
- **Datalog queries** -- full `:find`, `:where`, `:in`, `:with` support compiled to efficient SQL
- **Pull API** -- declarative entity retrieval with nested navigation, recursion, and cycle detection
- **Time travel** -- query the database as-of any past transaction, or view full history
- **Immutable audit trail** -- every fact ever asserted or retracted is retained
- **Multi-store isolation** -- run multiple independent databases within a single PostgreSQL instance
- **mentatd HTTP server** -- Datomic client protocol-compatible HTTP interface

## Key Facts

| Property | Value |
|----------|-------|
| Version | 1.3.0 |
| PostgreSQL | 13, 14, 15, 16 (default), 17, 18 |
| Rust | 1.88+ |
| pgrx | 0.17.0 |
| License | Apache-2.0 |
| Repository | <https://github.com/gburd/pg_mentat> |

## Why pg_mentat?

Datomic pioneered the idea of an immutable, accumulate-only database with first-class time semantics. pg_mentat brings that model into PostgreSQL, which means:

1. **No separate infrastructure** -- your EAV store lives in the same database as your relational data.
2. **SQL interop** -- join Datalog query results with regular tables, use PostgreSQL's full ecosystem of tools.
3. **Operational simplicity** -- backup, replication, monitoring, and access control all use standard PostgreSQL mechanisms.
4. **Performance** -- queries compile to native SQL executed through PostgreSQL's SPI, benefiting from its query optimizer and index infrastructure.

## How It Works

pg_mentat stores facts (datoms) in nine type-specific narrow tables, each with four covering indexes (EAVT, AEVT, AVET, VAET). Datalog queries are parsed from EDN, compiled into SQL with parameterized bindings, and executed via PostgreSQL's Server Programming Interface (SPI). Results are returned as JSON.

```sql
-- Install the extension
CREATE EXTENSION pg_mentat;

-- Define a schema attribute
SELECT mentat.t('[
  {:db/ident       :person/name
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Assert a fact
SELECT mentat.t('[{:person/name "Alice"}]');

-- Query
SELECT mentat.q('[:find ?name :where [?e :person/name ?name]]');

-- Pull all attributes for the entity
SELECT mentat.pull('[*]', 10001);
```

> **Note:** `mentat.t`, `mentat.q`, and `mentat.pull` are convenience aliases for the
> underlying `mentat.mentat_transact`, `mentat.mentat_query`, and `mentat.mentat_pull`
> functions. The `mentat_` prefix exists so names read naturally when the extension is
> installed into a non-default schema (e.g., `myapp.mentat_query(...)`). See
> [SQL Function Reference](sql-functions.md) for the full mapping.
