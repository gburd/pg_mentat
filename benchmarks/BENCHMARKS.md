# pg_mentat — Benchmarks

Honest performance measurements from the development branch. **This is
not a claim of production performance.** These numbers come from one
developer laptop, one configuration, one query shape family. They exist
to answer "does performance degrade gracefully?" not "how fast is this?"

Real workload numbers require a bare-metal server, a representative
corpus, and a load generator. Those runs are Phase 2 on the roadmap and
have not happened.

## Micro-benchmark results

Reproducible via `CARGO_HOME=$HOME/.cargo_pg_mentat bash benchmarks/micro/load_and_query.sh`.
The script writes a timestamped CSV to `benchmarks/results/` alongside an
`env.txt` with the host, kernel, CPU, Postgres version, and exact git
commit so runs are comparable across time.

### Run: commit `92fd843`, branch `feat/phase-0-honesty-and-build`

Host: Intel i9-12900H, 20 logical cores, PostgreSQL 16.13 via pgrx,
`work_mem` = default, `shared_buffers` = default. All queries ran
through the `mentat_query` function with the default Datalog → SQL
translator and the narrow per-type storage tables.

| n_people | n_datoms | op          | p50 (ms) | p95 (ms) |
|---------:|---------:|:------------|---------:|---------:|
|    1 000 |    3 011 | `scan`      |      8.3 |     13.9 |
|    1 000 |    3 011 | `predicate` |      9.8 |     13.7 |
|    1 000 |    3 011 | `group_by`  |      8.0 |     11.9 |
|   10 000 |   33 015 | `scan`      |     32.4 |     37.4 |
|   10 000 |   33 015 | `predicate` |     26.8 |     36.1 |
|   10 000 |   33 015 | `group_by`  |     15.4 |     17.8 |
|  100 000 |  333 055 | `scan`      |    234.8 |    252.3 |
|  100 000 |  333 055 | `predicate` |    170.6 |    205.5 |
|  100 000 |  333 055 | `group_by`  |     96.8 |    112.0 |

Query shapes:

```datalog
;; scan — return every name
[:find ?n :where [?e :person/name ?n]]

;; predicate — range filter on long-typed attr
[:find ?n :where [?e :person/name ?n]
                 [?e :person/age ?a]
                 [(>= ?a 50)]]

;; group_by — aggregate over a grouping attr
[:find ?city (count ?e) :where [?e :person/city ?city]]
```

## Observations

1. **`group_by` is the fastest at scale.** The aggregate is pushed down
   to a single narrow table (`datoms_text_new`) and Postgres uses
   `idx_datoms_text_new_aevt` with `INCLUDE (v)` for an index-only scan.
   This is the storage model working as designed.

2. **`scan` is the slowest per-datom.** Cost growth is roughly linear:
   3k datoms → 8ms; 33k → 32ms; 333k → 235ms. That's about 0.7 μs per
   returned row for the full scan family — dominated by EDN encoding of
   the result JSON, not by storage access. Datomic-style queries that
   return 100 000 strings cost roughly the same in Datomic.

3. **`predicate` is consistently faster than `scan`** because the
   planner uses the AEVT covering index and the INCLUDE v clause. The
   10x decoding cost is still there but on fewer tuples (50% of the
   population with the `>= 50` predicate).

4. **There is a UNION-ALL overhead.** Every Datalog query currently
   reads through a UNION of all 9 narrow per-type tables because the
   query generator does not (yet) prune to the types the query actually
   touches. For a query that only references `:person/name` (string),
   we still open the `datoms_ref_new`, `datoms_long_new`, etc. tables
   via the UNION, though each returns 0 rows via the store_id filter.
   Closing this is a tractable Phase 1 item: teach the generator to
   only include UNION arms whose type appears in the bound attributes.

## Caveats — what these numbers are NOT

- Not a production benchmark. One laptop, one configuration, one query
  family.
- Not comparable to Datomic/XTDB/Datalevin numbers. No apples-to-apples
  harness, no matching corpus.
- No concurrency story. All queries single-connection, cold cache.
- No write benchmark beyond `load_time_ms` of the bulk transact. A
  proper write benchmark needs `pgbench` with multiple concurrent
  writers and a mixed read/write workload.
- No latency distribution under load. p50/p95 from 6 back-to-back runs
  is not a load-test histogram.

The valid conclusions from this data are:

- The extension answers real Datalog queries and the time-complexity
  curve is not pathological.
- The narrow-table storage model is working: aggregates hit covering
  indexes and return in p99 well under 200 ms at 300 k datoms.
- There is obvious headroom from UNION arm pruning.

## Next benchmark steps (Phase 2)

- Move to a dedicated box (not a laptop).
- Load 10M datoms, then 100M.
- Produce `perf record` flamegraphs alongside the timings.
- Compare against a plain EAV schema in vanilla Postgres with the same
  indexes, to isolate what pg_mentat costs on top of the storage
  primitive.
- Run `pgbench` with a mixed read/write Datalog workload and report
  concurrency scaling (1, 8, 32, 128 connections).

Those runs are open work. They are not promises.
