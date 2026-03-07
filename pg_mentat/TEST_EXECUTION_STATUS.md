# Test Execution Status

**Last updated:** 2026-03-06
**Branch:** claude
**Platform:** Linux (Fedora 43, x86_64)

## Current State

### Test Migration: Complete

All 38 pgrx tests have been consolidated into `src/lib.rs` under the
`#[cfg(any(test, feature = "pg_test"))]` test module. The external test files in
`tests/*.rs` remain as reference but are superseded by the inline tests.

| Category | Tests | Lines | Status |
|---|---|---|---|
| EDN Type roundtrips | 5 | ~40 | Migrated to lib.rs |
| Core Queries | 11 | ~280 | Migrated to lib.rs |
| Time-Travel | 7 | ~240 | Migrated to lib.rs |
| Rules / Recursive | 8 | ~270 | Migrated to lib.rs |
| Full-Text Search | 7 | ~250 | Migrated to lib.rs |
| **Total** | **38** | **~1080** | **All in lib.rs** |

### Environment Workarounds

#### CARGO_HOME Override

The standard `~/.cargo` directory is inaccessible in sandboxed environments.
Setting `CARGO_HOME` to a project-local path allows `cargo` and `cargo-pgrx`
to find the registry, crate cache, and installed binaries:

```bash
export CARGO_HOME=/home/gburd/ws/pg_mentat/.cargo
```

This must be set before any `cargo` invocation (build, test, pgrx commands).

#### libclang / LLVM Dependency

`pgrx` uses `bindgen` to generate Rust bindings from PostgreSQL C headers. This
requires `libclang` (LLVM) to be installed and discoverable. The build fails at
the bindgen step if:

- `libclang.so` is not on the library search path, or
- `LIBCLANG_PATH` is not set, or
- The `clang-sys` crate cannot locate a compatible LLVM installation.

**Resolution options:**

1. Install system packages: `sudo dnf install clang-devel llvm-devel` (Fedora)
   or `sudo apt install libclang-dev` (Debian/Ubuntu).
2. Set `LIBCLANG_PATH` to the directory containing `libclang.so`.
3. Use the `LLVM_CONFIG_PATH` variable if multiple LLVM versions are installed.

**Current status:** Being resolved (see task #1).

### Previous Blocker: macOS ARM64 Linker

The earlier macOS development environment hit a linker error
(`symbol(s) not found for architecture arm64`) at the pgrx test link stage.
This is a known pgrx limitation on Apple Silicon. The migration to Linux
x86_64 removes this blocker.

## Test Execution Attempts

### Attempt 1 -- macOS ARM64 (2026-03-05)

- **Environment:** macOS, Apple Silicon
- **Command:** `cargo pgrx test pg16`
- **Outcome:** Compilation succeeded; linking failed with arm64 symbol errors.
- **Diagnosis:** Known pgrx issue on macOS ARM64. Not a code defect.

### Attempt 2 -- Linux x86_64 (2026-03-06)

- **Environment:** Linux (Fedora 43), x86_64
- **Command:** `CARGO_HOME=/home/gburd/ws/pg_mentat/.cargo cargo pgrx test pg16`
- **Outcome:** Blocked on libclang dependency during bindgen compilation.
- **Diagnosis:** System needs `clang-devel` / `llvm-devel` packages or
  `LIBCLANG_PATH` configured.

### Next Attempt

Once the libclang dependency is resolved:

```bash
export CARGO_HOME=/home/gburd/ws/pg_mentat/.cargo
export LIBCLANG_PATH=/usr/lib64   # adjust to actual path
cd /home/gburd/ws/pg_mentat/pg_mentat
cargo pgrx test pg16
```

Expected: all 38 inline tests execute. Some may need minor fixes for runtime
behavior differences between the test assertions and actual PostgreSQL output.

## What Is Working

- Rust code compiles (modulo the bindgen/libclang step)
- All 38 tests have valid structure and use correct pgrx APIs
- CARGO_HOME workaround allows cargo registry access
- Linux x86_64 eliminates the macOS ARM64 linker issue

## What Is Blocked

| Blocker | Impact | Owner |
|---|---|---|
| libclang not found | Cannot complete bindgen step, so no .so is produced | Task #1 |
| No running PostgreSQL | pgrx test needs a PG instance (pgrx manages its own) | Resolved once libclang is fixed |

## Test File Locations

### Primary (inline, authoritative)

`/home/gburd/ws/pg_mentat/pg_mentat/src/lib.rs` -- lines 43-1343

### Reference (external, older copies)

```
/home/gburd/ws/pg_mentat/pg_mentat/tests/test_common.rs
/home/gburd/ws/pg_mentat/pg_mentat/tests/test_query.rs
/home/gburd/ws/pg_mentat/pg_mentat/tests/test_fulltext.rs
/home/gburd/ws/pg_mentat/pg_mentat/tests/test_rules.rs
/home/gburd/ws/pg_mentat/pg_mentat/tests/test_timetravel.rs
```
