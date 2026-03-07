# Nix Development Environment for pg_mentat

This project uses Nix flakes to provide a reproducible development environment with all necessary dependencies.

## Prerequisites

1. **Install Nix** (with flakes enabled):
   ```bash
   # Install Nix
   sh <(curl -L https://nixos.org/nix/install) --daemon

   # Enable flakes
   mkdir -p ~/.config/nix
   echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
   ```

2. **Optional: Install direnv** (for automatic environment activation):
   ```bash
   # On NixOS
   nix-env -iA nixpkgs.direnv

   # On other systems
   curl -sfL https://direnv.net/install.sh | bash

   # Add to your shell config (~/.bashrc or ~/.zshrc)
   eval "$(direnv hook bash)"  # or: eval "$(direnv hook zsh)"
   ```

## Quick Start

### Option 1: Using direnv (Automatic)

```bash
# Allow direnv to load the environment
direnv allow

# The environment will be automatically activated when you cd into the directory
cd /home/gburd/ws/pg_mentat

# You should see a welcome message with available commands
```

### Option 2: Manual Activation

```bash
# Enter the Nix development shell
nix develop

# You'll see a welcome message with available commands
```

## What's Included

The Nix flake provides:

- **Rust 1.90.0** with clippy, rustfmt, and rust-analyzer
- **PostgreSQL 13-17** (default: 16)
- **cargo-pgrx 0.17.0** (installed via helper command)
- **LLVM/Clang 18** with libclang for bindgen
- **Build tools**: pkg-config, git, make, gcc, perl
- **Libraries**: OpenSSL, zlib, readline, libicu

## Environment Variables

The following environment variables are automatically set:

| Variable | Purpose |
|----------|---------|
| `CARGO_HOME` | `./.cargo` (workaround for filesystem restrictions) |
| `LIBCLANG_PATH` | Path to libclang for bindgen |
| `LLVM_CONFIG_PATH` | Path to llvm-config |
| `LD_LIBRARY_PATH` | Runtime library paths |
| `PKG_CONFIG_PATH` | Package config paths |
| `PGDATA` | PostgreSQL data directory (`./.postgres-data`) |
| `RUST_BACKTRACE` | Enabled for debugging |

## Helper Commands

Once in the Nix shell, the following commands are available:

### 1. Setup pgrx

Install and initialize cargo-pgrx with all PostgreSQL versions:

```bash
setup-pgrx
```

This will:
- Install cargo-pgrx 0.17.0
- Initialize pgrx with PostgreSQL 13-17
- Configure ~/.pgrx/config.toml

### 2. Test with PostgreSQL 16

Run the full test suite:

```bash
test-pg16
```

Run specific tests:

```bash
test-pg16 -- test_name
```

Run tests with verbose output:

```bash
test-pg16 -- --nocapture
```

### 3. Build the Extension

Build the extension in release mode:

```bash
build-extension
```

Build in debug mode:

```bash
cd pg_mentat
cargo build
```

### 4. Install the Extension

Install to PostgreSQL:

```bash
install-extension
```

Install to a specific PostgreSQL version:

```bash
cd pg_mentat
cargo pgrx install --release pg15
```

### 5. Start PostgreSQL

Start a local PostgreSQL instance for testing:

```bash
start-postgres
```

This will:
- Initialize a PostgreSQL data directory (if needed)
- Start PostgreSQL on localhost:5432
- Print the process ID for stopping later

To stop: `kill <PID>` (shown in output)

## Development Workflow

### Initial Setup

```bash
# 1. Enter Nix shell
nix develop

# 2. Install and initialize pgrx
setup-pgrx

# 3. Build the project
cd pg_mentat
cargo build
```

### Running Tests

```bash
# Run all tests
test-pg16

# Run specific test
test-pg16 -- test_pg_query

# Run with verbose output
test-pg16 -- --nocapture

# Run with backtrace
RUST_BACKTRACE=full test-pg16
```

### Testing with PostgreSQL

```bash
# 1. Start PostgreSQL
start-postgres

# 2. In another terminal, install extension
install-extension

# 3. Connect and test
psql postgres
CREATE EXTENSION pg_mentat;
SELECT mentat.mentat_schema();

# 4. Run manual tests
SELECT mentat.mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

SELECT mentat.mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);
```

### Working on Code

```bash
# Edit code
vim pg_mentat/src/functions/query.rs

# Build
cd pg_mentat
cargo build

# Run tests
test-pg16

# Format code
cargo fmt

# Lint
cargo clippy
```

## Building for Production

```bash
# Build optimized release
nix build

# The extension will be in result/lib/libpg_mentat.so
ls -l result/
```

## Testing Different PostgreSQL Versions

The flake includes PostgreSQL 13-17. To test against different versions:

```bash
# Test with PostgreSQL 15
cd pg_mentat
cargo pgrx test pg15

# Test with PostgreSQL 17
cargo pgrx test pg17
```

## Troubleshooting

### Issue: libclang not found

The Nix environment automatically sets `LIBCLANG_PATH`. If you still see errors:

```bash
# Check if LIBCLANG_PATH is set
echo $LIBCLANG_PATH

# Verify libclang exists
ls $LIBCLANG_PATH/libclang.so*

# Re-enter the Nix shell
exit
nix develop
```

### Issue: cargo-pgrx not found

```bash
# Install pgrx
setup-pgrx

# Verify installation
which cargo-pgrx
cargo-pgrx --version
```

### Issue: PostgreSQL connection failed

```bash
# Check if PostgreSQL is running
ps aux | grep postgres

# Start PostgreSQL
start-postgres

# Check data directory
ls -la .postgres-data/
```

### Issue: Tests fail to compile

```bash
# Clean build artifacts
cargo clean

# Rebuild
cargo build

# Try tests again
test-pg16
```

## CI/CD Integration

### GitHub Actions

```yaml
name: Test pg_mentat
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: cachix/install-nix-action@v22
        with:
          extra_nix_config: |
            experimental-features = nix-command flakes
      - name: Run tests
        run: |
          nix develop --command bash -c "
            setup-pgrx
            test-pg16
          "
```

### Local CI Simulation

```bash
# Run the same checks as CI
nix flake check
```

## Additional Resources

- [Nix Flakes Documentation](https://nixos.wiki/wiki/Flakes)
- [pgrx Documentation](https://github.com/pgcentralfoundation/pgrx)
- [Project README](./README.md)
- [Test Migration Guide](./TEST_MIGRATION_COMPLETE.md)

## Environment Variables Reference

All environment variables can be overridden if needed:

```bash
# Custom CARGO_HOME
CARGO_HOME=/custom/path nix develop

# Custom PostgreSQL port
PGPORT=5433 nix develop

# Custom data directory
PGDATA=/custom/postgres/data nix develop
```

## Cleaning Up

```bash
# Remove cargo cache
rm -rf .cargo

# Remove PostgreSQL data
rm -rf .postgres-data

# Remove pgrx config (to reinitialize)
rm -rf ~/.pgrx

# Clean Nix build artifacts
nix-collect-garbage
```

## Support

For issues related to:
- **Nix setup**: Check the Nix documentation
- **pgrx issues**: See [pgrx repository](https://github.com/pgcentralfoundation/pgrx)
- **pg_mentat code**: See project documentation in this repository
