# Datalog Query Reference

This document provides a comprehensive reference for the Datalog query language as implemented by pg_mentat. Queries are executed via the `mentat_query` SQL function. The query language is broadly compatible with Datomic's query syntax, with differences noted at the end.

---

## Query Anatomy

A pg_mentat query is an EDN vector beginning with `:find`:

```
[:find <find-spec>
 :in <binding-spec>         ; optional
 :where <clause> ...]       ; required
 :order <ordering> ...]     ; optional (pg_mentat extension)
 :limit <n>]                ; optional
 :with [<rule-defs>]]       ; for rules
```

Invocation in SQL:

```sql
SELECT mentat_query(
  '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]',
  '{}'::jsonb
);
```

The first argument is the Datalog query string. The second is a JSONB object containing input bindings, temporal options (`asOf`, `since`, `history`), and pagination (`limit`, `offset`).

---

## Find Specifications

The `:find` clause determines the shape of the result. This is analogous to the SELECT clause in SQL, but with four distinct output modes.

### FindRel (relation -- default)

Returns a table of tuples. This is the most common form. Results are deduplicated (set semantics).

```clojure
[:find ?name ?age :where ...]
```

Response: `{"columns": ["?name", "?age"], "results": [["Alice", 30], ["Bob", 25]]}`

Use FindRel when you need tabular data for display or further processing.

### FindColl (collection)

Returns a flat list of values from a single variable. Specified by wrapping the variable in `[... ...]` with an ellipsis.

```clojure
[:find [?name ...] :where ...]
```

Response: `{"result": ["Alice", "Bob", "Carol"]}`

Use FindColl when you need a list of values for a dropdown, autocomplete, or further filtering.

### FindScalar (scalar)

Returns a single value (first result). Specified by adding `.` after the variable.

```clojure
[:find ?name . :where ...]
```

Response: `{"result": "Alice"}`

Use FindScalar when you expect exactly one result (e.g., looking up by unique attribute).

### FindTuple (tuple)

Returns a single tuple (first row). Specified by wrapping variables in `[...]` without ellipsis.

```clojure
[:find [?name ?age] :where ...]
```

Response: `{"result": ["Alice", 30]}`

Use FindTuple when you need one row of multiple columns.

---

## Pattern Matching

Patterns are the fundamental building block of `:where` clauses. Each pattern matches against the Entity-Attribute-Value (EAV) datom store.

### Basic Pattern

```clojure
[?entity :attribute ?value]
```

- `?entity` -- logic variable bound to the entity ID (always a `BIGINT`)
- `:attribute` -- a keyword ident referencing a schema attribute
- `?value` -- logic variable bound to the attribute's value

### Constant Value

Bind a specific value in the value position:

```clojure
[?e :person/name "Alice"]
[?e :person/age 30]
```

### Entity ID Lookup

Use a numeric constant in the entity position:

```clojure
[10001 :person/name ?name]
```

### Placeholder (Wildcard)

Use `_` to ignore a position:

```clojure
[?e :person/name _]    ; match any entity that has a :person/name
```

### Multi-Pattern Joins

Multiple patterns sharing variables create implicit joins:

```clojure
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]]
```

The shared `?e` variable joins on entity ID, correlating names with ages. In SQL terms, this generates a JOIN between the text datoms table (for `:person/name`) and the long datoms table (for `:person/age`) on the `e` column.

### Reference Traversal

Join across references to navigate entity relationships:

```clojure
[:find ?person-name ?friend-name
 :where
 [?p :person/name ?person-name]
 [?p :person/friend ?f]
 [?f :person/name ?friend-name]]
```

Here `?f` binds to the entity ID stored in the ref-typed `:person/friend` attribute, and the third pattern resolves that entity's name.

---

## Variables

Variables begin with `?` and are scoped to the query. They unify across patterns -- if the same variable appears in multiple patterns, they must bind to the same value.

```clojure
?e          ; entity variable
?name       ; value variable
?target-age ; hyphenated names allowed
?x1         ; alphanumeric names allowed
```

Variables in the `:find` clause are projected into the result. Variables only in `:where` are existentially quantified (they constrain but are not returned).

