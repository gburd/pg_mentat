# pg_mentat Common Issues and Solutions

Quick reference for troubleshooting common problems with pg_mentat.

## Installation Issues

### Error: "extension 'pg_mentat' does not exist"

**Cause**: Extension not installed or not in PostgreSQL's extension directory.

**Solution**:
```bash
# Install the extension
cd pg_mentat
cargo pgrx install --release

# Then in PostgreSQL:
CREATE EXTENSION pg_mentat;
```

**Verify installation**:
```bash
# Check extension files exist
ls $(pg_config --sharedir)/extension/pg_mentat*
```

### Error: "cargo-pgrx: command not found"

**Cause**: cargo-pgrx not installed.

**Solution**:
```bash
cargo install --locked cargo-pgrx --version 0.12.13
cargo pgrx init
```

### Error: "could not find pgrx"

**Cause**: Wrong pgrx version or dependency mismatch.

**Solution**:
```bash
# Clean and rebuild
cargo clean
cargo update
cargo pgrx install
```

## Schema Issues

### Error: "Attribute not found"

**Example error**:
```
ERROR: Attribute :person/name not found in schema
```

**Cause**: Attribute not defined in schema before use.

**Solution**:

1. **Check if attribute exists**:
```sql
SELECT mentat.mentat_query(
  '[:find ?ident
    :where [?e :db/ident ?ident]]',
  '{}'::jsonb
);
```

2. **Define the attribute**:
```sql
SELECT mentat.mentat_transact($$
[{:db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one}]
$$);
```

3. **Then use it**:
```sql
SELECT mentat.mentat_transact($$
[{:person/name "Alice"}]
$$);
```

**Best practice**: Always define schema before inserting data.

### Error: "Type mismatch for attribute"

**Example error**:
```
ERROR: Type mismatch for attribute 100: expected type string (tag 7), got tag 2
```

**Cause**: Value type doesn't match schema definition.

**Solution**:

1. **Check attribute definition**:
```sql
SELECT s.ident, s.value_type
FROM mentat.schema s
WHERE s.ident = ':person/age';
```

2. **Use correct type**:
```sql
-- WRONG: age defined as long, but given string
{:person/age "30"}

-- CORRECT: age is long (integer)
{:person/age 30}
```

**Type mappings**:
- `:db.type/string` → `"text"`
- `:db.type/long` → `42` (integer, no quotes)
- `:db.type/double` → `3.14` (float)
- `:db.type/boolean` → `true` / `false`
- `:db.type/keyword` → `:keyword/value`

### Error: "Unique constraint violation"

**Example error**:
```
ERROR: Unique constraint violation: attribute 105 has unique constraint 'identity'
but value already exists for entity 10001
```

**Cause**: Attempting to insert duplicate value for unique attribute.

**Solution**:

1. **Check existing values**:
```sql
SELECT mentat.mentat_query(
  '[:find ?e ?email
    :where [?e :person/email ?email]]',
  '{}'::jsonb
);
```

2. **Either**:
   - Use different value, OR
   - Use upsert (for `:db.unique/identity` attributes)

```sql
-- Upsert: updates existing entity if email exists
SELECT mentat.mentat_transact($$
[{:person/email "alice@example.com"
  :person/name "Alice Updated"}]
$$);
```

3. **Or retract old value first**:
```sql
SELECT mentat.mentat_transact($$
[[:db/retract 10001 :person/email "old@example.com"]
 {:db/id 10002
  :person/email "new@example.com"}]
$$);
```

### Error: "Cardinality violation"

**Example error**:
```
ERROR: Cardinality violation: attribute 103 has cardinality 'one'
but transaction contains 2 assertions for entity 10001
```

**Cause**: Multiple values for cardinality-one attribute in same transaction.

**Solution**:

1. **Check cardinality**:
```sql
SELECT s.ident, s.cardinality
FROM mentat.schema s
WHERE s.entid = 103;
```

2. **Fix transaction**:
```sql
-- WRONG: :person/name has cardinality one
[{:db/id "alice"
  :person/name "Alice"}
 [:db/add "alice" :person/name "Alice Johnson"]]

-- CORRECT: Only one value
[{:db/id "alice"
  :person/name "Alice Johnson"}]
```

3. **Or change to cardinality many**:
```sql
SELECT mentat.mentat_transact($$
[{:db/id 103
  :db/cardinality :db.cardinality/many}]
$$);
```

