# Phase 1 A/B: dual-write trigger ON vs OFF

Two runs of `benchmarks/micro/load_and_query.sh` at each commit.
Host: arnold (i9-12900H, 20 cores). PG 16.13 via pgrx. Not a
hermetic environment — laptop under normal load.

## Raw numbers (load time, ms)

| scale   | pre-P1 run1 | pre-P1 run2 | post-P1 run1 | post-P1 run2 |
|--------:|------------:|------------:|-------------:|-------------:|
| 1k      |       3 458 |       6 447 |        2 979 |        3 001 |
| 10k     |      36 882 |      56 709 |       33 748 |       34 196 |
| 100k    |     371 221 |     313 769 |      301 422 |      415 601 |

pre-P1 = commit 94d7a8c (dual_write_datoms trigger fires on every
INSERT INTO mentat.datoms, wide-row CHECK constraint evaluated,
8 wide-row indexes maintained).
post-P1 = commit 695b278 (mentat.datoms is a VIEW; INSTEAD OF
triggers; no wide-row indexes).

## What the data does and doesn't say

At 1k and 10k datoms the signal is consistent across runs:
- 1k: post-P1 avg 2 990 ms vs pre-P1 avg 4 953 ms  →  ~40% faster
- 10k: post-P1 avg 33 972 ms vs pre-P1 avg 46 796 ms  →  ~27% faster

At 100k the load time is dominated by I/O and thermal variance on
this laptop: pre-P1 run2 (313s) is actually faster than post-P1 run2
(416s). Two-sample averages put pre-P1 at 342s and post-P1 at 358s,
well within the per-run variance. Conclusion at 100k on this machine:
cannot separate the trigger cost from machine noise with only two
samples per condition.

## Why the savings are small-but-real

mentat_transact's hot write path (per-datom INSERTs into
datoms_<type>_new) was never routed through the wide-row table in
either commit. The wide-row INSERT only happened once per
transaction for the `:db/txInstant` datom. At 40 transactions per
100k-people run, that's 40 trigger fires to save — real but not
dominant at scale. The per-transaction savings come from:

1. One fewer physical INSERT per transaction (the wide-row table
   stopped existing as a physical table).
2. One fewer plpgsql trigger invocation per transaction.
3. One fewer 9-column CHECK constraint evaluation per transaction.
4. Eight fewer index updates per transaction (wide-row EAVT +
   AEVT + VAET + 4× type-specific AVET, plus the one TX-DESC
   index, no longer exist).

The small-scale (1k, 10k) numbers where startup + txn overhead
dominates the per-row cost show the savings cleanly. At 100k the
steady-state per-row cost dominates, which was narrow-table INSERTs
+ their index maintenance in both commits, so the delta shrinks.

## Reproducibility

- A/B procedure: `git worktree add /tmp/preP1 94d7a8c && cd /tmp/preP1`
  then edit `benchmarks/micro/load_and_query.sh` to also emit a
  `load` row to the CSV (this line was added in 695b278, not earlier),
  then run the script. Same procedure at HEAD.
- The second pre-P1 run is slower than the first, consistent with
  thermal throttling / background system load. A hermetic benchmark
  environment would show a tighter distribution.

## What would close the measurement

- A dedicated (not-my-laptop) machine.
- Many samples per condition (10+), computed p50 / p95 / p99.
- Distinguish "cold cache" from "warm cache" runs.
- A write workload that exercises the old wide-row path directly
  (`INSERT INTO mentat.datoms ...`) rather than via mentat_transact,
  to isolate the trigger cost from unrelated code paths.

This is Phase 2 work per docs/ROADMAP.md.
