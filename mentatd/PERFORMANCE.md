# mentatd Performance Guide

## Running Benchmarks

### Serialization Micro-benchmarks (no database required)

```bash
# Run all serialization benchmarks
cargo bench -p mentatd --bench serialization

# Run only EDN benchmarks
cargo bench -p mentatd --bench serialization -- edn_

# Run only Transit+JSON benchmarks
cargo bench -p mentatd --bench serialization -- transit_json_

# Run only Transit+MessagePack benchmarks
cargo bench -p mentatd --bench serialization -- transit_msgpack_

# Run format comparison (all three formats, same data)
cargo bench -p mentatd --bench serialization -- format_comparison

# Run output size comparison
cargo bench -p mentatd --bench serialization -- output_size
```

### Cache Micro-benchmarks (no database required)

```bash
# Run all cache benchmarks
cargo bench -p mentatd --bench cache

# Run only cache hit benchmarks
cargo bench -p mentatd --bench cache -- cache_hit

# Run only cache invalidation benchmarks
cargo bench -p mentatd --bench cache -- cache_invalidate
```

### Load Tests (requires running mentatd + PostgreSQL)

```bash
# Start mentatd
cargo run -p mentatd --release

# In another terminal, run load tests
./mentatd/benches/load_test.sh localhost 8484

# Custom host/port
./mentatd/benches/load_test.sh 192.168.1.10 8484
```

Criterion reports are generated in `target/criterion/` with HTML charts.

## Serialization Format Comparison

mentatd supports three response formats, selected via the `Accept` header:

| Format | Header | Use Case |
|--------|--------|----------|
| EDN | `application/edn` (default) | Human-readable, REPL use |
| Transit+JSON | `application/transit+json` | Web clients, debugging |
| Transit+MessagePack | `application/transit+msgpack` | Production, max throughput |

### Expected Characteristics

- **EDN**: Simplest format, minimal overhead. Good baseline.
- **Transit+JSON**: ~10-20% overhead vs EDN for serialization, but parseable by
  standard JSON parsers with Transit decoding layer.
- **Transit+MessagePack**: Smallest payload size (binary), fastest to parse on
  the client side. Slightly more serialization overhead than EDN due to binary
  encoding, but significantly less data over the wire.

## Query Cache

The built-in LRU query cache (`mentatd/src/cache.rs`) provides:

- **Cache hits**: Near-zero latency (mutex lock + LRU lookup)
- **Cache misses**: Full PostgreSQL round-trip
- **Invalidation**: Entire cache cleared after every transaction
- **TTL**: Entries expire after configurable duration (default 300s)

### Cache Configuration

In `mentatd.toml`:

```toml
[cache]
enabled = true
capacity = 1000    # Maximum number of cached query results
ttl_secs = 300     # Time-to-live in seconds
```

### Cache Tuning

- **High read, low write**: Increase `capacity` to 5000-10000
- **High write (frequent transactions)**: Cache may provide less benefit since
  it is fully invalidated on each transaction. Consider reducing `capacity` to
  save memory.
- **Memory constraint**: Each cached entry stores the raw JSON string from
  PostgreSQL. For large result sets, monitor memory usage and reduce `capacity`
  accordingly.

## PostgreSQL Tuning

mentatd performance is dominated by PostgreSQL query execution time. Key
settings to tune:

```
# postgresql.conf

# Memory
shared_buffers = 256MB           # 25% of available RAM
effective_cache_size = 768MB     # 75% of available RAM
work_mem = 16MB                  # Per-operation memory

# Connections
max_connections = 100            # Match mentatd pool size

# Query planning
random_page_cost = 1.1           # SSD storage
effective_io_concurrency = 200   # SSD storage

# WAL
wal_buffers = 16MB
checkpoint_completion_target = 0.9
```

### Connection Pool

mentatd uses `deadpool-postgres` for connection pooling. Configure in
`mentatd.toml`:

```toml
[database]
pool_size = 16     # Number of PostgreSQL connections
```

Rule of thumb: `pool_size` = number of CPU cores * 2. Too many connections
cause contention; too few cause queuing.

## Performance Targets

These are approximate targets for a typical deployment (4-core server,
PostgreSQL on SSD, ~1M datoms):

| Operation | Target Latency | Notes |
|-----------|---------------|-------|
| Health check | < 1ms | No DB access |
| List databases | < 10ms | Simple pg_database query |
| Connect | < 10ms | Database existence check |
| Simple query (1-2 patterns) | < 50ms | Depends on data size |
| Complex query (5+ patterns) | < 500ms | Depends on join complexity |
| Transaction (10 datoms) | < 50ms | Single PG function call |
| Transaction (100 datoms) | < 200ms | Batch insert |

## Monitoring

mentatd exposes Prometheus metrics at `/metrics`:

```
# Request counts
mentatd_requests_total
mentatd_errors_total

# Query performance
mentatd_queries_total
mentatd_query_duration_seconds (histogram)

# Cache performance
mentatd_cache_hits_total
mentatd_cache_misses_total

# Transactions
mentatd_transactions_total

# Connection pool
mentatd_connection_pool_size
```

### Key Metrics to Watch

- **Cache hit ratio**: `cache_hits / (cache_hits + cache_misses)`. Target > 80%
  for read-heavy workloads.
- **Query p99 latency**: From the `mentatd_query_duration_seconds` histogram.
  Spikes indicate PostgreSQL contention or complex queries.
- **Error rate**: `errors / requests`. Should be < 1% in normal operation.
- **Pool saturation**: If `connection_pool_size` equals `pool_size` config and
  latency is rising, increase the pool or optimize queries.

## Profiling

For deeper analysis, use Rust profiling tools:

```bash
# CPU profiling with flamegraph
cargo install flamegraph
cargo flamegraph -p mentatd --bench serialization

# Memory profiling with DHAT
# Add to Cargo.toml: dhat = { version = "0.3", optional = true }
# Run with: cargo run --features dhat -p mentatd
```