## Query Issues

### Error: "Query returned NULL"

**Cause**: Query has no results, and you're using scalar find spec (`. `)

**Example**:
```sql
-- Scalar find expects exactly one result
SELECT mentat.mentat_query(
  '[:find ?name .
    :where [?e :person/name ?name]
    [(= ?e 99999)]]',  -- Entity doesn't exist
  '{}'::jsonb
);
-- Returns: {"result": null}
```

**Solution**:

1. **Use collection find instead**:
```sql
[:find ?name
 :where [?e :person/name ?name]]
-- Returns: {"results": [...]}  -- Empty array if no results
```

2. **Or handle null in application**:
```python
result = execute_query(query)
if result["result"] is None:
    print("No results found")
```

### Error: "Variable ?x not bound"

**Example error**:
```
ERROR: Variable ?x used in predicate but not bound in any pattern
```

**Cause**: Variable used before it's defined in a pattern.

**Solution**:

```sql
-- WRONG: ?age used before it's bound
[:find ?name
 :where
 [(> ?age 25)]           -- ERROR: ?age not bound yet
 [?e :person/name ?name]
 [?e :person/age ?age]]

-- CORRECT: bind ?age first
[:find ?name
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(> ?age 25)]]          -- OK: ?age is bound
```

**Rule**: Variables must appear in a pattern before use in predicates.

### Query returns too many results

**Issue**: Query is too broad, returns all entities.

**Example**:
```sql
-- BAD: Matches ALL datoms (very slow, huge result)
[:find ?e ?a ?v
 :where [?e ?a ?v]]
```

**Solution**: Add more specific patterns

```sql
-- BETTER: Only person entities
[:find ?e ?name
 :where
 [?e :person/name ?name]]

-- EVEN BETTER: Filtered results
[:find ?e ?name
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(> ?age 25)]]
```

### Query timeout / too slow

**Issue**: Query takes too long to execute.

**Diagnosis**:
```sql
-- View query plan
EXPLAIN ANALYZE
SELECT mentat.mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);
```

**Solutions**:

1. **Add index**:
```sql
SELECT mentat.mentat_transact($$
[{:db/id 100  -- Attribute entity ID
  :db/index true}]
$$);
```

2. **Be more specific** - Use constants where possible:
```sql
-- SLOW: broad query
[:find ?v
 :where [?e ?a ?v]]

-- FAST: specific attribute
[:find ?v
 :where [?e :person/name ?v]]
```

3. **Limit results**:
```sql
-- In application, limit result processing
SELECT mentat.mentat_query(
  '[:find ?e :where [?e :person/name]]',
  '{}'::jsonb
) LIMIT 100;
```

4. **Use aggregates instead of fetching all**:
```sql
-- Instead of fetching all and counting in app:
[:find (count ?e)
 :where [?e :person/name]]
```

## Transaction Issues

### Error: "Failed to allocate entity ID"

**Cause**: Database connection issue or partition problem.

**Solution**:

1. **Check database connection**:
```sql
SELECT 1;  -- Should return 1
```

2. **Verify partition exists**:
```sql
SELECT * FROM mentat.partitions;
```

3. **Re-bootstrap if needed**:
```sql
-- Only if database is empty/corrupted
SELECT mentat.bootstrap_schema();
```

### Error: "Tempid not found in transaction"

**Example**:
```sql
SELECT mentat.mentat_transact($$
[{:db/id "alice"
  :person/name "Alice"}
 {:db/id "bob"
  :person/friend "alicce"}]  -- Typo: "alicce" not "alice"
$$);
```

**Cause**: Typo in tempid reference.

**Solution**: Verify tempid spelling matches exactly.

### Error: "Transaction failed: <constraint violation>"

**Cause**: PostgreSQL constraint violation (unique, check, foreign key).

**Solution**:

1. **Read the full error message** - it contains the constraint name

2. **Check constraints**:
```sql
SELECT * FROM mentat.schema WHERE ident = ':person/email';
```

3. **Fix data or constraint** based on intended behavior

## Pull API Issues

### Pull returns empty object

**Example**:
```sql
SELECT mentat.mentat_pull('[*]', 10001);
-- Returns: {:db/id 10001}  -- No other attributes
```

**Cause**: Entity has no datoms, or datoms are retracted.

**Solution**:

