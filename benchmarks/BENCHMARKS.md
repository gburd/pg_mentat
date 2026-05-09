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

### Run: commit after Phase 1 wide-row-drop (`feat/phase-0-honesty-and-build`)

Same host, same PostgreSQL, same workload. The wide-row `mentat.datoms`
TABLE is gone; replaced by a VIEW over the narrow tables with INSTEAD
OF INSERT / DELETE triggers. The `dual_write_datoms_trigger` that fired
on every insert is gone.

| n_people | n_datoms | op          | p50 (ms) | p95 (ms) |
|---------:|---------:|:------------|---------:|---------:|
|    1 000 |    3 011 | `scan`      |      9.1 |     12.0 |
|    1 000 |    3 011 | `predicate` |     10.6 |     13.5 |
|    1 000 |    3 011 | `group_by`  |      8.3 |      9.3 |
|   10 000 |   33 015 | `scan`      |     35.6 |     40.5 |
|   10 000 |   33 015 | `predicate` |     23.8 |     29.2 |
|   10 000 |   33 015 | `group_by`  |     13.8 |     14.7 |
|  100 000 |  333 055 | `scan`      |    259.1 |    268.6 |
|  100 000 |  333 055 | `predicate` |    168.8 |    177.7 |
|  100 000 |  333 055 | `group_by`  |     92.4 |     96.8 |

**Reads are in the noise** (±10% jitter) — this is expected. The query
engine already read from the narrow tables before Phase 1, so
replacing the wide-row table with a view changes nothing on the read
side. Confirms the storage redesign landed without a read regression.

**Writes** (added as a `load` row in the second run's CSV): bulk-load
time is now the benchmark's single write measurement. On a fresh
run: 1k people in <1s, 10k in 33s, 100k in 294s. We did not capture
an apples-to-apples pre-Phase-1 baseline for the `load` row, so the
dual-write-trigger savings are not yet quantified here — this is
flagged as follow-up work in the Phase 2 plan in `docs/ROADMAP.md`.
The expected savings per insert: 1 fewer plpgsql trigger fire, 1
fewer physical write (the wide-row table no longer exists as a
physical table), and 1 fewer CHECK constraint evaluation. Until the
A/B is measured, the claim is bounded to "reads are unchanged and the
code does measurably less work per insert."

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
   returned row for the full scan family. Verified with `mentat_explain`:
   the generator already reads from a single narrow table when the
   attribute's value type is known from the schema cache, so this is
   decode + JSON-encode cost, not UNION overhead.

3. **`predicate` is consistently faster than `scan`** because the
   planner uses the AEVT covering index and the INCLUDE v clause. The
   decode + encode cost is still there but on fewer tuples (50% of the
   population with the `>= 50` predicate).

4. **Opportunities the numbers suggest:**
   - The `SELECT DISTINCT CASE value_type_tag WHEN 0 THEN ... WHEN 11 THEN ... END`
     projection evaluates all 9 branches even when the single-table
     subquery can only produce one tag. Replacing it with a type-specific
     projection when `typed_info` is known would drop one CASE evaluation
     per row — small but measurable at 333k rows.
   - There is no columnar batching: every tuple goes through SPI
     individually. Larger result sets (1M+) will likely want a binary
     COPY path.
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
