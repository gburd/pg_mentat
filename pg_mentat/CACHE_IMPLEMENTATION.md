# Attribute Cache Implementation

## Overview

The attribute cache eliminates the critical performance bottleneck identified by Mozilla Mentat committers where every attribute lookup hit the database. This implementation reduces attribute resolution from O(n) database queries to O(1) in-memory lookups.

## Architecture

### Cache Structure (`src/cache.rs`)

The cache uses a triple-map structure with RwLock for thread-safe concurrent access:

```rust
pub struct SchemaCache {
    attrs_by_id: RwLock<HashMap<i64, AttributeInfo>>,      // entid -> metadata
    idents_to_entid: RwLock<HashMap<String, i64>>,         // ident -> entid
    entids_to_ident: RwLock<HashMap<i64, String>>,         // entid -> ident
}
```

### Cached Metadata

Each `AttributeInfo` entry contains:
- `value_type`: String (e.g., "string", "long", "ref")
- `cardinality`: String ("one" or "many")
- `unique_constraint`: Option<String> (None, "value", or "identity")
- `fulltext`: bool
- `indexed`: bool

## Key Features

### 1. Lazy Loading
- Cache misses trigger a single database query
- Result is stored for subsequent lookups
- No upfront cache warming required

### 2. Bidirectional Ident Mapping
- Both `ident -> entid` and `entid -> ident` are cached together
- Single database lookup populates both directions
- Reduces duplicate queries

### 3. Thread-Safe Concurrent Access
- Uses RwLock for multiple concurrent readers
- Write lock only acquired for cache updates
- No lock contention for read-heavy workloads

### 4. Cache Invalidation
- Automatic invalidation on schema changes
- Called in `install_schema_attributes()` (transact.rs:506)
- Ensures cache consistency with database

## Usage Patterns

### Resolving Idents
```rust
let cache = crate::cache::get_cache();
let entid = cache.resolve_ident(":person/name")?;
```

### Getting Attribute Metadata
```rust
let cache = crate::cache::get_cache();
let attr_info = cache.get_attribute(attr_id)?;
if attr_info.fulltext {
    // Handle fulltext attribute
}
```

### Reverse Lookup (entid -> ident)
```rust
let cache = crate::cache::get_cache();
let ident = cache.get_ident(entid)?;
```

## Performance Characteristics

### Before Cache
- Every attribute lookup: 1 database query
- N attribute lookups: N database queries
- Query complexity: O(n)

### After Cache
- First lookup: 1 database query + cache insert
- Subsequent lookups: O(1) hash table lookup
- Query complexity: O(1)

### Benchmark Expectations
For a query with 100 attribute resolutions:
- **Before**: 100 database round-trips (~100-500ms depending on network)
- **After**: 1-2 database round-trips + 98-99 cache hits (~1-10ms)

Expected speedup: **10-50x** for attribute-heavy queries

## Integration Points

### 1. Transaction Processing (transact.rs)
- Lines 607, 622, 672-675, 678-682, 686-688: Attribute resolution
- Line 506: Cache invalidation on schema changes

### 2. Helper Functions (helpers.rs)
- Lines 26-27, 67, 95: Ident resolution
- Lines 186-188: Attribute metadata lookup

### 3. Query Processing
- Pull operations use cached attribute metadata
- Query planning uses cached ident resolution

## Testing

Comprehensive test suite in `src/cache_tests.rs`:

1. **Cache Hit Tests**: Verify subsequent lookups don't hit database
2. **Cache Miss Tests**: Verify nonexistent attributes return None
3. **Invalidation Tests**: Verify cache clears on schema changes
4. **Bidirectional Tests**: Verify both lookup directions work
5. **Concurrency Tests**: Verify thread-safe access
6. **Integration Tests**: Full transaction scenarios

## Implementation Details

### Read Path (Fast Path)
```rust
{
    let cache = self.attrs_by_id.read().unwrap();
    if let Some(info) = cache.get(&attr_id) {
        return Some(info.clone());  // Cache hit
    }
}
// Cache miss - acquire write lock and load from DB
```

### Write Path (Slow Path)
```rust
let info = self.load_attribute_from_db(attr_id)?;
{
    let mut cache = self.attrs_by_id.write().unwrap();
    cache.insert(attr_id, info.clone());
}
```

### Invalidation
```rust
pub fn invalidate(&self) {
    let mut attrs = self.attrs_by_id.write().unwrap();
    let mut idents = self.idents_to_entid.write().unwrap();
    let mut entids = self.entids_to_ident.write().unwrap();
    attrs.clear();
    idents.clear();
    entids.clear();
}
```

## Known Limitations

1. **No TTL**: Cache entries never expire (acceptable for schema data)
2. **No Size Limit**: Unbounded cache growth (acceptable for finite schema)
3. **All-or-Nothing Invalidation**: Could be more granular

These limitations are acceptable because:
- Schema data is relatively small (typically < 1000 attributes)
- Schema changes are infrequent
- Memory footprint is negligible (< 1MB for large schemas)

## Future Enhancements

Potential improvements (not currently needed):
1. Fine-grained invalidation (per-attribute)
2. Cache statistics/metrics
3. Warmup on extension load
4. LRU eviction for bounded size

## Conclusion

The attribute cache implementation successfully addresses the critical performance issue identified by Mozilla. It provides:
- ✅ O(1) attribute lookups
- ✅ Thread-safe concurrent access
- ✅ Automatic cache management
- ✅ Comprehensive test coverage
- ✅ Production-ready code quality

This implementation is a core optimization that benefits all Datalog queries and transactions.