**Unification semantics**: When `?e` appears in two patterns `[?e :person/name ?name]` and `[?e :person/age ?age]`, the engine generates a SQL JOIN on the entity column. This is how Datalog achieves relational joins without explicit JOIN syntax -- shared variables create the linkage automatically.

**Naming conventions**: Use descriptive names (`?person-name` rather than `?x`) for readability. The engine treats all `?`-prefixed symbols identically regardless of naming.

---

## Predicates

Predicates filter results by applying comparison operators to bound variables or constants. They appear as function-call forms in the `:where` clause.

### Comparison Operators

| Operator | SQL equivalent | Description |
|----------|---------------|-------------|
| `<`      | `<`           | Less than |
| `>`      | `>`           | Greater than |
| `<=`     | `<=`          | Less than or equal |
| `>=`     | `>=`          | Greater than or equal |
| `=`      | `=`           | Equality |
| `!=`     | `!=`          | Inequality |
| `like`   | `LIKE`        | SQL LIKE pattern matching (case-sensitive) |
| `ilike`  | `ILIKE`       | Case-insensitive LIKE pattern matching |

### Examples

```clojure
; Numeric comparison
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(>= ?age 30)]]

; String equality
[:find ?name
 :where
 [?e :person/name ?name]
 [(= ?name "Alice")]]

; String inequality
[:find ?name
 :where
 [?e :person/name ?name]
 [(!= ?name "Alice")]]

; Pattern matching with LIKE
[:find ?name
 :where
 [?e :person/name ?name]
 [(like ?name "Al%")]]

; Case-insensitive pattern matching
[:find ?name
 :where
 [?e :person/name ?name]
 [(ilike ?name "%alice%")]]
```

### Predicate with Two Variables

Compare two bound variables against each other:

```clojure
[:find ?name1 ?name2
 :where
 [?e1 :person/name ?name1]
 [?e2 :person/name ?name2]
 [?e1 :person/age ?age1]
 [?e2 :person/age ?age2]
 [(> ?age1 ?age2)]]
```

### Type-Aware Predicates

pg_mentat is type-aware. Text predicates operate correctly on text columns, and numeric predicates on numeric columns, without manual casting. Comparing incompatible types (e.g., a numeric operator against a string variable) produces no results rather than a runtime error. This prevents the class of bugs described in Mozilla Mentat issue #520 where inequality predicates could match wrong types.

---

## Where-Functions

Where-functions are expressions in `:where` that compute values or filter entities.

### ground

Bind a constant value to a variable:

```clojure
; Bind integer constant
[:find ?name
 :where
 [(ground 30) ?age]
 [?e :person/age ?age]
 [?e :person/name ?name]]

; Bind string constant
[:find ?e ?age
 :where
 [(ground "Alice") ?name]
 [?e :person/name ?name]
 [?e :person/age ?age]]

; Use ground for computed labels
[:find ?name ?label
 :where
 [(ground "senior") ?label]
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(>= ?age 30)]]
```

### get-else

Return an attribute's value, or a default if the attribute is missing:

```clojure
[:find ?name ?email
 :where
 [?e :person/name ?name]
 [(get-else $ ?e :person/email "no-email") ?email]]
```

Arguments: `(get-else <src> <entity-var> <attr-keyword> <default-value>)`

The `$` is the implicit data source. The result variable (`?email`) is bound to the attribute's value if present, or the default if the entity lacks that attribute.

### missing?

Filter for entities that do NOT have a specific attribute:

```clojure
[:find ?name
 :where
 [?e :person/name ?name]
 [(missing? $ ?e :person/email)]]
```

This returns names of people who have no `:person/email` attribute asserted.

### fulltext

Full-text search across indexed text attributes:

```clojure
[:find ?e ?name ?score
 :where
 [(fulltext $ :person/bio "engineer") [[?e ?name ?score]]]]
```

Arguments: `(fulltext <src> <attr-keyword> <search-term>)`

Returns tuples of `[entity-id, matched-text, relevance-score]`. The attribute must have `:db/fulltext true` in its schema definition. Uses PostgreSQL's built-in `tsvector`/`tsquery` with `ts_rank_cd` for BM25-like relevance scoring.

