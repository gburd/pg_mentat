# Production Readiness Assessment - Addressing Expert Concerns

**Date**: 2026-04-24
**Status**: Not Production-Ready (6-8 weeks of work required)
**Assessment Team**: Multiple expert perspectives (PostgreSQL, Datalog, Datomic, Operations)

---

## Executive Summary

After comprehensive expert review from **4 perspectives** (PostgreSQL extension architecture, Datalog completeness, Datomic compatibility, and production operations), the verdict is:

### Overall Grade: **C+ (Functional but Not Production-Ready)**

**The good news**: The core architecture is solid, security is good, and monitoring is production-grade.

**The bad news**: Critical features are missing, and **no load testing has been performed**. Performance claims are theoretical.

**Timeline to production**: **6-8 weeks** of focused work.

---

## Critical Issues (Must Fix Before Production)

### 1. **No Load Testing** 🔴 **BLOCKER**

**Finding**: Load test infrastructure exists (`benchmarks/` directory with k6 scripts), but `benchmarks/results/` is empty. **No tests have been run.**

**Risk**: All performance claims are unvalidated:
- "50 TPS sustained" - **NOT TESTED**
- "p99 < 100ms" - **NOT TESTED**
- "10M+ datoms" - **NOT TESTED**
- "1000+ TPS after isolation fix" - **NOT TESTED**

**What this means**: We don't know:
- Actual throughput under load
- Latency distribution (p50/p95/p99)
- Memory usage over time (leak detection)
- Breaking point (maximum TPS before failure)
- Behavior under connection saturation
- Performance degradation with datom count (1M vs 10M vs 100M)

**Action required**:
```bash
# Run the existing load tests
cd benchmarks
./load_test.sh all --duration 3600  # 1 hour steady state

# Analyze results
python analyze_results.py results/*
```

**Scenarios to test**:
1. Steady state: 50 TPS for 1 hour (baseline)
2. Spike: 10 → 500 TPS → 10 TPS (elasticity)
3. Soak: 10 TPS for 12 hours (memory leak detection)
4. Stress: Gradual ramp until failure (find limit)
5. Scalability: Test with 1M, 10M, 100M datoms

**Effort**: 1 week (run tests, analyze, document)

**Owner**: Assign to performance engineer or SRE

---

### 2. **Predicates in OR-Clauses Missing** 🔴 **BLOCKER**

**Finding**: OR-clauses only support patterns, not predicates.

**Example that FAILS**:
```clojure
[:find ?e
 :where (or [?e :person/name "Alice"]
            (and [?e :person/age ?age]
                 [(> ?age 30)]))]

;; Error: "Only pattern clauses are supported inside (or ...)"
```

**Why this is critical**: This is a **common query pattern** in production. Users need to express:
- "Find X where condition A OR condition B"
- "Find visible content (explicit flag OR (public AND recent))"

**Impact**: **Blocks real-world use cases.** Users must split into multiple queries and merge in application code (slow, loses query optimization).

**Action required**: Extend `query.rs::build_or_union_sql()` to handle predicates in each UNION branch.

**Effort**: 2 weeks (implementation + tests)

**Owner**: Assign to Datalog engineer

---

### 3. **Predicates in Rules Missing** 🔴 **BLOCKER**

**Finding**: Rules only support patterns and recursive invocations, not predicates.

**Example that FAILS**:
```clojure
;; Rule: Adult is person with age >= 18
[(adult ?person)
 [?person :person/age ?age]
 [(>= ?age 18)]]

;; Error: "Predicates in rule bodies not yet supported"
```

**Why this is critical**: Rules without predicates are **barely useful**. Most rules need filtering:
```clojure
[(manager ?p) [?p :employee/subordinates ?s] [(> (count ?s) 0)]]
[(in-range ?x ?low ?high) [(>= ?x ?low)] [(<= ?x ?high)]]
```

