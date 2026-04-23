# pg_mentat Code Audit Findings

Audit scope: `transact.rs`, `query.rs`, `pull.rs`, `entity.rs`, `helpers.rs`,
`edn_helpers.rs`, `cache.rs`

Date: 2026-04-23

---

## Critical (P0) -- Fix before v1.0

### 1. SQL injection via predicate string constants

**File:** `pg_mentat/src/functions/query.rs:2039-2042`

`pred_arg_to_sql` interpolates user-supplied constant values directly into
generated SQL using single quotes:

```rust
FnArg::Constant(NonIntegerConstant::Text(s)) => Ok(format!("'{}'", s.as_ref())),
```

If a Datalog query contains a string predicate constant like
`[(= ?name "O'Brien")]`, the generated SQL will be `... = 'O'Brien'`, which
is syntactically broken. A deliberately crafted string could inject arbitrary
SQL.

All other value binding throughout `query.rs` correctly uses parameterized
queries via `SqlBuilder::bind_*`. Only `pred_arg_to_sql` bypasses this.

**Impact:** SQL syntax errors on benign input; potential SQL injection.

**Fix:** Use `SqlBuilder::bind_text` / `bind_bigint` / `bind_bytea` for
predicate constant arguments, the same pattern used everywhere else.

---

### 2. `encode_value` does not encode `double`, `instant`, `uuid`, or `bytes` types

**File:** `pg_mentat/src/functions/transact.rs:931-956`

`encode_value` only handles boolean, integer, string, and keyword. Any attempt
to transact a float (double), instant, UUID, or bytes value will return:

```
:db.error/unsupported-value-type Cannot encode value of type float ...
```

The schema correctly declares these types (`value_type_to_tag` maps all 9), the
query engine decodes them (`build_value_decode_expr`, `decode_typed_value`), and
the type validation in `validate_datom_constraints` checks for them. But
`encode_value` cannot produce the BYTEA for them.

**Impact:** Cannot transact `double`, `instant`, `uuid`, or `bytes` attribute
values. This limits the system to ref, boolean, long, string, and keyword types.

**Fix:** Add encoding branches for `edn::Value::Float` (f64 LE bytes, tag 3),
`edn::Value::Instant` (micros since epoch as i64 LE, tag 4), and the Uuid/Bytes
EDN types (or accept them as tagged literals).

---

### 3. `entity.rs` `decode_value` missing support for double, instant, uuid, bytes

**File:** `pg_mentat/src/functions/entity.rs:62-111`

The `mentat_entity` function's `decode_value` helper only handles tags 0 (ref),
1 (boolean), 2 (long), 7 (string), 8 (keyword). It explicitly returns an error
for tags 3 (double), 4 (instant), 10 (uuid), 11 (bytes):

```
:db.error/unsupported-type Unsupported value type tag: ...
Tags 3=double, 4=instant, 10=uuid, 11=bytes are not yet implemented in mentat_entity.
```

Meanwhile, `pull.rs` and `helpers.rs` both have `decode_typed_value` functions
that handle all 9 types. `entity.rs` should reuse one of those implementations
or at minimum duplicate the logic.

**Impact:** `mentat_entity()` will error on any entity that has a double,
instant, uuid, or bytes attribute, even though the data was stored correctly.

**Fix:** Either call the shared `decode_typed_value` from `helpers.rs` or add
the missing match arms (double: f64 LE, instant: i64 micros, uuid: 16 bytes
formatted, bytes: hex encoded).

---

## High (P1) -- Should fix before v1.0

### 4. Duplicated `decode_typed_value` implementations

**Files:**
- `pg_mentat/src/functions/pull.rs:1108-1195`
- `pg_mentat/src/functions/helpers.rs:208-295`
- `pg_mentat/src/functions/entity.rs:65-111` (partial)
- `pg_mentat/src/functions/edn_helpers.rs:376-458` (`value_to_edn`)

Four independent implementations of value decoding exist. They are mostly
consistent but diverge in subtle ways:

| Aspect | pull.rs | helpers.rs | entity.rs | edn_helpers.rs |
|--------|---------|------------|-----------|----------------|
| double | yes | yes | no | yes |
| instant | as micros | as micros | no | as `#inst N` |
| uuid | formatted | hex | no | `#uuid "hex"` |
| uuid tag | 10 | 9 (!) | n/a | 9 (!) |

