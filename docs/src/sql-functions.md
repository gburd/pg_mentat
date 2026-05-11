# SQL Function Reference

All pg_mentat functions live in the `mentat` schema. After `CREATE EXTENSION pg_mentat`, they are accessible as `mentat.function_name()` or (if the schema is in your `search_path`) as `function_name()`.

Most functions come in two forms: a default-store variant and a store-aware variant that takes a store name as the first argument.

## Transaction Functions

### `mentat_transact(edn_tx TEXT) -> TEXT`

Execute a transaction against the default store. Returns a JSON transaction report.

```sql
SELECT mentat_transact('[
  {:db/id "tempid-1"
   :person/name "Alice"
   :person/age 30}
]');
```

**Returns:** JSON with `tx_id`, `tx_instant`, `tempids` (mapping tempid strings to allocated entity IDs).

### `mentat.t(store_name TEXT, edn_tx TEXT) -> TEXT`

Execute a transaction against a named store.

```sql
SELECT mentat.t('mystore', '[{:db/id "t1" :item/name "Widget"}]');
```

### `mentat_with(edn_tx TEXT) -> TEXT`

Execute a speculative (dry-run) transaction against the default store. The transaction is computed but not persisted. Useful for validation or "what-if" analysis.

```sql
SELECT mentat_with('[
  {:db/id "t1" :person/name "Test"}
]');
```

**Returns:** Same format as `mentat_transact`, but no data is written.

---

## Query Functions

### `mentat_query(query TEXT, inputs JSONB) -> JSONB`

Execute a Datalog query against the default store.

```sql
SELECT mentat_query(
  '[:find ?name ?age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    [(> ?age 21)]]',
  '{}'
);
```

**Parameters:**
- `query` -- EDN Datalog query string
- `inputs` -- JSON object with optional keys:
  - `"inputs"` -- array of input binding values (positional, matching `:in` clause order)
  - `"as_of"` -- transaction ID for point-in-time query
  - `"since"` -- transaction ID for "changes since" query
  - `"limit"` -- maximum result rows

**Returns:** JSONB with `columns` (array of variable names) and `results` (array of result tuples).

### `mentat.q(store_name TEXT, query TEXT, inputs JSONB) -> JSONB`

Execute a query against a named store.

```sql
SELECT mentat.q('analytics', '[:find ?e :where [?e :event/type "click"]]', '{}');
```

### `mentat.mentat_q_full(store_name TEXT, query TEXT, inputs JSONB, limit INT) -> JSONB`

Query with explicit limit parameter (overrides any limit in inputs JSON).

### `mentat.mentat_q_default(query TEXT, inputs JSONB, limit INT) -> JSONB`

Default-store query with explicit limit.

### `mentat_explain(query TEXT, inputs JSONB) -> JSONB`

Show the query execution plan without running the query. Returns the generated SQL and PostgreSQL's EXPLAIN output.

```sql
SELECT mentat_explain(
  '[:find ?name :where [?e :person/name ?name]]',
  '{}'
);
```

### `mentat.mentat_explain_store(store_name TEXT, query TEXT, inputs JSONB) -> JSONB`

Explain a query against a named store.

### `mentat_query_sql(query TEXT, inputs JSONB) -> TEXT`

Return the generated SQL without executing it. Useful for debugging or integration with external tools.

```sql
SELECT mentat_query_sql(
  '[:find ?name :where [?e :person/name ?name]]',
  '{}'
);
```

### `mentat.mentat_query_sql_store(store_name TEXT, query TEXT, inputs JSONB) -> TEXT`

Return generated SQL for a named store.

### `mentat_query_view(query TEXT, inputs JSONB, view_name TEXT) -> TEXT`

Create a PostgreSQL VIEW from a Datalog query. The view can then be used in regular SQL.

```sql
SELECT mentat_query_view(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}',
  'people_view'
);

-- Now usable as a regular view
SELECT * FROM mentat.people_view WHERE name = 'Alice';
```

### `mentat.mentat_query_view_store(store_name TEXT, query TEXT, inputs JSONB, view_name TEXT) -> TEXT`

Create a view for a named store.

---

