# Revised Production Readiness Plan
## Based on Actual Implementation Status

**Date**: 2026-04-28
**Timeline**: 6 weeks (reduced from 13 weeks)
**Critical Discovery**: Phase 1 features already implemented with comprehensive tests

---

## Summary of Changes

The original PRODUCTION_READINESS_PLAN.md assumed critical Datalog features were missing. Code review revealed:

- ✅ **Predicates in OR-clauses**: IMPLEMENTED (query.rs:1784-1895)
- ✅ **Predicates in rule bodies**: IMPLEMENTED + 5 comprehensive tests (rule_predicate_tests.rs)
- ✅ **Unique identity upsert**: IMPLEMENTED + 7 tests (datalog_feature_tests.rs)
- ✅ **Transaction functions**: IMPLEMENTED (`:db.fn/retractEntity`, `:db.fn/cas`)

**Result**: Skip 4 weeks of Phase 1 work, proceed directly to performance validation and optimization.

---

## Phase 1: Performance Validation & Load Testing (2 weeks)

### Goal: Validate performance claims with actual measurements

**Problem**: Expert review notes "no actual load testing performed despite 600 TPS claims"

### 1.1 Benchmark Dataset Creation (2 days)

Create realistic test datasets at scale:

```sql
-- File: /home/gburd/ws/pg_mentat/benchmarks/create_test_data.sql

-- 1M datoms dataset
CREATE OR REPLACE FUNCTION create_benchmark_data_1m() RETURNS void AS $$
BEGIN
    -- Create 100k entities with 10 attributes each
    FOR i IN 1..100000 LOOP
        PERFORM mentat_transact(format('[
            {:db/id "e%s"
             :person/name "Person%s"
             :person/age %s
             :person/email "person%s@test.com"
             :person/dept %s
             :person/salary %s.0
             :person/active %s
             :person/score %s.5
             :person/joined %s
             :person/bio "Bio for person %s"
             :person/tags ["tag%s" "tag%s"]}
        ]',
        i, i, 20 + (i % 50), i,
        CASE (i % 5) WHEN 0 THEN 'Engineering' WHEN 1 THEN 'Sales' WHEN 2 THEN 'Marketing' WHEN 3 THEN 'Support' ELSE 'Product' END,
        50000 + (i % 100000),
        (i % 2 = 0)::text,
        75.0 + (i % 25),
        '2020-01-01'::timestamp + (i || ' days')::interval,
        i, (i % 100), ((i + 1) % 100)
        ));

        IF i % 1000 = 0 THEN
            RAISE NOTICE 'Created % entities', i;
        END IF;
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- 10M datoms dataset
CREATE OR REPLACE FUNCTION create_benchmark_data_10m() RETURNS void AS $$
BEGIN
    FOR i IN 1..1000000 LOOP
        PERFORM mentat_transact(format('[...same pattern, 1M entities...]'));
        IF i % 10000 = 0 THEN
            RAISE NOTICE 'Created % entities', i;
        END IF;
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- 100M datoms dataset (for scale testing)
CREATE OR REPLACE FUNCTION create_benchmark_data_100m() RETURNS void AS $$
BEGIN
    FOR i IN 1..10000000 LOOP
        PERFORM mentat_transact(format('[...same pattern, 10M entities...]'));
        IF i % 100000 = 0 THEN
            RAISE NOTICE 'Created % entities', i;
        END IF;
    END LOOP;
END;
$$ LANGUAGE plpgsql;
```

### 1.2 Query Performance Benchmarks (3 days)

**File**: `/home/gburd/ws/pg_mentat/benchmarks/query_performance.sql`

