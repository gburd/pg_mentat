# Datalog Query Language

pg_mentat implements the Datomic dialect of Datalog. Queries are written in EDN (Extensible Data Notation) and passed to `mentat_query()` as text strings.

## Query Structure

A query has the general form:

```clojure
[:find <find-spec>
 :in <input-bindings>       ;; optional
 :with <with-vars>          ;; optional
 :where <clauses>]
```

## Find Specifications

The `:find` clause determines the shape of the result.

### Relation (default)

Returns a collection of tuples (array of arrays):

```sql
SELECT mentat_query('[:find ?name ?age
                      :where
                      [?e :person/name ?name]
                      [?e :person/age ?age]]', '{}');
-- Result: {"columns":["name","age"],"results":[["Alice",30],["Bob",25]]}
```

### Collection

Returns a flat array of values (single variable, all matches):

```sql
SELECT mentat_query('[:find [?name ...]
                      :where [?e :person/name ?name]]', '{}');
-- Result: {"results":["Alice","Bob","Carol"]}
```

### Tuple

Returns a single tuple (first match only):

```sql
SELECT mentat_query('[:find [?name ?age]
                      :where
                      [?e :person/name ?name]
                      [?e :person/age ?age]]', '{}');
-- Result: {"results":["Alice",30]}
```

### Scalar

Returns a single value (first match, first variable):

```sql
SELECT mentat_query('[:find ?name .
                      :where [?e :person/name ?name]]', '{}');
-- Result: {"results":"Alice"}
```

## Where Clauses

### Basic Pattern

A pattern matches datoms against the EAV model:

```clojure
[?entity :attribute ?value]
[?entity :attribute "literal"]
[?entity :attribute]          ;; value ignored (existence check)
```

Each position can be a variable (`?x`), a literal, or blank (`_`).

```sql
SELECT mentat_query('[:find ?e
                      :where
                      [?e :person/name "Alice"]]', '{}');
```

### Multiple Patterns (Implicit AND)

Multiple patterns in `:where` are joined -- a variable must unify across all patterns:

```sql
SELECT mentat_query('[:find ?name ?email
                      :where
                      [?e :person/name ?name]
                      [?e :person/email ?email]]', '{}');
```

### NOT Clauses

Exclude results matching a pattern:

```clojure
[:find ?name
 :where
 [?e :person/name ?name]
 (not [?e :person/age 25])]
```

Compiles to `NOT EXISTS (subquery)`.

### NOT-JOIN

Explicit variable binding for NOT:

```clojure
[:find ?name
 :where
 [?e :person/name ?name]
 (not-join [?e]
   [?e :person/retired true])]
```

### OR Clauses

Match any of several alternatives:

```clojure
[:find ?name
 :where
 [?e :person/name ?name]
 (or [?e :person/role "admin"]
     [?e :person/role "superuser"])]
```

Compiles to `UNION`.

### OR-JOIN

Explicit variable binding for OR:

```clojure
[:find ?name
 :where
 [?e :person/name ?name]
 (or-join [?e]
   [?e :person/active true]
   [?e :person/admin true])]
```

## Predicates

Predicates filter results using comparison operators. They appear in the `:where` clause with function-call syntax:

```clojure
[(> ?age 21)]
[(< ?price 100.0)]
[(<= ?start ?end)]
[(>= ?score 90)]
[(= ?status "active")]
[(!= ?role "banned")]
```

### Arithmetic

Arithmetic expressions bind their result to a variable:

```clojure
[:find ?name ?total
 :where
 [?e :person/name ?name]
 [?e :order/price ?price]
 [?e :order/quantity ?qty]
 [(* ?price ?qty) ?total]
 [(> ?total 1000)]]
```

Supported operators: `+`, `-`, `*`, `/`.

### String Predicates (Extension)

pg_mentat extends Datomic's predicate set with PostgreSQL's pattern matching:

```clojure
[(like ?name "Ali%")]
[(ilike ?email "%@EXAMPLE.COM")]
```

### Type Safety

Predicates that compare incompatible types (e.g., comparing a string variable against an integer) produce an empty result set rather than an error. This matches Datomic's behavior.

## Query Functions

### ground

Bind a constant value to a variable:

```clojure
[:find ?name
 :where
 [(ground 42) ?answer]
 [?e :person/age ?answer]
 [?e :person/name ?name]]
```

### get-else

Return a default value when an attribute is missing:

```clojure
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [(get-else $ ?e :person/age 0) ?age]]
```

Compiles to `LEFT JOIN ... COALESCE(v, default)`.

### missing?

Test that an entity lacks an attribute:

```clojure
[:find ?name
 :where
 [?e :person/name ?name]
 [(missing? $ ?e :person/email)]]
```

