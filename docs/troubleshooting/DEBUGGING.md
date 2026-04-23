# Debugging pg_mentat

Advanced debugging techniques for diagnosing and fixing issues with pg_mentat.

## Debugging Queries

### View Query Plan

Use PostgreSQL's `EXPLAIN` to see how queries are executed:

```sql
EXPLAIN ANALYZE
SELECT mentat.mentat_query(
  '[:find ?name ?age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    [(> ?age 25)]]',
  '{}'::jsonb
);
```

**Look for**:
- Sequential Scans (should use indexes where possible)
- High cost estimates
- Long execution time

### Enable Statement Logging

```sql
-- Show all statements
SET log_statement = 'all';

-- Show query plans
SET auto_explain.log_min_duration = 0;

-- Show more detail
SET client_min_messages = 'DEBUG1';
```

### Inspect Generated SQL

pg_mentat converts Datalog to SQL. To see the generated SQL:

```sql
-- Add verbose logging to mentat_query
SET client_min_messages = 'LOG';

SELECT mentat.mentat_query(
  '[:find ?e :where [?e :person/name]]',
  '{}'::jsonb
);

-- Check PostgreSQL logs for the generated SQL
```

### Test Datalog Patterns Incrementally

Build complex queries step by step:

```sql
-- Step 1: Find all persons
SELECT mentat.mentat_query(
  '[:find ?e :where [?e :person/name]]',
  '{}'::jsonb
);

-- Step 2: Add name binding
SELECT mentat.mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);

-- Step 3: Add age filter
SELECT mentat.mentat_query(
  '[:find ?e ?name
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    [(> ?age 25)]]',
  '{}'::jsonb
);
```

## Debugging Transactions

### Inspect Schema

View defined attributes:

```sql
SELECT
  s.entid,
  s.ident,
  s.value_type,
  s.cardinality,
  s.unique_constraint,
  s.indexed,
  s.fulltext
FROM mentat.schema s
ORDER BY s.entid;
```

### Check Ident Mappings

```sql
SELECT * FROM mentat.idents ORDER BY entid;
```

### View Recent Transactions

```sql
SELECT
  t.tx,
  t.tx_instant
FROM mentat.transactions t
ORDER BY t.tx DESC
LIMIT 10;
```

### Inspect Datoms for Entity

```sql
-- Current datoms for entity 10001
SELECT
  d.e,
  s.ident AS attribute,
  d.v,
  d.value_type_tag,
  d.tx,
  d.added
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE d.e = 10001
  AND d.added = true
ORDER BY s.ident;
```

### View Full History for Entity

```sql
-- All assertions and retractions
SELECT
  d.e,
  s.ident AS attribute,
  d.v,
  d.tx,
  t.tx_instant,
  d.added
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
JOIN mentat.transactions t ON d.tx = t.tx
WHERE d.e = 10001
ORDER BY d.tx, s.ident;
```

### Decode Datom Values

Type tags determine how to interpret the `v` column (bytea):

| Tag | Type | Decode |
|-----|------|--------|
| 0 | ref | `decode(v, 'hex')::bigint` |
| 1 | boolean | `v[0] != 0` |
| 2 | long | `decode(v, 'hex')::bigint` |
| 3 | double | `decode(v, 'hex')::double precision` |
| 4 | instant | `decode(v, 'hex')::bigint` (microseconds since epoch) |
| 7 | string | `convert_from(v, 'UTF8')` |
| 8 | keyword | `convert_from(v, 'UTF8')` |
| 10 | uuid | `encode(v, 'hex')::uuid` |
| 11 | bytes | `v` (raw bytes) |

**Example**: Decode string values
```sql
SELECT
  d.e,
  s.ident,
  convert_from(d.v, 'UTF8') AS string_value
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.value_type = 'string'
  AND d.added = true
LIMIT 10;
```