## Statement Cache Functions

### `mentat_stmt_cache_stats() -> JSONB`

Return statistics about the prepared statement cache (size, capacity, hit counts).

### `mentat_stmt_cache_clear() -> TEXT`

Clear the prepared statement cache. Call after schema changes or when debugging query plan issues.

---

## Pull Functions

### `mentat_pull(pattern TEXT, entity_id BIGINT) -> JSONB`

Pull attributes for a single entity from the default store.

```sql
-- Pull all attributes
SELECT mentat_pull('[*]', 10001);

-- Pull specific attributes with nested refs
SELECT mentat_pull(
  '[:person/name :person/age {:person/friends [:person/name]}]',
  10001
);

-- Reverse lookup
SELECT mentat_pull('[:person/name :person/_friends]', 10001);
```

### `mentat.pull(store_name TEXT, pattern TEXT, entity_id BIGINT) -> JSONB`

Pull from a named store.

### `mentat_pull_many(pattern TEXT, entity_ids BIGINT[]) -> JSONB`

Pull the same pattern for multiple entities. Returns an array of entity maps.

```sql
SELECT mentat_pull_many('[:person/name :person/age]', ARRAY[10001, 10002, 10003]);
```

### `mentat.pull_many(store_name TEXT, pattern TEXT, entity_ids BIGINT[]) -> JSONB`

Pull many from a named store.

### `mentat_entity(entity_id BIGINT) -> JSONB`

Return all attributes for an entity as a flat JSON map (equivalent to `mentat_pull('[*]', id)`).

```sql
SELECT mentat_entity(10001);
```

### `mentat.entity(store_name TEXT, entity_id BIGINT) -> JSONB`

Entity lookup for a named store.

---

## Schema Functions

### `mentat_schema() -> JSONB`

Return the full schema for the default store as JSON.

```sql
SELECT mentat_schema();
```

**Returns:** JSON object keyed by attribute ident, with value type, cardinality, uniqueness, and other properties.

### `mentat.schema(store_name TEXT) -> JSONB`

Return schema for a named store.

---

## Store Management Functions

### `mentat.create_store(store_name TEXT, description TEXT DEFAULT NULL) -> TEXT`

Create a new isolated store. This creates a new PostgreSQL schema with all required tables and indexes.

```sql
SELECT mentat.create_store('analytics', 'Event tracking store');
```

### `mentat.drop_store(store_name TEXT) -> TEXT`

Drop a store and all its data. This is irreversible.

```sql
SELECT mentat.drop_store('analytics');
```

### `mentat.list_stores() -> JSONB`

List all stores with their metadata.

```sql
SELECT mentat.list_stores();
```

### `mentat.rename_store(old_name TEXT, new_name TEXT) -> TEXT`

Rename an existing store.

---

## Time Travel Functions

### `mentat_as_of(store TEXT, tx_id BIGINT, query TEXT, inputs JSONB) -> JSONB`

Query the database as it existed at a specific transaction. Only datoms asserted at or before `tx_id` are visible.

This is also accessible via the `"as_of"` key in the inputs JSON of `mentat_query`.

### `mentat_since(store TEXT, tx_id BIGINT, query TEXT, inputs JSONB) -> JSONB`

Query only datoms asserted after `tx_id`.

### `mentat_history(store TEXT, entity_id BIGINT, attribute TEXT) -> JSONB`

Return the complete history of an attribute for an entity, including retractions.

```sql
SELECT mentat_history('default', 10001, ':person/name');
```

**Returns:** Array of `{value, tx, added}` objects showing each assertion and retraction.

### `mentat_tx_range(store TEXT, start_tx BIGINT, end_tx BIGINT) -> JSONB`

Return all datoms asserted or retracted in a range of transactions.

### `mentat.log(store_name TEXT, start_tx BIGINT, end_tx BIGINT) -> JSONB`

Return the transaction log for a range. Each entry includes the transaction ID, timestamp, and all datoms in that transaction.

### `mentat.diff(store_name TEXT, from_tx BIGINT, to_tx BIGINT) -> JSONB`

Compute the diff between two points in time. Shows what was added and retracted.

