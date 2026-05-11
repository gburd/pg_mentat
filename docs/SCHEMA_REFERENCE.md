# Schema Reference

pg_mentat stores data as immutable datoms in an Entity-Attribute-Value-Transaction (EAVT) model. Every fact in the database is a tuple `(entity, attribute, value, transaction, added)`. Attributes must be defined in the schema before data can be asserted against them.

This document covers attribute definition, value types, cardinality, uniqueness, transaction formats, tempid resolution, transaction functions, schema evolution, and store management.

---

## Attribute Definition

Attributes are defined by transacting special maps with schema-describing keywords. Each attribute requires at minimum `:db/ident`, `:db/valueType`, and `:db/cardinality`.

```sql
SELECT mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');
```

### Schema Attribute Properties

| Keyword | Required | Values | Description |
|---------|----------|--------|-------------|
| `:db/ident` | Yes | Namespaced keyword | Unique name for the attribute (e.g., `:person/name`) |
| `:db/valueType` | Yes | Type keyword | Data type of values (see Value Types below) |
| `:db/cardinality` | Yes | `:db.cardinality/one` or `:db.cardinality/many` | Single-valued or multi-valued |
| `:db/unique` | No | `:db.unique/value` or `:db.unique/identity` | Uniqueness constraint |
| `:db/index` | No | `true` | Create an AVET index for fast value lookups |
| `:db/fulltext` | No | `true` | Enable full-text search (text attributes only) |
| `:db/isComponent` | No | `true` | Component semantics (cascading retraction) |
| `:db/noHistory` | No | `true` | Do not retain history for this attribute |
| `:db/doc` | No | String | Documentation string (stored but not used by the engine) |

Multiple attributes can be defined in a single transaction:

```sql
SELECT mentat_transact('[
  {:db/ident :project/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
  {:db/ident :project/members
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/many
   :db/isComponent true}
  {:db/ident :project/created
   :db/valueType :db.type/instant
   :db/cardinality :db.cardinality/one
   :db/index true}
]');
```

---

## Value Types

pg_mentat supports nine value types, each backed by a dedicated narrow storage table with a native PostgreSQL column type.

| Type Keyword | pg_mentat Type Tag | PostgreSQL Type | Description | EDN Literal |
|---|---|---|---|---|
| `:db.type/ref` | 0 | `BIGINT` | Reference to another entity | Integer entity ID |
| `:db.type/boolean` | 1 | `BOOLEAN` | True or false | `true`, `false` |
| `:db.type/long` | 2 | `BIGINT` | 64-bit signed integer | `42`, `-17` |
| `:db.type/double` | 3 | `DOUBLE PRECISION` | 64-bit IEEE 754 float | `3.14`, `-2.5e10` |
| `:db.type/instant` | 4 | `TIMESTAMPTZ` | Timestamp with timezone | `#inst "2025-01-15T10:30:00Z"` |
| `:db.type/string` | 7 | `TEXT` | UTF-8 string (unlimited length) | `"hello world"` |
| `:db.type/keyword` | 8 | `TEXT` | Namespaced keyword | `:person/active` |
| `:db.type/uuid` | 10 | `UUID` | 128-bit UUID | `#uuid "550e8400-e29b-41d4-a716-446655440000"` |
| `:db.type/bytes` | 11 | `BYTEA` | Binary data | Base64-encoded in JSON |

### Storage Architecture

Each value type has its own table (`mentat.datoms_ref_new`, `mentat.datoms_long_new`, `mentat.datoms_text_new`, etc.) with columns `(store_id, e, a, v, tx, added)`. This narrow-table design eliminates NULL waste and enables type-specific indexing. The query engine resolves attribute types from the schema cache and generates SQL targeting only the appropriate table.

The nine tables are:
- `mentat.datoms_ref_new` -- entity references (BIGINT)
- `mentat.datoms_boolean_new` -- booleans (BOOLEAN)
- `mentat.datoms_long_new` -- integers (BIGINT)
- `mentat.datoms_double_new` -- floating point (DOUBLE PRECISION)
- `mentat.datoms_instant_new` -- timestamps (TIMESTAMPTZ)
- `mentat.datoms_text_new` -- strings (TEXT)
- `mentat.datoms_keyword_new` -- keywords (TEXT)
- `mentat.datoms_uuid_new` -- UUIDs (UUID)
- `mentat.datoms_bytes_new` -- binary data (BYTEA)

Each table has partial indexes predicated on `added = true` in EAVT and AEVT order for efficient scans. A compatibility VIEW named `mentat.datoms` unions all nine tables for backward compatibility, but new code should not depend on it.

### Value Type Examples