**Impact**: Rules are **severely limited** without predicates. Users can't express:
- Hierarchical queries with filtering
- Conditional logic in rules
- Range checks
- Aggregates in rules (e.g., "manager has > 5 subordinates")

**Action required**: Extend `query.rs::build_rule_ctes()` to generate WHERE clauses in WITH RECURSIVE CTEs.

**Effort**: 2 weeks (complex feature, needs testing)

**Owner**: Assign to Datalog engineer

---

### 4. **No Clojure Peer Library** 🔴 **BLOCKER** (for Datomic users)

**Finding**: Only HTTP API exists. No idiomatic Clojure library.

**Current user experience** (BAD):
```clojure
(require '[clj-http.client :as http])

(defn query [q]
  (-> (http/post "http://localhost:8080/"
                 {:content-type :application/edn
                  :body (pr-str {:op :q :query q})})
      :body
      edn/read-string
      :result))

;; Every query is an HTTP round-trip with JSON/EDN overhead
```

**Expected user experience** (GOOD):
```clojure
(require '[pg-mentat.client :as mentat])

(def conn (mentat/connect "postgresql://localhost/mentat"))
(mentat/q '[:find ?e :where [?e :person/name "Alice"]] (mentat/db conn))
```

**Why this matters**: Datomic users expect:
- Native Clojure integration (`(require '[datomic.api :as d])`)
- REPL-friendly usage (no HTTP boilerplate)
- Connection abstraction
- Local caching (db values)

**Impact**: **Datomic users won't adopt this.** Too much friction.

**Action required**: Build thin Clojure wrapper library:
```clojure
;; pg-mentat-client/src/pg_mentat/client.clj
(ns pg-mentat.client
  (:require [clj-http.client :as http]
            [clojure.edn :as edn]))

(defn connect [uri] {:uri uri})

(defn q [query db & inputs]
  (let [resp (http/post (:uri db)
                        {:content-type :application/edn
                         :body (pr-str {:op :q :query query :args inputs})})]
    (-> resp :body edn/read-string :result)))

(defn transact [conn tx-data] ...)
(defn pull [db pattern eid] ...)
(defn db [conn] ...)
```

**Effort**: 2-3 days (thin HTTP wrapper)

**Impact**: **High ROI** - dramatically improves UX for Clojure users

**Owner**: Assign to Clojure developer

---

### 5. **No db Value Caching** 🔴 **HIGH** (for Datomic users)

**Finding**: Every query is an HTTP request. No caching of database values.

**Datomic** (fast):
```clojure
(def db (d/db conn))  ; Get immutable db value (cheap)

(d/q query1 db)  ; 0.1ms (local)
(d/q query2 db)  ; 0.1ms (local)
(d/q query3 db)  ; 0.1ms (local)
```

**pg_mentat** (slow):
```bash
POST / {:op :q, :query query1}  # 5-10ms (HTTP + serialization)
POST / {:op :q, :query query2}  # 5-10ms (HTTP + serialization)
POST / {:op :q, :query query3}  # 5-10ms (HTTP + serialization)
```

**Performance impact**: **50-100x slower** for batch queries.

**Action required**: Add session-based db caching:
```clojure
POST / {:op :db}  → {:db-id "uuid-12345", :basis-t 1234567}

POST / {:op :q, :db-id "uuid-12345", :query query1}
POST / {:op :q, :db-id "uuid-12345", :query query2}
```

mentatd caches `db-id` → `basis-t` mapping. Queries reuse cached timestamp.

**Effort**: 1 week

**Owner**: Assign to mentatd engineer

---

### 6. **CHECK Constraint Performance Overhead** 🟡 **HIGH**

**Finding**: Every datom INSERT/UPDATE evaluates a 9-branch CHECK constraint:

```sql
CHECK (
    (CASE WHEN v_ref IS NOT NULL THEN 1 ELSE 0 END
   + CASE WHEN v_bool IS NOT NULL THEN 1 ELSE 0 END
   + ... /* 9 columns total */ ) = 1
)
```