### Arithmetic Functions

Compute values from variables:

```clojure
[:find ?name ?double-age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(* ?age 2) ?double-age]]
```

Supported arithmetic operators: `*`, `+`, `-`, `/`.

---

## Clauses

### and (implicit)

Multiple patterns in `:where` are implicitly conjoined (AND):

```clojure
[:find ?name
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(> ?age 25)]]
```

### or

Match if ANY branch succeeds:

```clojure
; Find people aged 25 OR 35
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 (or [?e :person/age 25]
     [?e :person/age 35])]
```

OR branches can contain compound expressions:

```clojure
; age < 26 OR age > 34
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 (or (and [?e :person/age ?a1] [(< ?a1 26)])
     (and [?e :person/age ?a2] [(> ?a2 34)]))]
```

Each branch generates a SQL `UNION ALL` subquery. Variables used outside the `or` must be bound in every branch.

### not

Exclude entities matching a pattern:

```clojure
; Find people NOT younger than 30
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 (not [?e :person/age ?a] [(< ?a 30)])]

; Exclude a specific name
[:find ?name
 :where
 [?e :person/name ?name]
 (not [?e :person/name "Alice"])]
```

NOT generates a `NOT EXISTS (...)` SQL subquery. Variables referenced inside `not` that also appear outside create correlated subquery joins.

NOT can include predicates:

```clojure
; Exclude based on text predicate
[:find ?name
 :where
 [?e :person/name ?name]
 (not [?e :person/name ?n] [(= ?n "Alice")])]
```

---

## Rules

Rules define reusable logic patterns, similar to views or named predicates.

```clojure
[:find ?name
 :where
 (adult ?e)
 [?e :person/name ?name]
 :with [[(adult ?p) [?p :person/age ?a] [(>= ?a 18)]]]]
```

Rule definitions appear in the `:with` clause as vectors of `[(rule-name ?arg ...) <patterns>...]`.

Rules can be recursive (e.g., transitive closure over graph relationships). Recursive rules are compiled to PostgreSQL CTEs (Common Table Expressions). The maximum recursion depth is controlled by the `mentat.max_recursion_depth` GUC (default: 100).

### Recursive Rule Example

```clojure
; Find all people reachable via :person/friend (transitive closure)
[:find ?name
 :in ?start
 :where
 (reachable ?start ?target)
 [?target :person/name ?name]
 :with [[(reachable ?from ?to) [?from :person/friend ?to]]
        [(reachable ?from ?to) [?from :person/friend ?mid] (reachable ?mid ?to)]]]
```

For complex recursive queries requiring fine-grained control over the SQL CTE structure, pg_mentat also provides `mentat.recursive()` which accepts raw SQL fragments for the base and recursive cases.

---

## Aggregates

Aggregate functions reduce result sets:

| Function | Description |
|----------|-------------|
| `count`  | Count of values (uses `COUNT(DISTINCT ...)`) |
| `sum`    | Sum of numeric values |
| `avg`    | Average of numeric values |
| `min`    | Minimum value |
| `max`    | Maximum value |

### Syntax

Aggregates wrap a variable in the `:find` clause:

```clojure
; Count all people
[:find (count ?e) :where [?e :person/name _]]

; Average age
[:find (avg ?age) :where [?e :person/age ?age]]

; Min and max age together
[:find (min ?age) (max ?age) :where [?e :person/age ?age]]

; Count with grouping
[:find ?name (count ?hobby)
 :where
 [?e :person/name ?name]
 [?e :person/hobbies ?hobby]]
```

When aggregates and non-aggregate variables coexist in `:find`, the non-aggregate variables form the GROUP BY clause.

### Aggregates with Predicates

Combine aggregates with filtering:

```clojure
; Count people over 25
[:find (count ?e)
 :where
 [?e :person/age ?age]
 [(> ?age 25)]]

; Sum of ages by department
[:find ?dept (sum ?age)
 :where
 [?e :person/department ?dept]
 [?e :person/age ?age]]
```

---

## Pull Expressions

Pull retrieves structured entity data. It is invoked via `mentat_pull` (single entity) or `mentat_pull_many` (batch).