```sql
-- String attribute
SELECT mentat_transact('[{:db/ident :item/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]');

-- Reference attribute (points to another entity)
SELECT mentat_transact('[{:db/ident :order/customer :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}]');

-- UUID attribute
SELECT mentat_transact('[{:db/ident :session/id :db/valueType :db.type/uuid :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}]');

-- Instant (timestamp) attribute
SELECT mentat_transact('[{:db/ident :event/occurred-at :db/valueType :db.type/instant :db/cardinality :db.cardinality/one :db/index true}]');

-- Boolean attribute
SELECT mentat_transact('[{:db/ident :user/active :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}]');

-- Bytes attribute
SELECT mentat_transact('[{:db/ident :file/data :db/valueType :db.type/bytes :db/cardinality :db.cardinality/one}]');
```

---

## Cardinality

### :db.cardinality/one

A cardinality-one attribute holds at most one value per entity. Asserting a new value implicitly retracts the old value:

```sql
-- Initial assertion
SELECT mentat_transact('[[:db/add "e1" :person/name "Alice"]]');
-- Subsequent assertion retracts "Alice" and asserts "Alicia"
SELECT mentat_transact('[[:db/add 10001 :person/name "Alicia"]]');
```

### :db.cardinality/many

A cardinality-many attribute can hold multiple values simultaneously:

```sql
SELECT mentat_transact('[
  [:db/add 10001 :person/hobbies "reading"]
  [:db/add 10001 :person/hobbies "hiking"]
  [:db/add 10001 :person/hobbies "cooking"]
]');
```

Duplicate values are idempotent (asserting the same datom twice is a no-op).

To retract a single value from a cardinality-many attribute:

```sql
SELECT mentat_transact('[
  [:db/retract 10001 :person/hobbies "hiking"]
]');
```

---

## Uniqueness

### :db.unique/value

Enforces that no two entities share the same value for this attribute. Attempting to assert a duplicate raises an error.

### :db.unique/identity

Uniqueness with upsert semantics. If a transaction asserts a value for a unique-identity attribute that already exists on another entity, the transaction operates on the existing entity rather than creating a new one.

```sql
-- Define email as unique identity
SELECT mentat_transact('[
  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
]');

-- First insertion creates entity
SELECT mentat_transact('[
  {:person/email "alice@example.com" :person/name "Alice"}
]');

-- Second insertion with same email UPSERTs (updates existing entity)
SELECT mentat_transact('[
  {:person/email "alice@example.com" :person/age 31}
]');
```

---

## Transaction Format

Transactions are EDN vectors containing either maps (for convenience) or explicit assertion/retraction vectors.

### Map Format (Convenience)

```edn
[{:person/name "Alice" :person/age 30}]
```

Each map becomes a set of `:db/add` assertions for a single entity. Include `:db/id` to target a specific entity or provide a temp ID string.

### Vector Format (Explicit)

```edn
[[:db/add "tempid" :person/name "Alice"]
 [:db/add "tempid" :person/age 30]
 [:db/retract 10001 :person/name "Alice"]]
```

| Operation | Syntax |
|-----------|--------|
| Assert | `[:db/add entity attr value]` |
| Retract | `[:db/retract entity attr value]` |

The entity position accepts:
- **String** -- temp ID (resolved to a new entity ID)
- **Integer** -- existing entity ID
- **Lookup ref** -- `[:person/email "alice@example.com"]` (resolves via unique attribute)

---

## Tempid Resolution

String values in the entity position are "temp IDs." Within a single transaction, all occurrences of the same temp ID string resolve to the same entity ID.

```sql
SELECT mentat_transact('[
  {:db/id "alice"
   :person/name "Alice"
   :person/email "alice@example.com"}
  {:db/id "bob"
   :person/name "Bob"
   :person/friend "alice"}
]');
```

Here `"alice"` and `"bob"` each get a permanent entity ID. The `:person/friend` reference using `"alice"` correctly resolves to the same entity.

The transaction report includes a `tempids` map showing the resolution:

```json
{
  "tempids": {"alice": 10001, "bob": 10002},
  "tx-data": [...],
  "db-before": {"basis-t": 1000001},
  "db-after": {"basis-t": 1000002}
}
```

Entity IDs are allocated from PostgreSQL sequences (`mentat.partition_user_seq`) with `CACHE 100` for high-concurrency performance.

---

## Transaction Functions

Transaction functions execute within the transactional context, enabling read-then-write atomic operations.

### :db.fn/retractEntity

Retract all datoms for an entity. If the entity has `:db/isComponent` references, those components are also recursively retracted.

```sql
SELECT mentat_transact('[
  [:db.fn/retractEntity 10001]
]');
```

Also accepted as `:db/retractEntity`.

### :db.fn/cas (Compare-and-Swap)

Atomically update an attribute only if its current value matches an expected value:

```sql
SELECT mentat_transact('[
  [:db.fn/cas 10001 :person/age 30 31]
]');
```

Arguments: `[:db.fn/cas entity-id attribute old-value new-value]`

- If the current value of `:person/age` for entity 10001 is `30`, it is retracted and `31` is asserted.
- If the current value does NOT match `30`, the entire transaction aborts with an error.
- Use `nil` as old-value to assert the attribute must not currently exist.

CAS is essential for optimistic concurrency control in multi-client scenarios.

---

## Schema Evolution

pg_mentat schemas are additive. You can add new attributes at any time without downtime or migration scripts:

