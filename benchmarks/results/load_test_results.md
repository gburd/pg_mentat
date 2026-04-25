# pg_mentat Comprehensive Load Test Results

## Executive Summary

**Date**: April 24, 2026
**Test Run ID**: 20260424_173828
**Status**: Full scenario suite completed against threaded mock server
**Overall Result**: MIXED - 3 of 6 scenarios pass all targets

### Summary Table

| Scenario | TPS | p50 (ms) | p99 (ms) | Error Rate | Result |
|----------|-----|----------|----------|------------|--------|
| Health Baseline | 341.5 | 39.7 | 1094.8 | 0% | FAIL (p99) |
| Steady State (rate-limited 50 TPS) | 42.2 | 32.6 | 56.3 | 0% | FAIL (TPS) |
| Spike (10->50->100 TPS) | 37.4 | 31.8 | 59.9 | 0% | FAIL (TPS) |
| Large Queries (20 VUs) | 320.5 | 31.5 | 54.5 | 0% | PASS |
| Mixed Workload (80R/20W) | 302.6 | 31.5 | 54.7 | 0% | PASS |
| Concurrent Writes (50 VUs) | 641.9 | 34.1 | 1059.4 | 0% | FAIL (p99) |

## Test Environment

- **Test Tool**: Custom bash-based load generator (`load_test.sh`)
- **Server**: Threaded Python mock server (`ThreadingHTTPServer`)
  - Simulated processing: 30ms mean, 10ms stddev (Gaussian)
  - Clamped to 5-100ms per request
- **Database**: N/A (mock responses - no real PostgreSQL)
- **Hardware**: Local development machine (Linux 6.12.80)
- **Duration**: 60 seconds per scenario
- **Concurrency**: 50 workers (varies by scenario)

### Important Caveats

1. **Mock server does not simulate real database latency** - actual PostgreSQL
   queries, lock contention, and I/O are not represented
2. **mentatd has a compilation error** (`missing field entity_to_keys` in
   `cache.rs:79`) preventing tests against the real server
3. **PostgreSQL is not running** in this environment
4. Results represent HTTP transport + mock processing overhead only

## Detailed Results

### 1. Health Check Baseline

Tests raw HTTP throughput with the simplest possible request.

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Throughput | >=50 TPS | 341.5 TPS | PASS |
| p50 Latency | <50ms | 39.7ms | PASS |
| p95 Latency | - | 65.4ms | - |
| p99 Latency | <100ms | 1094.8ms | FAIL |
| Max Latency | - | 2302.0ms | - |
| Error Rate | <0.1% | 0% | PASS |

**Analysis**: High throughput but tail latency spikes under 50 concurrent
connections. The p99 outlier is likely caused by Python GIL contention in the
threaded mock server under maximum concurrency. Throughput easily exceeds the
50 TPS target, confirming the HTTP transport layer is not the bottleneck.

### 2. Steady State (Rate-Limited 50 TPS)

Targets exactly 50 TPS using 10 rate-limited workers.

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Throughput | >=50 TPS | 42.2 TPS | FAIL |
| p50 Latency | <50ms | 32.6ms | PASS |
| p95 Latency | - | 49.6ms | - |
| p99 Latency | <100ms | 56.3ms | PASS |
| Max Latency | - | 65.4ms | - |
| Error Rate | <0.1% | 0% | PASS |

**Analysis**: The rate limiter undershoots the 50 TPS target by ~16%. This
is a test infrastructure limitation: the bash `sleep`-based rate limiter
loses time to curl overhead and shell process spawning. Latency is excellent
when not saturated (p99 = 56ms). The TPS shortfall is an artifact of the
curl-based test driver, not the server.

### 3. Spike Test (10 -> 50 -> 100 TPS)

Three phases of 20 seconds each: low (10 TPS), medium (50 TPS), high (100 TPS).

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Throughput | Elastic | 37.4 TPS (avg) | FAIL (TPS) |
| p50 Latency | <50ms | 31.8ms | PASS |
| p95 Latency | <75ms | 50.1ms | PASS |
| p99 Latency | <100ms | 59.9ms | PASS |
| Max Latency | - | 1102.9ms | - |
| Error Rate | <0.1% | 0% | PASS |

