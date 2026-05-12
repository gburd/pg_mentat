# SQL Function Reference

All pg_mentat functions live in the `mentat` schema. After `CREATE EXTENSION pg_mentat`, they are accessible as `mentat.function_name()`.

## Function Naming Convention

pg_mentat provides two sets of function names:

**Convenience aliases** (recommended for everyday use in the default `mentat` schema):

```sql
SELECT mentat.t('[{:person/name "Alice"}]');
SELECT mentat.q('[:find ?e :where [?e :person/name "Alice"]]');
SELECT mentat.pull('[*]', 10001);
```

**Full-name functions** use a `mentat_` prefix. These read naturally when installed into a custom schema:

```sql
-- If you install into a custom schema:
CREATE EXTENSION pg_mentat SCHEMA myapp;
SELECT myapp.mentat_transact('[{:person/name "Alice"}]');
SELECT myapp.mentat_query('[:find ?e :where [?e :person/name "Alice"]]');
SELECT myapp.mentat_pull('[*]', 10001);
```

The full-name functions exist because pgrx derives the SQL function name from the Rust function name. Since any schema can host the extension, the `mentat_` prefix ensures the function names read sensibly regardless of the schema choice. The convenience aliases eliminate redundancy for the common default case.

### Quick Reference

| Convenience alias | Full function | Description |
|-------------------|--------------|-------------|
| `mentat.t(edn)` | `mentat_transact(edn)` | Transact EDN data |
| `mentat.q(query, inputs)` | `mentat_query(query, inputs)` | Run a Datalog query |
| `mentat.pull(pattern, eid)` | `mentat_pull(pattern, eid)` | Pull entity attributes |
| `mentat.pull_many(pattern, eids)` | `mentat_pull_many(pattern, eids)` | Pull multiple entities |
| `mentat.entity(eid)` | `mentat_entity(eid)` | All attributes as JSON |
| `mentat.schema()` | `mentat_schema()` | Current schema |
| `mentat.explain(query)` | `mentat_explain(query)` | Show generated SQL |
| `mentat.stats()` | `mentat_query_stats()` | Execution statistics |
| `mentat.storage()` | `mentat_storage_stats()` | Storage statistics |
| `mentat.cache_stats()` | `mentat_stmt_cache_stats()` | Statement cache info |
| `mentat.cache_clear()` | `mentat_stmt_cache_clear()` | Clear statement cache |

---

## Transaction Functions

### `mentat.t(edn)` / `mentat_transact(edn)`

Execute a transaction. Returns a JSON transaction report with `tx_id`, `tx_instant`, and `tempids`.

```sql
SELECT mentat.t('[
  {:db/id "tempid-1"
   :person/name "Alice"
   :person/age 30}
]');
```

The `t` alias transacts against the default store. Use the full function with a store argument for named stores:

```sql
SELECT mentat.mentat_transact_store('analytics', '[{:event/type "click"}]');
```

### `mentat_with(edn)` — Speculative Transaction

Execute a transaction without persisting it. Returns the same report format, but writes nothing. Useful for validation or "what-if" analysis.

```sql
SELECT mentat.mentat_with('[
  {:person/name "Test" :person/age 99}
]');
```

---

## Query Functions

### `mentat.q(query, inputs)` / `mentat_query(query, inputs)`

Execute a Datalog query. Returns JSONB with `columns` and `results`.

```sql
-- Simple query
SELECT mentat.q('
  [:find ?name ?age
   :where [?e :person/name ?name]
          [?e :person/age ?age]
          [(> ?age 21)]]
');

-- With input bindings (positional, matching :in clause order)
SELECT mentat.q('
  [:find ?name
   :in $ ?min-age
   :where [?e :person/name ?name]
          [?e :person/age ?age]
          [(>= ?age ?min-age)]]
', '[25]');
```

The `inputs` parameter is a JSON value:
- Simple array for positional bindings: `'[25]'`
- Empty for no inputs: `'{}'` or `'[]'`

### `mentat.explain(query)` / `mentat_explain(query)`

Show the generated SQL and PostgreSQL's EXPLAIN output without executing the query.

```sql
SELECT mentat.explain('[:find ?name :where [?e :person/name ?name]]');
```

### `mentat_query_sql(query)` — Generated SQL

Return only the generated SQL string (no execution, no EXPLAIN).

```sql
SELECT mentat.mentat_query_sql('[:find ?name :where [?e :person/name ?name]]');
```

### `mentat_query_view(name, query)` — Create SQL VIEW from Datalog

Create a PostgreSQL VIEW backed by a Datalog query:

```sql
SELECT mentat.mentat_query_view('people_over_30', '
  [:find ?name ?age ?email
   :where [?e :person/name ?name]
          [?e :person/age ?age]
          [?e :person/email ?email]
          [(> ?age 30)]]
');

-- Now use it like any SQL view
SELECT * FROM mentat.people_over_30 WHERE name LIKE 'A%';
```

---

## Pull Functions

### `mentat.pull(pattern, eid)` / `mentat_pull(pattern, eid)`

Pull attributes for a single entity. Returns a nested JSON document.

