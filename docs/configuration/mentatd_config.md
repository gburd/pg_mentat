# mentatd Configuration Guide

Complete configuration reference for the mentatd server.

## Configuration Methods

mentatd can be configured through:
1. Configuration file (`mentatd.toml`)
2. Environment variables
3. Command-line arguments

Priority (highest to lowest): CLI args → Environment variables → Config file → Defaults

## Configuration File

### Location

mentatd looks for `mentatd.toml` in:
1. Current working directory
2. `$HOME/.config/mentat/mentatd.toml`
3. `/etc/mentat/mentatd.toml`
4. Custom path via `--config` flag

### Full Configuration Example

```toml
[server]
# Server bind address
# Default: "127.0.0.1"
host = "127.0.0.1"

# Server port
# Default: 8080
port = 8080

# Request timeout in seconds
# Default: 30
timeout = 30

# Maximum request body size in bytes
# Default: 10485760 (10MB)
max_body_size = 10485760

# Worker threads (0 = number of CPUs)
# Default: 0
workers = 0

[database]
# PostgreSQL connection string
# Required
connection_string = "postgresql://postgres:postgres@localhost:5432/mentat"

# Connection pool size
# Default: 10
pool_size = 10

# Maximum connection lifetime in seconds
# Default: 1800 (30 minutes)
max_lifetime_secs = 1800

# Minimum idle connections
# Default: 2
min_idle = 2

# Connection timeout in seconds
# Default: 30
connection_timeout = 30

# Statement timeout in milliseconds
# Default: 30000 (30 seconds)
statement_timeout = 30000

[logging]
# Log level: error, warn, info, debug, trace
# Default: "info"
level = "info"

# Log format: compact, pretty, json
# Default: "compact"
format = "compact"

# Log file path (optional, stdout if not set)
# Default: none
file = "/var/log/mentatd/mentatd.log"

# Log rotation size in bytes
# Default: 104857600 (100MB)
rotation_size = 104857600

# Number of log files to keep
# Default: 5
rotation_count = 5

[security]
# Enable authentication (when implemented)
# Default: false
auth_enabled = false

# API key header name
# Default: "X-API-Key"
api_key_header = "X-API-Key"

# Allowed origins for CORS
# Default: []
cors_origins = ["http://localhost:3000", "https://app.example.com"]

# Enable HTTPS
# Default: false
tls_enabled = false

# TLS certificate path
tls_cert = "/etc/mentat/tls/cert.pem"

# TLS private key path
tls_key = "/etc/mentat/tls/key.pem"

[metrics]
# Enable metrics endpoint
# Default: true
enabled = true

# Metrics endpoint path
# Default: "/metrics"
path = "/metrics"

# Prometheus format
# Default: true
prometheus_format = true

[performance]
# Query cache size (number of queries)
# Default: 1000
query_cache_size = 1000

# Query cache TTL in seconds
# Default: 300 (5 minutes)
query_cache_ttl = 300

# Enable query result streaming
# Default: false
streaming_enabled = false

# Streaming chunk size
# Default: 100
streaming_chunk_size = 100
```

## Environment Variables

All configuration options can be set via environment variables:

### Server Settings

```bash
# Server bind address
export MENTATD_HOST="127.0.0.1"

# Server port
export MENTATD_PORT="8080"

# Request timeout (seconds)
export MENTATD_TIMEOUT="30"

# Maximum request body size (bytes)
export MENTATD_MAX_BODY_SIZE="10485760"

# Worker threads
export MENTATD_WORKERS="4"
```

### Database Settings

```bash
# PostgreSQL connection string (required)
export DATABASE_URL="postgresql://postgres:postgres@localhost:5432/mentat"

# Connection pool size
export DATABASE_POOL_SIZE="10"

# Connection lifetime (seconds)
export DATABASE_MAX_LIFETIME="1800"

# Minimum idle connections
export DATABASE_MIN_IDLE="2"

# Connection timeout (seconds)
export DATABASE_CONNECTION_TIMEOUT="30"

# Statement timeout (milliseconds)
export DATABASE_STATEMENT_TIMEOUT="30000"
```

### Logging Settings

```bash
# Log level
export RUST_LOG="info"
# Or more granular:
export RUST_LOG="mentatd=debug,hyper=info"

# Log format
export LOG_FORMAT="compact"  # compact, pretty, json

# Log file path
export LOG_FILE="/var/log/mentatd/mentatd.log"

# Log rotation size
export LOG_ROTATION_SIZE="104857600"

# Log rotation count
export LOG_ROTATION_COUNT="5"
```

### Security Settings

```bash
# Enable authentication
export MENTATD_AUTH_ENABLED="false"

# API key header
export MENTATD_API_KEY_HEADER="X-API-Key"

# CORS origins (comma-separated)
export MENTATD_CORS_ORIGINS="http://localhost:3000,https://app.example.com"

# Enable TLS
export MENTATD_TLS_ENABLED="false"

# TLS certificate
export MENTATD_TLS_CERT="/etc/mentat/tls/cert.pem"

# TLS private key
export MENTATD_TLS_KEY="/etc/mentat/tls/key.pem"
```

