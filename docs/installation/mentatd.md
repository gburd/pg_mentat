# Installing mentatd Server

Complete installation guide for mentatd, the HTTP server implementing the Datomic wire protocol.

## Overview

mentatd is an HTTP server that translates Datomic client requests into PostgreSQL queries via the pg_mentat extension. This allows existing Datomic clients to connect to a PostgreSQL-backed Mentat database.

```
Datomic Client → mentatd (HTTP/EDN) → PostgreSQL (pg_mentat extension)
```

## Prerequisites

- PostgreSQL 13+ with pg_mentat extension installed
- Rust stable toolchain (via rustup)
- PostgreSQL database with CREATE EXTENSION privilege

## Quick Install

```bash
# Clone repository
git clone https://github.com/qpdb/mentat.git
cd mentat

# Build mentatd
cargo build --release -p mentatd

# Binary created at:
# target/release/mentatd
```

## Installation

### Step 1: Install pg_mentat Extension

mentatd requires the pg_mentat extension to be installed in PostgreSQL. Follow the [pg_mentat installation guide](./pg_mentat.md) first.

### Step 2: Build mentatd

```bash
cd mentat

# Build release binary
cargo build --release -p mentatd

# Verify build
./target/release/mentatd --version
```

### Step 3: Install Binary

```bash
# Option 1: Copy to system path
sudo cp target/release/mentatd /usr/local/bin/

# Option 2: Add to PATH
export PATH="$PATH:$(pwd)/target/release"

# Option 3: Run directly
./target/release/mentatd
```

## Configuration

### Configuration File

Create `mentatd.toml`:

```toml
[server]
# Server bind address
host = "127.0.0.1"

# Server port
port = 8080

# Request timeout in seconds
timeout = 30

[database]
# PostgreSQL connection string
connection_string = "postgresql://postgres:postgres@localhost:5432/mentat"

# Connection pool size
pool_size = 10

# Maximum connection lifetime in seconds
max_lifetime_secs = 1800

[logging]
# Log level: error, warn, info, debug, trace
level = "info"

# Log format: compact, pretty, json
format = "compact"
```

### Environment Variables

Alternatively, configure via environment variables:

```bash
# Server settings
export MENTATD_HOST="127.0.0.1"
export MENTATD_PORT="8080"
export MENTATD_TIMEOUT="30"

# Database settings
export DATABASE_URL="postgresql://postgres:postgres@localhost:5432/mentat"
export DATABASE_POOL_SIZE="10"
export DATABASE_MAX_LIFETIME="1800"

# Logging settings
export RUST_LOG="info"
export LOG_FORMAT="compact"
```

Configuration priority (highest to lowest):
1. Environment variables
2. Configuration file (`mentatd.toml`)
3. Default values

## Running mentatd

### Foreground Mode

```bash
# Run with configuration file
mentatd

# Run with custom config path
mentatd --config /path/to/mentatd.toml

# Run with environment variables only
DATABASE_URL="postgresql://localhost/mentat" mentatd
```

### Background Mode (Linux)

Using systemd:

```bash
# Create service file
sudo nano /etc/systemd/system/mentatd.service
```

```ini
[Unit]
Description=mentatd - Datomic-compatible HTTP server for Mentat
After=postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=mentat
Group=mentat
WorkingDirectory=/opt/mentatd
ExecStart=/usr/local/bin/mentatd
Restart=on-failure
RestartSec=5

# Environment
Environment="DATABASE_URL=postgresql://mentat:password@localhost:5432/mentat"
Environment="RUST_LOG=info"

# Resource limits
LimitNOFILE=65536
MemoryLimit=2G

[Install]
WantedBy=multi-user.target
```

```bash
# Reload systemd
sudo systemctl daemon-reload

# Enable autostart
sudo systemctl enable mentatd

# Start service
sudo systemctl start mentatd

# Check status
sudo systemctl status mentatd

# View logs
sudo journalctl -u mentatd -f
```

