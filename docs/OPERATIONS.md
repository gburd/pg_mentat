# Operations Guide

This document covers installation, configuration, monitoring, backup/restore, performance tuning, mentatd deployment, and troubleshooting for pg_mentat in production environments.

---

## Installation and Upgrade

### Fresh Installation

```sql
CREATE EXTENSION IF NOT EXISTS pg_mentat;
```

Requires the shared library to be installed in `$libdir` (handled by `cargo pgrx install --release`).

### Upgrading

The extension uses PostgreSQL's standard `ALTER EXTENSION ... UPDATE` mechanism:

```sql
ALTER EXTENSION pg_mentat UPDATE TO '1.1.0';
```

Upgrade scripts (e.g., `pg_mentat--1.0.0--1.1.0.sql`) are shipped with the extension and handle schema migrations.

### Verifying Installation

```sql
-- Check extension version
SELECT extversion FROM pg_extension WHERE extname = 'pg_mentat';

-- Check that core tables exist
SELECT count(*) > 0 AS healthy FROM mentat.stores;

-- Verify bootstrap schema
SELECT mentat_schema();
```

---

## Configuration (GUCs)

pg_mentat registers several Grand Unified Configuration (GUC) parameters. These are session-local by default and can be set at the session, transaction, or database level.

### Planner and Query Execution

| GUC | Type | Default | Description |
|-----|------|---------|-------------|
| `mentat.enable_optimizer_hints` | bool | `false` | When true, pg_mentat applies `SET LOCAL work_mem` and other optimizer hints before executing queries with multiple joins or aggregates. |
| `mentat.default_work_mem` | string | `'64MB'` | The `work_mem` value applied when optimizer hints are enabled. Increase for queries with heavy sorts or hash joins. |
| `mentat.query_timeout_ms` | integer | `30000` | Maximum execution time per query in milliseconds. Queries exceeding this are cancelled. Set to 0 to disable. |
| `mentat.max_result_rows` | integer | `100000` | Safety limit on returned rows. If a query would return more rows and no explicit `:limit` is set, it fails with an error. Prevents cartesian explosions. |
| `mentat.max_recursion_depth` | integer | `100` | Maximum depth for recursive rule evaluation (CTE `MAXRECURSION`). |
| `mentat.temp_file_limit` | string | `'1GB'` | Maximum temporary disk space per query. Prevents runaway sorts from filling disk. |

### Monitoring

| GUC | Type | Default | Description |
|-----|------|---------|-------------|
| `mentat.slow_query_threshold_ms` | integer | `100` | Queries slower than this threshold are logged at WARNING level with their Datalog source and generated SQL. Set to 0 to disable. |
| `mentat.log_all_queries` | bool | `false` | Log generated SQL for every query at NOTICE level. Useful for debugging, verbose in production. |

### Query Explain

| GUC | Type | Default | Description |
|-----|------|---------|-------------|
| `mentat.explain_format` | string | `'text'` | Output format for `mentat_explain`. Options: `text`, `json`, `yaml`, `xml`. |

### Setting GUCs

```sql
-- Session-level
SET mentat.query_timeout_ms = 5000;
SET mentat.max_result_rows = 500;
SET mentat.enable_optimizer_hints = on;
SET mentat.default_work_mem = '128MB';

-- Transaction-level (reverts after COMMIT/ROLLBACK)
SET LOCAL mentat.query_timeout_ms = 60000;

-- Database-level (persistent default for all sessions)
ALTER DATABASE mydb SET mentat.max_result_rows = 50000;

-- Check current value
SHOW mentat.query_timeout_ms;

-- Reset to default
RESET mentat.explain_format;
```

Note: GUCs are not available until the library is loaded. Either `CREATE EXTENSION pg_mentat` or `LOAD 'pg_mentat'` must have been executed in the session.

---

## Monitoring

### Query Statistics

Per-backend statistics (reset on disconnect):

```sql
SELECT mentat_query_stats();
```

Returns JSON with:
- Total queries executed
- Cache hit/miss counts and hit ratio
- Average and max execution times
- Slow query count

### Slow Query Log

Queries exceeding `mentat.slow_query_threshold_ms` are logged at WARNING level with:
- The original Datalog query
- The generated SQL
- Execution time in milliseconds

Monitor via PostgreSQL's standard logging infrastructure (`log_min_messages`, `csvlog`, etc.).

