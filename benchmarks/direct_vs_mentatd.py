#!/usr/bin/env python3
"""
Benchmark: Direct PostgreSQL vs mentatd (HTTP).

Measures latency and throughput for the two access paths:
  1. Direct PostgreSQL -- psycopg2 calling SQL functions
  2. Via mentatd -- HTTP/EDN requests to the daemon

Prerequisites:
    pip install psycopg2-binary requests

Usage:
    # Both paths (mentatd must be running on :8080):
    python direct_vs_mentatd.py

    # Direct PostgreSQL only:
    python direct_vs_mentatd.py --direct-only

    # Custom endpoints:
    python direct_vs_mentatd.py --pg-dsn "dbname=mydb" --mentatd-url "http://host:8080"

Environment variables:
    PG_MENTAT_DSN       PostgreSQL connection string (default: dbname=postgres)
    MENTATD_URL         mentatd base URL (default: http://localhost:8080)
    BENCH_ITERATIONS    Number of iterations per test (default: 1000)
"""

import argparse
import json
import os
import statistics
import sys
import time

# ---------------------------------------------------------------------------
# Direct PostgreSQL path
# ---------------------------------------------------------------------------

def bench_direct_query(dsn, query, inputs, iterations):
    """Benchmark a Datalog query via direct PostgreSQL."""
    import psycopg2

    conn = psycopg2.connect(dsn)
    conn.autocommit = True
    cur = conn.cursor()
    inputs_json = json.dumps(inputs)

    latencies = []
    for _ in range(iterations):
        start = time.perf_counter()
        cur.execute("SELECT mentat_query(%s, %s::jsonb)", (query, inputs_json))
        cur.fetchone()
        latencies.append((time.perf_counter() - start) * 1000)  # ms

    cur.close()
    conn.close()
    return latencies


def bench_direct_transact(dsn, edn_tx, iterations):
    """Benchmark a transaction via direct PostgreSQL."""
    import psycopg2

    conn = psycopg2.connect(dsn)
    conn.autocommit = True
    cur = conn.cursor()

    latencies = []
    for _ in range(iterations):
        start = time.perf_counter()
        cur.execute("SELECT mentat_transact(%s)", (edn_tx,))
        cur.fetchone()
        latencies.append((time.perf_counter() - start) * 1000)

    cur.close()
    conn.close()
    return latencies


def bench_direct_pull(dsn, pattern, entity_id, iterations):
    """Benchmark a pull via direct PostgreSQL."""
    import psycopg2

    conn = psycopg2.connect(dsn)
    conn.autocommit = True
    cur = conn.cursor()

    latencies = []
    for _ in range(iterations):
        start = time.perf_counter()
        cur.execute("SELECT mentat_pull(%s, %s)", (pattern, entity_id))
        cur.fetchone()
        latencies.append((time.perf_counter() - start) * 1000)

    cur.close()
    conn.close()
    return latencies


# ---------------------------------------------------------------------------
# mentatd (HTTP/EDN) path
# ---------------------------------------------------------------------------

def bench_mentatd_query(base_url, query, inputs, iterations):
    """Benchmark a Datalog query via mentatd HTTP."""
    import requests

    session = requests.Session()
    edn_body = f'{{:op :query :query "{query}" :args {{}}}}'

    latencies = []
    for _ in range(iterations):
        start = time.perf_counter()
        resp = session.post(
            base_url,
            data=edn_body,
            headers={"Content-Type": "application/edn"},
        )
        resp.raise_for_status()
        latencies.append((time.perf_counter() - start) * 1000)

    return latencies


def bench_mentatd_transact(base_url, edn_tx, iterations):
    """Benchmark a transaction via mentatd HTTP."""
    import requests

    session = requests.Session()
    edn_body = f'{{:op :transact :tx-data {edn_tx}}}'

    latencies = []
    for _ in range(iterations):
        start = time.perf_counter()
        resp = session.post(
            base_url,
            data=edn_body,
            headers={"Content-Type": "application/edn"},
        )
        resp.raise_for_status()
        latencies.append((time.perf_counter() - start) * 1000)

    return latencies