```sql
-- Pull everything
SELECT mentat.pull('[*]', 10001);

-- Pull specific attributes with nested refs
SELECT mentat.pull('[
  :person/name
  :person/age
  {:person/friends [:person/name :person/age]}
]', 10001);

-- Reverse lookup: who has this entity as a friend?
SELECT mentat.pull('[:person/name :person/_friends]', 10001);

-- With modifiers
SELECT mentat.pull('[
  :person/name
  {(:person/friends :limit 5 :as :top-friends) [:person/name]}
  {(:person/_friends :as :admirers) [:person/name]}
]', 10001);
```

### `mentat.pull_many(pattern, eids)` / `mentat_pull_many(pattern, eids)`

Pull the same pattern for multiple entities. Returns a JSON array.

```sql
SELECT mentat.pull_many('[:person/name :person/age]', ARRAY[10001, 10002, 10003]);
```

### `mentat.entity(eid)` / `mentat_entity(eid)`

Return all current attributes for an entity as a flat JSON map (equivalent to `pull('[*]', eid)`).

```sql
SELECT mentat.entity(10001);
```

---

## Schema Functions

### `mentat.schema()` / `mentat_schema()`

Return the full schema as JSON, keyed by attribute ident.

```sql
SELECT mentat.schema();
```

---

## Store Management

### `mentat.create_store(name, description)`

Create a new isolated store with its own schema, tables, and indexes.

```sql
SELECT mentat.create_store('analytics', 'Event tracking store');
```

### `mentat.drop_store(name)`

Drop a store and all its data (irreversible).

### `mentat.list_stores()`

List all stores with metadata.

### `mentat.rename_store(old_name, new_name)`

Rename an existing store.

---

## Time Travel Functions

### `mentat.log(store, from_tx, to_tx)`

Return the transaction log for a range of transactions.

```sql
SELECT mentat.log('default', 1000001, 1000010);
```

### `mentat.diff(store, from_tx, to_tx)`

Compute the diff between two points in time — what was added and retracted.

```sql
SELECT mentat.diff('default', 1000003, 1000007);
```

### Time-travel via query parameters

Pass `as_of_tx` or `since_tx` to query functions:

```sql
-- Query the database as of transaction 1000005
SELECT mentat.q('
  [:find ?name :where [?e :person/name ?name]]
', '[]', 1000005, NULL);
```

---

## Excision Functions

### `mentat_excise(store, entity_id, attribute)`

Permanently remove datoms from the database, including all history. This is the only operation that truly deletes data (GDPR compliance).

```sql
-- Remove all data for an entity
SELECT mentat.mentat_excise('default', 10042, NULL);

-- Remove only a specific attribute
SELECT mentat.mentat_excise('default', 10042, ':person/email');
```

---

## Subscription Functions

### `mentat.subscribe(store, name, query)`

Subscribe to changes matching a Datalog query pattern. Uses PostgreSQL `LISTEN`/`NOTIFY`.

```sql
SELECT mentat.subscribe('default', 'new_people',
  '[:find ?e :where [?e :person/name]]');

-- In another session:
LISTEN mentat_subscription_new_people;
```

### `mentat.unsubscribe(store, name)`

Remove a subscription.

---

## Materialized View Functions

### `mentat.materialize(store, name, query)`

Create a materialized view from a Datalog query for faster repeated access.

```sql
SELECT mentat.materialize('default', 'active_users',
  '[:find ?e ?name :where [?e :person/name ?name] [?e :person/active true]]');
```

### `mentat.refresh(store, name)`

Refresh a materialized view with current data.

---

## Statistics & Monitoring

### `mentat.stats()` / `mentat_query_stats()`

Query execution statistics: call counts, timing, cache hit rates.

### `mentat.storage()` / `mentat_storage_stats()`

Storage statistics: row counts, table sizes, index sizes.

### `mentat.cache_stats()` / `mentat_stmt_cache_stats()`

Prepared statement cache statistics.

### `mentat.cache_clear()` / `mentat_stmt_cache_clear()`

Clear the statement cache.

### `mentat_health_check()`

Extension health check (returns JSON with status, version, store count).

### `mentat_slow_queries(threshold_ms)`

Return recently logged slow queries exceeding the given threshold.

---

## EDN Helper Functions

These operate on EDN-formatted text values and are installed in the `public` schema for convenience.

| Function | Description |
|----------|-------------|
| `edn_get(edn, key)` | Extract a value from an EDN map |
| `edn_nth(edn, index)` | Extract Nth element from an EDN vector |
| `edn_count(edn)` | Count elements in an EDN collection |
| `edn_keys(edn)` | Keys of an EDN map as a vector |
| `edn_values(edn)` | Values of an EDN map as a vector |
| `edn_contains(edn, key)` | Check if a map contains a key |
| `edn_type(edn)` | Type of an EDN value |
| `edn_pretty(edn, width)` | Pretty-print EDN with indentation |

---

## Bootstrap Functions

### `mentat.bootstrap_schema()`

Re-run the bootstrap schema installation. Called automatically during `CREATE EXTENSION` but can be invoked to repair a corrupted schema.
