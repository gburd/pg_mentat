# EDN Type Guide

pg_mentat provides a native PostgreSQL `edn` type for storing and manipulating [Extensible Data Notation](https://github.com/edn-format/edn) values directly in PostgreSQL. This guide covers the type system, I/O functions, operators, and EDN manipulation functions.

## Table of Contents

- [Overview](#overview)
- [Creating EDN Values](#creating-edn-values)
- [Supported EDN Forms](#supported-edn-forms)
- [I/O Functions](#io-functions)
- [Operators](#operators)
- [Collection Access Functions](#collection-access-functions)
- [Type Predicate Functions](#type-predicate-functions)
- [Collection Operation Functions](#collection-operation-functions)
- [Using EDN in Tables](#using-edn-in-tables)
- [Using EDN in Queries](#using-edn-in-queries)
- [Limits and Validation](#limits-and-validation)

---

## Overview

The `edn` type is a first-class PostgreSQL type implemented via pgrx. It supports:

- Text I/O: Parse EDN text on input, print EDN text on output
- Binary I/O: Efficient binary serialization for COPY and replication
- Equality comparison: `=` and `<>` operators (auto-derived via `PostgresEq`)
- Collection access: `edn_get`, `edn_nth`, `edn_count`
- Type checking: `edn_is_nil`, `edn_is_map`, etc.
- Collection operations: `edn_contains`, `edn_keys`, `edn_values`

The type is stored internally as a serialized EDN value. All functions that operate on it are marked `IMMUTABLE` and `PARALLEL SAFE`, making them safe for use in indexes, generated columns, and parallel query plans.

---

## Creating EDN Values

Cast a text literal to the `edn` type:

```sql
-- Scalar values
SELECT 'nil'::edn;
SELECT 'true'::edn;
SELECT '42'::edn;
SELECT '3.14'::edn;
SELECT '"hello world"'::edn;
SELECT ':my/keyword'::edn;

-- Collections
SELECT '[1 2 3]'::edn;
SELECT '(a b c)'::edn;
SELECT '#{1 2 3}'::edn;
SELECT '{:name "Alice" :age 30}'::edn;

-- Nested structures
SELECT '{:person {:name "Alice" :hobbies ["reading" "chess"]}}'::edn;
```

---

## Supported EDN Forms

| Form | Syntax | Example |
|------|--------|---------|
| Nil | `nil` | `'nil'::edn` |
| Boolean | `true`, `false` | `'true'::edn` |
| Integer | digits | `'42'::edn` |
| Float | digits.digits | `'3.14'::edn` |
| String | `"..."` | `'"hello"'::edn` |
| Keyword | `:name` or `:ns/name` | `':person/name'::edn` |
| Symbol | `name` or `ns/name` | `'my-symbol'::edn` |
| Vector | `[...]` | `'[1 2 3]'::edn` |
| List | `(...)` | `'(+ 1 2)'::edn` |
| Set | `#{...}` | `'#{1 2 3}'::edn` |
| Map | `{k v ...}` | `'{:a 1 :b 2}'::edn` |
| Tagged | `#tag value` | `'#inst "2024-01-01"'::edn` |
| UUID | `#uuid "..."` | `'#uuid "550e8400-e29b-41d4-a716-446655440000"'::edn` |

---

## I/O Functions

These functions handle conversion between text/binary representations and the internal `edn` type. They are called implicitly by PostgreSQL during casts and COPY operations.

| Function | Signature | Description |
|----------|-----------|-------------|
| `edn_in` | `(text) -> edn` | Parse EDN text into an edn value |
| `edn_out` | `(edn) -> text` | Convert edn value to EDN text |
| `edn_send` | `(edn) -> bytea` | Serialize edn value for binary transmission |
| `edn_recv` | `(bytea) -> edn` | Deserialize edn value from binary data |

```sql
-- Explicit parse
SELECT edn_in('{:name "Alice"}');

-- Explicit render
SELECT edn_out('{:name "Alice"}'::edn);

-- These are normally called implicitly via casts
SELECT '{:name "Alice"}'::edn;  -- calls edn_in
```

---

## Operators

The `edn` type supports equality operators derived from `PostgresEq`:

| Operator | Description | Example |
|----------|-------------|---------|
| `=` | Equality | `'42'::edn = '42'::edn` -> `true` |
| `<>` | Inequality | `'42'::edn <> '43'::edn` -> `true` |

```sql
SELECT '[:a :b :c]'::edn = '[:a :b :c]'::edn;  -- true
SELECT '{:x 1}'::edn <> '{:x 2}'::edn;          -- true
```

Equality is structural: two edn values are equal if they represent the same EDN data.

---

## Collection Access Functions

### edn_get

Retrieve a value from a map by key.

```sql
edn_get(map edn, key edn) -> edn (or NULL)
```

```sql
SELECT edn_get('{:name "Alice" :age 30}'::edn, ':name'::edn);
-- Returns: "Alice"

SELECT edn_get('{:name "Alice"}'::edn, ':missing'::edn);
-- Returns: NULL

-- Nested access (chained calls)
SELECT edn_get(
  edn_get('{:person {:name "Alice" :age 30}}'::edn, ':person'::edn),
  ':name'::edn
);
-- Returns: "Alice"
```

Returns NULL if the value is not a map or the key is not found.

### edn_nth

Retrieve a value from a vector by 0-based index.

```sql
edn_nth(vec edn, index BIGINT) -> edn (or NULL)
```

```sql
SELECT edn_nth('[10 20 30]'::edn, 0);  -- Returns: 10
SELECT edn_nth('[10 20 30]'::edn, 2);  -- Returns: 30
SELECT edn_nth('[10 20 30]'::edn, 5);  -- Returns: NULL (out of bounds)
SELECT edn_nth('[10 20 30]'::edn, -1); -- Returns: NULL (negative index)
```

Returns NULL if the value is not a vector or the index is out of bounds.

### edn_count

Get the number of elements in a collection.

```sql
edn_count(value edn) -> BIGINT
```

```sql
SELECT edn_count('[1 2 3]'::edn);        -- Returns: 3
SELECT edn_count('#{:a :b}'::edn);       -- Returns: 2
SELECT edn_count('{:a 1 :b 2}'::edn);    -- Returns: 2
SELECT edn_count('(x y z w)'::edn);      -- Returns: 4
SELECT edn_count('"not a collection"'::edn); -- Returns: 0
```

Returns 0 for non-collection values.

---

## Type Predicate Functions

Each function takes an `edn` value and returns `boolean`.

| Function | Returns true for |
|----------|-----------------|
| `edn_is_nil(value edn)` | `nil` |
| `edn_is_boolean(value edn)` | `true` or `false` |
| `edn_is_integer(value edn)` | Integer values like `42` |
| `edn_is_float(value edn)` | Float values like `3.14` |
| `edn_is_text(value edn)` | String values like `"hello"` |
| `edn_is_keyword(value edn)` | Keywords like `:name` |
| `edn_is_vector(value edn)` | Vectors like `[1 2 3]` |
| `edn_is_list(value edn)` | Lists like `(1 2 3)` |
| `edn_is_set(value edn)` | Sets like `#{1 2 3}` |
| `edn_is_map(value edn)` | Maps like `{:a 1}` |

```sql
-- Filter rows by EDN type
SELECT * FROM my_table
WHERE edn_is_map(data) AND edn_count(data) > 0;

-- Type dispatch
SELECT
  CASE
    WHEN edn_is_integer(val) THEN 'number'
    WHEN edn_is_text(val) THEN 'string'
    WHEN edn_is_map(val) THEN 'object'
    WHEN edn_is_vector(val) THEN 'array'
    ELSE 'other'
  END AS val_type
FROM my_values;
```

---

## Collection Operation Functions

### edn_contains

Check if a collection contains a specific element.

```sql
edn_contains(collection edn, element edn) -> boolean
```

```sql
SELECT edn_contains('[1 2 3]'::edn, '2'::edn);       -- true
SELECT edn_contains('#{:a :b :c}'::edn, ':b'::edn);   -- true
SELECT edn_contains('{:x 1 :y 2}'::edn, ':x'::edn);   -- true (checks keys)
SELECT edn_contains('(a b c)'::edn, 'b'::edn);         -- true
SELECT edn_contains('[1 2 3]'::edn, '4'::edn);          -- false
```

For maps, `edn_contains` checks whether the element exists as a key.

### edn_keys

Extract keys from a map as a vector.

```sql
edn_keys(map edn) -> edn (or NULL)
```

```sql
SELECT edn_keys('{:name "Alice" :age 30}'::edn);
-- Returns: [:name :age]
```

Returns NULL for non-map values.

### edn_values

Extract values from a map as a vector.

```sql
edn_values(map edn) -> edn (or NULL)
```

```sql
SELECT edn_values('{:name "Alice" :age 30}'::edn);
-- Returns: ["Alice" 30]
```

Returns NULL for non-map values.

---

## Using EDN in Tables

The `edn` type can be used as a column type in regular PostgreSQL tables.

```sql
CREATE TABLE config (
  key TEXT PRIMARY KEY,
  value edn NOT NULL
);

INSERT INTO config VALUES
  ('database', '{:host "localhost" :port 5432}'::edn),
  ('features', '[:auth :logging :metrics]'::edn),
  ('version', '42'::edn);

-- Query with EDN functions
SELECT key, edn_get(value, ':host'::edn)
FROM config
WHERE key = 'database';

-- Filter by type
SELECT key FROM config WHERE edn_is_vector(value);
```

### Indexing EDN columns

Since `edn` supports equality, you can create B-tree indexes for exact-match lookups:

```sql
CREATE INDEX idx_config_value ON config (value);
```

For filtered queries based on EDN functions, use expression indexes:

```sql
-- Index on a specific key extraction
CREATE INDEX idx_config_host ON config (edn_get(value, ':host'::edn))
  WHERE edn_is_map(value);
```

---

## Using EDN in Queries

### Storing structured data alongside Mentat

```sql
-- Store query templates as EDN
CREATE TABLE saved_queries (
  name TEXT PRIMARY KEY,
  query edn NOT NULL,
  description TEXT
);

INSERT INTO saved_queries VALUES (
  'active-engineers',
  '[:find ?name ?email
    :where
    [?e :person/name ?name]
    [?e :person/email ?email]
    [?e :person/department ?d]
    [?d :dept/name "Engineering"]]'::edn,
  'All engineers with name and email'
);
```

### Passing EDN through SQL pipelines

```sql
-- Build EDN dynamically
SELECT ('{:name "' || name || '" :count ' || count || '}')::edn
FROM (
  SELECT 'test' AS name, 42 AS count
) sub;
```

---

## Limits and Validation

The `edn` type enforces safety limits during parsing:

| Limit | Value | Description |
|-------|-------|-------------|
| Maximum input size | 10 MB | Prevents memory exhaustion from large inputs |
| Maximum nesting depth | 100 levels | Prevents stack overflow from deeply nested structures |
| Maximum collection size | 1,000,000 elements | Prevents memory exhaustion from huge collections |

Exceeding any limit raises a PostgreSQL error:

```sql
-- This would fail with "EDN nesting depth exceeds maximum of 100"
SELECT ('[' || repeat('[', 101) || repeat(']', 101) || ']')::edn;
```

---

## Function Reference Summary

| Function | Signature | Description |
|----------|-----------|-------------|
| `edn_in` | `(text) -> edn` | Parse EDN text |
| `edn_out` | `(edn) -> text` | Render EDN text |
| `edn_send` | `(edn) -> bytea` | Binary serialize |
| `edn_recv` | `(bytea) -> edn` | Binary deserialize |
| `edn_get` | `(edn, edn) -> edn` | Map key lookup |
| `edn_nth` | `(edn, bigint) -> edn` | Vector index access |
| `edn_count` | `(edn) -> bigint` | Collection size |
| `edn_is_nil` | `(edn) -> bool` | Nil check |
| `edn_is_boolean` | `(edn) -> bool` | Boolean check |
| `edn_is_integer` | `(edn) -> bool` | Integer check |
| `edn_is_float` | `(edn) -> bool` | Float check |
| `edn_is_text` | `(edn) -> bool` | String check |
| `edn_is_keyword` | `(edn) -> bool` | Keyword check |
| `edn_is_vector` | `(edn) -> bool` | Vector check |
| `edn_is_list` | `(edn) -> bool` | List check |
| `edn_is_set` | `(edn) -> bool` | Set check |
| `edn_is_map` | `(edn) -> bool` | Map check |
| `edn_contains` | `(edn, edn) -> bool` | Membership test |
| `edn_keys` | `(edn) -> edn` | Map keys as vector |
| `edn_values` | `(edn) -> edn` | Map values as vector |
