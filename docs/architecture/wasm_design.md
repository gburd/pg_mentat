# WASM Runtime Architecture for pg_mentat

## Overview

This document specifies the wasmer integration design for enabling WASM function execution in pg_mentat, allowing dynamic extension of database functionality without JVM dependencies.

## Architecture Decision: wasmer 5.0+

**Chosen Runtime:** wasmer v5.0+ (latest stable)

**Rationale:**
- Production-ready with extensive PostgreSQL usage examples
- Excellent security sandbox (gas metering, memory limits, WASI capabilities)
- Zero-copy memory access for performance
- Support for multiple compilation backends (Cranelift, LLVM, Singlepass)
- Active development and security updates

## Integration Strategy

### Component Architecture

```
PostgreSQL Extension (pg_mentat)
    ├── WASM Module Loader
    │   ├── Module validation (wasmer::Module::validate)
    │   ├── Compilation (with gas metering)
    │   └── Module cache (LRU, configurable size)
    ├── Function Registry
    │   ├── Module → Function mapping
    │   ├── Type signature validation
    │   └── Instance pool
    ├── Execution Engine
    │   ├── Wasmer Store (memory management)
    │   ├── Gas limit enforcement
    │   └── WASI environment (restricted)
    └── SQL API
        ├── mentat_load_wasm(name TEXT, bytes BYTEA)
        ├── mentat_call_wasm(function TEXT, args JSONB)
        └── Transaction functions (automatic registration)
```

## Security Model

### Resource Limits

```rust
// wasmer configuration
let mut store = Store::new(engine);
store.limiter(|_| WasmerLimits {
    memory_size: 64 * 1024 * 1024,      // 64 MB max memory
    table_elements: 10_000,              // Max table size
    instances: 1,                        // Single instance per call
    tables: 1,                          // One table
    memories: 1,                        // One memory
});

// Gas metering (prevents infinite loops)
let metering = Arc::new(Metering::new(10_000_000, |_| 1)); // 10M ops max
config.set_middleware(metering);
```

### WASI Restrictions

**Allowed:**
- Environment variable reads (read-only, filtered)
- Memory allocation within limits
- Computation

**Denied:**
- File system access (no WASI filesystem)
- Network access (no sockets)
- Process spawning
- System calls
- Clock access (may allow deterministic time)

```rust
// Minimal WASI environment
let wasi_env = WasiState::new("wasm-function")
    .finalize()?;

// No directories mounted - pure computation only
```

## Function Registration API

### Module Loading

```sql
-- Load WASM module into extension
SELECT mentat_load_wasm(
    'validators',  -- module name
    pg_read_binary_file('/path/to/validators.wasm')
);
```

```rust
// Internal implementation
#[pg_extern]
fn mentat_load_wasm(module_name: &str, wasm_bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Validate WASM module
    wasmer::Module::validate(&store, wasm_bytes)?;

    // 2. Compile with gas metering
    let module = wasmer::Module::new(&store, wasm_bytes)?;

    // 3. Introspect exports - find callable functions
    let exports: Vec<String> = module.exports()
        .filter_map(|e| {
            if e.ty().is_function() {
                Some(e.name().to_string())
            } else {
                None
            }
        })
        .collect();

    // 4. Store in global registry (thread-safe)
    WASM_MODULES.lock().unwrap().insert(module_name.to_string(), module);

    Ok(())
}
```

### Function Invocation

```sql
-- Call WASM function
SELECT mentat_call_wasm(
    'validators:validate_email',  -- module:function
    '{"email": "test@example.com"}'::jsonb
) AS result;
```

```rust
#[pg_extern]
fn mentat_call_wasm(function_path: &str, args: JsonB) -> Result<JsonB, Box<dyn std::error::Error>> {
    // Parse module:function
    let (module_name, func_name) = function_path.split_once(':')
        .ok_or("Invalid function path")?;

    // Get module from registry
    let modules = WASM_MODULES.lock().unwrap();
    let module = modules.get(module_name)
        .ok_or("Module not found")?;

    // Create instance with gas metering
    let mut store = Store::new_with_limits(&engine, WasmerLimits::default());
    let instance = Instance::new(&mut store, module, &imports! {})?;

    // Get function export
    let func = instance.exports.get_function(func_name)?;

    // Convert JSONB args to WASM types (depends on function signature)
    let wasm_args = convert_json_to_wasm_args(&args, func.ty(&store))?;

    // Execute with timeout
    let result = func.call(&mut store, &wasm_args)?;

    // Check gas usage
    let gas_used = store.gas_used();
    if gas_used > GAS_LIMIT {
        return Err("Gas limit exceeded".into());
    }

    // Convert result back to JSONB
    Ok(convert_wasm_result_to_json(&result))
}
```