### Storage Statistics

```sql
SELECT mentat_storage_stats();
```

Returns per-table row counts, index sizes, and bloat estimates for all datom tables.

### Prepared Statement Cache

```sql
-- View cache statistics (hit rate, entries)
SELECT mentat_stmt_cache_stats();

-- Clear the cache (e.g., after schema changes)
SELECT mentat_stmt_cache_clear();
```

The LRU cache holds up to 256 prepared statements per backend. Schema-affecting transactions automatically invalidate the cache.

### Prometheus Metrics (mentatd)

When deployed with mentatd, metrics are exposed at `/metrics` in Prometheus text format:
- `mentatd_requests_total` (counter, labeled by operation)
- `mentatd_request_duration_seconds` (histogram)
- `mentatd_active_connections` (gauge)
- `mentatd_pool_size` (gauge)
- `mentatd_errors_total` (counter, labeled by type)

---

## Backup and Restore

pg_mentat stores all data in standard PostgreSQL tables. Any PostgreSQL backup method works:

### pg_dump (Logical)

```bash
# Full dump including extension
pg_dump -Fc -d mydb -f mydb.dump

# Restore
pg_restore -d mydb mydb.dump
```

### pg_basebackup (Physical)

```bash
pg_basebackup -D /backup/latest -Fp -Xs -P
```

### Point-in-Time Recovery

Since pg_mentat is append-only by nature (retractions are new datoms with `added=false`), WAL-based PITR provides fine-grained recovery. Combine with `mentat_query` temporal options (`asOf`) for application-level rollback without database restore.

### Store-Level Export

Export a single store's data:

```bash
pg_dump -d mydb -n mentat -n mentat_mystore -Fc -f store_backup.dump
```

---

## Performance Tuning

### Schema Design

- **Use `:db/index true`** on attributes frequently used in value lookups (`:where [?e :attr specific-value]`).
- **Use `:db/fulltext true`** only on text attributes that need keyword search.
- **Prefer `:db.cardinality/one`** where possible; cardinality-many requires additional deduplication in queries.
- **Namespace attributes** logically (`:person/name`, `:order/total`) to avoid unintentional cross-entity joins.

### Query Patterns

- **Bind constants early**: Place patterns with constant values first in `:where` to filter early.
- **Use explicit `:limit`**: Avoid fetching unlimited result sets.
- **Prefer `mentat_pull_many`** over loop-calling `mentat_pull` for batch entity retrieval.
- **Use collection bindings** (`[:find ?name :in [?id ...] :where ...]`) instead of OR for large value sets.
- **Inspect generated SQL**: Use `mentat_explain` or `mentat_query_sql` to examine the plan.

### PostgreSQL Tuning

Key PostgreSQL parameters for pg_mentat workloads:

```
shared_buffers = 25% of RAM
effective_cache_size = 75% of RAM
work_mem = 64MB                    # per-operation sort memory
maintenance_work_mem = 512MB       # for VACUUM and index builds
random_page_cost = 1.1             # for SSD storage
effective_io_concurrency = 200     # for SSD storage
max_parallel_workers_per_gather = 4
```

### Index Maintenance

The narrow tables have partial indexes (predicated on `added = true`) that exclude retracted datoms. Run `ANALYZE` after bulk loads:

```sql
ANALYZE mentat.datoms_text_new;
ANALYZE mentat.datoms_long_new;
ANALYZE mentat.datoms_ref_new;
-- etc. for all 9 tables
```

Autovacuum is configured on the transactions table (`autovacuum_vacuum_scale_factor = 0.1`). For high-throughput workloads, consider lowering it further on datom tables.

---

## Troubleshooting

### Common Errors

**"Unknown attribute ident: :foo/bar"**

The attribute has not been defined. Transact a schema definition first:
```sql
SELECT mentat_transact('[{:db/ident :foo/bar :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]');
```

**"Query returned more than N rows"**

The result exceeded `mentat.max_result_rows`. Either add `:limit` to your query, add more specific `:where` patterns, or increase the GUC:
```sql
SET mentat.max_result_rows = 500000;
```

**"Compare-and-swap failed: current value does not match"**

A `:db.fn/cas` transaction function found the attribute's current value differs from the expected old value. This indicates concurrent modification. Retry the transaction with fresh data.

**"Excision not allowed for entity"**

