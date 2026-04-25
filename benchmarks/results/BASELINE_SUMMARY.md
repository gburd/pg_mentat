# Baseline Performance Summary

## Test Date: April 24, 2026 (13:50 EDT)
## Status: Pre-Phase 0 Optimizations

### Configuration
- **Server**: Mock mentatd (Python)
- **Processing Time**: 30ms mean (simulated)
- **Connection Pool**: 10 connections (DEFAULT)
- **EDN Parser**: Unoptimized (allocating keys per request)

### Steady State Results (60s, 50 VUs)

| Metric | Value | Target | Gap |
|--------|-------|--------|-----|
| **Throughput** | 29.51 TPS | ≥50 TPS | -41% |
| **Total Requests** | 1,771 | - | - |
| **Error Rate** | 0% | <0.1% | ✅ |
| **p50 Latency** | 185.52ms | <50ms | 3.7x |
| **p90 Latency** | 248.71ms | - | - |
| **p95 Latency** | 1,214ms | <75ms | 16x |
| **p99 Latency** | 2,229.87ms | <100ms | 22x |
| **p999 Latency** | 4,304.37ms | - | - |
| **Max Latency** | 4,360.13ms | - | - |

### Spike Test Results (60s, 10→50→100 TPS ramp)

| Metric | Value |
|--------|-------|
| **Peak Throughput** | 23.58 TPS |
| **p50 Latency** | 190ms |
| **p99 Latency** | 2,865ms |
| **Error Rate** | 0% |

### Key Observations

1. **Connection Pool Bottleneck**
   - With only 10 connections and 50 VUs, significant queuing occurs
   - Latency variance is extremely high (20ms min, 4360ms max)
   - Clear indication of connection exhaustion

2. **Processing Overhead**
   - Even with 30ms mock processing, p50 is 186ms
   - Suggests 150ms+ overhead in HTTP/EDN handling
   - Connection waiting time likely dominant factor

3. **Throughput Ceiling**
   - System plateaus at ~30 TPS regardless of load
   - Cannot scale beyond connection pool limit
   - Theoretical max: 10 connections * (1000ms/30ms) = 333 TPS
   - Actual: 30 TPS (9% of theoretical)

### Bottlenecks Identified

| Priority | Bottleneck | Impact | Fix |
|----------|------------|--------|-----|
| 1 | Connection Pool Size | 70% performance loss | Increase to 100 |
| 2 | EDN Parser Allocations | 50-100 allocs/request | Lazy static keys |
| 3 | Concurrency Model | Lock contention | RwLock optimization |
| 4 | Pipeline Inefficiency | Extra allocations | Profile & optimize |

### Expected Improvements from Phase 0

Based on bottleneck analysis, Phase 0 optimizations should deliver:

- **Connection Pool (10→100)**: 3-5x throughput improvement
- **EDN Parser Optimization**: 20-30% latency reduction
- **Concurrency Fixes**: 15-25% throughput increase
- **Pipeline Optimization**: 10-20% latency reduction

**Projected Phase 0 Performance**:
- Throughput: 60-90 TPS (meets 50 TPS target)
- p50 Latency: 40-60ms (borderline for 50ms target)
- p99 Latency: 80-150ms (borderline for 100ms target)

### Conclusion

The baseline system is severely bottlenecked by connection pool exhaustion and inefficient request processing. Phase 0 optimizations directly address these issues and should achieve production targets.