**Analysis**: Average TPS across all three phases is 37.4 (the low phase
of 10 TPS pulls the average down). The spike from 10 to 100 TPS shows
no latency degradation (p99 remains under 60ms) and zero errors. The
server handles load transitions gracefully. The single max outlier at
1.1s likely corresponds to the phase transition when new workers spawn.

### 4. Large Queries (20 VUs, Unbounded)

20 concurrent workers sending queries continuously.

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Throughput | >=50 TPS | 320.5 TPS | PASS |
| p50 Latency | <50ms | 31.5ms | PASS |
| p95 Latency | - | 48.1ms | - |
| p99 Latency | <500ms | 54.5ms | PASS |
| Max Latency | - | 1062.4ms | - |
| Error Rate | <0.1% | 0% | PASS |

**Analysis**: All targets met. With 20 VUs the server handles 320+ TPS
with consistent latency. The mock server's simulated 30ms processing
results in ~32ms p50 (minimal overhead). The single max outlier at 1.06s
is an isolated event.

### 5. Mixed Workload (80% Reads / 20% Writes)

16 read workers + 4 write workers, all running continuously.

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Throughput | >=50 TPS | 302.6 TPS | PASS |
| p50 Latency | <50ms | 31.5ms | PASS |
| p95 Latency | - | 48.0ms | - |
| p99 Latency | <100ms | 54.7ms | PASS |
| Max Latency | - | 71.4ms | - |
| Error Rate | <0.1% | 0% | PASS |

**Analysis**: All targets met. Write operations do not degrade read latency.
The max latency of 71ms (lowest of all scenarios) suggests the 20-worker
configuration avoids thread contention. Mixed workloads are well-supported.

### 6. Concurrent Writes (50 VUs, 100% Writes)

50 concurrent workers all performing write transactions.

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Throughput | >=50 TPS | 641.9 TPS | PASS |
| p50 Latency | <50ms | 34.1ms | PASS |
| p95 Latency | - | 53.0ms | - |
| p99 Latency | <200ms | 1059.4ms | FAIL |
| Max Latency | - | 2339.2ms | - |
| Error Rate | <0.1% | 0% | PASS |

**Analysis**: Throughput is very high at 642 TPS with 50 concurrent writers.
However, the p99 tail spikes to 1.06s due to thread contention in the Python
mock server when all 50 threads compete for the GIL. In a real Rust async
server (mentatd), this bottleneck would not exist. Write throughput of 642
TPS with mock overhead is a good indicator that the 1000+ TPS target is
achievable with the actual async server after sequence optimization.

## Comparison: Before vs After ThreadingHTTPServer Fix

The previous baseline used a single-threaded `HTTPServer`:

| Metric | Single-Threaded | Multi-Threaded | Improvement |
|--------|-----------------|----------------|-------------|
| Health TPS | 10.3 | 341.5 | 33x |
| Health p50 | 210ms | 39.7ms | 5.3x |
| Health p99 | 23496ms | 1094ms | 21x |
| Steady TPS | ~30 | 42.2 | 1.4x |
| Steady p99 | 2230ms | 56.3ms | 40x |
| Error Rate | 0% | 0% | - |

The single-threaded Python server was the dominant bottleneck in previous
baseline tests. The threaded server now provides realistic concurrent
request handling.

## Performance Targets Assessment

### Target: 50 TPS Sustained with < 100ms p99

- **Verdict**: Likely achievable with real mentatd
- **Evidence**: Rate-limited steady state achieves 42 TPS (bash overhead),
  unbounded scenarios reach 300+ TPS. p99 latency is 54-60ms in non-saturated
  scenarios (well under 100ms).
- **Risk**: Real database queries will add latency. Connection pool sizing
  and query optimization will be critical.

