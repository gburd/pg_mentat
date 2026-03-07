# Setup Requirements for pg_mentat Testing

## System Dependencies Required

Before running `cargo pgrx init` and testing, install these packages:

```bash
# On Fedora/RHEL
sudo dnf install -y openssl-devel clang-devel llvm-devel postgresql-devel

# On Ubuntu/Debian
sudo apt-get install -y libssl-dev clang libclang-dev llvm-dev libpq-dev
```

## Installation Steps (After System Dependencies)

```bash
# 1. Install cargo-pgrx
cargo install --locked cargo-pgrx

# 2. Initialize pgrx (downloads & compiles PostgreSQL 14-18)
# This takes 15-30 minutes on first run
cargo pgrx init

# 3. Build the extension
cd pg_mentat
cargo build

# 4. Run tests
cargo pgrx test
```

## Current Blocker

The system dependencies are not installed, and cannot be installed without sudo access. Once these are installed, the full validation and testing can proceed.

## Work That Can Proceed Now

The following phases can be implemented without database access:
- Phase 3: Wire mentatd handlers to pg_mentat (code changes only)
- Phase 4: Fix SQL injection vulnerabilities (code changes only)
- Phase 5: Complete mentat_pull() implementation (code changes only)
- Phase 6: Improve query translation robustness (code changes only)

These are pure code changes that can be made, reviewed, and prepared. Testing (Phases 2 and 7) will happen after dependencies are installed.