Note that `helpers.rs:207` and `edn_helpers.rs:440` use tag 9 for uuid, while
`pull.rs` and `query.rs` use tag 10. The `type_tag` module in both files
defines UUID = 10. Tag 9 in `helpers.rs`/`edn_helpers.rs` is likely a bug
(there is no tag 9 in the canonical mapping).

**Impact:** If a UUID is stored with tag 10 (correct), `helpers.rs` and
`edn_helpers.rs` will fall through to the error arm. If somehow stored with
tag 9, `pull.rs` will error.

**Fix:** Consolidate into a single shared `decode_typed_value` function. Use
tag 10 consistently for UUID. Remove the tag 9 branches.

---

### 5. `unwrap()` calls that could panic on corrupt data

**File:** `pg_mentat/src/functions/transact.rs:1259,1276`

```rust
let id = i64::from_le_bytes(v_bytes.try_into().unwrap());
```

In `format_stored_value`, these `unwrap()` calls will panic if `v_bytes` is not
exactly 8 bytes. The surrounding `if v_bytes.len() == 8` guard protects the
first case, but this relies on programmer discipline. A simpler approach would
use `TryInto` with a match.

**File:** `pg_mentat/src/functions/entity.rs:44`

```rust
let mut arr = existing.as_array().unwrap().clone();
```

This is safe because the preceding `if existing.is_array()` check ensures the
value is an array, but if the code is refactored, the guard could be
accidentally removed.

**File:** `pg_mentat/src/functions/pull.rs:1441,1454,1472,1569`

```rust
let obj = json_val.as_object().expect("result should be an object");
let json_str = serde_json::to_string(&json_val).unwrap();
```

These are in `#[pg_test]` functions, so panics are expected there. Not an issue.

**Impact:** Low risk of panic in production since data lengths are guarded, but
these are defensive-programming violations.

---

### 6. Pagination LIMIT/OFFSET values not parameterized

**File:** `pg_mentat/src/functions/query.rs:434-445`

```rust
sql_query.push_str(&format!(" LIMIT {}", limit));
...
sql_query.push_str(&format!(" OFFSET {}", offset));
```

Limit and offset are `i64` values from the inputs JSON, so they cannot inject
SQL (they are integers). However, they bypass the `SqlBuilder` parameterization
pattern used everywhere else, which is inconsistent.

**Impact:** No actual vulnerability (integers cannot inject SQL), but
inconsistency with the rest of the codebase.

---

### 7. Statement cache unbounded growth

**File:** `pg_mentat/src/functions/query.rs:31-41`

The `STMT_CACHE` grows without bound. Each unique SQL string adds a new entry
with an `OwnedPreparedStatement` (which persists the plan in
`TopMemoryContext`). In a long-running PostgreSQL backend with diverse queries,
this could consume significant memory.

**Impact:** Gradual memory growth in long-lived backends. No upper bound.

**Fix:** Add an LRU eviction policy or a maximum cache size. Alternatively,
expose `mentat_stmt_cache_clear()` as a recommended periodic maintenance
operation and document the behavior.

---

### 8. Schema cache uses `RwLock` + `expect()` -- poisoning risk