def bench_mentatd_pull(base_url, pattern, entity_id, iterations):
    """Benchmark a pull via mentatd HTTP."""
    import requests

    session = requests.Session()
    edn_body = f'{{:op :pull :selector {pattern} :eid {entity_id}}}'

    latencies = []
    for _ in range(iterations):
        start = time.perf_counter()
        resp = session.post(
            base_url,
            data=edn_body,
            headers={"Content-Type": "application/edn"},
        )
        resp.raise_for_status()
        latencies.append((time.perf_counter() - start) * 1000)

    return latencies


# ---------------------------------------------------------------------------
# Reporting
# ---------------------------------------------------------------------------

def report(name, latencies):
    """Print latency statistics."""
    n = len(latencies)
    avg = statistics.mean(latencies)
    med = statistics.median(latencies)
    p95 = sorted(latencies)[int(n * 0.95)]
    p99 = sorted(latencies)[int(n * 0.99)]
    mn = min(latencies)
    mx = max(latencies)
    throughput = 1000.0 / avg  # ops/sec

    print(f"  {name}")
    print(f"    iterations : {n}")
    print(f"    avg        : {avg:.2f} ms")
    print(f"    median     : {med:.2f} ms")
    print(f"    p95        : {p95:.2f} ms")
    print(f"    p99        : {p99:.2f} ms")
    print(f"    min        : {mn:.2f} ms")
    print(f"    max        : {mx:.2f} ms")
    print(f"    throughput : {throughput:.0f} ops/sec")
    print()
    return {"avg": avg, "median": med, "p95": p95, "p99": p99, "throughput": throughput}