## Command-Line Arguments

```bash
mentatd --help

Usage: mentatd [OPTIONS]

Options:
  -c, --config <FILE>          Configuration file path
  -h, --host <HOST>            Server bind address
  -p, --port <PORT>            Server port
  -d, --database <URL>         Database connection string
      --log-level <LEVEL>      Log level (error, warn, info, debug, trace)
      --log-format <FORMAT>    Log format (compact, pretty, json)
      --help                   Print help
      --version                Print version
```

**Examples:**

```bash
# Basic usage
mentatd

# Custom config file
mentatd --config /etc/mentat/prod.toml

# Override port
mentatd --port 9090

# Override database
mentatd --database postgresql://localhost/mentat

# Debug logging
mentatd --log-level debug --log-format pretty

# Multiple overrides
mentatd --config /etc/mentat/base.toml \
        --port 9090 \
        --database postgresql://db.example.com/mentat
```

## Configuration Scenarios

### Development

```toml
[server]
host = "127.0.0.1"
port = 8080
timeout = 60  # Longer timeout for debugging

[database]
connection_string = "postgresql://localhost/mentat_dev"
pool_size = 5

[logging]
level = "debug"
format = "pretty"
```

Or with environment variables:

```bash
export DATABASE_URL="postgresql://localhost/mentat_dev"
export RUST_LOG="debug"
export LOG_FORMAT="pretty"
mentatd
```

### Production

```toml
[server]
host = "0.0.0.0"  # Bind to all interfaces
port = 8080
timeout = 30
workers = 8  # Use multiple workers

[database]
connection_string = "postgresql://mentat:${DB_PASSWORD}@db.internal:5432/mentat"
pool_size = 25
max_lifetime_secs = 1800
statement_timeout = 30000

[logging]
level = "info"
format = "json"
file = "/var/log/mentatd/mentatd.log"
rotation_size = 104857600  # 100MB
rotation_count = 10

[security]
tls_enabled = true
tls_cert = "/etc/mentat/tls/cert.pem"
tls_key = "/etc/mentat/tls/key.pem"
cors_origins = ["https://app.example.com"]

[metrics]
enabled = true
```

### High Availability

```toml
[server]
host = "0.0.0.0"
port = 8080
timeout = 30
workers = 16

[database]
# Read-write primary
connection_string = "postgresql://mentat@db-primary.internal/mentat"
pool_size = 50
max_lifetime_secs = 900  # Shorter lifetime for faster failover

# Read replica (when supported)
read_replica_url = "postgresql://mentat@db-replica.internal/mentat"

[logging]
level = "info"
format = "json"

[metrics]
enabled = true
prometheus_format = true
```

### Docker

```toml
[server]
host = "0.0.0.0"
port = 8080

[database]
# Use Docker service names
connection_string = "postgresql://postgres:postgres@postgres:5432/mentat"
pool_size = 10

[logging]
level = "info"
format = "json"
# Log to stdout for Docker
```

Or with Docker environment variables:

```bash
docker run -d \
  -e DATABASE_URL="postgresql://postgres@postgres/mentat" \
  -e RUST_LOG="info" \
  -e LOG_FORMAT="json" \
  -p 8080:8080 \
  mentat/mentatd
```

## Database Connection Strings

### Basic Format

```
postgresql://[user[:password]@][host][:port][/database][?parameters]
```

### Examples

```bash
# Local connection
postgresql://localhost/mentat

# With credentials
postgresql://mentat:secret@localhost/mentat

# Remote host with port
postgresql://mentat:secret@db.example.com:5432/mentat

# SSL mode
postgresql://mentat@localhost/mentat?sslmode=require

# Multiple parameters
postgresql://mentat@localhost/mentat?sslmode=require&connect_timeout=10

# Unix socket
postgresql:///mentat?host=/var/run/postgresql

# Using .pgpass file (no password in URL)
postgresql://mentat@localhost/mentat
# Requires ~/.pgpass: localhost:5432:mentat:mentat:password
```

### Connection Parameters

| Parameter | Values | Default | Description |
|-----------|--------|---------|-------------|
| sslmode | disable, allow, prefer, require, verify-ca, verify-full | prefer | SSL mode |
| connect_timeout | seconds | 0 (no timeout) | Connection timeout |
| application_name | string | mentatd | Application name in pg_stat_activity |
| options | string | | PostgreSQL options (-c key=value) |
| target_session_attrs | any, read-write, read-only | any | Session attributes |

## Security Configuration

### TLS/HTTPS Setup

Generate self-signed certificate for testing:

```bash
openssl req -x509 -newkey rsa:4096 \
  -keyout key.pem -out cert.pem \
  -days 365 -nodes \
  -subj "/CN=localhost"
```

Configure mentatd:

```toml
[security]
tls_enabled = true
tls_cert = "/etc/mentat/tls/cert.pem"
tls_key = "/etc/mentat/tls/key.pem"
```

### CORS Configuration