```sql
SELECT mentat_transact('[
  {:db/ident :person/phone
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');
```

### What Can Change

- **New attributes**: Always safe. Define and use immediately.
- **Existing attribute properties**: Limited. Changing `:db/valueType` or `:db/cardinality` of an attribute with existing data is NOT supported (would corrupt stored datoms).

### What Cannot Change

- Value type of an existing attribute (e.g., changing `:db.type/string` to `:db.type/long`)
- Cardinality of an attribute with existing data
- Removing an attribute from the schema (attributes are permanent; data can be retracted)

### Migration Pattern

To effectively "change" an attribute's type, create a new attribute with the desired type, migrate the data, and stop using the old attribute:

```sql
-- 1. Create new attribute with desired type
SELECT mentat_transact('[{:db/ident :person/age-v2 :db/valueType :db.type/double :db/cardinality :db.cardinality/one}]');

-- 2. Query old values and insert as new type (application logic)
-- 3. Retract old attribute values from entities
-- 4. Stop referencing :person/age in queries, use :person/age-v2
```

### Schema + Data in One Transaction

pg_mentat uses a three-pass approach allowing a single transaction to both define schema and use it:

```sql
SELECT mentat_transact('[
  {:db/ident :vehicle/make
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:vehicle/make "Toyota"}
]');
```

Pass 1 scans for schema definitions and installs them. Pass 2 resolves all idents (including the just-installed ones) and inserts datoms.

---

## Schema Inspection

### mentat_schema()

Returns all user-defined attributes as JSON:

```sql
SELECT mentat_schema();
```

### mentat.schema table

Direct SQL access to the schema catalog:

```sql
SELECT ident, value_type, cardinality, unique_constraint, indexed, fulltext
FROM mentat.schema
WHERE ident LIKE ':person/%';
```

---

## Store Management

pg_mentat supports multiple isolated stores within a single database. Each store has its own set of datom tables, its own schema, and its own transaction history.

### Create a Store

```sql
SELECT mentat.create_store('analytics', 'Analytics data store');
```

Creates a PostgreSQL schema `mentat_analytics` with full datom infrastructure.

### List Stores

```sql
SELECT mentat.list_stores();
```

Returns JSON array of all stores with metadata.

### Drop a Store

```sql
SELECT mentat.drop_store('analytics');
```

Drops the store schema (`CASCADE`) and removes metadata. The default store cannot be dropped.

### Rename a Store

```sql
SELECT mentat.rename_store('old_name', 'new_name');
```

### Multi-Store Operations

Most functions have store-aware variants:

| Default Store | Named Store |
|---------------|-------------|
| `mentat_transact(edn)` | `mentat.t(store, edn)` |
| `mentat_query(q, inputs)` | `mentat.q(store, q, inputs)` |
| `mentat_pull(pattern, eid)` | `mentat.pull(store, pattern, eid)` |
| `mentat_entity(eid)` | `mentat.entity(store, eid)` |
| `mentat_with(edn)` | `mentat.with(store, edn)` |

---

## Excision (GDPR Compliance)

Unlike standard retraction (which marks datoms as `added=false` but retains them for history), excision permanently removes all traces of an entity:

```sql
SELECT mentat_excise(ARRAY[10001, 10002]);
```

Requirements:
- The entity must not be a schema entity (entid < 10000 are protected)
- The entity's partition must have `allow_excision = true`
- No other entities may reference the target entities (dangling ref check)

To enable excision on the user partition:

```sql
UPDATE mentat.partitions SET allow_excision = true WHERE name = 'db.part/user';
```

---

## Bootstrap Schema

pg_mentat ships with built-in attributes (entids 10-99) that define the schema system itself:

| Entid | Ident | Purpose |
|-------|-------|---------|
| 10 | `:db/ident` | Attribute's keyword name |
| 11 | `:db/valueType` | Value type reference |
| 12 | `:db/cardinality` | Cardinality reference |
| 13 | `:db/unique` | Uniqueness constraint |
| 14 | `:db/index` | Index flag |
| 15 | `:db/fulltext` | Full-text search flag |
| 16 | `:db/isComponent` | Component flag |
| 17 | `:db/noHistory` | No-history flag |
| 50 | `:db/txInstant` | Transaction timestamp |

These are immutable and always present. User entities start at entid 10000 (partition `db.part/user`). Transaction entities start at 1000000 (partition `db.part/tx`).

### Partitions

pg_mentat allocates entity IDs from three partitions:

| Partition | Range | Purpose |
|-----------|-------|---------|
| `db.part/db` | 0 -- 9,999 | Schema and system entities |
| `db.part/user` | 10,000 -- 999,999 | User-defined entities |
| `db.part/tx` | 1,000,000 -- 1,999,999 | Transaction entities |

Entity IDs are allocated from PostgreSQL sequences (`mentat.partition_user_seq`, `mentat.partition_tx_seq`) with `CACHE 100` for lock-free concurrent allocation. You never need to manage IDs manually -- they are assigned during transaction processing.