**Example**: Decode long values
```sql
SELECT
  d.e,
  s.ident,
  ('x' || encode(d.v, 'hex'))::bit(64)::bigint AS long_value
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.value_type = 'long'
  AND d.added = true
LIMIT 10;
```

### Find Orphaned Datoms

Datoms referencing non-existent entities (data integrity check):

```sql
-- Find ref-type datoms pointing to entities with no attributes
SELECT
  d.e AS source_entity,
  s.ident AS ref_attribute,
  ('x' || encode(d.v, 'hex'))::bit(64)::bigint AS target_entity
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.value_type = 'ref'
  AND d.added = true
  AND NOT EXISTS (
    SELECT 1
    FROM mentat.datoms d2
    WHERE d2.e = ('x' || encode(d.v, 'hex'))::bit(64)::bigint
      AND d2.added = true
  );
```

## Debugging Time Travel

### Find Transaction by Time

```sql
SELECT mentat.mentat_query(
  '[:find ?tx .
    :where
    [?tx :db/txInstant ?time]
    [(> ?time #inst "2024-01-15T00:00:00Z")]
    [(< ?time #inst "2024-01-16T00:00:00Z")]]',
  '{}'::jsonb
);
```

### Compare Data Across Time

```sql
-- Current state
SELECT mentat.mentat_query(
  '[:find ?name :where [10001 :person/name ?name]]',
  '{}'::jsonb
);

-- Historical state (transaction 1000005)
SELECT mentat.mentat_query(
  '[:find ?name :where [10001 :person/name ?name]]',
  '{"asOf": 1000005}'::jsonb
);
```

### Find When Attribute Changed

```sql
SELECT mentat.mentat_query(
  '[:find ?tx ?time ?value ?added
    :where
    [10001 :person/name ?value ?tx ?added]
    [?tx :db/txInstant ?time]]',
  '{"history": true}'::jsonb
);
```

## Debugging Pull API

### Test Pull Patterns Incrementally

```sql
-- Basic: single attribute
SELECT mentat.mentat_pull('[:person/name]', 10001);

-- Wildcard: all attributes
SELECT mentat.mentat_pull('[*]', 10001);

-- Navigation: follow ref
SELECT mentat.mentat_pull(
  '[:person/name {:person/friend [:person/name]}]',
  10001
);
```

### Check Ref Attribute Values

```sql
-- Find what :person/friend points to
SELECT
  d.e AS person,
  ('x' || encode(d.v, 'hex'))::bit(64)::bigint AS friend_entity_id
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.ident = ':person/friend'
  AND d.added = true;
```

## Debugging mentatd

### Enable Debug Logging

```bash
# Set log level
RUST_LOG=debug ./target/release/mentatd

# Or specific modules
RUST_LOG=mentatd::server=debug,mentatd::protocol=trace ./target/release/mentatd
```

### Capture Request/Response

```bash
# Test with curl
curl -X POST http://localhost:8080/query \
  -H "Content-Type: application/edn" \
  -d '[:find ?e :where [?e :person/name]]' \
  -v  # Verbose output shows headers and response
```

### Check Connection Pool

```sql
-- Active connections
SELECT
  pid,
  usename,
  application_name,
  client_addr,
  state,
  query
FROM pg_stat_activity
WHERE application_name LIKE '%mentatd%';
```

### Test Serialization

```bash
# Test EDN parsing
echo '[:find ?e :where [?e :person/name]]' | curl -X POST http://localhost:8080/query \
  -H "Content-Type: application/edn" \
  --data-binary @-
```

## Performance Debugging

### Identify Slow Queries

```sql
-- Enable pg_stat_statements extension
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

-- View slow queries
SELECT
  query,
  calls,
  mean_exec_time,
  total_exec_time
FROM pg_stat_statements
WHERE query LIKE '%mentat%'
ORDER BY mean_exec_time DESC
LIMIT 10;
```

### Check Index Usage