### Background Mode (macOS)

Using launchd:

```bash
# Create plist file
nano ~/Library/LaunchAgents/com.mentat.mentatd.plist
```

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.mentat.mentatd</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/mentatd</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>DATABASE_URL</key>
        <string>postgresql://localhost/mentat</string>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/mentatd.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/mentatd.error.log</string>
</dict>
</plist>
```

```bash
# Load service
launchctl load ~/Library/LaunchAgents/com.mentat.mentatd.plist

# Unload service
launchctl unload ~/Library/LaunchAgents/com.mentat.mentatd.plist
```

## Verification

### Health Check

```bash
# Check server is running
curl http://127.0.0.1:8080/health

# Expected response: HTTP 200 OK
{"status":"healthy"}
```

### List Databases

```bash
# Using curl with EDN
curl -X POST http://127.0.0.1:8080/ \
  -H "Content-Type: application/edn" \
  -H "Accept: application/edn" \
  -d '{:op :list-dbs}'

# Expected response:
# {:result ["mentat" "test-db"]}
```

### Connect to Database

```bash
curl -X POST http://127.0.0.1:8080/ \
  -H "Content-Type: application/edn" \
  -d '{:op :connect :args {:db-name "mentat"}}'

# Expected response:
# {:result {:connection-id "uuid-here" :db-name "mentat" :status "connected"}}
```

## Database Setup

### Initialize Database

```bash
# Create PostgreSQL database
createdb mentat

# Initialize with pg_mentat extension
psql mentat -c "CREATE EXTENSION pg_mentat;"
```

### Configure Connection Pool

Adjust pool size based on expected load:

```toml
[database]
# Small workload
pool_size = 5

# Medium workload
pool_size = 10

# High workload
pool_size = 25

# Maximum lifetime before recycling connections
max_lifetime_secs = 1800
```

## Security

### Authentication

mentatd currently supports no authentication (localhost only). For production:

**Coming Soon:**
- Token-based authentication
- Mutual TLS
- API keys

**Current Recommendation:**
- Run on localhost only
- Use firewall rules to restrict access
- Place behind reverse proxy with authentication

### Network Security

```bash
# Bind to localhost only (default)
export MENTATD_HOST="127.0.0.1"

# Or bind to specific interface
export MENTATD_HOST="10.0.0.5"

# NEVER bind to all interfaces in production
# export MENTATD_HOST="0.0.0.0"  # DON'T DO THIS
```

### PostgreSQL Credentials

Secure connection string:

```bash
# Use environment variable (not config file)
export DATABASE_URL="postgresql://mentat:$(cat /run/secrets/db-password)@localhost/mentat"

# Or use .pgpass file
echo "localhost:5432:mentat:mentat:secret-password" > ~/.pgpass
chmod 600 ~/.pgpass

# Then connect without password in URL
export DATABASE_URL="postgresql://mentat@localhost/mentat"
```

## Monitoring

### Logs

```bash
# Tail logs (systemd)
sudo journalctl -u mentatd -f

# View specific time range
sudo journalctl -u mentatd --since "1 hour ago"

# JSON format for processing
export LOG_FORMAT="json"
mentatd | jq .
```

### Metrics

mentatd exposes metrics for monitoring:

```bash
# Get server metrics
curl http://127.0.0.1:8080/metrics

# Response includes:
# - Request count
# - Response times
# - Error rates
# - Connection pool stats
```

### Health Checks

```bash
# Simple health check
curl http://127.0.0.1:8080/health

# Detailed health with dependencies
curl http://127.0.0.1:8080/health/detailed

# Expected response:
{
  "status": "healthy",
  "database": "connected",
  "pool": "10/10 available"
}
```

## Performance Tuning

### Connection Pooling

```toml
[database]
# Set pool size based on expected concurrency
pool_size = 10

# Connections idle longer than this are closed
max_lifetime_secs = 1800

