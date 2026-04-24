# Load Testing Infrastructure for pg_mentat

## Overview

This directory contains comprehensive load testing infrastructure for validating pg_mentat's performance claims and identifying production bottlenecks.

## Status

**Infrastructure**: ✅ Complete
**Actual Test Execution**: ⏸️ Pending deployment of test environment

## What's Included

### 1. Test Scripts

- **`load_test.sh`**: Main test orchestration script
  - Supports multiple test scenarios (steady, spike, soak, stress, mixed)
  - Configurable duration, VUs (virtual users), and target TPS
  - Automatically saves results to `results/` directory

- **`analyze_results.py`**: Results analysis script
  - Parses k6 JSON output
  - Calculates statistics (p50/p95/p99 latency, TPS, error rates)
  - Generates performance reports

- **`mock_server.py`**: Mock mentatd server (NEW)
  - Simulates EDN API responses with realistic latencies (30ms ± 10ms)
  - Useful for infrastructure validation without full deployment
  - Tracks request metrics (throughput, error rates, response times)

### 2. Test Scenarios (`scenarios/`)

1. **steady_state.js**: Baseline performance testing
   - 50 VUs for extended duration (default 1 hour)
   - Validates sustained throughput claims (50+ TPS)
   - Measures p99 latency under normal load

2. **spike.js**: Elasticity testing
   - Rapid ramp from 10 → 500 → 10 VUs
   - Tests system recovery under sudden load spikes
   - Identifies connection pool exhaustion points

3. **large_queries.js**: Complex query testing
   - Tests queries with large result sets
   - Validates query timeout enforcement
   - Measures memory usage patterns

4. **mixed_workload.js**: Realistic workload simulation
   - 70% reads (queries), 20% writes (transactions), 10% pulls
   - Tests under production-like access patterns
   - Validates cache effectiveness

5. **concurrent_writes.js**: Transaction throughput testing
   - High concurrency write operations
   - Validates sequence-based entity allocation (no lock contention)
   - Tests for transaction serialization issues

## Performance Claims to Validate

From the expert reviews and production readiness plan:

| Claim | Target | Test Scenario | Status |
|-------|--------|---------------|--------|
| Sustained throughput | 50+ TPS | steady_state.js | ⏸️ Pending |
| p99 latency | < 100ms | steady_state.js | ⏸️ Pending |
| p50 latency | < 50ms | steady_state.js | ⏸️ Pending |
| Error rate | < 0.1% | All scenarios | ⏸️ Pending |
| Write throughput | 10K+ datoms/sec | concurrent_writes.js | ⏸️ Pending |
| Complex query latency | < 500ms p99 | large_queries.js | ⏸️ Pending |
| Spike recovery | < 5 seconds | spike.js | ⏸️ Pending |
| Memory stability | No leaks | Soak test (12h) | ⏸️ Pending |
| Scalability | No degradation up to 10M datoms | Stress test | ⏸️ Pending |

## How to Run Tests

### Prerequisites

1. **PostgreSQL with pg_mentat extension**:
   ```bash
   cargo pgrx install
   psql -c "CREATE EXTENSION pg_mentat;"
   ```

2. **mentatd gateway running**:
   ```bash
   cd mentatd
   cargo run --release
   # Server should be listening on http://localhost:8080
   ```

3. **k6 load testing tool**:
   ```bash
   # Install k6
   brew install k6  # macOS
   sudo apt install k6  # Debian/Ubuntu
   # Or download from https://k6.io/docs/getting-started/installation/
   ```

### Running Tests

#### 1. Steady State Test (Baseline Performance)
```bash
cd benchmarks
./load_test.sh steady --duration 3600  # 1 hour
```

**Expected results**:
- Throughput: 50-100 TPS sustained
- p99 latency: < 100ms
- Error rate: < 0.1%
- Memory usage: Stable (no growth)

#### 2. Spike Test (Elasticity)
```bash
./load_test.sh spike --duration 600  # 10 minutes
```

**Expected results**:
- System handles 50x spike (10 → 500 VUs)
- Recovery time < 5 seconds after spike
- No connection pool exhaustion
- Error rate during spike < 1%

#### 3. Soak Test (Memory Leak Detection)
```bash
./load_test.sh steady --duration 43200  # 12 hours
```

**Expected results**:
- Memory usage remains constant
- No performance degradation over time
- Connection pool remains healthy
- No resource leaks

#### 4. Stress Test (Breaking Point)
```bash
./load_test.sh stress --duration 1800  # 30 minutes
```

**Expected results**:
- Identify maximum sustainable throughput
- Graceful degradation (no crashes)
- Clear error messages when capacity exceeded
- System recovers after load reduction

#### 5. Mixed Workload (Production Simulation)
```bash
./load_test.sh mixed --duration 3600  # 1 hour
```

**Expected results**:
- 70% reads, 20% writes, 10% pulls
- Cache hit rate > 80% (after warm-up)
- Transaction throughput > 10 TPS
- Query throughput > 40 TPS

### Analyzing Results

After running tests, analyze the results:

```bash
python analyze_results.py results/*
```

This generates:
- Latency distribution (p50/p95/p99)
- Throughput over time
- Error rate analysis
- Resource usage patterns
- Comparison against targets

