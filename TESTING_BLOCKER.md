# Testing Blocker: cargo-pgrx Initialization Failure

## Issue

`cargo pgrx init` crashes with a segmentation fault (exit code 139) when attempting to download and compile PostgreSQL.

## Error Details

```bash
# First attempt (missing library path):
$ cargo pgrx init --pg16 download
/home/gburd/.cargo/bin/cargo-pgrx: error while loading shared libraries: libssl.so.3: cannot open shared object file: No such file or directory

# Second attempt (with LD_LIBRARY_PATH):
$ LD_LIBRARY_PATH=/usr/lib64:$LD_LIBRARY_PATH cargo pgrx init --pg16 download
Segmentation fault (core dumped)
Exit code: 139
```

## Root Cause

The `cargo-pgrx` binary was compiled with Nix store references:
```
$ ldd ~/.cargo/bin/cargo-pgrx
	linux-vdso.so.1 (0x00007faa73f0b000)
	libssl.so.3 => /lib64/libssl.so.3 (0x00007faa73dff000)
	libcrypto.so.3 => /lib64/libcrypto.so.3 (0x00007faa72a00000)
	/nix/store/vr7ds8vwbl2fz7pr221d5y0f8n9a5wda-glibc-2.40-218/lib/ld-linux-x86-64.so.2 => /lib64/ld-linux-x86-64.so.2 (0x00007faa73f0d000)
```

This Nix/glibc interaction may be causing the segfault during PostgreSQL download/compilation.

## Impact

**Cannot proceed with testing** because:
1. ❌ `cargo pgrx init` required to create `~/.pgrx/` and set `$PGRX_HOME`
2. ❌ `cargo build` fails without `$PGRX_HOME`: "Error: $PGRX_HOME does not exist"
3. ❌ Cannot run `cargo pgrx test` or `cargo pgrx run` without initialization

## Workarounds Attempted

### ✅ Successful:
- Installed system dependencies: openssl-devel, clang-devel, llvm-devel
- Installed cargo-pgrx: `cargo install --locked cargo-pgrx` (with LIBRARY_PATH fix)
- cargo-pgrx binary works: `cargo pgrx --version` shows 0.17.0

### ❌ Failed:
- `cargo pgrx init --pg16 download` - segfault
- `LD_LIBRARY_PATH=/usr/lib64 cargo pgrx init --pg16 download` - segfault
- `cargo build` without pgrx init - missing $PGRX_HOME error

## Possible Solutions

### Option 1: Install System PostgreSQL (Recommended)

Instead of downloading PostgreSQL via pgrx, use the system-installed version:

```bash
# Install PostgreSQL from Fedora repos
sudo dnf install -y postgresql-server postgresql-devel postgresql-contrib

# Check pg_config location
which pg_config
# Expected: /usr/bin/pg_config or /usr/lib64/pgsql/bin/pg_config

# Initialize pgrx with system PostgreSQL
cargo pgrx init --pg16 $(which pg_config)
```

**Advantages:**
- Avoids pgrx download/compilation (which is seg faulting)
- Uses known-working system packages
- Faster setup

**Disadvantages:**
- Tied to system PostgreSQL version (whatever Fedora 43 ships)
- May not test against multiple PostgreSQL versions

### Option 2: Fix Nix/glibc Interaction

The Nix store reference in the loader may be causing issues:

```bash
# Reinstall cargo-pgrx without Nix interference
# This might require rebuilding cargo-pgrx from source
# or using a different Rust toolchain

# TODO: Research Nix + pgrx compatibility issues
```

### Option 3: Use Docker/Container

Build and test in a clean environment without Nix:

```bash
# Create Dockerfile with Fedora 43 base
# Install dependencies
# Run cargo pgrx init inside container
# Mount code as volume

# This isolates from host Nix environment
```

### Option 4: Manual pgrx Setup

Manually create the pgrx directory structure (advanced, not recommended):

```bash
# This would require reverse-engineering what cargo pgrx init does
# Including: ~/.pgrx/ directory structure, pg_config locations, port assignments
# Very error-prone, not recommended
```

## Recommended Path Forward

**Immediate:** Try Option 1 (Install System PostgreSQL)

1. Install PostgreSQL packages:
   ```bash
   sudo dnf install -y postgresql-server postgresql-devel postgresql-contrib
   ```

2. Find pg_config:
   ```bash
   rpm -ql postgresql-devel | grep pg_config
   ```

3. Initialize pgrx with system PostgreSQL:
   ```bash
   export LD_LIBRARY_PATH=/usr/lib64:$LD_LIBRARY_PATH
   cargo pgrx init --pg16 /usr/bin/pg_config
   # Or wherever pg_config is located
   ```

4. Build extension:
   ```bash
   cd /home/gburd/src/pg_mentat/pg_mentat
   cargo build
   ```

5. Run tests:
   ```bash
   cargo pgrx test
   ```

**If Option 1 Fails:** Investigate Nix/glibc compatibility or use Docker

## Status

- ✅ All code implementations complete (4 agents, 5 tasks)
- ✅ System dependencies installed
- ✅ cargo-pgrx binary installed
- ❌ **BLOCKED:** Cannot initialize pgrx due to segfault
- ❌ **BLOCKED:** Cannot build extension without $PGRX_HOME
- ❌ **BLOCKED:** Cannot run tests without working build

## Work Completed (Despite Blocker)

The implementation work is complete and saved:
- mentatd handlers wired to pg_mentat
- All SQL injection vulnerabilities fixed
- mentat_pull() fully implemented
- Query translation improved (all 9 types, OR support)
- 7 new unit tests added
- ~800 lines of code changed across 10 files

**The code is ready to test, we just need a working pgrx environment.**

## Next Action Required

**User must:**
1. Decide which solution to pursue (Option 1 recommended)
2. If Option 1: Install system PostgreSQL packages
3. Retry pgrx initialization with system pg_config
4. Report results

**Alternative:**
- Debug the segfault (may require system administration knowledge)
- Use a different development machine without Nix
- Use Docker/container environment

---

**Date:** 2026-03-05
**Status:** Implementation complete, testing blocked by pgrx init segfault
**Blocker:** cargo-pgrx initialization crashes when downloading PostgreSQL
**Recommended Fix:** Install system PostgreSQL and init pgrx with system pg_config