**File:** `pg_mentat/src/cache.rs:62-73, 163-167, etc.**

All `RwLock::read()` and `RwLock::write()` calls use `.expect("RwLock poisoned
...")`, which will panic if a previous thread panicked while holding the lock.
In a PostgreSQL extension, a panic in any code path while holding the write lock
will permanently poison the lock for the rest of the backend's lifetime.

**Impact:** A single panic during schema cache operations will make every
subsequent query/transact fail with a panic, requiring a backend restart.

**Fix:** Use `RwLock::read().unwrap_or_else(|e| e.into_inner())` to recover
from poisoned locks, or switch to `parking_lot::RwLock` which does not have
the poisoning concept.

---

## Medium (P2) -- Nice to fix

### 9. `edn_helpers.rs` instant formatting is incomplete

**File:** `pg_mentat/src/functions/edn_helpers.rs:424`

```rust
Ok(format!("#inst {}", micros)) // Simplified - full impl would format timestamp
```

The comment acknowledges this is incomplete. Datomic expects `#inst` values
formatted as ISO-8601 strings: `#inst "2024-01-15T10:30:00.000000Z"`.

**Impact:** `export_edn()` and `batch` operations produce non-standard instant
representations that cannot be re-imported or consumed by Datomic clients.

---

### 10. `format_stored_value` missing double/instant/uuid/bytes branches

**File:** `pg_mentat/src/functions/transact.rs:1254-1300`

`format_stored_value` (used in CAS error messages) handles ref, boolean, long,
string, keyword but falls through to a generic `<type:N bytes>` for double,
instant, uuid, bytes. CAS error messages for those types will be unhelpful.

**Impact:** Poor error messages when CAS fails on non-basic types.

---

### 11. Multiple OR-join clauses not supported

**File:** `pg_mentat/src/functions/query.rs:891-895`

```rust
if or_joins.len() > 1 {
    return Err(":db.error/unsupported-query Multiple OR-join clauses ...");
}
```

Queries with more than one `(or ...)` clause will fail. Datomic supports
arbitrary combinations.

**Impact:** Limits complex queries. Most practical queries use at most one OR.

---

### 12. `Limit::Variable` silently ignored

**File:** `pg_mentat/src/functions/query.rs:1267`

```rust
Limit::Variable(_) => sql,
```

If a Datalog query uses a variable limit (`:limit ?n`), it is silently dropped
-- no limit is applied and no error is returned. Datomic resolves variable
limits from the `:in` bindings.

**Impact:** Queries with variable limits return all results instead of the
expected subset, with no warning.

---

### 13. `export_all_edn()` loads all entity IDs into memory

**File:** `pg_mentat/src/functions/edn_helpers.rs:357-373`

`export_all_edn()` collects all distinct entity IDs into a `Vec<i64>`, then
calls `export_edn` which issues one SPI query per entity. On a large database
this will be extremely slow and memory-intensive.

**Impact:** Production risk on large datasets. The function has a doc comment
warning ("can be very large"), but there is no pagination or streaming.

---

### 14. `batch` keyword matching uses empty namespace check

**File:** `pg_mentat/src/functions/edn_helpers.rs:58`

```rust
edn::Value::Keyword(kw) if kw.namespace() == Some("") => match kw.name() {
```

This checks `namespace() == Some("")`, but unqualified keywords in EDN (like
`:query`) typically have `namespace() == None`. This means the batch function
may never match operation keywords unless the EDN parser represents plain
keywords with an empty-string namespace.

**Impact:** Depends on EDN parser behavior. If plain keywords have
`namespace() == None`, the entire batch function is non-functional.

---

## Low (P3) -- Informational

### 15. `get_available_attributes_hint()` issues a DB query during error path

**File:** `pg_mentat/src/functions/transact.rs:99-127`

When an attribute is not found, `get_available_attributes_hint()` runs a new
SPI query to list available attributes for the error message. This is helpful
for debugging but adds a DB round-trip in the error path.

**Impact:** Negligible -- error paths are not performance-critical.

---

### 16. No tests for `mentat_entity` function

**File:** `pg_mentat/src/functions/entity.rs`

The file has no `#[pg_test]` functions (the entire function is untested at the
unit level). `helpers.rs` and `edn_helpers.rs` have only compilation-check
tests (`assert!(true)`).

**Impact:** Regressions in entity lookups would not be caught by unit tests.

---

### 17. Silent continuation on unrecognized vector operations in transact

**File:** `pg_mentat/src/functions/transact.rs:444-448, 505`

When a vector entity's first element is not `:db/add`, `:db/retract`,
`:db/retractEntity`, or `:db.fn/cas`, the code executes `continue` silently.
Map entities with unrecognized keys also `continue`. No warning or error is
produced.

Datomic would reject unknown operation keywords.

**Impact:** Typos in transaction operations (e.g., `:db/ad` instead of
`:db/add`) are silently ignored, making debugging difficult.

---

## Summary

| Priority | Count | Category |
|----------|-------|----------|
| P0 (Critical) | 3 | SQL injection, missing type encoding, missing type decoding |
| P1 (High) | 5 | Code duplication, unwrap panics, cache design |
| P2 (Medium) | 6 | Missing features, incomplete implementations |
| P3 (Low) | 3 | Testing gaps, silent failures |
| **Total** | **17** | |

### Recommended fix order

1. **P0-1** SQL injection in `pred_arg_to_sql` -- highest risk, straightforward fix
2. **P0-2** `encode_value` missing types -- blocks double/instant/uuid/bytes transact
3. **P0-3** `entity.rs` decode_value gaps -- blocks entity lookups for those types
4. **P1-4** UUID tag inconsistency (9 vs 10) -- data corruption risk
5. **P1-8** RwLock poisoning -- resilience
6. **P2-14** Batch keyword namespace check -- may be completely broken
7. Everything else in priority order