```sql
-- Benchmark 1: Simple pattern query
\timing on
EXPLAIN (ANALYZE, BUFFERS)
SELECT mentat_query('[:find ?name :where [?e :person/name ?name]]', '{}'::jsonb);

-- Benchmark 2: Join query (2 patterns)
EXPLAIN (ANALYZE, BUFFERS)
SELECT mentat_query('
    [:find ?name ?dept
     :where [?e :person/name ?name]
            [?e :person/dept ?dept]]', '{}'::jsonb);

-- Benchmark 3: Join with predicate
EXPLAIN (ANALYZE, BUFFERS)
SELECT mentat_query('
    [:find ?name ?age
     :where [?e :person/name ?name]
            [?e :person/age ?age]
            [(> ?age 40)]]', '{}'::jsonb);

-- Benchmark 4: OR-join query
EXPLAIN (ANALYZE, BUFFERS)
SELECT mentat_query('
    [:find ?name
     :where [?e :person/name ?name]
            (or [?e :person/dept "Engineering"]
                [?e :person/dept "Sales"])]', '{}'::jsonb);

-- Benchmark 5: Aggregate query
EXPLAIN (ANALYZE, BUFFERS)
SELECT mentat_query('
    [:find ?dept (avg ?salary) (count ?e)
     :where [?e :person/dept ?dept]
            [?e :person/salary ?salary]]', '{}'::jsonb);

-- Benchmark 6: Rule query (recursive)
EXPLAIN (ANALYZE, BUFFERS)
SELECT mentat_query('
    [:find ?subordinate ?levels
     :in $ %
     :where (reports-to ?subordinate ?boss ?levels)]',
    '{"rules": "[(reports-to ?sub ?boss ?levels) [?sub :person/manager ?boss] [(identity 1) ?levels]] [(reports-to ?sub ?boss ?levels) [?sub :person/manager ?mid] (reports-to ?mid ?boss ?prev-levels) [(+ ?prev-levels 1) ?levels]]"}'::jsonb);
```

### 1.3 Transaction Throughput Testing (3 days)

**File**: `/home/gburd/ws/pg_mentat/benchmarks/transaction_throughput.sql`

```sql
-- Benchmark: Single transaction performance
CREATE OR REPLACE FUNCTION benchmark_single_tx() RETURNS TABLE(
    tx_count int,
    duration_ms float,
    tps float
) AS $$
DECLARE
    start_time timestamp;
    end_time timestamp;
    tx_count int := 1000;
BEGIN
    start_time := clock_timestamp();

    FOR i IN 1..tx_count LOOP
        PERFORM mentat_transact(format('[
            {:db/id "bench%s"
             :person/name "BenchUser%s"
             :person/age %s
             :person/email "bench%s@test.com"}
        ]', i, i, 20 + (i % 50), i));
    END LOOP;

    end_time := clock_timestamp();

    RETURN QUERY SELECT
        tx_count,
        EXTRACT(EPOCH FROM (end_time - start_time)) * 1000 AS duration_ms,
        tx_count / EXTRACT(EPOCH FROM (end_time - start_time)) AS tps;
END;
$$ LANGUAGE plpgsql;

-- Run: SELECT * FROM benchmark_single_tx();
-- Expected: >600 TPS (per expert review claim)
```

**Concurrent Transaction Test** (using pgbench):

```bash
# File: /home/gburd/ws/pg_mentat/benchmarks/concurrent_tx.sh

#!/bin/bash

# Create pgbench transaction script
cat > tx_benchmark.sql <<'EOF'
SELECT mentat_transact(format('[
    {:db/id "user%s"
     :person/name "User%s"
     :person/age %s
     :person/email "user%s@test.com"}
]', random() * 1000000, random() * 1000000, 20 + random() * 50, random() * 1000000));
EOF

# Test with 1, 10, 50, 100 concurrent clients
for clients in 1 10 50 100; do
    echo "Testing with $clients concurrent clients..."
    pgbench -c $clients -j $clients -T 60 -f tx_benchmark.sql -n postgres
done

# Expected: Linear scaling up to ~50 clients, then plateau
# Target: >5000 datoms/sec sustained with 50 clients
```

### 1.4 UNION ALL Performance Analysis (2 days)

**Problem**: Query strategy uses UNION ALL across 9 type-specific tables

**File**: `/home/gburd/ws/pg_mentat/benchmarks/union_all_analysis.sql`