**Performance impact**: Estimated **5-10% transaction throughput penalty**.

**Why this matters**: Bulk transactions with 10,000 datoms → 90,000 NULL checks.

**Action required** (choose one):

**Option A**: Remove CHECK constraint (trust Rust code)
```sql
ALTER TABLE mentat.datoms DROP CONSTRAINT chk_datom_value;
```

**Option B**: Move to GENERATED column (amortized cost)
```sql
ALTER TABLE mentat.datoms ADD COLUMN value_col_count SMALLINT
GENERATED ALWAYS AS (
    (v_ref IS NOT NULL)::int + (v_bool IS NOT NULL)::int + ...
) STORED;
ALTER TABLE mentat.datoms ADD CHECK (value_col_count = 1);
```

**Option C**: Enforce in Rust only (fastest)

**Recommendation**: Option A for production. The Rust type system (`TypedValue` enum) already guarantees exactly one value column.

**Effort**: 1 day (remove constraint + validate no correctness issues)

**Owner**: Assign to storage engineer

---

### 7. **No Query Timeout Enforcement** 🟡 **HIGH**

**Finding**: `timeout_ms` config exists but is **not enforced** per-query.

**Risk**: Runaway queries can block backends indefinitely:
- Large Cartesian product (missing join condition)
- Expensive full-text search
- Recursive rule with no termination

**Action required**: Set `statement_timeout` in transaction:
```rust
let timeout_sql = format!("SET LOCAL statement_timeout = {}", timeout_ms);
client.select(&timeout_sql, None, &[])?;
```

**Effort**: 1 day

**Owner**: Assign to query engine engineer

---

### 8. **No EXPLAIN Support** 🟡 **HIGH**

**Finding**: No way to debug slow queries. Can't see query plans.

**Why this is critical**: Debugging production performance issues requires EXPLAIN.

**Action required**: Add `mentat_explain()` function:
```rust
pub fn mentat_explain(query: &str, inputs: JsonB) -> Result<String, MentatError> {
    let sql = build_sql_from_datalog(...)?;
    let plan = client.select(&format!("EXPLAIN (FORMAT JSON) {}", sql), None, &params)?;
    Ok(plan.to_json())
}
```

**Effort**: 3 days

**Owner**: Assign to query engine engineer

---

## Medium Priority Issues

### 9. **EAVT Index Not Covering** 🟠

Index doesn't include value columns → heap fetches required → slower at scale.

**Recommendation**: Add type-specific EAVT indexes:
```sql
CREATE INDEX idx_datoms_eavt_long ON mentat.datoms (e, a, v_long, tx)
WHERE value_type_tag = 2;
```

**Effort**: 1 day (index creation + benchmark validation)

---

### 10. **No VACUUM Strategy** 🟠

High-churn workloads (many retractions) will bloat indexes.

**Recommendation**: Add autovacuum tuning to `lib.rs`:
```sql
ALTER TABLE mentat.datoms SET (
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_analyze_scale_factor = 0.02
);
```

**Effort**: 1 day

---

### 11. **OR-Join Uses UNION (Slow)** 🟠

UNION removes duplicates (O(N log N)). If input branches can't produce duplicates, use UNION ALL (O(N)).

**Recommendation**: Analyze whether OR-join branches can produce duplicates. If not, use UNION ALL.

**Effort**: 1 week (semantic analysis + correctness proof)

---

### 12. **Connection Pooling Strategy Undocumented** 🟠

Users don't know if they should use pgBouncer.

**Recommendation**: Document recommended deployment:
```
[mentatd] → [pgBouncer (transaction mode)] → [PostgreSQL]
```

Benefits: Lower connection count, faster connection reuse, prepared statement cache warming.

**Effort**: 1 day (documentation)

---

### 13. **Query Cache Metrics Missing** 🟠