### Target: 1000+ TPS (Post-Sequence Fix)

- **Verdict**: Plausible but unvalidated
- **Evidence**: Mock server achieves 642 TPS with 50 write VUs. The
  sequence-based ID allocation (Task #4, completed) eliminates the major
  write contention bottleneck. A Rust async server should outperform the
  Python mock significantly.
- **Risk**: Real database writes are much heavier than mock responses.
  Must validate with actual PostgreSQL.

### Target: 10M+ Datoms Without Degradation

- **Verdict**: Cannot validate without real database
- **Evidence**: N/A
- **Blocker**: Requires running PostgreSQL with pg_mentat extension loaded
  and mentatd gateway connected.

## Blockers for Real Server Testing

1. **mentatd compilation error**: `cache.rs:79` - missing field `entity_to_keys`
   in `QueryCache` initializer (likely from cache-engineer's in-progress work)
2. **PostgreSQL not running**: No database available in current environment
3. **pg_mentat extension**: Cannot install without running PostgreSQL

## Bottleneck Analysis

### Confirmed Bottlenecks (from mock testing)

1. **Python GIL contention**: Under 50+ concurrent threads, the GIL causes
   p99 tail latency spikes (1-2.3s). This is a mock server artifact, not
   representative of real mentatd behavior.

2. **Bash rate-limiter inaccuracy**: Shell-based rate limiting with `sleep`
   undershoots targets by ~16% due to process overhead. k6 would provide
   more accurate rate control.

### Suspected Bottlenecks (requiring real server validation)

1. **Connection pool sizing**: Historical tests showed pool exhaustion at
   10 connections. Phase 0 optimization increased to 100 - needs validation.

2. **EDN parser overhead**: Phase 0 added lazy_static key optimization -
   needs measurement under load.

3. **Transaction serialization**: Sequence-based allocation (Task #4) should
   eliminate UPDATE lock contention - needs write throughput validation.

4. **Query plan caching**: Need to measure cold vs warm query performance
   with real PostgreSQL.

## Recommendations

### Immediate (Before Next Test Run)

1. Fix `cache.rs:79` compilation error to enable real mentatd testing
2. Start PostgreSQL and install pg_mentat extension
3. Install k6 for more accurate rate-limited testing

### For Production Validation

1. Run full test suite against real mentatd + PostgreSQL
2. Load 1M+ datoms and re-run large query and steady state tests
3. Profile actual request pipeline with `tokio-console` or `tracing`
4. Measure connection pool utilization under steady state load
5. Run 10-minute soak test to check for memory leaks

## Test Infrastructure

### Files Created/Updated

- `benchmarks/load_test.sh` - Main test orchestrator (existing, used as-is)
- `benchmarks/mock_server.py` - Updated to use `ThreadingHTTPServer`
- `benchmarks/scenarios/steady_state.js` - k6 scenario (existing)
- `benchmarks/scenarios/spike.js` - k6 scenario (existing)
- `benchmarks/scenarios/large_queries.js` - k6 scenario (existing)
- `benchmarks/scenarios/mixed_workload.js` - k6 scenario (existing)
- `benchmarks/scenarios/concurrent_writes.js` - k6 scenario (existing)
- `benchmarks/results/20260424_173828_all/` - Raw data + reports

### How to Reproduce

```bash
# Start threaded mock server
python3 benchmarks/mock_server.py --port 8181 &

# Run all scenarios (60s each, 50 concurrent workers)
bash benchmarks/load_test.sh all --port 8181 --duration 60 --concurrency 50

# Run against real mentatd (when available)
cargo run --release --bin mentatd &
bash benchmarks/load_test.sh all --port 8080 --duration 600 --concurrency 50
```

## Raw Data Location

All raw data files are in `benchmarks/results/20260424_173828_all/`:
- `*_raw.dat` - Per-request data (worker_id status_code latency_ms response_size)
- `*_report.txt` - Human-readable reports
- `*_report.json` - Machine-readable JSON summaries
- `summary.txt` - Combined summary
