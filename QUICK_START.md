# pg_mentat Quick Start

A one-page guide for getting started with pg_mentat development.

---

## Prerequisites

- **OS:** Linux (Fedora, Ubuntu, or NixOS). macOS ARM64 has known pgrx issues.
- **PostgreSQL:** 14+ (16 recommended)
- **Rust:** 1.88+ stable toolchain
- **LLVM/Clang:** Development libraries (for pgrx bindgen)

---

## Option A: Nix (Recommended)

Nix provides all dependencies automatically.

```bash
# 1. Install Nix with flakes enabled
sh <(curl -L https://nixos.org/nix/install) --daemon
mkdir -p ~/.config/nix
echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf

# 2. Enter the development shell
cd /path/to/pg_mentat
nix develop

# 3. Install and initialize pgrx (first time only)
setup-pgrx

# 4. Run tests
test-pg16

# 5. Build the extension
build-extension
```

Available shell commands after `nix develop`:

| Command | Description |
|---------|-------------|
| `setup-pgrx` | Install cargo-pgrx 0.17 and initialize with PG 13-17 |
| `test-pg16` | Run all 38 pgrx tests against PostgreSQL 16 |
| `build-extension` | Package the extension (release mode) |
| `install-extension` | Install to local PostgreSQL |
| `start-postgres` | Start a local PostgreSQL instance |

See [NIX_SETUP.md](NIX_SETUP.md) for the full environment guide.

---

## Option B: Manual Setup

### Fedora

```bash
sudo dnf install -y \
    postgresql-server-devel postgresql-private-devel postgresql-private-libs \
    clang-devel llvm-devel openssl-devel
```

### Debian / Ubuntu

```bash
sudo apt install -y \
    postgresql-server-dev-16 libclang-dev llvm-dev libssl-dev
```

### Rust and pgrx

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup toolchain install 1.90.0
rustup default 1.90.0

# Install cargo-pgrx
cargo install --locked cargo-pgrx --version '~0.17'

# Initialize pgrx (uses system pg_config or downloads PG)
cargo pgrx init --pg16=$(which pg_config)
# Or: cargo pgrx init --pg16 download
```

### Build and Test

```bash
cd pg_mentat
cargo pgrx test pg16      # run all tests
cargo pgrx package        # build extension package
cargo pgrx install        # install to local PostgreSQL
```

---

## Option C: Container

```bash
# Build container image
podman build -t pg_mentat_build -f Containerfile .

# Run tests in container
podman run --rm \
    -v $(pwd):/workspace:Z \
    -w /workspace/pg_mentat \
    pg_mentat_build \
    cargo pgrx test pg16
```

---

## Running the Extension

```bash
# Start an interactive PostgreSQL session with the extension loaded
cd pg_mentat
cargo pgrx run pg16
```

In the psql prompt:

```sql
-- Load the extension
CREATE EXTENSION pg_mentat;

-- Initialize schema (creates datom tables and indexes)
SELECT mentat.initialize_schema();

-- Define an attribute
SELECT mentat.mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Add data
SELECT mentat.mentat_transact('[
  [:db/add "p1" :person/name "Alice"]
]');

-- Query
SELECT mentat.mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);

-- View schema
SELECT mentat.mentat_schema();
```

---

## Running the mentatd Server

```bash
cd mentatd

# Run unit tests
cargo test

# Build
cargo build --release

# Configure (edit connection string)
cp mentatd.toml.example mentatd.toml
# Edit mentatd.toml: set postgresql connection string

# Run
./target/release/mentatd

# Test
curl http://localhost:8080/health
```

---

## Common Issues

**pgrx init fails with segfault:**
This is typically a glibc/Nix conflict. Use the Nix flake (`nix develop`)
which sets up library paths correctly, or use a container.

**libclang not found:**
Install LLVM/Clang development packages. In the Nix shell, `LIBCLANG_PATH`
is set automatically.

**Tests fail to compile:**
```bash
cargo clean
cargo pgrx test pg16
```

**PostgreSQL connection errors:**
Make sure PostgreSQL is running and accessible. With the Nix flake:
```bash
start-postgres
```

---

## Project Layout

```
pg_mentat/              # This repository
  pg_mentat/            # PostgreSQL extension crate
    src/lib.rs          # Entry point + 38 tests
    src/functions/      # mentat_transact, mentat_query, etc.
    src/types/edn.rs    # EdnValue custom type
    sql/                # Schema DDL
  mentatd/              # HTTP server crate
    src/server.rs       # Request handlers
    src/protocol/       # Datomic wire protocol
  flake.nix             # Nix development environment
  edn/                  # EDN parser (original)
  core/                 # Core types (original)
  db/                   # Storage logic (original, SQLite)
```

---

## Where to Go Next

- [CURRENT_STATUS.md](CURRENT_STATUS.md) -- What works, what doesn't, what's next
- [NIX_SETUP.md](NIX_SETUP.md) -- Full Nix environment documentation
- [pg_mentat/README.md](pg_mentat/README.md) -- Extension API reference
- [CONTRIBUTING.md](CONTRIBUTING.md) -- How to contribute
- [TEST_MIGRATION_COMPLETE.md](TEST_MIGRATION_COMPLETE.md) -- Test details