Compiles to `NOT EXISTS` subquery.

## Aggregates

Aggregate functions wrap find variables:

```clojure
[:find (count ?e)
 :where [?e :person/name]]

[:find ?dept (avg ?salary) (max ?salary)
 :where
 [?e :person/department ?dept]
 [?e :person/salary ?salary]]
```

### Supported Aggregates

| Aggregate | Description |
|-----------|-------------|
| `(count ?x)` | Count of values |
| `(count-distinct ?x)` | Count of distinct values |
| `(sum ?x)` | Sum of numeric values |
| `(avg ?x)` | Average of numeric values |
| `(min ?x)` | Minimum value |
| `(max ?x)` | Maximum value |
| `(sample N ?x)` | Random sample of N values |

Non-aggregated variables in the find spec become the `GROUP BY` columns automatically.

## Rules

Rules are named, reusable query fragments. Define them in the `:in` clause with the `%` symbol and provide rule definitions as the corresponding input.

### Basic Rules

```sql
SELECT mentat_query(
  '[:find ?name
    :in $ %
    :where (adult ?e)
           [?e :person/name ?name]]',
  '{"inputs": [null, [["(adult ?person)", "[?person :person/age ?age]", "[(>= ?age 18)]"]]]}'
);
```

### Recursive Rules

Rules can reference themselves for graph traversal:

```sql
-- Find all ancestors (transitive parent-of)
SELECT mentat_query(
  '[:find ?ancestor
    :in $ % ?person
    :where (ancestor ?person ?ancestor)]',
  '{"inputs": [null, [
    ["(ancestor ?p ?a)", "[?p :person/parent ?a]"],
    ["(ancestor ?p ?a)", "[?p :person/parent ?mid]", "(ancestor ?mid ?a)"]
  ], 10001]}'
);
```

Recursive rules compile to PostgreSQL `WITH RECURSIVE` CTEs with a depth limit controlled by the `mentat.max_recursion_depth` GUC (default 100).

### Multi-head Rules

Multiple rule definitions with the same name act as alternatives (logical OR):

```clojure
[[(related ?a ?b) [?a :person/friends ?b]]
 [(related ?a ?b) [?a :person/colleagues ?b]]]
```

## Input Bindings

The `:in` clause declares external parameters. The implicit first input is always `$` (the database).

### Scalar Binding

```clojure
[:find ?name
 :in $ ?min-age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(>= ?age ?min-age)]]
```

```sql
SELECT mentat_query(
  '[:find ?name :in $ ?min-age :where [?e :person/name ?name] [?e :person/age ?age] [(>= ?age ?min-age)]]',
  '{"inputs": [null, 21]}'
);
```

### Collection Binding

Match against a set of values:

```clojure
[:find ?name
 :in $ [?city ...]
 :where
 [?e :person/name ?name]
 [?e :person/city ?city]]
```

```sql
SELECT mentat_query(
  '[:find ?name :in $ [?city ...] :where [?e :person/name ?name] [?e :person/city ?city]]',
  '{"inputs": [null, ["NYC", "SF", "LA"]]}'
);
```

Compiles to `IN (...)` or a values join.

### Tuple Binding

Bind multiple variables at once:

```clojure
[:find ?name
 :in $ [?first ?last]
 :where
 [?e :person/first-name ?first]
 [?e :person/last-name ?last]
 [?e :person/name ?name]]
```

### Relation Binding

Pass a table of values:

```clojure
[:find ?name ?score
 :in $ [[?name ?score]]
 :where
 [?e :person/name ?name]
 [(> ?score 80)]]
```

```sql
SELECT mentat_query(
  '[:find ?name ?score :in $ [[?name ?score]] :where [?e :person/name ?name] [(> ?score 80)]]',
  '{"inputs": [null, [["Alice", 95], ["Bob", 72], ["Carol", 88]]]}'
);
```

Compiles to a `VALUES` join.

## With Clause

The `:with` clause includes variables in grouping without including them in results. This prevents unwanted deduplication:

```clojure
[:find (count ?name)
 :with ?e
 :where [?e :person/name ?name]]
```

Without `:with ?e`, duplicate names would be counted once. With it, each entity contributes separately.

## Query Options

Options are passed in the JSON `inputs` parameter:

| Key | Type | Description |
|-----|------|-------------|
| `"inputs"` | array | Positional input values (first is always `null` for `$`) |
| `"as_of"` | integer | Transaction ID for point-in-time query |
| `"since"` | integer | Transaction ID for "changes since" query |
| `"limit"` | integer | Maximum result rows |

```sql
SELECT mentat_query(
  '[:find ?name :where [?e :person/name ?name]]',
  '{"as_of": 1000, "limit": 10}'
);
```
