# Datomic Compatibility

pg_mentat implements the core Datomic data model and query language as a PostgreSQL extension. This document reflects the actual implementation status of the current codebase.

## Overview

pg_mentat provides Datomic-compatible schema, transactions, and Datalog queries via SQL functions. The companion `mentatd` server exposes an HTTP API compatible with Datomic client libraries.

---

## Transaction Operations

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| `:db/add` | Yes | Yes | **Complete** |
| `:db/retract` | Yes | Yes | **Complete** |
| `:db/retractEntity` | Yes | Yes | **Complete** -- cascade via `:db/isComponent` |
| `:db.fn/cas` | Yes | Yes | **Complete** -- compare-and-swap |
| `with` (speculative) | Yes | Yes | **Complete** -- `mentat_with()` |
| Map notation | Yes | Yes | **Complete** |
| Tempids | Yes | Yes | **Complete** |
| Lookup refs | Yes | Yes | **Complete** |
| Upsert (unique/identity) | Yes | Yes | **Complete** |
| Tx metadata (:db/txInstant) | Yes | Yes | **Complete** |
| Negative tempids | Yes | No | N/A -- use string tempids |
| `:db/fn` (database functions) | Yes | No | **Not Planned** -- security risk |

---

## Query Operations

### Find Specifications

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| Relation `[:find ?x ?y]` | Yes | Yes | **Complete** |
| Collection `[:find [?x ...]]` | Yes | Yes | **Complete** |
| Tuple `[:find [?x ?y]]` | Yes | Yes | **Complete** |
| Scalar `[:find ?x .]` | Yes | Yes | **Complete** |

### Where Clauses

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| Basic patterns | Yes | Yes | **Complete** |
| NOT clauses | Yes | Yes | **Complete** -- NOT EXISTS subquery |
| NOT-JOIN | Yes | Yes | **Complete** |
| OR clauses | Yes | Yes | **Complete** -- UNION |
| OR-JOIN | Yes | Yes | **Complete** |

### Input Bindings

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| Scalar `:in ?x` | Yes | Yes | **Complete** |
| Collection `:in [?x ...]` | Yes | Yes | **Complete** |
| Tuple `:in [?x ?y]` | Yes | Yes | **Complete** |
| Relation `:in [[?x ?y]]` | Yes | Yes | **Complete** |

### Query Functions

| Function | Datomic | pg_mentat | Status |
|----------|---------|-----------|--------|
| `get-else` | Yes | Yes | **Complete** -- LEFT JOIN + COALESCE |
| `missing?` | Yes | Yes | **Complete** -- NOT EXISTS subquery |
| `ground` | Yes | Yes | **Complete** -- constant binding |
| `get-some` | Yes | No | Not implemented |
| `tuple` | Yes | No | Not implemented |
| `untuple` | Yes | No | Not implemented |

### Predicates

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| `=`, `!=` | Yes | Yes | **Complete** |
| `<`, `>`, `<=`, `>=` | Yes | Yes | **Complete** |
| Arithmetic (`+`, `-`, `*`, `/`) | Yes | Yes | **Complete** |
| `like` / `ilike` | N/A | Yes | **Extension** -- PostgreSQL LIKE/ILIKE |
| Type-safety guard | Yes | Yes | **Complete** -- mismatched types produce empty set |

### Aggregates

| Aggregate | Datomic | pg_mentat | Status |
|-----------|---------|-----------|--------|
| `count` | Yes | Yes | **Complete** |
| `count-distinct` | Yes | Yes | **Complete** |
| `sum` | Yes | Yes | **Complete** |
| `min` | Yes | Yes | **Complete** |
| `max` | Yes | Yes | **Complete** |
| `avg` | Yes | Yes | **Complete** |
| `sample` | Yes | Yes | **Complete** |
| `median` | Yes | No | Not implemented |
| `variance` | Yes | No | Not implemented |
| `stddev` | Yes | No | Not implemented |

### Rules

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| Named rules | Yes | Yes | **Complete** |
| Recursive rules | Yes | Yes | **Complete** -- WITH RECURSIVE CTE |
| Rule sets (multiple heads) | Yes | Yes | **Complete** |
| Rules with predicates | Yes | Yes | **Complete** |

---

## Pull API

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| Attribute selection | Yes | Yes | **Complete** |
| Wildcard `[*]` | Yes | Yes | **Complete** |
| Reverse lookup `[:_attr]` | Yes | Yes | **Complete** |
| Nested/map specs | Yes | Yes | **Complete** |
| Recursion `{:attr ...}` | Yes | Yes | **Complete** -- cycle detection |
| Bounded recursion `{:attr N}` | Yes | Yes | **Complete** |
| Default values | Yes | Yes | **Complete** |
| Rename (`:as`) | Yes | Yes | **Complete** |
| Limit (`:limit N`) | Yes | Yes | **Complete** |
| Component auto-expand | Yes | Yes | **Complete** |
| `pull-many` | Yes | Yes | **Complete** |
| Namespace wildcard | Yes | No | Not implemented |