def compare(direct_stats, mentatd_stats, label):
    """Print a comparison between direct and mentatd."""
    if direct_stats and mentatd_stats:
        ratio = mentatd_stats["avg"] / direct_stats["avg"]
        print(f"  {label}: mentatd is {ratio:.1f}x slower on average")
        print(f"    Direct avg   : {direct_stats['avg']:.2f} ms")
        print(f"    mentatd avg  : {mentatd_stats['avg']:.2f} ms")
        print(f"    Overhead     : {mentatd_stats['avg'] - direct_stats['avg']:.2f} ms")
        print()


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Benchmark direct PostgreSQL vs mentatd")
    parser.add_argument("--pg-dsn", default=os.environ.get("PG_MENTAT_DSN", "dbname=postgres"))
    parser.add_argument("--mentatd-url", default=os.environ.get("MENTATD_URL", "http://localhost:8080"))
    parser.add_argument("--iterations", type=int,
                        default=int(os.environ.get("BENCH_ITERATIONS", "1000")))
    parser.add_argument("--direct-only", action="store_true",
                        help="Skip mentatd benchmarks")
    args = parser.parse_args()

    print("=" * 60)
    print("pg_mentat: Direct PostgreSQL vs mentatd benchmark")
    print("=" * 60)
    print(f"  PostgreSQL DSN  : {args.pg_dsn}")
    print(f"  mentatd URL     : {args.mentatd_url}")
    print(f"  Iterations      : {args.iterations}")
    print()

    # -- Test data -------------------------------------------------------
    simple_query = "[:find ?name :where [?e :person/name ?name]]"
    simple_inputs = {}
    small_tx = '[{:person/name "Benchmark User" :person/email "bench@test.com"}]'
    pull_pattern = "[*]"
    pull_entity_id = 10001  # adjust to an existing entity

    # -- Setup: ensure schema exists via direct connection ----------------
    try:
        import psycopg2
        conn = psycopg2.connect(args.pg_dsn)
        conn.autocommit = True
        cur = conn.cursor()
        cur.execute("""
            SELECT mentat_transact('[
              {:db/ident :person/name
               :db/valueType :db.type/string
               :db/cardinality :db.cardinality/one}
              {:db/ident :person/email
               :db/valueType :db.type/string
               :db/cardinality :db.cardinality/one
               :db/unique :db.unique/identity}
            ]')
        """)
        cur.close()
        conn.close()
        print("Schema initialized.\n")
    except Exception as e:
        print(f"Warning: Could not initialize schema: {e}")
        print("Continuing anyway -- schema may already exist.\n")

    # -- Benchmarks ------------------------------------------------------
    results = {}

    # 1. Query
    print("-" * 60)
    print("QUERY: [:find ?name :where [?e :person/name ?name]]")
    print("-" * 60)

    try:
        direct = bench_direct_query(args.pg_dsn, simple_query, simple_inputs, args.iterations)
        results["direct_query"] = report("Direct PostgreSQL", direct)
    except Exception as e:
        print(f"  Direct PostgreSQL: FAILED ({e})\n")
        results["direct_query"] = None

    mentatd_query_stats = None
    if not args.direct_only:
        try:
            mentatd = bench_mentatd_query(args.mentatd_url, simple_query, simple_inputs, args.iterations)
            mentatd_query_stats = report("Via mentatd (HTTP/EDN)", mentatd)
            results["mentatd_query"] = mentatd_query_stats
        except Exception as e:
            print(f"  Via mentatd: FAILED ({e})\n")

    compare(results.get("direct_query"), mentatd_query_stats, "Query")

    # 2. Transaction
    print("-" * 60)
    print("TRANSACT: single entity insert")
    print("-" * 60)

    try:
        direct = bench_direct_transact(args.pg_dsn, small_tx, args.iterations)
        results["direct_transact"] = report("Direct PostgreSQL", direct)
    except Exception as e:
        print(f"  Direct PostgreSQL: FAILED ({e})\n")
        results["direct_transact"] = None

    mentatd_tx_stats = None
    if not args.direct_only:
        try:
            mentatd = bench_mentatd_transact(args.mentatd_url, small_tx, args.iterations)
            mentatd_tx_stats = report("Via mentatd (HTTP/EDN)", mentatd)
            results["mentatd_transact"] = mentatd_tx_stats
        except Exception as e:
            print(f"  Via mentatd: FAILED ({e})\n")

    compare(results.get("direct_transact"), mentatd_tx_stats, "Transact")

    # 3. Pull
    print("-" * 60)
    print(f"PULL: [*] for entity {pull_entity_id}")
    print("-" * 60)

    try:
        direct = bench_direct_pull(args.pg_dsn, pull_pattern, pull_entity_id, args.iterations)
        results["direct_pull"] = report("Direct PostgreSQL", direct)
    except Exception as e:
        print(f"  Direct PostgreSQL: FAILED ({e})\n")
        results["direct_pull"] = None

    mentatd_pull_stats = None
    if not args.direct_only:
        try:
            mentatd = bench_mentatd_pull(args.mentatd_url, pull_pattern, pull_entity_id, args.iterations)
            mentatd_pull_stats = report("Via mentatd (HTTP/EDN)", mentatd)
            results["mentatd_pull"] = mentatd_pull_stats
        except Exception as e:
            print(f"  Via mentatd: FAILED ({e})\n")

    compare(results.get("direct_pull"), mentatd_pull_stats, "Pull")

    # -- Summary ---------------------------------------------------------
    print("=" * 60)
    print("SUMMARY")
    print("=" * 60)
    print()
    print("Direct PostgreSQL eliminates HTTP overhead (typically 0.5-2ms"),
    print("per request) and EDN serialization/deserialization on the")
    print("mentatd side. For latency-sensitive workloads, direct access")
    print("is the recommended approach.")
    print()
    print("Use mentatd when you need:")
    print("  - Datomic client protocol compatibility (Clojure clients)")
    print("  - Transit+JSON or Transit+MessagePack wire formats")
    print("  - The HTTP caching layer provided by mentatd")
    print("  - Network isolation (app servers cannot reach PostgreSQL directly)")
    print()


if __name__ == "__main__":
    main()
