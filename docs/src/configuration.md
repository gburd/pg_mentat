# Configuration (GUCs)

pg_mentat exposes its configuration through PostgreSQL's Grand Unified Configuration (GUC) system. All parameters use the `mentat.` prefix and can be set at the session, transaction, or system level.

## Setting Parameters

```sql
-- Session level
SET mentat.query_timeout_ms = 60000;

-- Transaction level (reverts after transaction)
SET LOCAL mentat.max_result_rows = 500000;

-- System level (requires superuser, persists across restarts)
ALTER SYSTEM SET mentat.slow_query_threshold_ms = 200;
SELECT pg_reload_conf();

-- Per-query (via SET LOCAL in a transaction block)
BEGIN;
SET LOCAL mentat.enable_optimizer_hints = true;
SELECT mentat_query('...', '{}');
COMMIT;
```

## Query Execution Parameters

### `mentat.query_timeout_ms`

| Property | Value |
|----------|-------|
| Type | integer |
| Default | 30000 (30 seconds) |
| Range | 0 - 2147483647 |
| Context | userset |

Maximum execution time for a single Datalog query in milliseconds. Queries exceeding this limit are cancelled. Set to 0 to disable (not recommended in production).

```sql
SET mentat.query_timeout_ms = 60000;  -- 60 seconds
```

### `mentat.max_result_rows`

| Property | Value |
|----------|-------|
| Type | integer |
| Default | 100000 |
| Range | 0 - 2147483647 |
| Context | userset |

Maximum number of result rows returned by a single query. Prevents cartesian explosions from consuming all available memory. Set to 0 for unlimited (not recommended in production).

```sql
SET mentat.max_result_rows = 50000;
```

### `mentat.max_recursion_depth`

| Property | Value |
|----------|-------|
| Type | integer |
| Default | 100 |
| Range | 1 - 10000 |
| Context | userset |

Maximum depth for recursive rule evaluation (`WITH RECURSIVE` CTEs). Limits traversal depth to prevent infinite loops from cyclic data. Applied as the iteration limit on generated recursive CTEs.

```sql
SET mentat.max_recursion_depth = 50;
```

### `mentat.temp_file_limit`

| Property | Value |
|----------|-------|
| Type | string |
| Default | `"1GB"` |
| Context | userset |

Maximum disk space for intermediate results during query execution (sorts, hash joins, materialization). Applied via `SET LOCAL temp_file_limit`. Prevents disk exhaustion from large queries.

```sql
SET mentat.temp_file_limit = '2GB';
```

## Optimizer Parameters

### `mentat.enable_optimizer_hints`

| Property | Value |
|----------|-------|
| Type | boolean |
| Default | false |
| Context | userset |

When enabled, pg_mentat applies optimizer hints during query execution:
- Sets `work_mem` to `mentat.default_work_mem` for complex queries (multiple joins, aggregates, CTEs)
- May disable sequential scan for queries that should use indexes

Enable this if you observe suboptimal query plans. Disable if it conflicts with your global PostgreSQL tuning.

```sql
SET mentat.enable_optimizer_hints = true;
```

### `mentat.default_work_mem`

| Property | Value |
|----------|-------|
| Type | string |
| Default | `"64MB"` |
| Context | userset |

The `work_mem` value applied during Mentat query execution when `mentat.enable_optimizer_hints` is true. Only affects queries with multiple joins, aggregates, or CTEs.

```sql
SET mentat.default_work_mem = '128MB';
```

## Explain Parameters

### `mentat.explain_format`

| Property | Value |
|----------|-------|
| Type | string |
| Default | `"text"` |
| Context | userset |

Output format for `mentat_explain()` plans. Valid values: `text`, `json`, `yaml`, `xml`.

```sql
SET mentat.explain_format = 'json';
SELECT mentat_explain('[:find ?e :where [?e :person/name]]', '{}');
```

## Monitoring Parameters

### `mentat.slow_query_threshold_ms`

| Property | Value |
|----------|-------|
| Type | integer |
| Default | 100 |
| Range | 0 - 2147483647 |
| Context | userset |

Queries exceeding this threshold (in milliseconds) are logged at `WARNING` level with their execution time and generated SQL. Set to 0 to disable slow query logging.

```sql
SET mentat.slow_query_threshold_ms = 200;
```

### `mentat.log_all_queries`

| Property | Value |
|----------|-------|
| Type | boolean |
| Default | false |
| Context | userset |

When enabled, logs the generated SQL for every query (not just slow ones). Useful for debugging but verbose. Not recommended in production.

```sql
SET mentat.log_all_queries = true;
```

## Recommended Production Configuration

```sql
-- postgresql.conf or ALTER SYSTEM

-- Prevent runaway queries
ALTER SYSTEM SET mentat.query_timeout_ms = 30000;
ALTER SYSTEM SET mentat.max_result_rows = 100000;
ALTER SYSTEM SET mentat.max_recursion_depth = 100;
ALTER SYSTEM SET mentat.temp_file_limit = '1GB';

-- Monitor performance
ALTER SYSTEM SET mentat.slow_query_threshold_ms = 100;
ALTER SYSTEM SET mentat.log_all_queries = false;

-- Let the optimizer help
ALTER SYSTEM SET mentat.enable_optimizer_hints = true;
ALTER SYSTEM SET mentat.default_work_mem = '64MB';

SELECT pg_reload_conf();
```

## Recommended Development Configuration

```sql
-- More permissive for development/testing
SET mentat.query_timeout_ms = 0;          -- no timeout
SET mentat.max_result_rows = 0;           -- unlimited
SET mentat.max_recursion_depth = 1000;    -- deep graphs
SET mentat.slow_query_threshold_ms = 0;   -- log everything
SET mentat.log_all_queries = true;        -- see all SQL
SET mentat.explain_format = 'json';       -- structured plans
```

## Viewing Current Settings

```sql
SHOW mentat.query_timeout_ms;
SHOW mentat.enable_optimizer_hints;

-- Or view all mentat settings
SELECT name, setting, short_desc
FROM pg_settings
WHERE name LIKE 'mentat.%'
ORDER BY name;
```