The partition has `allow_excision = false`. Enable it:
```sql
UPDATE mentat.partitions SET allow_excision = true WHERE name = 'db.part/user';
```

**Schema-related errors after concurrent transactions**

The schema cache may be stale. It auto-refreshes via `mentat.cache_generation`, but you can force it:
```sql
SELECT mentat_stmt_cache_clear();
```

### Diagnostic Tools

```sql
-- Inspect generated SQL without executing
SELECT mentat_query_sql('[:find ?name :where [?e :person/name ?name]]', '{}'::jsonb);

-- Full explain with PostgreSQL plan
SELECT mentat_explain('[:find ?name :where [?e :person/name ?name]]', '{}'::jsonb);

-- List available transaction functions
SELECT mentat.transaction_fns();

-- Check store health
SELECT mentat.list_stores();
SELECT mentat_storage_stats();
```

---

## mentatd Deployment

mentatd is a standalone HTTP server providing a Datomic-compatible API over pg_mentat. It connects to PostgreSQL as a client and exposes endpoints for remote query, transact, and pull operations.

### Architecture

```
Clients  -->  mentatd (HTTP/WebSocket)  -->  PostgreSQL + pg_mentat
                  |
            Prometheus <-- /metrics
```

### Configuration

mentatd is configured via a TOML file or environment variables:

| Setting | Env Var | Default | Description |
|---------|---------|---------|-------------|
| `server.host` | `MENTATD_HOST` | `0.0.0.0` | Listen address |
| `server.port` | `MENTATD_PORT` | `8080` | Listen port |
| `database.connection_string` | `DATABASE_URL` | -- | PostgreSQL connection string |
| `database.pool_size` | `MENTATD_POOL_SIZE` | `10` | Connection pool size |
| `cache.enabled` | -- | `true` | Query result caching |
| `cache.capacity` | -- | `1000` | LRU cache entries |
| `cache.ttl_secs` | -- | `60` | Cache TTL |
| `server.api_key` | `MENTATD_API_KEY` | None | Optional API key for auth |

### Endpoints

| Path | Method | Description |
|------|--------|-------------|
| `/` | POST | Primary endpoint (auto-detects operation) |
| `/api/query` | POST | Datalog query |
| `/api/transact` | POST | EDN transaction |
| `/api/pull` | POST | Pull entity data |
| `/api/db-stats` | POST | Database statistics |
| `/api/datoms` | POST | Raw datom access |
| `/api/list-dbs` | POST | List available stores |
| `/api/create-db` | POST | Create a new store |
| `/api/delete-db` | POST | Delete a store |
| `/stream/query` | POST | Streaming query results |
| `/health` | GET | Health check |
| `/metrics` | GET | Prometheus metrics |

### Content Types

mentatd accepts:
- `application/edn` (EDN format)
- `application/transit+json` (Transit JSON)
- `application/transit+msgpack` (Transit MessagePack)

Response format is negotiated via the `Accept` header.

### Docker Compose Deployment

The full stack (PostgreSQL, mentatd, Prometheus, Grafana) is available via Docker Compose:

```bash
docker compose -f docker/docker-compose.yml up -d
```

Services:
- PostgreSQL with pg_mentat: port 5432
- mentatd HTTP API: port 8080
- Prometheus: port 9090
- Grafana dashboards: port 3000
- PostgreSQL exporter: port 9187

---

## Known Limitations

| Limitation | Details | Workaround |
|------------|---------|------------|
| No attribute removal | Schema attributes are permanent | Retract all data for unused attributes; add `:db/noHistory true` to stop accumulating history |
| No value type change | Cannot alter `:db/valueType` after creation | Create a new attribute with the desired type and migrate data |
| Single-node only | No built-in replication beyond PostgreSQL | Use PostgreSQL streaming replication or logical replication |
| Rule recursion via CTE | Deep recursion may hit stack/memory limits | Tune `mentat.max_recursion_depth` and PostgreSQL `max_stack_depth` |
| No schema rollback | Schema transactions cannot be undone with `mentat_with` | Test schema changes in a separate store first |
| OR clause performance | Large OR branches generate UNION ALL | Consider restructuring as collection bindings for better performance |
| Prepared statement invalidation | Schema changes clear the entire cache | Batch schema changes to minimize cache churn |
| Excision constraints | Cannot excise entities with incoming references | Retract references first, then excise |