```sql
-- Compare UNION ALL vs single-table query performance

-- Query 1: UNION ALL across all 9 tables (current strategy for unknown types)
EXPLAIN (ANALYZE, BUFFERS)
SELECT e, a, v::text
FROM (
    SELECT e, a, v::text, tx FROM mentat.datoms_ref_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text, tx FROM mentat.datoms_long_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text, tx FROM mentat.datoms_double_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text, tx FROM mentat.datoms_text_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text, tx FROM mentat.datoms_keyword_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text, tx FROM mentat.datoms_instant_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text, tx FROM mentat.datoms_uuid_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text, tx FROM mentat.datoms_bytes_new WHERE store_id = 0 AND added = true
    UNION ALL
    SELECT e, a, v::text, tx FROM mentat.datoms_boolean_new WHERE store_id = 0 AND added = true
) u
WHERE a = 42  -- :person/name
LIMIT 1000;

-- Query 2: Single-table query (schema-aware optimization)
EXPLAIN (ANALYZE, BUFFERS)
SELECT e, a, v::text, tx
FROM mentat.datoms_text_new
WHERE store_id = 0 AND a = 42 AND added = true
LIMIT 1000;

-- Expected: Single-table query should be 5-10x faster
-- If not, investigate query planner behavior
```

### 1.5 Success Criteria

- ✅ Query latency: <50ms for simple patterns (1M datoms)
- ✅ Query latency: <200ms for complex joins (10M datoms)
- ✅ Transaction throughput: >600 TPS single-threaded
- ✅ Transaction throughput: >5k datoms/sec with 50 concurrent clients
- ✅ UNION ALL overhead: <2x vs single-table queries
- ✅ Memory usage: <2GB for 10M datom dataset
- ✅ Index size: <50% of table size

### Deliverables

1. `/home/gburd/ws/pg_mentat/BENCHMARKS_RESULTS.md` - Comprehensive performance report
2. `/home/gburd/ws/pg_mentat/benchmarks/` - All benchmark scripts
3. Decision: Keep current architecture or implement schema-aware optimization

---

## Phase 2: Index Strategy Optimization (1 week)

### Goal: Reduce index bloat and improve query performance

**Problem**: Current indexes are not optimal (non-covering, no partial indexes, missing VAET)

### 2.1 Partial Indexes for Tombstones (1 day)

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/sql/optimize_indexes_phase1.sql`

```sql
-- Drop old indexes (if not in use)
-- DO NOT run this in production without downtime window

-- Create new partial indexes (skip tombstones)
CREATE INDEX CONCURRENTLY datoms_ref_new_eavt_partial
    ON mentat.datoms_ref_new (store_id, e, a, v, tx)
    WHERE added = true;  -- 30-40% size reduction

CREATE INDEX CONCURRENTLY datoms_ref_new_aevt_partial
    ON mentat.datoms_ref_new (store_id, a, e, v, tx)
    WHERE added = true;

CREATE INDEX CONCURRENTLY datoms_long_new_eavt_partial
    ON mentat.datoms_long_new (store_id, e, a, v, tx)
    WHERE added = true;

CREATE INDEX CONCURRENTLY datoms_long_new_aevt_partial
    ON mentat.datoms_long_new (store_id, a, e, v, tx)
    WHERE added = true;

-- Repeat for all 9 type-specific tables
-- (text, double, keyword, instant, uuid, bytes, boolean)
```

### 2.2 VAET Indexes for Value Lookups (1 day)

```sql
-- File: /home/gburd/ws/pg_mentat/pg_mentat/sql/optimize_indexes_phase2.sql

-- Create VAET indexes for efficient value lookups
CREATE INDEX CONCURRENTLY datoms_ref_new_vaet_partial
    ON mentat.datoms_ref_new (store_id, v, a, e, tx)
    WHERE added = true;

CREATE INDEX CONCURRENTLY datoms_long_new_vaet_partial
    ON mentat.datoms_long_new (store_id, v, a, e, tx)
    WHERE added = true;

CREATE INDEX CONCURRENTLY datoms_text_new_vaet_partial
    ON mentat.datoms_text_new (store_id, v, a, e, tx)
    WHERE added = true;

-- Use cases:
-- 1. Reverse attribute lookups: "Find all entities that reference entity 123"
-- 2. Value range scans: "Find all products with price > 100"
-- 3. Unique constraint checks (faster than current AEVT scan)
```

### 2.3 Covering Indexes (2 days)

```sql
-- File: /home/gburd/ws/pg_mentat/pg_mentat/sql/optimize_indexes_phase3.sql

-- Covering indexes for common query patterns (PostgreSQL 11+)
CREATE INDEX CONCURRENTLY datoms_ref_new_covering
    ON mentat.datoms_ref_new (store_id, a, e, v)
    INCLUDE (tx, added)  -- Non-key columns included in index
    WHERE added = true;

