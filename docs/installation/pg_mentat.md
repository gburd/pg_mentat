# Installing pg_mentat Extension

Complete installation guide for the pg_mentat PostgreSQL extension.

## System Requirements

### PostgreSQL Versions

- PostgreSQL 13, 14, 15, 16, 17, or 18
- UTF-8 database encoding (SQL_ASCII not supported)

### Operating Systems

- Linux (x86_64, aarch64)
- macOS (aarch64 Apple Silicon, x86_64 Intel)
- Windows (x86_64 with MSVC)

### Build Dependencies

- Rust stable toolchain (install via [rustup](https://rustup.rs))
- PostgreSQL development files
- libclang 11+ (for bindgen)
- C compiler (gcc, clang, or MSVC)

## Installation Methods

### Method 1: Install from Source (Recommended)

This method builds and installs directly into your PostgreSQL installation.

#### Step 1: Install Rust

```bash
# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installation
rustc --version
cargo --version
```

#### Step 2: Install cargo-pgrx

```bash
# Install pgrx CLI tool
cargo install cargo-pgrx --locked

# Verify installation
cargo pgrx --version
```

#### Step 3: Initialize pgrx

This step configures pgrx to work with your PostgreSQL installation:

```bash
# Find your pg_config path
which pg_config

# Initialize pgrx (adjust version and path as needed)
cargo pgrx init --pg16=/path/to/pg_config

# Example on macOS with Homebrew:
cargo pgrx init --pg16=/opt/homebrew/bin/pg_config

# Example on Linux with apt:
cargo pgrx init --pg16=/usr/bin/pg_config

# Example on multiple versions:
cargo pgrx init --pg14=/usr/pgsql-14/bin/pg_config \
               --pg15=/usr/pgsql-15/bin/pg_config \
               --pg16=/usr/pgsql-16/bin/pg_config
```

#### Step 4: Clone Repository

```bash
git clone https://github.com/qpdb/mentat.git
cd mentat
```

#### Step 5: Build and Install

```bash
# Build and install pg_mentat
cd pg_mentat
cargo pgrx install --release

# This installs the extension files to:
# - $(pg_config --sharedir)/extension/pg_mentat.control
# - $(pg_config --sharedir)/extension/pg_mentat--*.sql
# - $(pg_config --pkglibdir)/pg_mentat.so
```

#### Step 6: Verify Installation

```bash
# Check that files were installed
ls $(pg_config --sharedir)/extension/pg_mentat*
ls $(pg_config --pkglibdir)/pg_mentat*

# Connect to a database and create extension
psql -d postgres -c "CREATE EXTENSION pg_mentat;"
```

### Method 2: Package for Distribution

Build a redistributable package:

```bash
cd pg_mentat

# Build package for your PostgreSQL version
cargo pgrx package --release

# Package files are created in:
# target/release/pg_mentat-pg16/

# Package structure:
# usr/
# ├── lib/postgresql/
# │   └── pg_mentat.so
# └── share/postgresql/extension/
#     ├── pg_mentat.control
#     └── pg_mentat--0.1.0.sql
```

#### Install Package Manually

```bash
# Copy files to system directories
sudo cp -r target/release/pg_mentat-pg16/usr/* /usr/

# Or use specific paths
sudo cp target/release/pg_mentat-pg16/usr/lib/postgresql/pg_mentat.so \
    $(pg_config --pkglibdir)/

sudo cp target/release/pg_mentat-pg16/usr/share/postgresql/extension/pg_mentat* \
    $(pg_config --sharedir)/extension/
```

### Method 3: Development Setup

For extension development with rapid testing:

```bash
cd pg_mentat

# Run tests
cargo pgrx test pg16

# Start PostgreSQL with extension loaded
cargo pgrx run pg16

# This starts a temporary PostgreSQL instance with pg_mentat loaded
# Connect with: psql -h localhost -p 28816 -d postgres
```

## Platform-Specific Instructions

### macOS (Homebrew)

```bash
# Install PostgreSQL
brew install postgresql@16

# Start PostgreSQL
brew services start postgresql@16

# Initialize pgrx
cargo pgrx init --pg16=/opt/homebrew/bin/pg_config

# Build and install
cd mentat/pg_mentat
cargo pgrx install --release
```

### Ubuntu/Debian

```bash
# Install PostgreSQL and development files
sudo apt update
sudo apt install postgresql-16 postgresql-server-dev-16 libclang-dev

# Start PostgreSQL
sudo systemctl start postgresql

# Initialize pgrx
cargo pgrx init --pg16=/usr/bin/pg_config

# Build and install
cd mentat/pg_mentat
cargo pgrx install --release
```

### RHEL/CentOS/Rocky Linux

```bash
# Install PostgreSQL repository
sudo dnf install -y https://download.postgresql.org/pub/repos/yum/reporpms/EL-9-x86_64/pgdg-redhat-repo-latest.noarch.rpm

# Install PostgreSQL and development files
sudo dnf install -y postgresql16-server postgresql16-devel clang

# Initialize and start PostgreSQL
sudo /usr/pgsql-16/bin/postgresql-16-setup initdb
sudo systemctl start postgresql-16

# Initialize pgrx
cargo pgrx init --pg16=/usr/pgsql-16/bin/pg_config

# Build and install
cd mentat/pg_mentat
cargo pgrx install --release
```

### Windows (MSVC)

```powershell
# Install PostgreSQL from EnterpriseDB installer
# https://www.enterprisedb.com/downloads/postgres-postgresql-downloads

# Install Visual Studio Build Tools
# https://visualstudio.microsoft.com/downloads/

# Install LLVM (for libclang)
# https://releases.llvm.org/download.html

# Initialize pgrx
cargo pgrx init --pg16="C:\Program Files\PostgreSQL\16\bin\pg_config.exe"

# Build and install
cd mentat\pg_mentat
cargo pgrx install --release
```

## Enabling the Extension

After installation, enable pg_mentat in your database:

```sql
-- Connect to your database
\c mydatabase

-- Create extension
CREATE EXTENSION pg_mentat;

-- Verify installation
SELECT extname, extversion FROM pg_extension WHERE extname = 'pg_mentat';

-- Check available functions
\df mentat.*
```

## Configuration

### PostgreSQL Settings

Add to `postgresql.conf` if using extension hooks:

```ini
# Not required for basic usage, but enables query planner hooks
shared_preload_libraries = 'pg_mentat'

# Optional: Set memory limits
mentat.max_query_mem = '512MB'
mentat.max_collection_size = 1000000
```

Restart PostgreSQL after editing:

```bash
# Linux/systemd
sudo systemctl restart postgresql

# macOS/Homebrew
brew services restart postgresql@16

# Or use pg_ctl
pg_ctl restart -D /path/to/data/directory
```

### Extension Parameters

Set parameters per-session:

```sql
-- Increase query memory limit
SET mentat.max_query_mem = '1GB';

-- Increase collection size limit
SET mentat.max_collection_size = 2000000;
```

## Upgrading

### Upgrading Extension Version

```sql
-- Check current version
SELECT extversion FROM pg_extension WHERE extname = 'pg_mentat';

-- Upgrade to latest version
ALTER EXTENSION pg_mentat UPDATE;

-- Upgrade to specific version
ALTER EXTENSION pg_mentat UPDATE TO '0.2.0';
```

### Rebuilding After PostgreSQL Upgrade

After upgrading PostgreSQL:

```bash
# Reinitialize pgrx with new version
cargo pgrx init --pg17=/path/to/new/pg_config

# Rebuild and reinstall
cd pg_mentat
cargo pgrx install --release

# Extension version unchanged, but binary recompiled
```

## Uninstalling

### Remove from Database

```sql
-- Drop extension from database
DROP EXTENSION pg_mentat CASCADE;
```

### Remove System Files

```bash
# Remove extension files
sudo rm $(pg_config --sharedir)/extension/pg_mentat*
sudo rm $(pg_config --pkglibdir)/pg_mentat*

# Restart PostgreSQL
sudo systemctl restart postgresql
```

## Troubleshooting

### pgrx init fails

**Error:** `$PGRX_HOME does not exist`

**Solution:** Run `cargo pgrx init` with appropriate pg_config paths.

**Error:** `could not find pg_config`

**Solution:** Install PostgreSQL development files or specify full path.

### Build fails with "cannot find -lpq"

**Solution:** Install PostgreSQL client library:

```bash
# Ubuntu/Debian
sudo apt install libpq-dev

# RHEL/CentOS
sudo dnf install postgresql16-devel

# macOS
brew install libpq
```

### Extension not found after installation

**Solution:** Verify installation directories:

```bash
# Check share directory
ls $(pg_config --sharedir)/extension/pg_mentat*

# Check lib directory
ls $(pg_config --pkglibdir)/pg_mentat*

# If files are missing, reinstall
cargo pgrx install --release
```

### Permission denied during installation

**Solution:** Use sudo for system directories:

```bash
# Install as root
sudo -E cargo pgrx install --release

# Or use --pg-config to specify user-writable location
```

### CREATE EXTENSION fails with "could not load library"

**Error:** `could not load library "/usr/lib/postgresql/16/lib/pg_mentat.so"`

**Solutions:**

1. Check library dependencies:
   ```bash
   ldd $(pg_config --pkglibdir)/pg_mentat.so
   ```

2. Verify PostgreSQL version matches build:
   ```bash
   psql --version
   pg_config --version
   ```

3. Rebuild for correct PostgreSQL version:
   ```bash
   cargo pgrx install --release --pg-config=/correct/pg_config
   ```

### Tests fail with "database connection failed"

**Solution:** Ensure PostgreSQL is running and accessible:

```bash
# Check status
pg_isready

# Start if needed
sudo systemctl start postgresql

# Or use pgrx's test database
cargo pgrx test pg16
```

## Security Considerations

### File Permissions

Extension files should be owned by root with 755 permissions:

```bash
# Check permissions
ls -l $(pg_config --pkglibdir)/pg_mentat.so
ls -l $(pg_config --sharedir)/extension/pg_mentat*

# Fix if needed
sudo chown root:root $(pg_config --pkglibdir)/pg_mentat.so
sudo chmod 755 $(pg_config --pkglibdir)/pg_mentat.so
```

### PostgreSQL Privileges

Only superusers can install extensions:

```sql
-- Grant CREATE privilege to database
GRANT CREATE ON DATABASE mydatabase TO myuser;

-- Or make user superuser
ALTER USER myuser WITH SUPERUSER;
```

### Resource Limits

Prevent resource exhaustion:

```sql
-- Set limits in postgresql.conf
work_mem = 256MB
statement_timeout = 30000  -- 30 seconds
```

## Performance Tuning

### Index Configuration

Enable indexes on frequently queried attributes:

```sql
-- Check which attributes are indexed
SELECT ident, indexed FROM mentat.schema;

-- Enable indexing for specific attributes
UPDATE mentat.schema
SET indexed = true
WHERE ident IN ('person:email', 'order:date');
```

### Query Memory

Adjust based on workload:

```sql
-- For complex queries
SET work_mem = '512MB';

-- For large result sets
SET temp_buffers = '128MB';
```

## Next Steps

- [Quickstart Guide](../guides/quickstart.md) - Get started in 5 minutes
- [SQL Function API](../api/sql_functions.md) - Complete function reference
- [Configuration Guide](../configuration/pg_mentat_config.md) - GUC settings
- [Migration Guide](../guides/migration_guide.md) - Migrate from Datomic/SQLite