# Minimum idle connections
min_idle = 2
```

### Request Timeouts

```toml
[server]
# Overall request timeout
timeout = 30

# Database query timeout (PostgreSQL side)
# Set in postgresql.conf:
# statement_timeout = 30000  # 30 seconds
```

### Memory Limits

```bash
# Limit memory usage (systemd)
sudo systemctl edit mentatd

[Service]
MemoryLimit=2G
MemoryMax=4G
```

## Upgrading

### Upgrade mentatd

```bash
# Pull latest code
cd mentat
git pull

# Rebuild
cargo build --release -p mentatd

# Stop service
sudo systemctl stop mentatd

# Install new binary
sudo cp target/release/mentatd /usr/local/bin/

# Start service
sudo systemctl start mentatd

# Verify
curl http://127.0.0.1:8080/health
```

### Upgrade pg_mentat Extension

After upgrading the pg_mentat extension:

```bash
# Restart mentatd to use new extension features
sudo systemctl restart mentatd
```

## Troubleshooting

### Server won't start

**Error:** `Address already in use`

**Solution:** Change port or stop conflicting service:

```bash
# Check what's using port 8080
lsof -i :8080

# Use different port
export MENTATD_PORT="8081"
mentatd
```

### Database connection fails

**Error:** `connection refused`

**Solution:** Verify PostgreSQL is running and accessible:

```bash
# Test connection
psql -h localhost -U postgres -d mentat

# Check connection string
echo $DATABASE_URL
```

**Error:** `extension "pg_mentat" not found`

**Solution:** Install pg_mentat extension:

```bash
psql mentat -c "CREATE EXTENSION pg_mentat;"
```

### Requests timeout

**Solution:** Increase timeout:

```bash
export MENTATD_TIMEOUT="60"
mentatd
```

Or in PostgreSQL:

```sql
ALTER DATABASE mentat SET statement_timeout = '60s';
```

### High memory usage

**Solution:** Reduce connection pool size:

```toml
[database]
pool_size = 5
max_lifetime_secs = 900
```

### Slow queries

**Solution:** Check PostgreSQL logs and add indexes:

```sql
-- Enable query logging
ALTER DATABASE mentat SET log_min_duration_statement = 1000;

-- Check slow queries
SELECT * FROM pg_stat_statements ORDER BY mean_exec_time DESC LIMIT 10;

-- Add indexes on frequently queried attributes
UPDATE mentat.schema SET indexed = true WHERE ident = 'person:email';
```

## Development Setup

### Running in Development

```bash
cd mentat/mentatd

# Run with debug logging
RUST_LOG=debug cargo run

# Run with auto-reload
cargo install cargo-watch
cargo watch -x 'run'

# Run tests
cargo test -p mentatd
```

### Testing with Datomic Client

```clojure
;; Clojure client example
(require '[datomic.client.api :as d])

(def cfg {:server-type :peer-server
          :access-key "myaccesskey"
          :secret "mysecret"
          :endpoint "localhost:8080"})

(def client (d/client cfg))
(def conn (d/connect client {:db-name "mentat"}))

(def db (d/db conn))

;; Run query
(d/q '[:find ?name
       :where [?e :person/name ?name]]
     db)
```

## Uninstalling

### Remove Binary

```bash
# Remove binary
sudo rm /usr/local/bin/mentatd

# Remove systemd service
sudo systemctl stop mentatd
sudo systemctl disable mentatd
sudo rm /etc/systemd/system/mentatd.service
sudo systemctl daemon-reload
```

### Remove Data

```bash
# Drop PostgreSQL database
dropdb mentat

# Remove configuration
rm mentatd.toml
```

## Next Steps

- [Datomic Protocol Specification](../architecture/datomic_protocol.md) - Wire protocol details
- [Configuration Guide](../configuration/mentatd_config.md) - Detailed configuration
- [Migration Guide](../guides/migration_guide.md) - Migrate from Datomic
- [Performance Guide](../guides/performance.md) - Tuning and optimization