```sql
-- Index scan stats
SELECT
  schemaname,
  tablename,
  indexname,
  idx_scan,
  idx_tup_read,
  idx_tup_fetch
FROM pg_stat_user_indexes
WHERE schemaname = 'mentat'
ORDER BY idx_scan;
```

### Monitor Table Size

```sql
SELECT
  schemaname,
  tablename,
  pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) AS total_size,
  pg_size_pretty(pg_relation_size(schemaname||'.'||tablename)) AS table_size,
  pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename) - pg_relation_size(schemaname||'.'||tablename)) AS indexes_size
FROM pg_tables
WHERE schemaname = 'mentat';
```

### Find Missing Indexes

```sql
-- Tables with high sequential scan counts (candidates for indexing)
SELECT
  schemaname,
  tablename,
  seq_scan,
  seq_tup_read,
  idx_scan,
  seq_tup_read / seq_scan AS avg_seq_tup
FROM pg_stat_user_tables
WHERE schemaname = 'mentat'
  AND seq_scan > 0
ORDER BY seq_tup_read DESC;
```

## Testing and Validation

### Validate Schema Consistency

```sql
-- Check for missing ident mappings
SELECT s.entid, s.ident
FROM mentat.schema s
WHERE NOT EXISTS (
  SELECT 1 FROM mentat.idents i
  WHERE i.ident = s.ident
);
```

### Check Referential Integrity

```sql
-- Verify all ref-type datoms point to existing entities
SELECT COUNT(*)
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.value_type = 'ref'
  AND d.added = true
  AND NOT EXISTS (
    SELECT 1
    FROM mentat.datoms d2
    WHERE d2.e = ('x' || encode(d.v, 'hex'))::bit(64)::bigint
      AND d2.added = true
  );
-- Should return 0
```

### Verify Unique Constraints

```sql
-- Find duplicate values for unique attributes
SELECT
  s.ident,
  d.v,
  COUNT(*) as entity_count
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.unique_constraint IS NOT NULL
  AND d.added = true
GROUP BY s.ident, d.v
HAVING COUNT(*) > 1;
-- Should return no rows
```

### Test Cardinality Constraints

```sql
-- Find multiple values for cardinality-one attributes
SELECT
  d.e,
  s.ident,
  COUNT(*) as value_count
FROM mentat.datoms d
JOIN mentat.schema s ON d.a = s.entid
WHERE s.cardinality = 'one'
  AND d.added = true
GROUP BY d.e, s.ident
HAVING COUNT(*) > 1;
-- Should return no rows
```

## Memory Debugging

### Check Memory Usage

```bash
# Server memory (Linux)
ps aux | grep mentatd

# PostgreSQL memory
SELECT
  pid,
  usename,
  application_name,
  pg_size_pretty(pg_stat_get_memory_context(pid)) AS memory
FROM pg_stat_activity
WHERE application_name LIKE '%mentatd%';
```

### Find Large Result Sets

```sql
-- Queries returning many rows
SELECT
  query,
  calls,
  rows,
  rows / calls AS avg_rows
FROM pg_stat_statements
WHERE query LIKE '%mentat%'
ORDER BY rows DESC
LIMIT 10;
```

## Debugging Tools

### psql Commands

```sql
-- List extensions
\dx

-- Describe mentat tables
\dt mentat.*

-- Describe mentat.datoms table
\d+ mentat.datoms

-- List functions
\df mentat.*

-- Show indexes
\di mentat.*

-- Connection info
\conninfo
```

### PostgreSQL Logs

**Location** (varies by OS):
- Linux: `/var/log/postgresql/postgresql-*-main.log`
- macOS (Homebrew): `/usr/local/var/log/postgres.log`
- Docker: `docker logs <container-id>`

**Enable detailed logging** in `postgresql.conf`:
```
log_statement = 'all'
log_duration = on
log_min_duration_statement = 0
```

### EDN Validation

Use Clojure REPL to validate EDN syntax:

```bash
clj
```

