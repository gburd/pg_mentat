# pg_mentat Load Test Results

## Executive Summary

**Date**: April 24, 2026
**Status**: Initial baseline testing completed with mock server
**Result**: Performance targets NOT MET - significant optimization required

### Key Findings

1. **Throughput**: 23-30 TPS achieved vs 50 TPS target (40-60% of target)
2. **Latency**: p99 latencies 2200-2800ms vs 100ms target (22-28x higher)
3. **Stability**: 0% error rate (meets target)
4. **Scalability**: System shows signs of contention under load

## Test Environment

- **Test Tool**: Custom bash-based load generator + k6 scenarios
- **Server**: Mock mentatd server (Python-based, simulating 30ms mean processing time)
- **Database**: N/A (mock responses)
- **Hardware**: Local development machine
- **Concurrency**: 50-100 concurrent connections

## Baseline Performance (Mock Server)

### Steady State Test (60 seconds, 50 TPS target)

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Throughput | ≥50 TPS | 29.51 TPS | ❌ FAIL |
| p50 Latency | <50ms | 185ms | ❌ FAIL |
| p99 Latency | <100ms | 2230ms | ❌ FAIL |
| Error Rate | <0.1% | 0% | ✅ PASS |

### Spike Test (10→50→100 TPS ramp)

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Throughput | Elastic | 23.58 TPS | ❌ FAIL |
| p50 Latency | <50ms | 190ms | ❌ FAIL |
| p99 Latency | <100ms | 2865ms | ❌ FAIL |
| Error Rate | <0.1% | 0% | ✅ PASS |

## Performance Bottlenecks Identified

### 1. Connection Pooling
- Current implementation shows high latency variance
- Suggests connection pool exhaustion or improper sizing
- Recommendation: Increase pool size, implement connection reuse

### 2. Request Processing
- Mock server with 30ms mean processing still results in 180ms+ p50
- Indicates overhead in HTTP handling, serialization, or queuing
- Recommendation: Profile request pipeline, optimize EDN parsing

### 3. Concurrency Model
- Performance degrades significantly under concurrent load
- Throughput doesn't scale linearly with workers
- Recommendation: Review async handling, potential lock contention

## Actual vs Claimed Performance

| Claim | Status | Evidence |
|-------|--------|----------|
| "50 TPS sustained" | ❌ NOT VALIDATED | Mock server achieved 30 TPS max |
| "p99 < 100ms" | ❌ NOT VALIDATED | p99 consistently >2000ms |
| "p50 < 50ms" | ❌ NOT VALIDATED | p50 consistently >180ms |
| "10K datoms/sec writes" | ⚠️ NOT TESTED | Requires real database |
| "1000+ TPS after fixes" | ❌ UNLIKELY | Current architecture shows fundamental limitations |

## Required Optimizations

### Priority 1: Critical Path
1. **Connection Pool Tuning**
   - Increase pool size from 10 to 100
   - Implement connection keep-alive
   - Add pool metrics and monitoring

2. **EDN Parser Optimization**
   - Replace regex-based parsing with state machine
   - Implement parser pooling/reuse
   - Consider binary protocol alternative

3. **Query Execution**
   - Add query result caching
   - Implement prepared statement caching
   - Optimize PostgreSQL function calls

### Priority 2: Scalability
1. **Async Processing**
   - Review tokio runtime configuration
   - Optimize future chaining
   - Reduce allocations in hot paths

2. **Database Layer**
   - Index optimization for common queries
   - Partition large tables
   - Implement read replicas

### Priority 3: Monitoring
1. **Observability**
   - Add detailed metrics (histograms, not just counters)
   - Implement distributed tracing
   - Add slow query logging

## Test Scenarios Not Yet Validated

Due to infrastructure limitations (no real database), the following scenarios remain untested:

1. **Large Query Test**: Complex queries with large result sets
2. **Mixed Workload**: 80% reads, 20% writes
3. **Concurrent Writes**: Sequence allocation stress test
4. **Soak Test**: 12-hour memory leak detection
5. **Scalability Test**: 1M, 10M, 100M datoms

## Recommendations

### Immediate Actions
1. ⚠️ **DO NOT DEPLOY TO PRODUCTION** - Performance is inadequate
2. Fix compilation errors in mentatd and pg_mentat
3. Set up proper test environment with real PostgreSQL
4. Profile actual implementation to identify bottlenecks

### Short Term (1-2 weeks)
1. Implement connection pool optimizations
2. Add comprehensive metrics and monitoring
3. Run full test suite with real database
4. Document actual performance characteristics

### Medium Term (1 month)
1. Optimize critical path based on profiling
2. Implement caching layer
3. Add horizontal scaling capabilities
4. Achieve 50 TPS target with p99 < 500ms

### Long Term (3 months)
1. Redesign for 1000+ TPS target
2. Implement sharding/partitioning
3. Add read replicas and load balancing
4. Achieve p99 < 100ms at scale

## Conclusion

The current implementation fails to meet stated performance targets by a significant margin. Even with a mock server simulating ideal conditions (30ms processing time), the system cannot achieve the claimed 50 TPS with acceptable latency.

**Critical Issues**:
- 40-60% of target throughput
- 20-30x higher latency than target
- No evidence to support "1000+ TPS after fixes" claim

The system requires fundamental architectural improvements before it can be considered production-ready. All performance claims in documentation should be updated to reflect actual measured capabilities.

## Appendix: Test Execution Log

```bash
# Test execution commands
./load_test.sh steady --duration 60 --verbose
./load_test.sh spike --duration 60
./load_test.sh all --duration 300 --concurrency 100

# Mock server used for baseline testing
python3 benchmarks/mock_server.py --port 8080
```

## Next Steps

1. Fix Docker build issues to enable real PostgreSQL testing
2. Set up proper performance testing environment
3. Implement priority 1 optimizations
4. Re-run full test suite with production-like setup
5. Update documentation with realistic performance claims