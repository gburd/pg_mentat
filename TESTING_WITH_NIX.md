# Testing pg_mentat with Nix

This guide covers how to build, test, and develop pg_mentat using the Nix flake.

## Prerequisites

1. **Nix with flakes enabled**:
   ```bash
   # Install Nix (multi-user, recommended)
   sh <(curl -L https://nixos.org/nix/install) --daemon

   # Enable flakes (add to ~/.config/nix/nix.conf or /etc/nix/nix.conf)
   mkdir -p ~/.config/nix
   echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
   ```

2. **Optional: direnv** for automatic environment activation:
   ```bash
   nix profile install nixpkgs#direnv

   # Add to ~/.bashrc:
   eval "$(direnv hook bash)"
   # Or ~/.zshrc:
   eval "$(direnv hook zsh)"
   ```

## Quick Start

```bash
# Clone and enter the project
git clone --recurse-submodules <repo-url>
cd pg_mentat

# Enter the development shell
nix develop

# First-time setup: install cargo-pgrx
setup-pgrx

# Run tests
test-pg16
```

With direnv, `nix develop` is automatic:
```bash
direnv allow   # one time
cd pg_mentat   # shell activates automatically
```

## What the Flake Provides

### Software

| Tool               | Version / Source        |
|--------------------|------------------------|
| Rust toolchain     | 1.90.0 (stable)        |
| clippy, rustfmt    | Bundled with toolchain |
| rust-analyzer      | Bundled with toolchain |
| PostgreSQL         | 16 (from nixpkgs)      |
| LLVM / Clang       | 18                     |
| cargo-pgrx         | ~0.17 (installed via helper) |

### Environment Variables

| Variable           | Value                  | Purpose                        |
|--------------------|------------------------|--------------------------------|
| `CARGO_HOME`       | `./.cargo`             | Local cargo cache              |
| `LIBCLANG_PATH`    | nix store path         | bindgen / pgrx code generation |
| `LLVM_CONFIG_PATH` | nix store path         | LLVM discovery                 |
| `LD_LIBRARY_PATH`  | nix store paths        | Runtime library resolution     |
| `PKG_CONFIG_PATH`  | nix store paths        | Build-time library discovery   |
| `PGDATA`           | `./.postgres-data`     | Local PostgreSQL data dir      |
| `RUST_BACKTRACE`   | `1`                    | Backtraces on panic            |
| `RUST_SRC_PATH`    | nix store path         | rust-analyzer source lookup    |

### Shell Helper Commands

| Command             | Description                                    |
|---------------------|------------------------------------------------|
| `setup-pgrx`       | Install cargo-pgrx and init with PG 16         |
| `test-pg16 [args]` | Run `cargo pgrx test pg16` in pg_mentat/       |
| `build-extension`  | Package the extension for distribution         |
| `install-extension` | Install extension into local PostgreSQL        |
| `start-postgres`   | Init (if needed) and start local PG instance   |

## Testing Scenarios

### 1. Run the Full Test Suite

```bash
nix develop
setup-pgrx          # first time only
test-pg16
```

Expected output: 38 tests across 5 categories (EDN types, query, time-travel, rules, full-text search).

### 2. Run a Single Test

```bash
test-pg16 -- test_pg_query_basic
```

### 3. Run Tests with Verbose Output

```bash
test-pg16 -- --nocapture
```

### 4. Run Tests with Full Backtrace

```bash
RUST_BACKTRACE=full test-pg16
```

### 5. Build the Extension Without Tests

```bash
cd pg_mentat
cargo build --release
```

### 6. Build a Distributable Package

```bash
nix build
ls -la result/lib/ result/share/postgresql/extension/
```

### 7. Manual Integration Testing

```bash
# Start PostgreSQL
start-postgres

# Install the extension
install-extension

# Connect and test
psql postgres <<'SQL'
CREATE EXTENSION pg_mentat;
SELECT mentat.mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');
SELECT mentat.mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);
SQL

# Stop PostgreSQL when done
pg_ctl -D "$PGDATA" stop
```

### 8. Validate the Environment

```bash
bash verify-nix-env.sh
```

This checks all tools, environment variables, and helper commands.

## CI/CD Usage

### GitHub Actions (Nix-based)

See `.github/workflows/nix-test.yml` for the full workflow. The key pattern:

```yaml
- uses: cachix/install-nix-action@v27
  with:
    extra_nix_config: |
      experimental-features = nix-command flakes
- run: |
    nix develop --command bash -c '
      setup-pgrx
      test-pg16
    '
```

### Running `nix flake check`

```bash
nix flake check
```

This runs the `checks.${system}.build` output, which builds the full extension package.

## Troubleshooting

### "error: attribute 'nls' missing"

The `nls` package does not exist in nixpkgs. If you see this, you are running an older version of the flake. Pull the latest version -- it uses `gettext` instead.

### "libclang not found" or bindgen errors

Verify the environment variable is set:
```bash
echo $LIBCLANG_PATH
ls "$LIBCLANG_PATH"/libclang.so*
```

If empty, re-enter the shell:
```bash
exit
nix develop
```

### "cargo-pgrx: command not found"

Run `setup-pgrx` to install it. It is not pre-installed because it needs to be compiled, which takes several minutes.

### PostgreSQL connection failures during tests

pgrx manages its own PostgreSQL instances for testing. If tests fail with connection errors:
```bash
# Clean pgrx state
rm -rf ~/.pgrx

# Re-initialize
setup-pgrx

# Retry
test-pg16
```

### Build fails with "permission denied" writing to cargo cache

Ensure `CARGO_HOME` points to a writable location:
```bash
echo $CARGO_HOME   # should be ./.cargo
mkdir -p "$CARGO_HOME"
```

### Slow first build

The first `nix develop` downloads and builds dependencies. Subsequent runs use the Nix store cache and are fast. To speed things up for a team, consider setting up a binary cache with Cachix.

### "error: experimental feature 'flakes' is disabled"

Add to `~/.config/nix/nix.conf`:
```
experimental-features = nix-command flakes
```

Then restart the Nix daemon:
```bash
sudo systemctl restart nix-daemon
```

## Migration from Containerfile

The project previously used a `Containerfile` (Podman/Docker) for builds. The Nix flake replaces this approach.

| Aspect              | Containerfile              | Nix Flake                         |
|---------------------|----------------------------|-----------------------------------|
| Enter environment   | `podman build && run`      | `nix develop`                     |
| Build isolation     | Full container             | Nix store (no container overhead) |
| Reproducibility     | Dockerfile layers          | Lockfile-pinned inputs            |
| Startup time        | Container boot             | Near-instant (cached)             |
| IDE integration     | Requires bind mounts       | Native filesystem access          |
| CI/CD               | Needs container runtime    | Just Nix                          |
| Disk usage          | Container images           | Shared Nix store                  |

The Containerfile is kept for reference but is not the recommended workflow.

## Directory Layout After Setup

```
pg_mentat/
  .cargo/              # Local cargo cache (CARGO_HOME)
  .direnv/             # direnv cache (gitignored)
  .postgres-data/      # Local PG data dir (gitignored)
  flake.nix            # Nix flake definition
  flake.lock           # Pinned dependency versions
  .envrc               # direnv integration
  verify-nix-env.sh    # Environment verification script
```

All local state directories are gitignored.