-- Benefit: Index-only scans (no heap access needed)
-- Query: SELECT e, v, tx FROM datoms_ref_new WHERE store_id = 0 AND a = 42
-- Plan: Index Only Scan using datoms_ref_new_covering

CREATE INDEX CONCURRENTLY datoms_long_new_covering
    ON mentat.datoms_long_new (store_id, a, e, v)
    INCLUDE (tx, added)
    WHERE added = true;

-- Repeat for other heavily-queried tables (text, keyword)
```

### 2.4 Index Maintenance Monitoring (1 day)

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/sql/index_monitoring_views.sql`

```sql
-- View: Index bloat monitoring
CREATE VIEW mentat.index_health AS
SELECT
    schemaname,
    tablename,
    indexname,
    pg_size_pretty(pg_relation_size(indexrelid)) AS size,
    idx_scan AS scans,
    idx_tup_read AS tuples_read,
    idx_tup_fetch AS tuples_fetched,
    100 * idx_scan / NULLIF(seq_scan + idx_scan, 0) AS index_usage_pct,
    CASE
        WHEN pg_relation_size(indexrelid) > pg_relation_size(relid) * 0.3
        THEN 'WARNING: Index larger than 30% of table'
        ELSE 'OK'
    END AS health_status
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
ORDER BY pg_relation_size(indexrelid) DESC;

-- View: Unused indexes (candidates for removal)
CREATE VIEW mentat.unused_indexes AS
SELECT
    schemaname,
    tablename,
    indexname,
    pg_size_pretty(pg_relation_size(indexrelid)) AS size,
    idx_scan AS scans
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
  AND idx_scan = 0
  AND indexname NOT LIKE '%_pkey'  -- Exclude primary keys
ORDER BY pg_relation_size(indexrelid) DESC;

-- Query: SELECT * FROM mentat.index_health;
-- Query: SELECT * FROM mentat.unused_indexes;
```

### 2.5 Success Criteria

- ✅ Index size reduced by 30-40% (via partial indexes)
- ✅ Query performance improved by 20-50% (via covering indexes)
- ✅ VAET indexes enable efficient reverse lookups
- ✅ Index-only scan plans for common queries
- ✅ Monitoring views catch index bloat

### Deliverables

1. SQL migration scripts for all index optimizations
2. Index health monitoring views
3. Before/after performance comparison
4. Updated BENCHMARKS_RESULTS.md with index optimization impact

---

## Phase 3: Client Libraries & User Experience (1 week)

### Goal: Improve Datomic migration experience

**Problem**: No Clojure peer library, Python client requires HTTP overhead

### 3.1 Clojure Peer Library (3 days)

**File**: `/home/gburd/ws/pg_mentat/clients/clojure/src/pg_mentat/client.clj`