### Basic Attribute Selection

```sql
SELECT mentat_pull('[:person/name :person/age]', 10001);
```

### Wildcard

```sql
SELECT mentat_pull('[*]', 10001);
```

### Nested References (Join)

```sql
SELECT mentat_pull('[:person/name {:person/friend [:person/name :person/age]}]', 10001);
```

### Reverse Lookups

Fetch entities that reference this entity:

```sql
SELECT mentat_pull('[:person/name :person/_friend]', 10001);
```

The `_` prefix reverses the direction: finds entities whose `:person/friend` points to 10001.

### Recursion

```sql
-- Unlimited recursion
SELECT mentat_pull('[{:person/friend ...}]', 10001);

-- Bounded recursion (max depth 3)
SELECT mentat_pull('[{:person/friend 3}]', 10001);
```

### Options

```sql
-- Limit multi-valued results
SELECT mentat_pull('[(:person/hobbies :limit 5)]', 10001);

-- Default value for missing attribute
SELECT mentat_pull('[(:person/email :default "none")]', 10001);

-- Rename in output
SELECT mentat_pull('[(:person/name :as "Name")]', 10001);

-- Wildcard with specific nested patterns
SELECT mentat_pull('[* {:person/friend [:person/name]}]', 10001);
```

---

## Binding Forms (:in)

The `:in` clause declares input parameters, enabling parameterized queries.

### Scalar Binding (default)

Bind a single value to a single variable:

```clojure
[:find ?name :in ?age :where [?e :person/age ?age] [?e :person/name ?name]]
```

Input JSON: `{"?age": 30}`

### Collection Binding

Bind multiple values to one variable (generates SQL `IN` clause):

```clojure
[:find ?name :in [?age ...] :where [?e :person/age ?age] [?e :person/name ?name]]
```

Input JSON: `{"inputs": [[25, 30, 35]]}`

This finds names for people whose age is 25, 30, or 35.

### Tuple Binding

Bind multiple variables simultaneously:

```clojure
[:find ?name :in [?first ?last] :where [?e :person/first ?first] [?e :person/last ?last] [?e :person/name ?name]]
```

Input JSON: `{"inputs": [["Alice", "Smith"]]}`

### Relation Binding

Bind a set of tuples (generates SQL `VALUES` join):

```clojure
[:find ?name :in [[?age ?prefix]] :where [?e :person/age ?age] [?e :person/name ?name]]
```

Input JSON: `{"inputs": [[[25, "Alice"], [30, "Bob"]]]}`

---

## Input Format (JSON)

The second argument to `mentat_query` is a JSONB object supporting these keys:

| Key | Type | Description |
|-----|------|-------------|
| `"?variable"` | scalar | Scalar binding for `:in` variable |
| `"inputs"` | array | Positional bindings for collection/tuple/relation |
| `"asOf"` | integer | Transaction ID for as-of temporal query |
| `"since"` | integer | Transaction ID for since temporal query |
| `"history"` | boolean | Include retracted datoms |
| `"limit"` | integer | Maximum rows to return |
| `"offset"` | integer | Skip first N rows (pagination) |

### Examples

```sql
-- Scalar binding
SELECT mentat_query('[:find ?name :in ?age :where ...]', '{"?age": 30}'::jsonb);

-- Collection binding
SELECT mentat_query('[:find ?name :in [?age ...] :where ...]', '{"inputs": [[25, 30]]}'::jsonb);

-- Temporal + pagination
SELECT mentat_query('[:find ?name :where ...]', '{"asOf": 1000005, "limit": 10, "offset": 0}'::jsonb);
```

---

## Ordering

pg_mentat extends standard Datalog with an `:order` clause (not present in Datomic):

```clojure
[:find ?name ?age
 :where [?e :person/name ?name] [?e :person/age ?age]
 :order (asc ?name)]

[:find ?name ?age
 :where [?e :person/name ?name] [?e :person/age ?age]
 :order (desc ?age)]
```

Multiple orderings are supported:

```clojure
[:find ?name ?age
 :where [?e :person/name ?name] [?e :person/age ?age]
 :order (desc ?age) (asc ?name)]
```