---

## Schema

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| `:db/ident` | Yes | Yes | **Complete** |
| `:db/valueType` | Yes | Yes | **Complete** (9 types) |
| `:db/cardinality` | Yes | Yes | **Complete** (one/many) |
| `:db/unique` (value) | Yes | Yes | **Complete** |
| `:db/unique` (identity) | Yes | Yes | **Complete** + upsert |
| `:db/index` | Yes | Yes | **Complete** |
| `:db/fulltext` | Yes | Yes | **Complete** -- BM25 scoring |
| `:db/isComponent` | Yes | Yes | **Complete** -- cascade retraction |
| `:db/noHistory` | Yes | Yes | **Complete** |
| `:db/doc` | Yes | Yes | **Complete** |
| Schema alteration | Yes | Yes | **Complete** |

### Value Types

| Type | Datomic | pg_mentat | Storage |
|------|---------|-----------|---------|
| `:db.type/boolean` | Yes | Yes | `BOOLEAN` in `datoms_boolean_new` |
| `:db.type/long` | Yes | Yes | `BIGINT` in `datoms_long_new` |
| `:db.type/double` | Yes | Yes | `DOUBLE PRECISION` in `datoms_double_new` |
| `:db.type/string` | Yes | Yes | `TEXT` in `datoms_text_new` |
| `:db.type/keyword` | Yes | Yes | `TEXT` in `datoms_keyword_new` |
| `:db.type/ref` | Yes | Yes | `BIGINT` in `datoms_ref_new` |
| `:db.type/instant` | Yes | Yes | `TIMESTAMPTZ` in `datoms_instant_new` |
| `:db.type/uuid` | Yes | Yes | `UUID` in `datoms_uuid_new` |
| `:db.type/bytes` | Yes | Yes | `BYTEA` in `datoms_bytes_new` |
| `:db.type/bigint` | Yes | No | Not implemented |
| `:db.type/bigdec` | Yes | No | Not implemented |
| `:db.type/uri` | Yes | No | **Not Planned** -- use string |
| `:db.type/tuple` | Yes | No | **Not Planned** -- Datomic Cloud only |

---

## Time-Travel

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| `as-of` | Yes | Yes | **Complete** -- `mentat_as_of()` |
| `since` | Yes | Yes | **Complete** -- `mentat_since()` |
| `history` | Yes | Yes | **Complete** -- `mentat_history()` |
| `tx-range` | Yes | Yes | **Complete** -- `mentat_tx_range()` |
| Basis-t | Yes | Yes | **Complete** -- MAX(tx) query |

---

## Excision

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| Entity excision | Yes | Yes | **Complete** -- `mentat_excise()` |
| Partition gating | Yes | Yes | **Complete** -- `allow_excision` flag |
| Schema protection | Yes | Yes | **Complete** -- entid < 10000 blocked |
| Dangling ref check | Yes | Yes | **Complete** |
| Excision log | N/A | Yes | **Extension** -- audit trail |

---

## Subscriptions / tx-report-queue

| Feature | Datomic | pg_mentat | Status |
|---------|---------|-----------|--------|
| tx-report-queue | Yes | Yes | **Complete** -- LISTEN/NOTIFY triggers |
| Subscribe by query | No | Yes | **Extension** -- `mentat_subscribe()` |
| Unsubscribe | Yes | Yes | **Complete** -- `mentat_unsubscribe()` |

---

## mentatd HTTP Protocol

| Feature | Datomic Client | mentatd | Status |
|---------|---------------|---------|--------|
| list-databases | Yes | Yes | **Complete** |
| create-database | Yes | Yes | **Complete** |
| delete-database | Yes | Yes | **Complete** |
| connect | Yes | Yes | **Complete** |
| transact | Yes | Yes | **Complete** |
| query | Yes | Yes | **Complete** |
| pull | Yes | Yes | **Complete** |
| entity | Yes | Yes | **Complete** |
| EDN serialization | Yes | Yes | **Complete** |
| Transit+JSON | Yes | Yes | **Complete** |
| CORS | N/A | Yes | **Complete** |
| HTTP/2 | Yes | No | Not implemented |
| Streaming responses | Yes | No | Not implemented |

---

## Known Incompatibilities

| Feature | Reason | Workaround |
|---------|--------|------------|
| `:db/fn` | Security risk -- arbitrary code execution | Application logic |
| Peer mode | Architecture incompatible (embedded JVM) | Use mentatd HTTP API |
| Datomic Analytics | Proprietary | PostgreSQL analytics tools |
| Negative tempids | Not supported | Use string tempids |
| Peer cache | No equivalent | Application-layer caching |

---

## Migration from Datomic

Schema definitions transfer directly. Remove `#db/id` partition references:

```clojure
;; Datomic
[{:db/id #db/id[:db.part/db]
  :db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one
  :db.install/_attribute :db.part/db}]

;; pg_mentat (same minus partition boilerplate)
[{:db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one}]
```

Queries work unchanged. Transactions work unchanged. Pull patterns work unchanged.