1. **Check entity exists**:
```sql
SELECT mentat.mentat_query(
  '[:find ?a ?v
    :where
    [?e ?a ?v]
    [(= ?e 10001)]]',
  '{}'::jsonb
);
```

2. **Check history** (maybe entity was retracted):
```sql
SELECT mentat.mentat_query(
  '[:find ?a ?v ?tx ?added
    :where
    [?e ?a ?v ?tx ?added]
    [(= ?e 10001)]]',
  '{"history": true}'::jsonb
);
```

3. **Transact data if missing**:
```sql
SELECT mentat.mentat_transact($$
[{:db/id 10001
  :person/name "Alice"}]
$$);
```

### Pull doesn't follow ref attributes

**Example**:
```sql
SELECT mentat.mentat_pull(
  '[:person/name :person/friend]',
  10002
);
-- Returns: {:person/friend 10001}  -- Just the ID, not friend's data
```

**Cause**: Not using navigation pattern.

**Solution**: Use map pattern for navigation:
```sql
SELECT mentat.mentat_pull(
  '[:person/name {:person/friend [:person/name :person/email]}]',
  10002
);
-- Returns: {:person/friend {:person/name "Alice", ...}}
```

## mentatd Daemon Issues

### Error: "Connection refused" (port 8080)

**Cause**: mentatd daemon not running.

**Solution**:

1. **Start mentatd**:
```bash
cd mentatd
cargo build --release
./target/release/mentatd
```

2. **Check it's listening**:
```bash
lsof -i :8080
# or
netstat -an | grep 8080
```

3. **Check logs** for errors:
```bash
./target/release/mentatd --log-level debug
```

### Error: "Database connection failed"

**Cause**: mentatd can't connect to PostgreSQL.

**Solution**:

1. **Check PostgreSQL is running**:
```bash
pg_isready
```

2. **Check connection string** in mentatd config:
```bash
# Default: postgresql://localhost:5432/postgres
# Set via environment:
export DATABASE_URL="postgresql://user:pass@host:port/dbname"
./target/release/mentatd
```

3. **Test connection manually**:
```bash
psql $DATABASE_URL
```

### mentatd returns malformed response

**Issue**: Response is not valid EDN/JSON.

**Diagnosis**:
```bash
# Test with curl
curl -X POST http://localhost:8080/query \
  -H "Content-Type: application/edn" \
  -d '[:find ?e :where [?e :person/name]]'
```

**Solution**: Check mentatd logs for errors:
```bash
./target/release/mentatd 2>&1 | tee mentatd.log
```

## Time Travel Issues

### As-of query returns unexpected results

**Issue**: Results don't match expected historical data.

**Diagnosis**:

1. **Check transaction IDs**:
```sql
SELECT mentat.mentat_query(
  '[:find ?tx ?time
    :where
    [_ _ _ ?tx]
    [?tx :db/txInstant ?time]]',
  '{}'::jsonb
);
```

2. **Check what datoms existed at that time**:
```sql
SELECT mentat.mentat_query(
  '[:find ?e ?a ?v ?tx
    :where [?e ?a ?v ?tx]]',
  '{"asOf": 1000005}'::jsonb
);
```

**Common issue**: Using wrong transaction ID (too old or too new).

### History query is very slow

**Issue**: History queries scan all datoms including retractions.

**Solution**:

1. **Be specific** - limit to specific entity or attribute:
```sql
-- SLOW: all history
[:find ?e ?a ?v ?tx ?added
 :where [?e ?a ?v ?tx ?added]]

-- FAST: history of one entity
[:find ?a ?v ?tx ?added
 :where
 [?e ?a ?v ?tx ?added]
 [(= ?e 10001)]]
```

2. **Use time range**:
```sql
-- History between two transactions
SELECT mentat.mentat_query(
  '[:find ?e ?a ?v ?tx ?added
    :where
    [?e ?a ?v ?tx ?added]
    [(>= ?tx 1000010)]
    [(<= ?tx 1000020)]]',
  '{"history": true}'::jsonb
);
```

## Performance Issues

### Slow query execution

**See "Query timeout / too slow" above for diagnosis and solutions.**

### High memory usage

**Cause**: Large result sets loaded into memory.

**Solution**:

1. **Limit results**:
```sql
-- Use LIMIT in SQL
SELECT mentat.mentat_query(...) LIMIT 1000;
```

2. **Use aggregates instead of fetching all**:
```sql
-- Instead of counting in application:
[:find (count ?e) . :where [?e :person/name]]
```

