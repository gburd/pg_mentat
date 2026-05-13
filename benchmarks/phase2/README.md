# Phase 2 benchmark harness for pg_mentat

**This is a scaled-down Phase 2 run on a developer laptop.** The ROADMAP's
Phase 2 target is 10M+ datoms on dedicated hardware with a hermetic
environment. This harness runs the same workload at 100K / 300K / 1M
datoms on a laptop so we have a defensible curve, a flamegraph, and an
apples-to-apples EAV baseline — but the numbers are **not a production
performance claim**.

## Layout

```
benchmarks/phase2/
├── README.md                  # this file
├── schema.edn                 # Datalog schema (issue tracker)
├── eav_baseline/
│   └── schema.sql             # equivalent plain-EAV schema (Postgres tables)
├── gen_dataset.py             # deterministic dataset generator (seed = fixed)
├── queries/
│   ├── q1_point_lookup.edn    # find user by email
│   ├── q1_point_lookup.sql    # same as plain EAV SQL
│   ├── q2_ref_traversal.edn   # issues assigned to a user
│   ├── q2_ref_traversal.sql   # ditto
│   ├── q3_aggregate.edn       # count of issues per state
│   ├── q3_aggregate.sql       # ditto
│   ├── q4_predicate.edn       # high-priority open issues
│   └── q4_predicate.sql       # ditto
├── run.sh                     # main driver: gen + load + warm + measure + CSV
├── perf_capture.sh            # profile the hot path, produce flamegraph.svg
└── tools/
    ├── stackcollapse-perf.pl  # Brendan Gregg's FlameGraph scripts (BSD)
    └── flamegraph.pl
```

## What we measure

- **Load throughput**: wall time for bulk insert.
- **Query latency**: p50 / p95 / p99 from 30 repetitions per query.
- **Plan**: `EXPLAIN (ANALYZE, BUFFERS)` for every query, captured.
- **pg_stat_statements**: top-10 slowest normalised statements.
- **Index/table sizes**: `pg_total_relation_size` per narrow table.
- **CPU flamegraph**: `perf record` during the query replay at the
  middle scale, then `perf script | stackcollapse | flamegraph`.

## Reproduce

```
CARGO_HOME=$HOME/.cargo_pg_mentat bash benchmarks/phase2/run.sh
```

Results land in `benchmarks/results/phase2-<timestamp>/`.

## Caveats (read these before citing any number)

1. **Laptop, not a benchmark box.** Intel i9-12900H with thermal
   throttling, background browser tabs, and whatever Chrome is doing.
   Run-to-run variance is visible at larger scales.
2. **pgrx-managed Postgres**, not a tuned production instance.
   `shared_buffers`, `work_mem`, `effective_cache_size`, `max_wal_size`
   are all defaults.
3. **Single connection, no concurrency.** Nothing here speaks to how
   pg_mentat behaves under contention.
4. **One workload family.** Issue-tracker schema; YMMV on document-
   heavy or graph-heavy workloads.
5. **The EAV baseline is intentionally dumb.** A plain `(entity,
   attribute, value_long, value_text, ...)` table with EAVT/AEVT
   indexes. It isolates the overhead pg_mentat adds on top of an
   equivalent storage layout; it is **not** an optimised Datalog
   implementation. A query-engineered SQL rewrite against the same
   EAV tables would beat both.

The hermetic-environment, 10M-datom, long-duration version of this
suite is the real Phase 2 and remains open.