This orders by age descending first, then by name ascending as a tiebreaker.

---

## Limits

A `:limit` clause can be placed inside the query:

```clojure
[:find ?name :where [?e :person/name ?name] :limit 10]
```

Alternatively, pass `"limit"` in the input JSON (which takes precedence if both are present).

---

## mentat_explain

Inspect the generated SQL and query plan without executing:

```sql
SELECT mentat_explain(
  '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]',
  '{}'::jsonb
);
```

Returns a JSON object with:
- `generated_sql` -- the SQL that would be executed
- `datalog_plan` -- structural analysis of the query
- `pg_explain` -- PostgreSQL's EXPLAIN output for the generated SQL

The explain format is controlled by `SET mentat.explain_format = 'json'` (or `text`, `yaml`, `xml`).

---

## Differences from Datomic

| Feature | Datomic | pg_mentat |
|---------|---------|-----------|
| Query invocation | `(d/q query db inputs)` | `SELECT mentat_query(query, inputs)` |
| Transaction | `(d/transact conn tx-data)` | `SELECT mentat_transact(edn)` |
| Pull | `(d/pull db pattern eid)` | `SELECT mentat_pull(pattern, eid)` |
| Input format | EDN in-process | JSON via second argument |
| Collection binding input | EDN vectors | `{"inputs": [[val1, val2, ...]]}` |
| `:order` clause | Not supported natively | Supported via `(asc ?var)` / `(desc ?var)` |
| `:limit` clause | Not in query syntax | Supported in query and in inputs JSON |
| LIKE/ILIKE predicates | Not available | Available via `[(like ?v "pat%")]` |
| Pagination | Client-side | Server-side via `limit`/`offset` in inputs |
| Rules syntax | `:in` with `%` rule source | `:with` clause inline |
| Data source (`$`) | Explicit in `:in` | Implicit (current store) |
| Speculative tx | `(d/with db tx-data)` | `SELECT mentat_with(edn)` |
| Result shape | Sets of tuples | JSON with `columns` + `results` |
| Full-text search | Lucene-backed | PostgreSQL tsvector/tsquery |
| Excision | Not supported | `SELECT mentat_excise(entity_ids)` |
| Named stores | Separate connections | `mentat.create_store()` / multi-store functions |
| History | `(d/history db)` | `{"history": true}` in inputs |
| Explain/debug | None | `mentat_explain()` with format control |

### Key Compatibility Notes

1. **EDN parsing**: pg_mentat accepts the same EDN syntax as Datomic for queries and transactions. Commas are optional whitespace.

2. **Rule placement**: Datomic passes rules as an explicit rules source in `:in`. pg_mentat uses `:with` for inline rule definitions (simpler for single-query usage).

3. **Data source**: Datomic requires `$` in `:in` to reference the database. pg_mentat uses the current store implicitly. The `$` in `get-else`, `missing?`, and `fulltext` is accepted syntactically but does not select a different data source.

4. **Transaction reports**: The JSON shape is compatible with Datomic's transit-encoded reports, but uses native JSON types instead of transit tags.

5. **Type coercion**: pg_mentat is stricter about type matching in predicates. Comparing a string variable against an integer literal produces no results rather than a runtime error, matching Datomic's behavior for issue #520.

---

## Function Reference Summary

| SQL Function | Description |
|---|---|
| `mentat_query(query, inputs)` | Execute Datalog query on default store |
| `mentat.q(store, query, inputs)` | Execute query on named store |
| `mentat_q_full(store, query, inputs, as_of_tx)` | Query with explicit temporal control |
| `mentat_explain(query, inputs)` | Show generated SQL and plan |
| `mentat_query_sql(query, inputs)` | Return only the generated SQL string |
| `mentat_pull(pattern, entity_id)` | Pull entity data |
| `mentat_pull_many(pattern, entity_ids)` | Batch pull |
| `mentat_entity(entity_id)` | Flat entity lookup |
| `mentat_transact(edn)` | Execute transaction |
| `mentat_with(edn)` | Speculative transaction |
| `mentat_stmt_cache_stats()` | Cache diagnostics |
| `mentat_stmt_cache_clear()` | Clear prepared statement cache |