3. **Stream results** (when implemented):
```bash
# Future: streaming API for large results
curl -X POST http://localhost:8080/stream/query ...
```

### Disk space growing rapidly

**Cause**: EAVT model stores full history, disk usage grows over time.

**Solution**:

1. **Normal growth**: This is expected behavior (immutable history).

2. **VACUUM periodically**:
```sql
VACUUM ANALYZE mentat.datoms;
```

3. **Compress old data** (PostgreSQL feature):
```sql
ALTER TABLE mentat.datoms SET (toast_compression = lz4);
```

4. **Archive old transactions** (custom solution needed):
   - Export old transactions to archive storage
   - Keep recent transactions in main database

**Note**: pg_mentat doesn't support excision (deleting history) like Datomic.

## EDN Parsing Issues

### Error: "Failed to parse EDN"

**Cause**: Invalid EDN syntax.

**Common mistakes**:

1. **Missing colons on keywords**:
```clojure
// WRONG
{db/ident person/name}

// CORRECT
{:db/ident :person/name}
```

2. **Wrong quote types**:
```clojure
// WRONG: single quotes
'person/name'

// CORRECT: double quotes for strings
"person/name"
```

3. **Missing brackets**:
```clojure
// WRONG
{:person/name "Alice"}

// CORRECT (for transaction)
[{:person/name "Alice"}]
```

4. **Unbalanced brackets**:
```clojure
// WRONG
[:find ?e :where [?e :person/name]

// CORRECT
[:find ?e :where [?e :person/name]]
```

**Solution**: Use EDN validator to check syntax:
```bash
# Install clojure CLI tools
clj -M -e '(read-string "your-edn-here")'
```

## Getting Help

If you're still stuck after trying these solutions:

1. **Check logs**:
   - PostgreSQL logs: `/var/log/postgresql/...`
   - mentatd logs: `./target/release/mentatd 2>&1 | tee mentatd.log`

2. **Enable debug logging**:
```sql
SET log_statement = 'all';
SET client_min_messages = 'DEBUG1';
```

3. **Search issues**:
   - GitHub: https://github.com/your-org/pg_mentat/issues

4. **File a bug report** with:
   - PostgreSQL version: `SELECT version();`
   - pg_mentat version: `SELECT extversion FROM pg_extension WHERE extname = 'pg_mentat';`
   - Full error message
   - Minimal reproduction case
   - Schema definition
   - Query/transaction that failed

5. **Check documentation**:
   - [Quickstart Guide](../getting_started/QUICKSTART.md)
   - [Concepts](../getting_started/CONCEPTS.md)
   - [API Reference](../api/POSTGRESQL_FUNCTIONS.md)
   - [Debugging Guide](./DEBUGGING.md)

## Pr preventing Future Issues

### Best Practices

1. **Always define schema first** before inserting data
2. **Use unique constraints** on attributes that should be unique
3. **Validate EDN syntax** before executing (use Clojure REPL or tools)
4. **Test queries with EXPLAIN** before running on large datasets
5. **Monitor disk space** - history grows over time
6. **Backup regularly** - standard PostgreSQL backup procedures apply
7. **Use transactions** - group related operations
8. **Handle errors gracefully** in application code
9. **Limit result sizes** to avoid OOM
10. **Index frequently-queried attributes** (`:db/index true`)

### Testing Strategies

1. **Test schema changes** on small datasets first
2. **Use transactions** to test operations (can rollback if needed)
3. **Query history** to verify data changes are correct
4. **Test time-travel queries** to ensure historical accuracy
5. **Load test** mentatd before production deployment
6. **Monitor query performance** over time as data grows

### Monitoring

1. **PostgreSQL stats**:
```sql
SELECT * FROM pg_stat_user_tables WHERE schemaname = 'mentat';
SELECT * FROM pg_stat_user_indexes WHERE schemaname = 'mentat';
```

2. **Query performance**:
```sql
-- Planned: mentat.query_stats() function
SELECT * FROM mentat.query_stats();
SELECT * FROM mentat.slow_queries(1000);  -- Queries > 1000ms
```

3. **Disk usage**:
```sql
SELECT pg_size_pretty(pg_total_relation_size('mentat.datoms'));
```

See [Performance Tuning Guide](../operations/PERFORMANCE_TUNING.md) for more monitoring strategies.