```toml
[security]
# Allow specific origins
cors_origins = [
  "http://localhost:3000",
  "https://app.example.com"
]

# Allow credentials
cors_credentials = true

# Allowed methods
cors_methods = ["GET", "POST", "OPTIONS"]

# Allowed headers
cors_headers = ["Content-Type", "Authorization"]
```

### API Key Authentication (Coming Soon)

```toml
[security]
auth_enabled = true
api_key_header = "X-API-Key"

# Keys stored in separate file
api_keys_file = "/etc/mentat/api-keys.txt"
```

Format of api-keys.txt:
```
# Comment lines start with #
key1:read,write
key2:read
key3:admin
```

## Performance Tuning

### Connection Pooling

```toml
[database]
# Set based on expected concurrent requests
pool_size = 25

# Keep some connections warm
min_idle = 5

# Recycle connections periodically
max_lifetime_secs = 1800

# Don't wait too long for a connection
connection_timeout = 10
```

Guidelines:
- `pool_size` ≈ expected concurrent requests
- `pool_size` should not exceed PostgreSQL max_connections
- `min_idle` = 10-20% of pool_size
- `max_lifetime_secs` = 15-30 minutes

### Worker Threads

```toml
[server]
# 0 = number of CPU cores (recommended)
workers = 0

# Or set explicitly
workers = 8
```

Guidelines:
- Start with 0 (auto-detect)
- Increase if CPU-bound
- Decrease if memory-constrained

### Query Caching

```toml
[performance]
# Cache up to 1000 query plans
query_cache_size = 1000

# Cache entries expire after 5 minutes
query_cache_ttl = 300
```

### Request Limits

```toml
[server]
# Maximum request body size
max_body_size = 10485760  # 10MB

# Request timeout
timeout = 30

# Concurrent request limit (per worker)
max_concurrent_requests = 100
```

## Monitoring Configuration

### Metrics

```toml
[metrics]
enabled = true
path = "/metrics"
prometheus_format = true

# Metrics to expose
include = [
  "requests_total",
  "request_duration",
  "active_connections",
  "pool_connections"
]
```

### Health Checks

```toml
[health]
# Health check endpoint
path = "/health"

# Detailed health info
detailed_path = "/health/detailed"

# Include dependency checks
check_database = true
check_redis = false
```

### Logging Configuration

```toml
[logging]
# Structured JSON logging for aggregation
format = "json"

# Log file with rotation
file = "/var/log/mentatd/mentatd.log"
rotation_size = 104857600  # 100MB
rotation_count = 10

# Log levels per module
[logging.modules]
mentatd = "info"
mentatd::query = "debug"
hyper = "warn"
tokio = "error"
```

## Validation

Validate configuration file:

```bash
# Test configuration
mentatd --config mentatd.toml --validate

# Dry run (don't start server)
mentatd --config mentatd.toml --dry-run

# Print effective configuration
mentatd --config mentatd.toml --print-config
```

## Environment-Specific Configs

### Using Multiple Files

```bash
# Base configuration
# config/base.toml

# Environment-specific overrides
# config/dev.toml
# config/staging.toml
# config/prod.toml

# Load with precedence
mentatd --config config/base.toml --config config/prod.toml
```

### Environment Variable Substitution

```toml
[database]
# Use ${VAR} or ${VAR:-default} syntax
connection_string = "postgresql://mentat:${DB_PASSWORD}@${DB_HOST:-localhost}/mentat"

[security]
tls_cert = "${CERTS_DIR}/cert.pem"
tls_key = "${CERTS_DIR}/key.pem"
```

## Troubleshooting

### Configuration Not Loaded

Check configuration search paths:

```bash
# Print search paths
mentatd --config-paths

# Use absolute path
mentatd --config /absolute/path/to/mentatd.toml

# Verify file exists and is readable
ls -l mentatd.toml
```

### Database Connection Fails

Test connection string:

```bash
# Test with psql
psql "postgresql://mentat@localhost/mentat"

# Check connection parameters
export DATABASE_URL="postgresql://mentat@localhost/mentat"
psql $DATABASE_URL
```

### Port Already in Use

```bash
# Find what's using the port
lsof -i :8080

# Use different port
mentatd --port 8081

# Or kill conflicting process
sudo kill $(lsof -t -i:8080)
```

### Permission Denied

```bash
# Check file permissions
ls -l mentatd.toml

# Make readable
chmod 644 mentatd.toml

# Check log directory permissions
ls -ld /var/log/mentatd
sudo chown mentat:mentat /var/log/mentatd
```

## Best Practices

1. **Never commit secrets** - Use environment variables for passwords
2. **Use TLS in production** - Always enable TLS for remote connections
3. **Limit connection pool** - Set pool_size < PostgreSQL max_connections
4. **Enable monitoring** - Always enable metrics in production
5. **Rotate logs** - Configure log rotation to prevent disk full
6. **Validate configuration** - Test config before deploying
7. **Document changes** - Comment non-obvious configuration
8. **Version control** - Keep configs in git (without secrets)

## See Also

- [Installation Guide](../installation/mentatd.md)
- [Datomic Protocol](../architecture/datomic_protocol.md)
- [Performance Guide](../guides/performance.md)
- [Security Best Practices](../guides/security.md)