---

## Excision Functions

### `mentat_excise(store TEXT DEFAULT 'default', entity_ids BIGINT[]) -> JSONB`

Permanently remove entities from the database, including all history. This is the only operation that truly deletes data. Requires `allow_excision = true` on the entity's partition.

```sql
-- Enable excision on the user partition
UPDATE mentat.partitions SET allow_excision = true WHERE name = 'db.part/user';

-- Excise entities
SELECT mentat_excise('default', ARRAY[10001, 10002]);
```

**Returns:** JSON summary with count of deleted datoms, affected transactions, and any dangling reference warnings.

---

## Subscription Functions

### `mentat.subscribe(store_name TEXT, subscription_name TEXT, query TEXT) -> TEXT`

Subscribe to changes matching a Datalog query. Uses PostgreSQL LISTEN/NOTIFY to push notifications when matching datoms are transacted.

```sql
SELECT mentat.subscribe('default', 'new_people',
  '[:find ?e :where [?e :person/name]]');

-- In another session:
LISTEN mentat_subscription_new_people;
```

### `mentat.unsubscribe(store_name TEXT, subscription_name TEXT) -> TEXT`

Remove a subscription.

### `mentat.list_subscriptions(store_name TEXT DEFAULT NULL) -> JSONB`

List active subscriptions, optionally filtered by store.

---

## Materialized View Functions

### `mentat.materialize(store_name TEXT, view_name TEXT, query TEXT) -> TEXT`

Create a materialized view from a Datalog query for faster repeated access.

```sql
SELECT mentat.materialize('default', 'active_users',
  '[:find ?e ?name :where [?e :person/name ?name] [?e :person/active true]]');
```

### `mentat.refresh(store_name TEXT, view_name TEXT) -> TEXT`

Refresh a materialized view with current data.

### `mentat.drop_matview(store_name TEXT, view_name TEXT) -> TEXT`

Drop a materialized view.

### `mentat.list_matviews(store_name TEXT DEFAULT NULL) -> JSONB`

List materialized views.

---

## Recursive Query Functions

### `mentat.recursive(store_name TEXT, view_name TEXT, ...) -> TEXT`

Create a recursive view for graph traversal queries.

### `mentat.drop_recursive(store_name TEXT, view_name TEXT) -> TEXT`

Drop a recursive view.

### `mentat.list_recursive(store_name TEXT) -> JSONB`

List recursive views for a store.

---

## Virtual Table Functions

### `mentat.create_virtual_tables(store_name TEXT) -> TEXT`

Create PostgreSQL views that expose datoms in a relational-friendly format for SQL integration.

---

## Statistics Functions

### `mentat_query_stats() -> JSONB`

Return query execution statistics: call counts, timing percentiles, and cache hit rates for all mentat functions.

### `mentat_slow_queries(threshold_ms FLOAT DEFAULT 100.0) -> JSONB`

Return recently logged slow queries exceeding the given threshold.

### `mentat_storage_stats() -> JSONB`

Return storage statistics: row counts, table sizes, and index sizes for all mentat tables.

---

## EDN Helper Functions

These functions operate on EDN-formatted text values.

### `edn_get(edn TEXT, key TEXT) -> TEXT`

Extract a value from an EDN map by key.

### `edn_nth(edn TEXT, index INT) -> TEXT`

Extract the Nth element from an EDN vector.

### `edn_count(edn TEXT) -> INT`

Count elements in an EDN collection.

### `edn_keys(edn TEXT) -> TEXT`

Return the keys of an EDN map as an EDN vector.

### `edn_values(edn TEXT) -> TEXT`

Return the values of an EDN map as an EDN vector.

### `edn_contains(edn TEXT, key TEXT) -> BOOLEAN`

Check if an EDN map contains a key.

### `edn_type(edn TEXT) -> TEXT`

Return the type of an EDN value (map, vector, list, keyword, string, long, double, boolean, nil).

---

## Bootstrap Functions

### `mentat.bootstrap_schema() -> VOID`

Re-run the bootstrap schema installation. Called automatically during `CREATE EXTENSION` but can be invoked manually to repair a corrupted schema.
