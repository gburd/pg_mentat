# Query Optimization

pg_mentat automatically applies PostgreSQL optimizer hints when executing
Datalog queries translated to SQL. This document explains the available
strategies and how to configure them.

## Automatic Optimizer Hints

When `mentat.enable_optimizer_hints` is `on` (the default), the extension
issues `SET LOCAL` statements inside the SPI transaction before running
the generated SQL. These settings revert automatically at transaction end
and do not affect other sessions.

### Sequential Scan Suppression

Every Mentat query over the `datoms` table sets:

```sql
SET LOCAL enable_seqscan = off;
```

This tells PostgreSQL to prefer the covering indexes (`idx_datoms_eavt`,
`idx_datoms_aevt`, `idx_datoms_avet`, `idx_datoms_vaet`) over a
sequential scan.  Because Datalog queries always filter on at least one
of entity, attribute, or value, an index scan is almost always faster
than a full table scan.

### Work Memory for Complex Queries

Queries that meet any of the following criteria are classified as
"complex":

- More than 2 pattern-clause joins
- Use of aggregate functions (`count`, `sum`, `avg`, `min`, `max`)
- Common Table Expressions (recursive rules)
- UNION branches (`or`-join clauses)

For complex queries, the extension additionally sets:

```sql
SET LOCAL work_mem = '<mentat.default_work_mem>';
```

The default value is `64MB`, which gives PostgreSQL more memory for
in-flight sorts and hash operations. Adjust this based on your workload
and available RAM.

## GUC Configuration Parameters

### mentat.enable_optimizer_hints

| Property   | Value                              |
|------------|------------------------------------|
| Type       | boolean                            |
| Default    | `on`                               |
| Context    | user (any session can change it)   |
| Scope      | session                            |

Toggle all automatic optimizer hints on or off:

```sql
-- Disable hints for this session
SET mentat.enable_optimizer_hints = off;

-- Re-enable
SET mentat.enable_optimizer_hints = on;
```

### mentat.default_work_mem

| Property   | Value                              |
|------------|------------------------------------|
| Type       | string                             |
| Default    | `64MB`                             |
| Context    | user (any session can change it)   |
| Scope      | session                            |

The value passed to `SET LOCAL work_mem` for complex queries:

```sql
-- Increase to 256MB for a session doing heavy analytical queries
SET mentat.default_work_mem = '256MB';

-- Reduce to 32MB for a low-memory environment
SET mentat.default_work_mem = '32MB';
```

You can also set these in `postgresql.conf` for cluster-wide defaults:

```
mentat.enable_optimizer_hints = on
mentat.default_work_mem = '128MB'
```

## Index Strategy

The `datoms` table has four covering indexes that match classic Datomic
access patterns:

| Index               | Column Order                  | Best For                                  |
|---------------------|-------------------------------|-------------------------------------------|
| `idx_datoms_eavt`   | (e, a, value_type_tag, v, tx) | Entity lookups -- pull all attributes      |
| `idx_datoms_aevt`   | (a, e, value_type_tag, v, tx) | Attribute scans -- find entities by attr   |
| `idx_datoms_avet`   | (a, value_type_tag, v, e, tx) | Value lookups -- find by attr + value      |
| `idx_datoms_vaet`   | (v, a, e, tx) WHERE tag = 0  | Reverse ref lookups -- who points to whom  |
| `idx_datoms_tx`     | (tx)                          | Transaction-based filtering                |

The Datalog-to-SQL compiler picks the appropriate access path by the
columns it constrains in each pattern clause.  Disabling `enable_seqscan`
ensures PostgreSQL chooses these indexes rather than falling back to a
sequential scan when cost estimates are close.

## Helper SQL Functions

pg_mentat exposes several SQL functions for manual query analysis:

```sql
-- Suggest the best index for a given access pattern
SELECT mentat.suggest_index('a');       -- 'idx_datoms_aevt'
SELECT mentat.suggest_index('av');      -- 'idx_datoms_avet'

-- Estimate relative cost of an access pattern
SELECT mentat.estimate_query_cost('e', 1000);

-- Analyze a SQL string for optimization opportunities
SELECT mentat.analyze_query('SELECT * FROM mentat.datoms WHERE a = 42');

-- List all indexes with usage recommendations
SELECT * FROM mentat.get_index_info();
```

## Verifying Hint Effectiveness

Use `EXPLAIN (ANALYZE, BUFFERS)` to verify that index scans are being
used. You can call `mentat_query` inside a transaction where you first
run `EXPLAIN ANALYZE` on the generated SQL:

```sql
BEGIN;
-- Get the generated SQL from the NOTICE log, or use mentat_stmt_cache_stats()
-- to see cached queries, then run:
EXPLAIN (ANALYZE, BUFFERS)
SELECT DISTINCT ...
FROM mentat.datoms datoms0
WHERE datoms0.a = (SELECT entid FROM mentat.schema WHERE ident = ':person/name')
  AND datoms0.added = true;
ROLLBACK;
```

Look for `Index Scan` or `Index Only Scan` in the output. If you see
`Seq Scan`, check that the table has been analyzed (`ANALYZE mentat.datoms`)
and that the indexes exist.

## Tuning Recommendations

1. **Run ANALYZE regularly.** After bulk imports or schema changes, run
   `ANALYZE mentat.datoms;` so the planner has accurate statistics.

2. **Monitor index usage.** Use `mentat.mentat_storage_stats()` or query
   `pg_stat_user_indexes` to verify indexes are being used.

3. **Adjust work_mem for your workload.** If you run many concurrent
   sessions with complex queries, reduce `mentat.default_work_mem` to
   avoid total memory exceeding available RAM.  A rule of thumb:
   `max_connections * mentat.default_work_mem` should fit within your
   memory budget.

4. **Disable hints for debugging.** If a query performs unexpectedly,
   try `SET mentat.enable_optimizer_hints = off;` and compare `EXPLAIN`
   plans with and without hints to see if the planner makes better
   choices on its own.
