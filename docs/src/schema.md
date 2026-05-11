# Schema Reference

In pg_mentat, schema is data. Attributes are defined by transacting schema entities with special built-in attributes. There is no DDL -- all schema changes are regular transactions.

## Defining Attributes

An attribute is defined by transacting a map with `:db/ident` and at minimum `:db/valueType` and `:db/cardinality`:

```sql
SELECT mentat_transact('[
  {:db/ident       :person/name
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one}
]');
```

Multiple attributes can be defined in a single transaction:

```sql
SELECT mentat_transact('[
  {:db/ident       :person/name
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique      :db.unique/identity
   :db/doc         "A persons full name"}

  {:db/ident       :person/age
   :db/valueType   :db.type/long
   :db/cardinality :db.cardinality/one
   :db/index       true}

  {:db/ident       :person/email
   :db/valueType   :db.type/string
   :db/cardinality :db.cardinality/many
   :db/unique      :db.unique/value}

  {:db/ident       :person/friends
   :db/valueType   :db.type/ref
   :db/cardinality :db.cardinality/many}

  {:db/ident       :order/line-items
   :db/valueType   :db.type/ref
   :db/cardinality :db.cardinality/many
   :db/isComponent true}
]');
```

## Schema Attributes

### Required

| Attribute | Type | Description |
|-----------|------|-------------|
| `:db/ident` | keyword | Namespaced keyword identifying the attribute (e.g., `:person/name`) |
| `:db/valueType` | ref | The type of values this attribute holds |
| `:db/cardinality` | ref | Whether the attribute holds one or many values |

### Optional

| Attribute | Type | Default | Description |
|-----------|------|---------|-------------|
| `:db/unique` | ref | none | Uniqueness constraint |
| `:db/index` | boolean | false | Whether to maintain an AVET index for this attribute |
| `:db/fulltext` | boolean | false | Enable full-text search (BM25 scoring) |
| `:db/isComponent` | boolean | false | Component semantics (cascade retract, auto-expand in pull) |
| `:db/noHistory` | boolean | false | Do not retain history for this attribute |
| `:db/doc` | string | none | Documentation string |

## Value Types

pg_mentat supports nine value types, each stored in a dedicated narrow table with native PostgreSQL types:

| Value Type | PostgreSQL Type | Storage Table | Description |
|------------|----------------|---------------|-------------|
| `:db.type/boolean` | `BOOLEAN` | `datoms_boolean_new` | true/false |
| `:db.type/long` | `BIGINT` | `datoms_long_new` | 64-bit integer |
| `:db.type/double` | `DOUBLE PRECISION` | `datoms_double_new` | IEEE 754 double |
| `:db.type/string` | `TEXT` | `datoms_text_new` | UTF-8 text |
| `:db.type/keyword` | `TEXT` | `datoms_keyword_new` | Namespaced keywords (stored as text) |
| `:db.type/ref` | `BIGINT` | `datoms_ref_new` | Reference to another entity |
| `:db.type/instant` | `TIMESTAMPTZ` | `datoms_instant_new` | Timestamp with timezone |
| `:db.type/uuid` | `UUID` | `datoms_uuid_new` | 128-bit UUID |
| `:db.type/bytes` | `BYTEA` | `datoms_bytes_new` | Binary data |

### Type Examples

```sql
SELECT mentat_transact('[
  ;; Boolean
  {:db/ident :user/active :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}

  ;; Long (integer)
  {:db/ident :user/login-count :db/valueType :db.type/long :db/cardinality :db.cardinality/one}

  ;; Double (floating point)
  {:db/ident :sensor/reading :db/valueType :db.type/double :db/cardinality :db.cardinality/one}

  ;; String
  {:db/ident :article/body :db/valueType :db.type/string :db/cardinality :db.cardinality/one}

  ;; Keyword
  {:db/ident :item/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}

  ;; Ref (entity reference)
  {:db/ident :item/category :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}

  ;; Instant (timestamp)
  {:db/ident :event/timestamp :db/valueType :db.type/instant :db/cardinality :db.cardinality/one}

  ;; UUID
  {:db/ident :session/id :db/valueType :db.type/uuid :db/cardinality :db.cardinality/one}

  ;; Bytes
  {:db/ident :file/content :db/valueType :db.type/bytes :db/cardinality :db.cardinality/one}
]');
```

## Cardinality

### `:db.cardinality/one`