## Type System

### WASM ↔ PostgreSQL Type Mapping

| WASM Type | PostgreSQL Type | JSONB Representation |
|-----------|-----------------|---------------------|
| i32       | INTEGER         | number              |
| i64       | BIGINT          | number (string if >2^53) |
| f32       | REAL            | number              |
| f64       | DOUBLE PRECISION | number            |
| (bytes)   | BYTEA           | base64 string       |
| (string)  | TEXT            | string              |

**Complex types:** Use JSONB serialization for structs/records.

```rust
// Example: WASM function signature
// validate_email(email: String) -> bool

// WASM ABI (strings as ptr + len)
#[no_mangle]
pub extern "C" fn validate_email(email_ptr: u32, email_len: u32) -> u32 {
    // Implementation
}

// Helper: allocate/deallocate in WASM memory
#[no_mangle]
pub extern "C" fn allocate(size: u32) -> u32 { /* ... */ }

#[no_mangle]
pub extern "C" fn deallocate(ptr: u32, size: u32) { /* ... */ }
```

## Transaction Functions

WASM functions can be used in transactions for validation/transformation:

```clojure
;; Transaction with WASM validator
[{:db/id #db/id[:db.part/user]
  :user/email "test@example.com"
  :user/email-validator "validators:validate_email"}]
```

**Implementation:**
1. Parse transaction entities
2. Look for `:attribute/validator` patterns
3. Call WASM function with attribute value
4. Accept or reject transaction based on result

```rust
// Transaction function hook
fn validate_entity_with_wasm(entity: &Entity) -> Result<(), TxError> {
    for (attr, value) in entity.iter() {
        if let Some(validator) = schema.get_validator(attr) {
            let result = mentat_call_wasm(
                validator,
                json!({ "value": value })
            )?;

            if !result.as_bool().unwrap_or(false) {
                return Err(TxError::ValidationFailed(attr.clone()));
            }
        }
    }
    Ok(())
}
```

## Performance Considerations

### Module Compilation Caching

```rust
// LRU cache for compiled modules
use lru::LruCache;

static WASM_CACHE: Mutex<LruCache<String, wasmer::Module>> =
    Mutex::new(LruCache::new(NonZeroUsize::new(100).unwrap()));
```

### Instance Pooling

```rust
// Reuse instances where possible
// Create instance pool per module to avoid repeated instantiation
struct InstancePool {
    module: Arc<wasmer::Module>,
    instances: Vec<wasmer::Instance>,
    max_size: usize,
}
```

### Overhead Estimates

- **Module load/compile:** ~10-100ms (one-time, cached)
- **Instance creation:** ~1-5ms (pooled)
- **Function call:** ~10-100µs (near-native with Cranelift)
- **Gas metering:** ~5-10% overhead

**Optimization:** Use Cranelift backend for fast compilation, or LLVM for maximum runtime performance.

## GUC Configuration

```sql
-- Enable/disable WASM
SET mentat.enable_wasm = on;

-- Gas limit (operations)
SET mentat.wasm_gas_limit = 10000000;

-- Memory limit (bytes)
SET mentat.wasm_memory_limit = 67108864;  -- 64 MB

-- Cache size (number of compiled modules)
SET mentat.wasm_cache_size = 100;

-- Compilation backend
SET mentat.wasm_compiler = 'cranelift';  -- or 'llvm', 'singlepass'
```

## Example Use Cases

### 1. Custom Validators

```rust
// validators.wasm (compiled from Rust)
#[no_mangle]
pub extern "C" fn validate_email(email_ptr: u32, email_len: u32) -> u32 {
    let email = unsafe {
        std::slice::from_raw_parts(email_ptr as *const u8, email_len as usize)
    };
    let email = std::str::from_utf8(email).unwrap();

    // Email validation logic
    if email.contains('@') && email.contains('.') {
        1 // valid
    } else {
        0 // invalid
    }
}
```

### 2. Custom Aggregates

```rust
// aggregates.wasm
#[no_mangle]
pub extern "C" fn percentile_95(values_ptr: u32, count: u32) -> f64 {
    // Custom statistical function
    // ...
}
```

### 3. Data Transformers

```rust
// transformers.wasm
#[no_mangle]
pub extern "C" fn markdown_to_html(md_ptr: u32, md_len: u32) -> u32 {
    // Parse markdown, return HTML
    // Return pointer to result in WASM memory
}
```

## Build Toolchain

### Rust → WASM

