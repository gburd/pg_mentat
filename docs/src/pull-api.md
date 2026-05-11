# Pull API

The Pull API provides a declarative way to retrieve structured data from entities. Instead of writing Datalog queries, you specify a pattern describing which attributes to retrieve and how to navigate references.

## Basic Usage

```sql
-- Pull specific attributes
SELECT mentat_pull('[:person/name :person/age]', 10001);
-- {"person/name": "Alice", "person/age": 30}

-- Pull all attributes
SELECT mentat_pull('[*]', 10001);
-- {"db/id": 10001, "person/name": "Alice", "person/age": 30, "person/email": ["a@b.com"]}
```

## Pattern Syntax

A pull pattern is an EDN vector containing attribute specs:

```clojure
[<attr-spec> ...]
```

Where each `<attr-spec>` can be:

| Form | Description |
|------|-------------|
| `:keyword` | Simple attribute |
| `[*]` | Wildcard (all attributes) |
| `{:ref-attr <sub-pattern>}` | Map spec (navigate ref) |
| `{:ref-attr N}` | Bounded recursion (N levels deep) |
| `{:ref-attr ...}` | Unbounded recursion (with cycle detection) |
| `(:attr :as :alias)` | Rename in output |
| `(default :attr value)` | Default for missing attribute |
| `(limit :attr N)` | Limit cardinality-many results |
| `:_ref-attr` | Reverse reference lookup |

## Attribute Selection

### Simple Attributes

```sql
SELECT mentat_pull('[:person/name :person/age :person/email]', 10001);
```

Returns only the specified attributes. Missing attributes are omitted from the result.

### Wildcard

```sql
SELECT mentat_pull('[*]', 10001);
```

Returns all attributes for the entity, including `:db/id`. Reference attributes return entity IDs (not expanded).

### Wildcard with Overrides

Combine wildcard with specific navigation for refs:

```sql
SELECT mentat_pull('[* {:person/friends [:person/name]}]', 10001);
```

## Reference Navigation

### Forward References (Map Specs)

Navigate a reference attribute and pull sub-attributes from the referenced entity:

```sql
SELECT mentat_pull(
  '[{:person/friends [:person/name :person/age]}]',
  10001
);
-- {"person/friends": [{"person/name": "Bob", "person/age": 25}]}
```

Map specs can be nested arbitrarily deep:

```sql
SELECT mentat_pull(
  '[{:person/friends [:person/name {:person/friends [:person/name]}]}]',
  10001
);
```

### Reverse References

Use the `_` prefix on a reference attribute to find entities that reference the target entity:

```sql
-- Find who has entity 10001 as a friend
SELECT mentat_pull('[:person/name :person/_friends]', 10001);
-- {"person/name": "Alice", "person/_friends": [{"db/id": 10002}]}
```

Reverse references can also use map specs:

```sql
SELECT mentat_pull('[{:person/_friends [:person/name]}]', 10001);
```

## Recursion

### Unbounded Recursion

Use `...` to traverse a reference attribute to arbitrary depth. Cycle detection prevents infinite loops.

```sql
SELECT mentat_pull(
  '[:person/name {:person/manager ...}]',
  10001
);
-- Returns the full management chain up to the root
```

### Bounded Recursion

Specify a maximum depth as an integer:

```sql
SELECT mentat_pull(
  '[:person/name {:person/friends 2}]',
  10001
);
-- Navigates friends-of-friends (2 levels) then stops
```

### Cycle Detection

When traversing cyclic graphs (e.g., mutual friendships), pg_mentat tracks visited entity IDs and stops when a cycle is detected. Cyclic references appear as `{:db/id <id>}` without further expansion.

## Modifiers

### Default Values

Provide a fallback value when an attribute is missing:

```sql
SELECT mentat_pull(
  '[(default :person/nickname "N/A") :person/name]',
  10001
);
-- {"person/nickname": "N/A", "person/name": "Alice"}
```

### Rename (`:as`)

Rename an attribute in the output:

```sql
SELECT mentat_pull(
  '[(:person/name :as :name) (:person/age :as :years)]',
  10001
);
-- {"name": "Alice", "years": 30}
```

### Limit

Cap the number of values returned for cardinality-many attributes:

```sql
SELECT mentat_pull(
  '[(limit :person/email 3)]',
  10001
);
-- Returns at most 3 email addresses
```

## Component Auto-Expansion

Attributes marked with `:db/isComponent true` in the schema are automatically expanded (pulled recursively) without needing an explicit map spec:

```sql
-- If :order/line-items is a component attribute:
SELECT mentat_pull('[*]', 20001);
-- Line items are fully expanded, not just entity IDs
```

## Pull Many

Pull the same pattern for multiple entities:

```sql
SELECT mentat_pull_many(
  '[:person/name :person/age]',
  ARRAY[10001, 10002, 10003]
);
-- [{"person/name":"Alice","person/age":30}, {"person/name":"Bob","person/age":25}, ...]
```

Entities that do not exist return empty maps `{}` in their position.

## Pull in Queries

While pg_mentat does not currently support pull expressions directly inside `:find` clauses (as Datomic does), you can compose queries and pulls:

```sql
-- First, find entity IDs
WITH people AS (
  SELECT jsonb_array_elements(
    (SELECT mentat_query('[:find [?e ...] :where [?e :person/age ?a] [(> ?a 21)]]', '{}'))->'results'
  )::bigint AS eid
)
-- Then pull full details
SELECT mentat_pull('[:person/name :person/age :person/email]', eid)
FROM people;
```

## Performance Considerations

- **Wildcard** queries scan all nine type tables for the entity -- use specific attributes when you know what you need.
- **Deep recursion** can generate many SPI calls. Use bounded recursion in production.
- **Pull many** is more efficient than calling `mentat_pull` in a loop because it batches schema lookups.
- The pull implementation uses the schema cache, so the first call after a schema change may be slightly slower.