The attribute holds at most one value per entity. Asserting a new value for an entity implicitly retracts the old value.

```sql
-- Sets name to "Alice"
SELECT mentat_transact('[[:db/add 10001 :person/name "Alice"]]');
-- Implicitly retracts "Alice", asserts "Alicia"
SELECT mentat_transact('[[:db/add 10001 :person/name "Alicia"]]');
```

### `:db.cardinality/many`

The attribute holds a set of values per entity. Asserting a value adds to the set; explicit retraction is required to remove values.

```sql
SELECT mentat_transact('[
  [:db/add 10001 :person/email "alice@work.com"]
  [:db/add 10001 :person/email "alice@home.com"]
]');
-- Entity 10001 now has both emails
```

## Uniqueness

### `:db.unique/value`

No two entities can have the same value for this attribute. Attempting to assert a duplicate value raises an error.

```sql
{:db/ident :person/ssn
 :db/valueType :db.type/string
 :db/cardinality :db.cardinality/one
 :db/unique :db.unique/value}
```

### `:db.unique/identity`

Same uniqueness guarantee as `:db.unique/value`, but additionally enables **upsert** semantics: if a transaction asserts a value that already exists, the existing entity is reused instead of creating a new one.

```sql
{:db/ident :person/email
 :db/valueType :db.type/string
 :db/cardinality :db.cardinality/one
 :db/unique :db.unique/identity}
```

```sql
-- Creates entity if email is new; updates existing entity if email exists
SELECT mentat_transact('[
  {:person/email "alice@example.com"
   :person/name "Alice Updated"}
]');
```

## Component Attributes

When `:db/isComponent` is true on a ref attribute:

1. **Cascade retraction** -- retracting the parent entity also retracts all component children (recursively)
2. **Pull auto-expansion** -- `[*]` pull patterns automatically expand component references
3. **Ownership semantics** -- the referenced entity is logically "owned" by the referencing entity

```sql
{:db/ident :order/line-items
 :db/valueType :db.type/ref
 :db/cardinality :db.cardinality/many
 :db/isComponent true}
```

## Fulltext Attributes

Attributes with `:db/fulltext true` support full-text search with BM25 relevance scoring:

```sql
{:db/ident :article/body
 :db/valueType :db.type/string
 :db/cardinality :db.cardinality/one
 :db/fulltext true}
```

## No-History Attributes

Attributes with `:db/noHistory true` do not retain historical values. Only the current assertion is kept; past values are not available via time-travel queries. Use this for high-churn data where history is not valuable (e.g., session tokens, ephemeral state).

```sql
{:db/ident :user/last-seen
 :db/valueType :db.type/instant
 :db/cardinality :db.cardinality/one
 :db/noHistory true}
```

## Schema Alteration

Existing attribute properties can be modified by transacting against the attribute's entity ID:

```sql
-- Add an index to an existing attribute
SELECT mentat_transact('[
  {:db/id :person/age
   :db/index true}
]');
```

**Restrictions:**
- `:db/valueType` cannot be changed after data exists (would require data migration)
- `:db/cardinality` change from many-to-one requires that no entity has multiple values

## Inspecting Schema

```sql
-- View the entire schema
SELECT mentat_schema();

-- Query schema as data
SELECT mentat_query(
  '[:find ?ident ?type
    :where
    [?a :db/ident ?ident]
    [?a :db/valueType ?type]]',
  '{}'
);
```

## Namespacing Conventions

Attribute idents use namespaced keywords by convention:

- `:person/name` -- the `name` attribute in the `person` namespace
- `:order/total` -- the `total` attribute in the `order` namespace
- `:db/ident` -- built-in attributes use the `db` namespace

Namespaces are purely organizational -- they have no runtime semantics. Use them to group related attributes and avoid naming collisions.

## Built-in Attributes

pg_mentat bootstraps these system attributes (entid < 100):

| Ident | Entid | Purpose |
|-------|-------|---------|
| `:db/ident` | 1 | Attribute naming |
| `:db/valueType` | 2 | Type declaration |
| `:db/cardinality` | 3 | One/many declaration |
| `:db/unique` | 4 | Uniqueness constraint |
| `:db/index` | 5 | Index hint |
| `:db/fulltext` | 6 | Fulltext flag |
| `:db/isComponent` | 7 | Component flag |
| `:db/noHistory` | 8 | No-history flag |
| `:db/doc` | 9 | Documentation |
| `:db/txInstant` | 10 | Transaction timestamp |