Results should be documented in `LOAD_TEST_RESULTS.md`.

## Using the Mock Server

For infrastructure validation without full deployment:

```bash
# Terminal 1: Start mock server
./mock_server.py --port 8080

# Terminal 2: Run tests against mock
./load_test.sh steady --duration 60  # 1 minute
```

**Note**: Mock server provides infrastructure validation only. Actual performance must be measured against real mentatd + PostgreSQL.

## What the Mock Server Tests

- HTTP request handling
- EDN protocol parsing
- Concurrent request handling
- Basic throughput metrics

**Mock server simulates**:
- Realistic latencies (30ms ± 10ms)
- Various operations (:q, :transact, :pull, etc.)
- Error scenarios
- Transaction responses

## Production Deployment Recommendations

### Before Load Testing

1. **Use production-like hardware**:
   - PostgreSQL: 4+ cores, 16GB+ RAM, SSD storage
   - mentatd: 2+ cores, 4GB+ RAM

2. **Configure PostgreSQL for performance**:
   ```sql
   -- In postgresql.conf
   shared_buffers = 4GB
   effective_cache_size = 12GB
   maintenance_work_mem = 1GB
   checkpoint_completion_target = 0.9
   wal_buffers = 16MB
   default_statistics_target = 100
   random_page_cost = 1.1  -- for SSD
   effective_io_concurrency = 200
   work_mem = 64MB  -- matches mentat.default_work_mem
   max_connections = 200
   ```

3. **Enable monitoring**:
   - Prometheus metrics (mentatd exports on :9090)
   - PostgreSQL stats (pg_stat_statements)
   - System metrics (CPU, memory, disk I/O)

4. **Populate test data**:
   ```sql
   -- Insert realistic data volume (1M-10M datoms)
   -- Include schema with various value types
   -- Mix of cardinality :one and :many attributes
   -- Entity IDs spanning realistic ranges
   ```

### During Load Testing

Monitor these metrics:

**mentatd**:
- Request rate (req/s)
- Response time (p50/p95/p99)
- Error rate (%)
- Cache hit rate (%)
- Connection pool utilization

**PostgreSQL**:
- Active connections
- Transaction rate (TPS)
- Buffer cache hit ratio (should be > 95%)
- Checkpoint frequency
- Slow queries (> 500ms)

**System**:
- CPU utilization (should be < 80%)
- Memory usage (watch for leaks)
- Disk I/O (write latency)
- Network throughput

### After Load Testing

Document results in `LOAD_TEST_RESULTS.md`:

```markdown
# Load Test Results - [Date]

## Test Environment
- Hardware: [specs]
- PostgreSQL version: [version]
- pg_mentat version: [version]
- mentatd version: [version]
- Data volume: [X million datoms]

## Results Summary

### Steady State (1 hour)
- Throughput: [X] TPS (target: 50+) ✅/❌
- p50 latency: [X]ms (target: < 50ms) ✅/❌
- p99 latency: [X]ms (target: < 100ms) ✅/❌
- Error rate: [X]% (target: < 0.1%) ✅/❌

[... results for each test scenario ...]

## Bottlenecks Identified
1. [Bottleneck description]
   - Impact: [performance impact]
   - Recommendation: [how to fix]

## Performance Tuning Recommendations
1. [Specific recommendation]
2. [Another recommendation]

## Production Readiness Assessment
- [ ] All performance targets met
- [ ] No memory leaks detected
- [ ] System recovers from spikes
- [ ] Error handling is graceful
- [ ] Monitoring is comprehensive
```

## Known Limitations

1. **Load test environment != production**: Results depend heavily on:
   - Hardware specs (CPU, RAM, disk)
   - Network latency (local vs remote)
   - Data volume and distribution
   - Concurrent user patterns

2. **Warm-up required**: First 100-1000 queries will be slower due to:
   - PostgreSQL query planner warming up
   - Statement cache population
   - Buffer cache warming up
   - OS page cache effects

3. **Test isolation**: Results can be affected by:
   - Other processes on the same machine
   - PostgreSQL autovacuum running
   - Disk I/O from other sources
   - Network congestion

## Next Steps

1. **Deploy test environment** with production-like specifications
2. **Populate test data** (1M-10M datoms with realistic distribution)
3. **Run all test scenarios** and collect results
4. **Analyze results** using analyze_results.py
5. **Document findings** in LOAD_TEST_RESULTS.md
6. **Tune performance** based on bottlenecks identified
7. **Re-test** after tuning to validate improvements
8. **Certify production readiness** once all targets are met

## References

- k6 documentation: https://k6.io/docs/
- PostgreSQL performance tuning: https://wiki.postgresql.org/wiki/Performance_Optimization
- pg_mentat planner hooks: `../pg_mentat/src/planner/hooks.rs`
- Performance targets: `../docs/PRODUCTION_READINESS_ASSESSMENT.md`
- Expert reviews: `../docs/EXPERT_REVIEWS.md`

## Contact

For questions about load testing infrastructure:
- Check existing test scenarios in `scenarios/`
- Review analyze_results.py for metrics calculation
- See mock_server.py for EDN protocol examples