```clojure
(ns pg-mentat.client
  "Datomic-compatible peer library for pg_mentat"
  (:require [next.jdbc :as jdbc]
            [clojure.edn :as edn]
            [clojure.string :as str]))

(defn connect
  "Create a connection to pg_mentat via PostgreSQL.

   config: {:dbtype \"postgresql\"
            :host \"localhost\"
            :port 5432
            :dbname \"mydb\"
            :user \"postgres\"
            :password \"...\"}"
  [config]
  {:datasource (jdbc/get-datasource config)
   :config config})

(defn db
  "Get current database value from connection"
  [conn]
  {:conn conn
   :as-of nil})

(defn as-of
  "Get database value as of a specific transaction or instant"
  [db tx-or-instant]
  (assoc db :as-of tx-or-instant))

(defn q
  "Execute Datalog query. Exactly matches datomic.api/q signature.

   Example:
   (q '[:find ?e ?name
        :where [?e :person/name ?name]]
      (db conn))"
  [query db & inputs]
  (let [query-str (pr-str query)
        inputs-json (pr-str (vec inputs))
        as-of (:as-of db)
        result (jdbc/execute-one!
                 (:datasource (:conn db))
                 [(if as-of
                    "SELECT mentat_query_as_of($1, $2, $3)::text"
                    "SELECT mentat_query($1, $2)::text")
                  query-str
                  (str "{\"inputs\": " inputs-json "}")
                  (when as-of (str as-of))])]
    (-> result :mentat_query_as_of (or (:mentat_query result)) edn/read-string :results)))

(defn transact
  "Execute transaction. Exactly matches datomic.api/transact signature.

   Example:
   (transact conn {:tx-data [{:person/name \"Alice\" :person/age 30}]})"
  [conn {:keys [tx-data]}]
  (let [tx-str (pr-str tx-data)
        result (jdbc/execute-one!
                 (:datasource conn)
                 ["SELECT mentat_transact($1)::text" tx-str])]
    (-> result :mentat_transact edn/read-string)))

(defn pull
  "Pull entity data. Exactly matches datomic.api/pull signature.

   Example:
   (pull (db conn) '[*] 123)"
  [db pattern eid]
  (let [pattern-str (pr-str pattern)
        result (jdbc/execute-one!
                 (:datasource (:conn db))
                 ["SELECT mentat_pull($1, $2)::text" pattern-str (str eid)])]
    (-> result :mentat_pull edn/read-string)))

(defn pull-many
  "Pull multiple entities. Exactly matches datomic.api/pull-many signature."
  [db pattern eids]
  (let [pattern-str (pr-str pattern)
        eids-str (pr-str (vec eids))
        result (jdbc/execute-one!
                 (:datasource (:conn db))
                 ["SELECT mentat_pull_many($1, $2)::text" pattern-str eids-str])]
    (-> result :mentat_pull_many edn/read-string)))

(defn entity
  "Get entity map. Exactly matches datomic.api/entity signature."
  [db eid]
  (let [result (jdbc/execute-one!
                 (:datasource (:conn db))
                 ["SELECT mentat_entity($1)::text" (str eid)])]
    (-> result :mentat_entity edn/read-string)))

;; Usage example:
;; (require '[pg-mentat.client :as d])
;; (def conn (d/connect {:dbtype "postgresql" :host "localhost" :dbname "test"}))
;; (d/q '[:find ?e ?name :where [?e :person/name ?name]] (d/db conn))
```

**project.clj**:
```clojure
(defproject pg-mentat "0.1.0"
  :description "Datomic-compatible Clojure client for pg_mentat"
  :dependencies [[org.clojure/clojure "1.11.1"]
                 [com.github.seancorfield/next.jdbc "1.3.894"]]
  :profiles {:dev {:dependencies [[org.postgresql/postgresql "42.6.0"]]}})
```

### 3.2 Python Native Client (2 days)

**File**: `/home/gburd/ws/pg_mentat/clients/python/pg_mentat/client.py`

