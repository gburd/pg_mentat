# Return Type Fixes Needed

## Problem
Many functions return `JsonB` instead of `Edn`, requiring ugly `::jsonb` casts in SQL:
```sql
SELECT jsonb_pretty(mentat.list_stores()::jsonb);  -- Ugly!
```

Should be:
```sql
SELECT edn_pretty(mentat.list_stores());  -- Clean!
```

## Root Cause
Functions were written to return `JsonB` instead of `Edn`:
```rust
pub fn list_stores() -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>>
```

Should be:
```rust
pub fn list_stores() -> Result<Edn, Box<dyn std::error::Error + Send + Sync>>
```

## Functions That Need Fixing

### store_management.rs
- `list_stores()` → returns JsonB, should return Edn

### materialized_views.rs
- `list_matviews()` → returns JsonB, should return Edn

### subscriptions.rs
- `list_subscriptions()` → returns JsonB, should return Edn

### recursive_queries.rs
- `list_recursive()` → returns JsonB, should return Edn

### time_travel.rs
- `diff()` → returns JsonB, should return Edn
- `log()` → returns JsonB, should return Edn

### query.rs
Already returns JsonB which might be intentional for backwards compat, but should consider:
- Could have `q()` return `Edn` directly
- Keep `mentat_query()` returning JsonB for backwards compat

## Implementation Notes

The `Edn` type is defined in:
- `src/lib.rs` (in the `mentat` pg_schema module)
- Wrapped type: `pub struct Edn { inner: edn::Value }`
- Already has PostgreSQL type support via `#[derive(PostgresType)]`

To convert JsonB to Edn:
```rust
use crate::mentat::Edn;
use serde_json::json;

// Instead of:
Ok(JsonB(json!({ "key": "value" })))

// Do:
let edn_value = edn::Value::Map(/* construct EDN map */);
Ok(Edn::new(edn_value))
```

Or parse from EDN string:
```rust
let edn_str = format!("{{:key \"value\"}}");
let parsed = edn::parse::value(&edn_str)?;
Ok(Edn::new(parsed))
```

## Why This Matters

1. **Type safety**: Edn is the correct semantic type
2. **No casting**: Users don't need `::jsonb` everywhere
3. **Better API**: `edn_pretty()` works directly
4. **Consistency**: Matches the rest of pg_mentat which uses Edn

## Migration Strategy

1. Change return types from `JsonB` to `Edn`
2. Update internal logic to construct `Edn` values
3. Update all SQL examples in docs
4. Update demo scripts
5. Test with: `SELECT edn_pretty(mentat.list_stores())`

## Example Fix

Before:
```rust
#[pg_extern]
pub fn list_stores() -> Result<JsonB, Box<dyn std::error::Error + Send + Sync>> {
    // ...
    Ok(JsonB(json!(stores)))
}
```

After:
```rust
#[pg_extern]
pub fn list_stores() -> Result<Edn, Box<dyn std::error::Error + Send + Sync>> {
    // ...
    let edn_str = format!("[{}]", stores.iter()
        .map(|s| format!("{{:store_name \"{}\" :schema_name \"{}\"}}", s.name, s.schema))
        .collect::<Vec<_>>()
        .join(" "));
    let parsed = edn::parse::value(&edn_str)
        .map_err(|e| format!("Failed to parse EDN: {}", e))?;
    Ok(Edn::new(parsed))
}
```

Or keep using JSON internally and convert:
```rust
// Many ways to bridge JSON → EDN:
// 1. Convert serde_json::Value to edn::Value
// 2. Serialize JSON to string, parse as EDN
// 3. Build EDN Value directly

// The challenge: JSON != EDN
// JSON: {"key": "value"}
// EDN:  {:key "value"}  (keywords, not strings)
```

## Complexity Note

Converting from JSON to EDN properly is non-trivial because:
- JSON string keys → EDN keywords (`:key`)
- JSON `null` → EDN `nil`
- JSON booleans → EDN booleans
- Need to decide which strings become keywords vs strings

**Recommendation**: For list functions, return EDN vectors/maps directly.
For query results, might keep JsonB since the data is already structured as JSON internally.

## Build Required

Cannot test these changes without fixing the build environment.
Nix is setting `CARGO_HOME` to a read-only path, preventing compilation.