```toml
# Cargo.toml for WASM module
[package]
name = "mentat-validators"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
# Keep dependencies minimal for small WASM size

[profile.release]
opt-level = "z"      # Optimize for size
lto = true           # Link-time optimization
codegen-units = 1    # Better optimization
panic = "abort"      # Smaller binary
```

```bash
# Build WASM module
cargo build --target wasm32-unknown-unknown --release

# Optimize with wasm-opt
wasm-opt -Oz target/wasm32-unknown-unknown/release/mentat_validators.wasm \
    -o validators.wasm
```

### AssemblyScript → WASM

```typescript
// validators.ts
export function validate_email(email: string): bool {
  return email.includes("@") && email.includes(".");
}
```

```bash
asc validators.ts -o validators.wasm --optimize
```

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_wasm_module() {
        let wasm_bytes = include_bytes!("../../test_modules/validators.wasm");
        let result = mentat_load_wasm("test", wasm_bytes);
        assert!(result.is_ok());
    }

    #[test]
    fn test_call_wasm_function() {
        // Load module
        let wasm_bytes = include_bytes!("../../test_modules/validators.wasm");
        mentat_load_wasm("validators", wasm_bytes).unwrap();

        // Call function
        let result = mentat_call_wasm(
            "validators:validate_email",
            json!({"email": "test@example.com"})
        ).unwrap();

        assert_eq!(result, json!(true));
    }
}
```

### Security Tests

```rust
#[test]
fn test_gas_limit_enforcement() {
    // WASM module with infinite loop
    let wasm_bytes = include_bytes!("../../test_modules/infinite_loop.wasm");
    mentat_load_wasm("malicious", wasm_bytes).unwrap();

    let result = mentat_call_wasm("malicious:infinite_loop", json!({}));

    // Should fail with gas limit error
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("gas"));
}

#[test]
fn test_memory_limit_enforcement() {
    // WASM module trying to allocate 1GB
    let result = mentat_call_wasm("malicious:allocate_huge", json!({}));

    // Should fail with memory limit error
    assert!(result.is_err());
}
```

### Integration Tests

```sql
-- Load test module
SELECT mentat_load_wasm('validators',
    pg_read_binary_file('/path/to/validators.wasm'));

-- Test in transaction
BEGIN;

-- Should succeed
INSERT INTO users (email) VALUES ('valid@example.com');

-- Should fail validation
INSERT INTO users (email) VALUES ('invalid-email');  -- Error

ROLLBACK;
```

## Implementation Checklist

### Phase 1: Foundation
- [ ] Add wasmer dependency (~5.0)
- [ ] Create `/wasm/` crate in workspace
- [ ] Implement module validation
- [ ] Basic module loading
- [ ] Module registry (thread-safe)

### Phase 2: Execution
- [ ] Wasmer store creation with limits
- [ ] Gas metering configuration
- [ ] Function invocation
- [ ] Type conversion (JSONB ↔ WASM)
- [ ] Error handling

### Phase 3: Security
- [ ] Memory limits enforcement
- [ ] Gas limit enforcement
- [ ] WASI restrictions
- [ ] Timeout handling
- [ ] Security tests

### Phase 4: Integration
- [ ] SQL function API (mentat_load_wasm, mentat_call_wasm)
- [ ] Transaction function hooks
- [ ] GUC configuration
- [ ] Module caching
- [ ] Instance pooling

### Phase 5: Documentation & Examples
- [ ] WASM module development guide
- [ ] Example modules (Rust, AssemblyScript)
- [ ] Security best practices
- [ ] Performance tuning guide
- [ ] API reference

## Known Limitations

1. **No filesystem access** - WASM functions are pure computation
2. **No network access** - Cannot make HTTP requests
3. **Synchronous only** - No async/await in WASM functions
4. **Limited debugging** - Use logging via return values
5. **Type marshaling overhead** - Complex types require serialization

## Future Enhancements

1. **WASI Preview 2** - When stable, enable component model
2. **Streaming execution** - For large data processing
3. **Parallel execution** - Run multiple WASM instances in parallel
4. **JIT compilation** - Hot code optimization
5. **Remote module loading** - Load WASM from URLs (with security validation)

## References

- [wasmer Documentation](https://docs.wasmer.io/)
- [WASI Specification](https://github.com/WebAssembly/WASI)
- [WebAssembly Specification](https://webassembly.github.io/spec/)
- [Rust WASM Book](https://rustwasm.github.io/docs/book/)
- [AssemblyScript Guide](https://www.assemblyscript.org/)

## Conclusion

This architecture provides a secure, performant foundation for WASM integration in pg_mentat. The wasmer runtime offers production-grade security sandboxing while maintaining near-native performance. The implementation prioritizes safety and simplicity, with clear extension points for future enhancements.
