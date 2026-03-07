# Environment Workarounds

This file documents environment-specific workarounds needed to build and test
the pg_mentat extension.

## 1. CARGO_HOME Override

### Problem

In sandboxed or restricted environments, the default `~/.cargo` directory may be
unreadable or unwritable. Cargo operations fail with permission errors when
trying to access the registry index or download crates.

### Solution

Point `CARGO_HOME` at a project-local `.cargo` directory:

```bash
export CARGO_HOME=/home/gburd/ws/pg_mentat/.cargo
```

The project repository already contains a populated `.cargo` directory with the
registry cache. Setting this variable before any cargo command ensures the
toolchain, registry, and installed binaries (such as `cargo-pgrx`) are found.

### Verification

```bash
export CARGO_HOME=/home/gburd/ws/pg_mentat/.cargo
cargo --version          # should work without errors
cargo pgrx --version     # should report 0.17.x
```

### Scope

This variable must be set in every shell session before running cargo. Add it
to your shell profile or a project-level `.envrc` if using direnv.

---

## 2. libclang / LLVM for bindgen

### Problem

`pgrx` depends on `bindgen`, which calls into `libclang` to parse PostgreSQL's
C headers and generate Rust FFI bindings. If `libclang` is not installed or
not on the library search path, the build fails with errors like:

```
thread 'main' panicked at 'Unable to find libclang'
```

or

```
error: failed to run custom build command for `pgrx-pg-sys`
```

### Solution

Install the LLVM/Clang development packages for your distribution:

**Fedora / RHEL:**
```bash
sudo dnf install clang-devel llvm-devel
```

**Debian / Ubuntu:**
```bash
sudo apt install libclang-dev llvm-dev
```

**macOS (Homebrew):**
```bash
brew install llvm
export LIBCLANG_PATH="$(brew --prefix llvm)/lib"
```

If the system has multiple LLVM versions, set `LIBCLANG_PATH` explicitly:

```bash
export LIBCLANG_PATH=/usr/lib64          # Fedora typical path
# or
export LIBCLANG_PATH=/usr/lib/llvm-17/lib  # Debian with versioned LLVM
```

You can also set `LLVM_CONFIG_PATH` if `llvm-config` is not on `$PATH`:

```bash
export LLVM_CONFIG_PATH=/usr/bin/llvm-config-17
```

### Verification

```bash
llvm-config --version    # should print a version number
ls $LIBCLANG_PATH/libclang.so*  # should find the shared library
```

---

## 3. macOS ARM64 Linker Issue

### Problem

On Apple Silicon (M1/M2/M3), `cargo pgrx test` may fail at link time:

```
ld: symbol(s) not found for architecture arm64
clang: error: linker command failed with exit code 1
```

This is a known pgrx issue with macOS ARM64 when linking PostgreSQL extension
shared objects.

### Solution

Use a Linux x86_64 environment for testing. Options:

1. **Linux machine or VM** -- the project is configured for Fedora 43 x86_64.
2. **Docker / Podman container** -- run tests inside a Linux container:
   ```bash
   podman run --rm -v /home/gburd/ws/pg_mentat:/workspace:Z \
     -w /workspace/pg_mentat \
     fedora:43 bash -c '
       dnf install -y clang-devel llvm-devel postgresql-server-devel rust cargo
       cargo install cargo-pgrx --version 0.17.0
       cargo pgrx init
       cargo pgrx test pg16
     '
   ```
3. **GitHub Actions CI** -- use a Linux runner for automated test execution.

macOS can still be used for editing and cargo check / clippy, but test execution
should target Linux.

---

## 4. pgrx Initialization

### Problem

`cargo pgrx test` or `cargo pgrx run` fails with:

```
$PGRX_HOME does not exist
```

### Solution

Initialize pgrx once per environment:

```bash
export CARGO_HOME=/home/gburd/ws/pg_mentat/.cargo
cargo pgrx init --pg16=$(which pg_config)
```

If `pg_config` is not on `$PATH`, provide the full path:

```bash
cargo pgrx init --pg16=/usr/pgsql-16/bin/pg_config
```

pgrx will download and compile its own PostgreSQL instance for testing if no
system PostgreSQL is provided:

```bash
cargo pgrx init    # downloads and builds PG 16
```

---

## 5. Manual Smoke Testing (without pgrx test harness)

If the full pgrx test pipeline is blocked, you can still validate the extension
manually against a running PostgreSQL instance.

### Prerequisites

- PostgreSQL 16 running and accessible
- Extension built and installed (`cargo pgrx install`)

### Steps

```sql
-- 1. Load the extension
CREATE EXTENSION pg_mentat;

-- 2. Test EDN type roundtrip
SELECT mentat.edn_out(mentat.edn_in('42'));
SELECT mentat.edn_out(mentat.edn_in('{:name "Alice" :age 30}'));

-- 3. Test schema initialization
SELECT mentat.initialize_schema();

-- 4. Test transact
SELECT mentat.mentat_transact('[
  [:db/add "p1" :person/name "Alice"]
  [:db/add "p1" :person/age 30]
]');

-- 5. Verify datoms
SELECT * FROM mentat.datoms LIMIT 10;

-- 6. Test query
SELECT mentat.mentat_query(
  '[:find ?e ?ident :where [?e :db/ident ?ident]]',
  '{}'::jsonb
);
```

---

## Quick Reference

| Variable | Value | Purpose |
|---|---|---|
| `CARGO_HOME` | `/home/gburd/ws/pg_mentat/.cargo` | Use project-local cargo registry |
| `LIBCLANG_PATH` | `/usr/lib64` (varies) | Help bindgen find libclang |
| `LLVM_CONFIG_PATH` | `/usr/bin/llvm-config` (varies) | Help clang-sys find LLVM |

### Full environment setup

```bash
export CARGO_HOME=/home/gburd/ws/pg_mentat/.cargo
export LIBCLANG_PATH=/usr/lib64
cd /home/gburd/ws/pg_mentat/pg_mentat
cargo pgrx test pg16
```
