# Time Travel

pg_mentat maintains a complete history of every assertion and retraction. Unlike conventional databases that overwrite data in place, pg_mentat is accumulate-only: retracting a fact records the retraction as a new event rather than deleting the original assertion.

This immutable history enables temporal queries -- looking at the database as it existed at any past transaction.

## Concepts

### Transaction IDs

Every transaction is assigned a monotonically increasing 64-bit integer (`tx`). Transaction IDs serve as both:
- A unique identifier for the transaction
- A logical timestamp (total ordering of all changes)

### Basis-t

The "basis-t" is the most recent transaction ID in the database. It represents the current state.

### Datom Lifecycle

A datom `[entity attribute value tx added]` has five components:
- `added = true` means the fact was asserted at transaction `tx`
- `added = false` means the fact was retracted at transaction `tx`

The "current" value of an attribute is determined by finding the most recent assertion that has not been subsequently retracted.

## As-Of Queries

Query the database as it appeared at a specific transaction. Only datoms with `tx <= target` are visible, and retracted datoms are excluded.

### Via Query Input

```sql
SELECT mentat_query(
  '[:find ?name ?age
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]]',
  '{"as_of": 1050}'
);
```

### Via Dedicated Function

```sql
SELECT mentat_as_of('default', 1050,
  '[:find ?name :where [?e :person/name ?name]]',
  '{}');
```

### Use Cases

- **Audit** -- "What did the data look like when we made that decision?"
- **Debugging** -- "When did this value change?"
- **Reproducibility** -- "Re-run analysis against the same data snapshot"
- **Undo exploration** -- "What was the state before this transaction?"

## Since Queries

Query only datoms asserted after a specific transaction. This shows what has changed.

```sql
SELECT mentat_query(
  '[:find ?e ?name
    :where
    [?e :person/name ?name]]',
  '{"since": 1050}'
);
```

### Via Dedicated Function

```sql
SELECT mentat_since('default', 1050,
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}');
```

### Use Cases

- **Change detection** -- "What entities were modified since my last sync?"
- **Incremental processing** -- "Process only new data since last batch"
- **Event sourcing** -- "What happened since transaction T?"

## History Queries

View the full history of an entity-attribute pair, including all assertions and retractions:

```sql
SELECT mentat_history('default', 10001, ':person/name');
```

**Returns:**

```json
[
  {"value": "Alice Smith", "tx": 1001, "added": true},
  {"value": "Alice Smith", "tx": 1050, "added": false},
  {"value": "Alice Johnson", "tx": 1050, "added": true}
]
```

This shows that:
1. "Alice Smith" was asserted at tx 1001
2. "Alice Smith" was retracted at tx 1050
3. "Alice Johnson" was asserted at tx 1050 (a name change)

## Transaction Range

View all datoms across a range of transactions:

```sql
SELECT mentat_tx_range('default', 1000, 1100);
```

**Returns:** All datoms (assertions and retractions) in transactions 1000 through 1100.

## Transaction Log

Get structured transaction log entries with metadata:

```sql
SELECT mentat.log('default', 1000, 1010);
```

**Returns:** Array of transaction objects, each containing:
- `tx` -- transaction ID
- `tx_instant` -- timestamp
- `datoms` -- array of `[e, a, v, tx, added]` tuples

## Diff

Compare two points in time:

```sql
SELECT mentat.diff('default', 1000, 1050);
```

**Returns:** Object with `added` and `retracted` arrays showing the net changes between the two transactions.

## How Time Travel Works Internally

### As-Of Implementation

As-of queries add a `tx <= ?target_tx` filter to every pattern in the generated SQL. The query compiler:
1. Adds `AND d.tx <= $as_of` to each table join
2. Applies retraction filtering: excludes datoms where a later retraction exists with `tx <= target`

### Since Implementation

Since queries add `AND d.tx > $since_tx` to include only newer assertions.

### History Implementation

History queries bypass the current-value filter entirely, returning all datoms (both assertions and retractions) for the specified entity-attribute pair, ordered by transaction ID.

## Combining Time Travel with Other Features

### Pull with As-Of

Currently, the pull API always operates on the current state. To pull at a specific point in time, use a Datalog query with `as_of` and then pull individual entities:

```sql
-- Find entities as of tx 1000
WITH historical_people AS (
  SELECT (jsonb_array_elements(
    (SELECT mentat_query(
      '[:find [?e ...] :where [?e :person/name]]',
      '{"as_of": 1000}'
    ))->'results')
  )::bigint AS eid
)
SELECT mentat_pull('[*]', eid) FROM historical_people;
```

### Subscriptions with Since

Subscriptions notify on new transactions. Combine with `since` to process missed changes after reconnection:

```sql
-- On reconnect, catch up from last known tx
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{"since": 1050}'
);
```

## Excision

Excision is the only operation that breaks the immutability guarantee. It permanently removes entities and all their history from the database. See [SQL Function Reference](./sql-functions.md#excision-functions) for details.

Excision is gated by a per-partition flag (`allow_excision`) and protected against accidentally excising schema entities (entid < 10000).