```python
"""
Datomic-compatible Python client for pg_mentat.
Uses native PostgreSQL connection (no HTTP daemon needed).
"""

import psycopg2
import psycopg2.extras
import json
from typing import List, Dict, Any, Optional, Union

class Connection:
    """PostgreSQL connection to pg_mentat"""

    def __init__(self, connection_string: str):
        """
        Create connection to pg_mentat.

        Args:
            connection_string: PostgreSQL connection string
                e.g., "postgresql://user:pass@localhost/dbname"
        """
        self.conn = psycopg2.connect(connection_string)
        self.cursor = self.conn.cursor(cursor_factory=psycopg2.extras.RealDictCursor)

    def db(self) -> 'Database':
        """Get current database value"""
        return Database(self)

    def transact(self, tx_data: List[Dict]) -> Dict:
        """
        Execute transaction.

        Args:
            tx_data: List of transaction operations in EDN format
                e.g., [{"db/id": "new-entity", "person/name": "Alice"}]

        Returns:
            Transaction report with tempids, tx-data, etc.
        """
        tx_str = json.dumps(tx_data)
        self.cursor.execute("SELECT mentat_transact(%s)::text", (tx_str,))
        result = self.cursor.fetchone()
        return json.loads(result['mentat_transact'])

    def close(self):
        """Close connection"""
        self.cursor.close()
        self.conn.close()


class Database:
    """Database value (immutable snapshot)"""

    def __init__(self, conn: Connection, as_of: Optional[int] = None):
        self.conn = conn
        self.as_of = as_of

    def as_of(self, tx_or_instant: Union[int, str]) -> 'Database':
        """Get database value as of a specific transaction or instant"""
        return Database(self.conn, tx_or_instant)

    def q(self, query: str, *inputs) -> List[List[Any]]:
        """
        Execute Datalog query.

        Args:
            query: Datalog query string in EDN format
                e.g., "[:find ?e ?name :where [?e :person/name ?name]]"
            *inputs: Query input parameters

        Returns:
            List of result tuples
        """
        inputs_json = json.dumps({"inputs": list(inputs)})

        if self.as_of:
            self.conn.cursor.execute(
                "SELECT mentat_query_as_of(%s, %s, %s)::text",
                (query, inputs_json, str(self.as_of))
            )
            result = self.conn.cursor.fetchone()
            return json.loads(result['mentat_query_as_of'])['results']
        else:
            self.conn.cursor.execute(
                "SELECT mentat_query(%s, %s)::text",
                (query, inputs_json)
            )
            result = self.conn.cursor.fetchone()
            return json.loads(result['mentat_query'])['results']

    def pull(self, pattern: List, eid: int) -> Dict:
        """
        Pull entity data.

        Args:
            pattern: Pull pattern (e.g., ['*'] or [:person/name :person/age])
            eid: Entity ID

        Returns:
            Entity map
        """
        pattern_str = json.dumps(pattern)
        self.conn.cursor.execute(
            "SELECT mentat_pull(%s, %s)::text",
            (pattern_str, str(eid))
        )
        result = self.conn.cursor.fetchone()
        return json.loads(result['mentat_pull'])

    def entity(self, eid: int) -> Dict:
        """Get entity as dictionary"""
        self.conn.cursor.execute(
            "SELECT mentat_entity(%s)::text",
            (str(eid),)
        )
        result = self.conn.cursor.fetchone()
        return json.loads(result['mentat_entity'])


# Usage example:
# import pg_mentat
# conn = pg_mentat.Connection("postgresql://localhost/test")
# db = conn.db()
# results = db.q("[:find ?e ?name :where [?e :person/name ?name]]")
# conn.transact([{"db/id": "new", "person/name": "Alice", "person/age": 30}])
```

**setup.py**:
```python
from setuptools import setup, find_packages

setup(
    name="pg-mentat",
    version="0.1.0",
    description="Datomic-compatible Python client for pg_mentat",
    packages=find_packages(),
    install_requires=[
        "psycopg2-binary>=2.9.0",
    ],
    python_requires=">=3.7",
)
```

### 3.3 Success Criteria

- ✅ Clojure client provides 100% Datomic API compatibility
- ✅ Python client provides idiomatic Python interface
- ✅ No HTTP daemon required (direct PostgreSQL connection)
- ✅ Performance within 10% of raw SQL calls
- ✅ Comprehensive documentation with examples

### Deliverables

1. `/home/gburd/ws/pg_mentat/clients/clojure/` - Clojure peer library
2. `/home/gburd/ws/pg_mentat/clients/python/` - Python native client
3. Documentation with migration guides from Datomic

---

## Phase 4: Production Monitoring & Operations (1 week)

### Goal: Add production-grade observability

### 4.1 Structured Logging (2 days)

**File**: `/home/gburd/ws/pg_mentat/pg_mentat/src/monitoring.rs`

```rust
use tracing::{info, warn, error, debug, instrument};
use std::time::Instant;

#[instrument(skip(query, inputs))]
pub fn log_query_execution(
    query: &str,
    inputs: &JsonB,
    duration_ms: u64,
) {
    if duration_ms > 100 {
        warn!(
            query = %query,
            duration_ms = duration_ms,
            "Slow query detected"
        );
    } else {
        debug!(
            query = %query,
            duration_ms = duration_ms,
            "Query executed"
        );
    }
}

#[instrument(skip(tx_data))]
pub fn log_transaction(
    tx_id: i64,
    datom_count: usize,
    duration_ms: u64,
) {
    info!(
        tx_id = tx_id,
        datom_count = datom_count,
        duration_ms = duration_ms,
        "Transaction committed"
    );
}
```

### 4.2 Prometheus Metrics (2 days)