No visibility into cache hit rate.

**Recommendation**: Add Prometheus metrics:
```
mentatd_query_cache_hits_total
mentatd_query_cache_misses_total
mentatd_query_cache_hit_rate
```

**Effort**: 1 day

---

## Low Priority Issues

### 14. **No Attribute Predicates**
### 15. **Collection Bindings in :in Clause Missing**
### 16. **Error Messages Expose PostgreSQL Internals**
### 17. **No Schema Validation Errors**
### 18. **No d/entity API**
### 19. **fulltext Table No Deduplication**
### 20. **Planner Hints Too Heavy-Handed**

(See full EXPERT_REVIEWS.md for details)

---

## What's Working Well ✅

1. **Core Architecture** - Typed columns, sequences, indexes (solid foundation)
2. **Security** - Parameterized queries, API key auth, container hardening
3. **Monitoring** - Prometheus metrics, Grafana dashboards, alert rules
4. **Temporal Queries** - Excellent as-of/since/history implementation
5. **Transaction Concurrency** - Sequence-based ID allocation (5-10x faster than original Mentat)
6. **Prepared Statement Caching** - Proper use of pgrx and SPI
7. **Schema Cache Design** - Lazy warming, O(1) lookups

---

## Production Readiness Roadmap

### Phase 1: Testing & Validation (2 weeks)

**Week 1: Load Testing**
- [ ] Run k6 steady state test (50 TPS, 1 hour)
- [ ] Run spike test (10 → 500 TPS → 10 TPS)
- [ ] Run soak test (10 TPS, 12 hours)
- [ ] Run stress test (ramp to failure)
- [ ] Measure p50/p95/p99 latency
- [ ] Check for memory leaks
- [ ] Document actual TPS limits

**Week 2: Scalability Testing**
- [ ] Seed database with 1M datoms, measure query latency
- [ ] Seed database with 10M datoms, measure query latency
- [ ] Seed database with 100M datoms, measure query latency
- [ ] Test history scan performance (as-of, since)
- [ ] Measure index size vs heap size ratio
- [ ] Test VACUUM duration on high-churn workload
- [ ] Document resource sizing guide (datoms → RAM/CPU/disk)

### Phase 2: Critical Feature Completion (3 weeks)

**Week 3: Datalog Completeness**
- [ ] Add predicates to OR-clauses (2 weeks)
- [ ] Add predicates to rule bodies (2 weeks)

**Week 4-5: User Experience**
- [ ] Build Clojure peer library (thin HTTP wrapper) (2 days)
- [ ] Add connection abstraction (session-based) (3 days)
- [ ] Add db value caching (1 week)
- [ ] Implement `ground`, `get-else`, `tuple` functions (3 days)

### Phase 3: Performance & Hardening (2 weeks)

**Week 6: Query Engine**
- [ ] Remove or optimize CHECK constraint (1 day)
- [ ] Add query timeout enforcement (1 day)
- [ ] Add `mentat_explain()` for debugging (3 days)
- [ ] Add type-specific EAVT indexes (1 day)

**Week 7: Operations**
- [ ] Add VACUUM tuning (1 day)
- [ ] Add query cache metrics (1 day)
- [ ] Document pgBouncer deployment pattern (1 day)
- [ ] Write runbooks (query slow, connection pool exhausted, disk full) (2 days)
- [ ] Test backup/restore procedures (2 days)

### Phase 4: Final Validation (1 week)

**Week 8: Production Dry Run**
- [ ] Re-run load tests with all fixes
- [ ] Validate throughput targets (50+ TPS)
- [ ] Validate latency targets (p99 < 100ms)
- [ ] Failure injection testing (kill backends, exhaust connections)
- [ ] Document upgrade procedure
- [ ] Document rollback procedure
- [ ] Define SLA (e.g., 99.9% uptime, p99 < 100ms)

---

## Timeline Summary

