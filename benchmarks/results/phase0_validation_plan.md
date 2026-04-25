# Phase 0 Validation Test Plan

## Date: April 24, 2026

## Current Status
- **Task #11**: Connection pool optimization - COMPLETE
- **Task #12**: EDN parser optimization - COMPLETE
- **Task #13**: Concurrency optimization - IN PROGRESS (concurrency-engineer)
- **Task #14**: Pipeline optimization - IN PROGRESS (pipeline-engineer)
- **Task #15**: Validation testing - PREPARING (validation-engineer)

## Baseline Performance (Pre-Phase 0)
Documented in LOAD_TEST_RESULTS.md:
- **Throughput**: 29.51 TPS (Target: ≥50 TPS)
- **p50 Latency**: 185ms (Target: <50ms)
- **p99 Latency**: 2230ms (Target: <100ms)
- **Error Rate**: 0% (Target: <0.1%)

## Test Infrastructure Status
✅ Mock server available (mock_server.py)
✅ Load test scripts available (load_test.sh)
✅ Test scenarios available:
  - steady_state.js/sh
  - spike.js/sh
  - large_queries.js/sh
  - mixed_workload.js/sh
  - concurrent_writes.js/sh

## Test Execution Plan

### Phase 1: Preparation (NOW)
1. Verify test infrastructure
2. Document baseline metrics
3. Prepare results directory structure
4. Wait for Tasks #13 and #14 completion

### Phase 2: Validation Testing (After #13 & #14)

#### Test 1: Steady State Performance
- **Duration**: 60 seconds
- **Virtual Users**: 50
- **Expected Metrics**:
  - Throughput ≥ 50 TPS
  - p50 < 50ms
  - p99 < 100ms

```bash
cd /home/gburd/ws/pg_mentat/benchmarks
./load_test.sh steady --duration 60 --concurrency 50
```

#### Test 2: Spike Test
- **Duration**: 300 seconds
- **Pattern**: 10 VUs → 500 VUs → 10 VUs
- **Purpose**: Test elasticity and recovery

```bash
./load_test.sh spike --duration 300
```

#### Test 3: Scaling Test
- **Virtual Users**: 10, 25, 50, 100
- **Duration**: 60 seconds each
- **Purpose**: Verify linear scaling

```bash
for vus in 10 25 50 100; do
    ./load_test.sh steady --duration 60 --concurrency $vus
done
```

#### Test 4: Extended Soak (Optional)
- **Duration**: 1800 seconds (30 minutes)
- **Purpose**: Memory leak detection
- **Execute if**: Time permits and initial tests pass

```bash
./load_test.sh steady --duration 1800 --concurrency 50
```

## Success Criteria

| Metric | Target | Priority |
|--------|--------|----------|
| Throughput | ≥50 TPS sustained | CRITICAL |
| p50 Latency | <50ms | CRITICAL |
| p99 Latency | <100ms | CRITICAL |
| Error Rate | <0.1% | CRITICAL |
| Linear Scaling | Up to resource limits | HIGH |
| Memory Stability | No leaks over 30min | MEDIUM |

## Results Documentation

All results will be documented in:
1. `/home/gburd/ws/pg_mentat/benchmarks/results/phase0_*.log` - Raw test output
2. `/home/gburd/ws/pg_mentat/benchmarks/LOAD_TEST_RESULTS.md` - Updated comparison
3. `/home/gburd/ws/pg_mentat/docs/PRODUCTION_READINESS_UPDATE.md` - Timeline update

## Communication Protocol

Regular updates to team-lead:
- "Preparation complete, waiting for optimization tasks"
- "Starting validation tests"
- "Test X/4 complete: [summary]"
- "All tests complete, analyzing results"
- "Validation complete: [PASS/FAIL summary]"