```rust
// Add to Cargo.toml: prometheus = "0.13"

use prometheus::{register_histogram, register_counter, Histogram, Counter};
use lazy_static::lazy_static;

lazy_static! {
    static ref QUERY_DURATION: Histogram = register_histogram!(
        "mentat_query_duration_seconds",
        "Query execution duration in seconds"
    ).unwrap();

    static ref QUERY_ERRORS: Counter = register_counter!(
        "mentat_query_errors_total",
        "Total number of query errors"
    ).unwrap();

    static ref TX_COUNT: Counter = register_counter!(
        "mentat_transactions_total",
        "Total number of transactions"
    ).unwrap();

    static ref DATOM_COUNT: Counter = register_counter!(
        "mentat_datoms_total",
        "Total number of datoms written"
    ).unwrap();
}

// Expose metrics endpoint
#[pg_extern(schema = "mentat", name = "metrics")]
pub fn mentat_metrics() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

// Usage: SELECT mentat.metrics();
```

### 4.3 Slow Query Logging (1 day)

```sql
-- File: /home/gburd/ws/pg_mentat/pg_mentat/sql/slow_query_log.sql

CREATE TABLE mentat.slow_query_log (
    id SERIAL PRIMARY KEY,
    query_text TEXT NOT NULL,
    execution_time_ms FLOAT NOT NULL,
    result_row_count INT,
    timestamp TIMESTAMPTZ DEFAULT NOW(),
    INDEX idx_slow_query_log_time (timestamp DESC),
    INDEX idx_slow_query_log_duration (execution_time_ms DESC)
);

-- Trigger to log slow queries (>100ms)
CREATE OR REPLACE FUNCTION mentat.log_slow_query()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.execution_time_ms > 100 THEN
        INSERT INTO mentat.slow_query_log (query_text, execution_time_ms, result_row_count)
        VALUES (NEW.query_text, NEW.execution_time_ms, NEW.result_row_count);
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- View: Recent slow queries
CREATE VIEW mentat.recent_slow_queries AS
SELECT
    query_text,
    execution_time_ms,
    result_row_count,
    timestamp
FROM mentat.slow_query_log
WHERE timestamp > NOW() - INTERVAL '1 day'
ORDER BY execution_time_ms DESC
LIMIT 100;
```

### 4.4 Success Criteria

- ✅ Structured logging with trace IDs
- ✅ Prometheus metrics exported
- ✅ Slow query log captures >100ms queries
- ✅ Index bloat monitoring views functional
- ✅ Grafana dashboard template provided

### Deliverables

1. Monitoring infrastructure code
2. Grafana dashboard JSON
3. Alerting rules (Prometheus AlertManager)
4. Runbook for common issues

---

## Phase 5: Documentation & Migration Guides (1 week)

### Goal: Production-ready documentation

### 5.1 Production Deployment Guide (2 days)

**File**: `/home/gburd/ws/pg_mentat/PRODUCTION_DEPLOYMENT.md`

Topics:
- System requirements (PostgreSQL 13+, RAM, CPU, storage)
- Installation steps (pgrx, extension setup)
- PostgreSQL tuning (shared_buffers, work_mem, etc.)
- High availability setup (streaming replication, pgbouncer)
- Backup/restore procedures
- Security hardening

### 5.2 Migration from Datomic Guide (2 days)

**File**: `/home/gburd/ws/pg_mentat/MIGRATION_FROM_DATOMIC.md`