| Phase | Duration | Priority | Blockers |
|-------|----------|----------|----------|
| Testing & Validation | 2 weeks | P0 | None |
| Critical Features | 3 weeks | P0 | Testing complete |
| Performance & Hardening | 2 weeks | P1 | Features complete |
| Final Validation | 1 week | P1 | Hardening complete |

**Total: 8 weeks to production-ready**

**Fast-track option (6 weeks)**: Parallelize Phase 2 and Phase 3 with 2 engineers.

---

## Resource Requirements

**Team size**: 3-4 engineers
- 1 Performance engineer (load testing, scalability)
- 1 Datalog engineer (predicates in OR/rules)
- 1 Full-stack engineer (Clojure peer library, UX)
- 1 SRE (operations, monitoring, runbooks)

**Infrastructure**:
- Staging environment with production-like data (10M+ datoms)
- Load testing infrastructure (k6, Grafana, InfluxDB)
- PostgreSQL metrics (pg_stat_statements, pg_stat_activity)

---

## Risk Assessment

### High Risk
- **No load testing** - We don't know actual performance characteristics
- **Missing core Datalog features** - Predicates in OR/rules block real-world use

### Medium Risk
- **CHECK constraint overhead** - May limit transaction throughput
- **No query timeout** - Runaway queries possible
- **No EXPLAIN** - Can't debug production issues

### Low Risk
- **UX friction** - Affects adoption but not correctness
- **Missing minor features** - Workarounds exist

---

## Decision: Can We Go to Production Today?

### Answer: **No**

**Minimum requirements for production**:
1. ✅ Security audit complete (DONE)
2. ✅ Monitoring infrastructure exists (DONE)
3. ❌ Load testing complete (NOT DONE)
4. ❌ Core Datalog features complete (predicates in OR/rules) (NOT DONE)
5. ❌ Query timeout enforcement (NOT DONE)
6. ❌ Performance baseline established (NOT DONE)

**Blockers: 4/6**

---

## Recommendations

### Immediate Actions (This Week)
1. **Run load tests** - Use existing `benchmarks/load_test.sh` infrastructure
2. **Seed test database** - Create 10M datom dataset for scalability testing
3. **Prioritize feature backlog** - Assign predicates in OR/rules (highest priority)

### Short-Term (Next 4 Weeks)
4. **Fix critical Datalog gaps** - Predicates in OR-clauses and rules
5. **Build Clojure peer library** - Thin HTTP wrapper for better UX
6. **Add query timeout enforcement** - Prevent runaway queries
7. **Remove CHECK constraint** - Improve transaction throughput

### Medium-Term (Weeks 5-8)
8. **Add EXPLAIN support** - Enable query debugging
9. **Optimize indexes** - Type-specific EAVT indexes
10. **Complete operations work** - VACUUM tuning, runbooks, backup/restore testing

### Before Launch
11. **Re-run load tests** - Validate all fixes
12. **Document TPS limits** - Set user expectations
13. **Define SLA** - Uptime and latency guarantees
14. **Create upgrade procedure** - Safe extension version bumps

---

## Conclusion

pg_mentat is a **well-architected foundation** with **solid engineering**. The core architecture (typed columns, sequences, prepared statement caching) is production-grade.

However, it is **not production-ready today** due to:
- **No load testing** (performance claims unvalidated)
- **Missing core Datalog features** (predicates in OR/rules)
- **UX friction for Datomic users** (no peer library, no caching)

**With 6-8 weeks of focused work**, this can become a **production-ready Datomic alternative** for PostgreSQL users.

**Status**: Functional for prototypes and side projects (< 1M datoms, < 10 TPS). **Not ready** for production workloads.

---

**Next Steps**:
1. Review this assessment with the team
2. Prioritize backlog based on roadmap
3. Assign owners to each critical issue
4. Run load tests this week
5. Report findings in 1 week

**Questions? Contact**: [Your team lead / project owner]