```clojure
; Test parsing
(read-string "[:find ?e :where [?e :person/name]]")

; Pretty print
(require '[clojure.pprint :as pprint])
(pprint/pprint (read-string "your-edn-here"))
```

### Regression Testing

Create test cases to prevent regressions:

```sql
-- Test basic query
DO $$
DECLARE
  result jsonb;
BEGIN
  result := mentat.mentat_query(
    '[:find ?e :where [?e :db/ident :db/ident]]',
    '{}'::jsonb
  );

  IF jsonb_array_length(result->'results') < 1 THEN
    RAISE EXCEPTION 'Basic query failed';
  END IF;

  RAISE NOTICE 'Test passed: basic query';
END $$;
```

## Common Debugging Workflows

### Workflow 1: Query Returns Wrong Results

1. **Simplify query** - Remove clauses until it works
2. **Test patterns individually** - Verify each pattern matches expected data
3. **Check data exists** - Query datoms table directly
4. **View query plan** - Use EXPLAIN ANALYZE
5. **Enable logging** - See generated SQL

### Workflow 2: Transaction Fails

1. **Check error message** - Read full error including hint
2. **Verify schema** - Ensure attributes defined
3. **Test EDN parsing** - Validate EDN syntax
4. **Check constraints** - Unique, cardinality, type
5. **Inspect database** - View current state before transaction

### Workflow 3: Performance Issue

1. **Identify slow query** - Use pg_stat_statements
2. **Analyze query plan** - EXPLAIN ANALYZE
3. **Check indexes** - Verify indexes exist and are used
4. **Add missing indexes** - Mark attributes with `:db/index true`
5. **Optimize query** - Be more specific, add predicates
6. **Monitor** - Verify improvement

### Workflow 4: Data Corruption

1. **Check referential integrity** - Run validation queries above
2. **View history** - Use history queries to see what happened
3. **Find problematic transaction** - Binary search through transactions
4. **Verify schema** - Check schema consistency
5. **Restore from backup** - If corruption is severe

## Advanced Debugging

### Core Dumps

If pg_mentat crashes PostgreSQL:

```bash
# Enable core dumps
ulimit -c unlimited

# After crash, analyze with gdb
gdb /usr/lib/postgresql/16/bin/postgres core
```

### Profiling

Profile SQL queries:

```sql
-- Enable auto_explain
CREATE EXTENSION IF NOT EXISTS auto_explain;
SET auto_explain.log_min_duration = 0;
SET auto_explain.log_analyze = true;
SET auto_explain.log_buffers = true;
```

### Debugging Extension Code

If building from source:

```bash
# Build with debug symbols
cargo pgrx install --debug

# Attach debugger
gdb -p $(pgrep postgres)
```

## Getting Expert Help

When filing a bug report, include:

1. **Environment**:
   ```sql
   SELECT version();  -- PostgreSQL version
   SELECT extversion FROM pg_extension WHERE extname = 'pg_mentat';
   ```

2. **Schema**:
   ```sql
   SELECT * FROM mentat.schema;
   ```

3. **Minimal reproduction**:
   - Schema definition
   - Transaction(s) to set up data
   - Query that fails
   - Expected vs actual results

4. **Logs**:
   - PostgreSQL logs (relevant excerpts)
   - mentatd logs (if applicable)
   - Error messages (full text)

5. **Query plan**:
   ```sql
   EXPLAIN ANALYZE <your-query>;
   ```

6. **System info**:
   - OS and version
   - PostgreSQL version
   - Rust version (if building from source)
   - Hardware (CPU, RAM, disk type)

## See Also

- [Common Issues](./COMMON_ISSUES.md) - Quick solutions to frequent problems
- [Performance Tuning](../operations/PERFORMANCE_TUNING.md) - Optimization guide
- [API Reference](../api/POSTGRESQL_FUNCTIONS.md) - Function documentation
- [Concepts](../getting_started/CONCEPTS.md) - Understanding pg_mentat internals