Topics:
- Schema translation (Datomic schema → pg_mentat schema)
- Data migration tools (export from Datomic, import to pg_mentat)
- API compatibility matrix (what works, what doesn't)
- Performance comparison benchmarks
- Client library migration examples

### 5.3 Operations Runbook (1 day)

**File**: `/home/gburd/ws/pg_mentat/OPERATIONS_RUNBOOK.md`

Topics:
- Common issues and solutions
- Performance debugging workflow
- Index maintenance procedures
- Capacity planning guidelines
- Monitoring and alerting setup

### 5.4 API Reference Documentation (2 days)

**File**: `/home/gburd/ws/pg_mentat/API_REFERENCE.md`

Topics:
- All SQL functions documented
- Datalog query syntax reference
- Pull API syntax reference
- Transaction format reference
- Client library API documentation

### Deliverables

1. PRODUCTION_DEPLOYMENT.md
2. MIGRATION_FROM_DATOMIC.md
3. OPERATIONS_RUNBOOK.md
4. API_REFERENCE.md

---

## Timeline Summary

| Phase | Duration | Effort | Start | End |
|-------|----------|--------|-------|-----|
| **Phase 1: Performance Validation** | 2 weeks | 80h | Week 1 | Week 2 |
| **Phase 2: Index Optimization** | 1 week | 40h | Week 3 | Week 3 |
| **Phase 3: Client Libraries** | 1 week | 40h | Week 4 | Week 4 |
| **Phase 4: Monitoring** | 1 week | 40h | Week 5 | Week 5 |
| **Phase 5: Documentation** | 1 week | 40h | Week 6 | Week 6 |
| **TOTAL** | **6 weeks** | **240h** | Week 1 | Week 6 |

---

## Success Criteria (Production Readiness Checklist)

### Performance ✅
- [ ] Query latency: <50ms for simple patterns (1M datoms)
- [ ] Query latency: <200ms for complex joins (10M datoms)
- [ ] Transaction throughput: >600 TPS single-threaded
- [ ] Transaction throughput: >5k datoms/sec with 50 clients
- [ ] UNION ALL overhead: <2x vs single-table queries

### Scalability ✅
- [ ] Supports 10M+ datoms without degradation
- [ ] Supports 100M+ datoms with acceptable performance
- [ ] Linear scaling with concurrent clients (up to 50)
- [ ] Index size <50% of table size

### Datalog Features ✅
- [x] Predicates in OR-clauses (ALREADY DONE)
- [x] Predicates in rule bodies (ALREADY DONE)
- [x] Unique identity upsert (ALREADY DONE)
- [x] Transaction functions (ALREADY DONE)
- [ ] Schema-aware query optimization (optional, future)

### User Experience ✅
- [ ] Clojure peer library (100% Datomic API compatible)
- [ ] Python native client (idiomatic interface)
- [ ] Documentation with migration guides
- [ ] Example applications ported from Datomic

### Operations ✅
- [ ] Structured logging with trace IDs
- [ ] Prometheus metrics exported
- [ ] Slow query monitoring (<100ms threshold)
- [ ] Index bloat monitoring views
- [ ] Backup/restore documentation
- [ ] High availability setup guide
- [ ] Troubleshooting runbook

---

## Risk Assessment

| Risk | Severity | Mitigation | Owner |
|------|----------|------------|-------|
| **Benchmark results don't meet targets** | High | Implement schema-aware optimization if needed | Phase 1 |
| **Index optimization breaks queries** | Medium | Extensive testing, gradual rollout | Phase 2 |
| **Client libraries missing features** | Low | Start with core features, expand iteratively | Phase 3 |
| **Monitoring overhead impacts performance** | Low | Use sampling, async logging | Phase 4 |
| **Documentation incomplete** | Low | Prioritize critical topics, iterate | Phase 5 |

---

## Next Steps

1. **Immediate**: Start Phase 1 (Performance Validation)
   - Create benchmark datasets (1M, 10M, 100M datoms)
   - Run query performance benchmarks
   - Measure transaction throughput
   - Analyze UNION ALL overhead

2. **Week 1**: Complete performance testing, compile results

3. **Week 2**: Decide on optimizations needed based on benchmark results

4. **Week 3-6**: Execute remaining phases in parallel where possible

---

## Comparison to Original Plan

| Aspect | Original Plan | Revised Plan | Change |
|--------|---------------|--------------|--------|
| **Timeline** | 13 weeks | 6 weeks | -54% |
| **Phase 1 (Datalog)** | 4 weeks | 0 weeks | **ALREADY DONE** |
| **Performance Testing** | Assumed working | 2 weeks (validate) | NEW: Critical |
| **Index Optimization** | Not mentioned | 1 week | NEW: High value |
| **Client Libraries** | Included | 1 week | Same |
| **Monitoring** | Included | 1 week | Same |
| **Documentation** | Included | 1 week | Same |

---

## Conclusion

The original PRODUCTION_READINESS_PLAN.md was based on the assumption that critical Datalog features were missing. Code review revealed these features are **already implemented and tested**:

- ✅ Predicates in OR-clauses
- ✅ Predicates in rule bodies
- ✅ Unique identity upsert
- ✅ Transaction functions

This reduces the timeline from **13 weeks to 6 weeks** while adding critical performance validation that was missing from the original plan.

**Recommended Action**: Begin Phase 1 (Performance Validation) immediately to validate benchmark claims and identify any actual performance issues before proceeding with optimizations.
