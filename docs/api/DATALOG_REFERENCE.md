# Datalog Query Language Reference

pg_mentat implements a Datalog query language compatible with Datomic's query
syntax. Queries are written in EDN (Extensible Data Notation) and executed
via `mentat_query` in SQL or the `:q` operation in the mentatd HTTP protocol.

---

## Query Structure

A query is an EDN vector with keyword-delimited clauses:

```edn
[:find <find-spec>
 :in <input-vars>
 :where <where-clauses>
 :order <order-specs>
 :limit <limit>
 :offset <offset>
 :with <with-vars>
 :rules <rules>]
```

| Clause    | Required | Description                                    |
|-----------|----------|------------------------------------------------|
| `:find`   | Yes      | What to return from matching datoms.           |
| `:where`  | Yes      | Patterns and constraints to match.             |
| `:in`     | No       | Input variables bound from external arguments. |
| `:order`  | No       | Sort order for results.                        |
| `:limit`  | No       | Maximum number of results.                     |
| `:offset` | No       | Number of results to skip.                     |
| `:with`   | No       | Additional variables to consider for distinctness. |
| `:rules`  | No       | Inline rule definitions.                       |

---

## Find Specifications

The `:find` clause determines the shape of results.

### Relation (default)

Returns a collection of tuples.

```edn
[:find ?e ?name
 :where [?e :person/name ?name]]
```

Result: `[[10001 "Alice"] [10002 "Bob"]]`

### Collection

Returns a flat collection of values. Indicated by `[?var ...]`.

```edn
[:find [?name ...]
 :where [?e :person/name ?name]]
```

Result: `["Alice" "Bob" "Carol"]`

### Scalar

Returns a single value. Indicated by `?var .`.

```edn
[:find ?name .
 :where [?e :person/name ?name]]
```

Result: `"Alice"`

### Tuple

Returns a single tuple. Indicated by `[?var1 ?var2]`.

```edn
[:find [?name ?age]
 :where [?e :person/name ?name] [?e :person/age ?age]]
```

Result: `["Alice" 30]`

---

## Where Clauses

The `:where` clause contains patterns and constraints that must all be satisfied.

### Data Patterns

The most common clause type. Matches datoms in the database.

```edn
[?entity :attribute ?value]
[?entity :attribute ?value ?transaction]
[?entity :attribute ?value ?transaction ?added]
```

| Position | Name        | Description                               |
|----------|-------------|-------------------------------------------|
| 1st      | Entity      | Entity ID -- variable, integer, or ident. |
| 2nd      | Attribute   | Attribute -- keyword or variable.         |
| 3rd      | Value       | Value -- variable, literal, or constant.  |
| 4th      | Transaction | Transaction ID -- variable or integer.    |
| 5th      | Added       | Boolean -- true for assertion, false for retraction. |

**Wildcards:** Use `_` as a placeholder when you don't need a value:

```edn
[?e :person/name _]  ;; Match entities with any :person/name value
```

**Constant Values:**

```edn
[?e :person/name "Alice"]      ;; String
[?e :person/age 30]            ;; Integer
[?e :person/active true]       ;; Boolean
[?e :person/type :type/admin]  ;; Keyword
```

**Multiple Patterns (Implicit Join):**

Variables with the same name must unify across patterns:

```edn
[:find ?name ?email
 :where
 [?e :person/name ?name]
 [?e :person/email ?email]]  ;; ?e must be the same entity in both patterns
```

---

### Predicates

Apply constraints to bound variables. Written as a list in a where clause.

```edn
[(predicate arg1 arg2 ...)]
```

**Comparison Predicates:**

| Predicate | Description     | Example                |
|-----------|-----------------|------------------------|
| `<`       | Less than       | `[(< ?age 65)]`       |
| `<=`      | Less or equal   | `[(<= ?age 65)]`      |
| `>`       | Greater than    | `[(> ?age 18)]`       |
| `>=`      | Greater or equal| `[(>= ?age 18)]`      |
| `!=`      | Not equal       | `[(!= ?name "Bob")]`  |

**Multi-variable predicates:**

```edn
[:find ?name
 :in ?max-age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(< ?age ?max-age)]]
```

---

### Function Expressions

Bind the result of a function to a variable.

```edn
[(function arg ...) ?result]
[(function arg ...) [?result ...]]
[(function arg ...) [[?a ?b ?c]]]
```

**Built-in Functions:**

| Function    | Binding      | Description                              |
|-------------|--------------|------------------------------------------|
| `ground`    | `?x`         | Bind a constant value.                   |
| `fulltext`  | `[[?e ?v]]`  | Full-text search on a fulltext attribute.|
| `tx-ids`    | `[?tx ...]`  | Transaction ID range.                    |
| `tx-data`   | `[[?e ?a ?v ?tx ?added]]` | Datoms for a transaction.  |

#### ground

Bind a constant to a variable:

```edn
[:find ?x
 :where [(ground 42) ?x]]
```

Result: `[[42]]`

#### fulltext

Full-text search on attributes with `:db/fulltext true`:

```edn
[:find ?e ?v
 :where [(fulltext $ :article/body "search terms") [[?e ?v]]]]
```

The `$` is the implicit data source. Returns entity-value pairs matching
the text search.

#### tx-ids

Get transaction IDs in a range:

```edn
[:find [?tx ...]
 :where [(tx-ids $ 1000001 1000010) [?tx ...]]]
```

#### tx-data

Get all datoms asserted/retracted in a transaction:

```edn
[:find ?e ?a ?v ?added
 :in ?tx
 :where [(tx-data $ ?tx) [[?e ?a ?v _ ?added]]]]
```

---

### Or Clauses

Match any of several alternative patterns, predicates, or combinations thereof.
All alternatives must bind the same set of variables.

```edn
(or <clause1> <clause2> ...)
```

**Basic pattern matching:**

```edn
[:find ?name
 :where
 [?e :person/name ?name]
 (or [?e :person/role :role/admin]
     [?e :person/role :role/superadmin])]
```

**Predicates in Or clauses:**

You can use comparison predicates (`<`, `>`, `<=`, `>=`, `=`, `!=`) inside OR branches:

```edn
[:find ?e ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 (or [(< ?age 20)]
     [(> ?age 60)])]
```

**Mixing patterns and predicates:**

```edn
[:find ?e ?name
 :where
 (or [?e :person/name "Alice"]
     (and [?e :person/name ?name]
          [?e :person/age ?age]
          [(> ?age 30)]))]
```

**And within Or:**

Combine multiple clauses (patterns and predicates) in a single alternative:

```edn
(or (and [?e :person/role :role/admin]
         [?e :person/active true]
         [(> ?age 25)])
    [?e :person/role :role/superadmin])
```

**Note:** All variables used in predicates within OR branches must be bound by patterns in the same branch or in the outer query context.

---

### Or-Join

Like `or`, but explicitly declares which variables must be unified with
the enclosing query:

```edn
(or-join [?e]
  [?e :person/role :role/admin]
  (and [?e :person/supervisor ?s]
       [?s :person/role :role/admin]))
```

Only `?e` is required to unify with the outer query. Other variables
introduced in the or-join alternatives are local.

---

### Not Clauses

Exclude results that match a pattern:

```edn
(not <clause1> <clause2> ...)
```

```edn
[:find ?name
 :where
 [?e :person/name ?name]
 (not [?e :person/retired true])]
```

---

### Not-Join

Like `not`, but explicitly declares which variables to unify:

```edn
(not-join [?e]
  [?e :person/role :role/banned])
```

---

### Type Annotations

Constrain a variable to a specific value type:

```edn
[:find ?v
 :where
 [?e :attr ?v]
 [(type ?v :db.type/string)]]
```

---

### Rule Invocation

Invoke a named rule (defined in the `:rules` clause):

```edn
(rule-name ?arg1 ?arg2 ...)
```

```edn
[:find ?name
 :where
 (ancestor 10001 ?desc)
 [?desc :person/name ?name]]
```

---

## Input Variables

The `:in` clause declares variables that receive values from external
arguments. Variables are bound positionally from the `inputs` array.

### Scalar Binding

```edn
[:find ?name
 :in ?age
 :where [?e :person/age ?age] [?e :person/name ?name]]
```

SQL: `mentat_query('...', '{"inputs": [30]}'::jsonb)`

### Collection Binding

Bind multiple values to test against:

```edn
[:find ?name
 :in [?age ...]
 :where [?e :person/age ?age] [?e :person/name ?name]]
```

SQL: `mentat_query('...', '{"inputs": [[25, 30, 35]]}'::jsonb)`

### Tuple Binding

Bind multiple variables at once:

```edn
[:find ?name
 :in [?min-age ?max-age]
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]
 [(>= ?age ?min-age)]
 [(<= ?age ?max-age)]]
```

SQL: `mentat_query('...', '{"inputs": [[18, 65]]}'::jsonb)`

### Relation Binding

Bind a set of tuples:

```edn
[:find ?name
 :in [[?name ?age]]
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]]
```

SQL: `mentat_query('...', '{"inputs": [[["Alice", 30], ["Bob", 25]]]}'::jsonb)`

### Data Sources

The implicit source `$` is always available. Named sources are declared
with `$name`:

```edn
[:find ?name
 :in $
 :where [$ ?e :person/name ?name]]
```

---

## Aggregates

Aggregate functions are applied in the `:find` clause.

### Available Aggregates

| Aggregate       | Return Type | Description                        |
|-----------------|-------------|------------------------------------|
| `(count ?x)`   | integer     | Count of values.                   |
| `(count-distinct ?x)` | integer | Count of distinct values.    |
| `(sum ?x)`     | number      | Sum of numeric values.             |
| `(avg ?x)`     | double      | Average of numeric values.         |
| `(min ?x)`     | varies      | Minimum value.                     |
| `(max ?x)`     | varies      | Maximum value.                     |
| `(sample N ?x)`| collection  | N random values.                   |
| `(median ?x)`  | number      | Median of numeric values.          |
| `(variance ?x)`| double      | Variance of numeric values.        |
| `(stddev ?x)`  | double      | Standard deviation of numeric values. |

### Examples

**Count all entities:**

```edn
[:find (count ?e)
 :where [?e :person/name]]
```

**Group and aggregate:**

```edn
[:find ?dept (count ?e) (avg ?salary)
 :where
 [?e :person/department ?dept]
 [?e :person/salary ?salary]]
```

Non-aggregate variables in `:find` become the grouping key. Each unique
combination of `?dept` produces one result row.

**Min/Max with corresponding values:**

```edn
[:find (min ?age) (the ?name)
 :where [?e :person/name ?name] [?e :person/age ?age]]
```

`(the ?var)` returns the corresponding value from the row that produced
the `min` or `max`.

---

## Pull Expressions in Find

Pull expressions can appear directly in `:find` to retrieve entity data
inline with query results:

```edn
[:find (pull ?e [:person/name :person/age])
 :where [?e :person/department :dept/engineering]]
```

Result:
```edn
[[{:db/id 10001 :person/name "Alice" :person/age 30}]
 [{:db/id 10002 :person/name "Bob" :person/age 25}]]
```

---

## Pull Patterns

Pull patterns specify which attributes to retrieve for an entity. They
are used with `mentat_pull` and in pull expressions within queries.

### Attribute Spec

Specific attributes by keyword:

```edn
[:person/name :person/age :person/email]
```

### Wildcard

All attributes:

```edn
[*]
```

### Map Spec (Ref Following)

Follow reference attributes with a sub-pattern:

```edn
[{:person/friends [:person/name]}]
```

Nested:

```edn
[{:person/department [{:department/company [:company/name]}]}]
```

### Reverse Lookup

Find entities that reference the target entity. Prefix the attribute
namespace with `_`:

```edn
[:person/_friends]
```

This finds all entities whose `:person/friends` attribute points to the
target entity.

### Recursive Pull

Follow references recursively:

```edn
;; Unbounded (cycles detected and terminated)
[{:person/manager ...}]

;; Bounded to depth N
[{:person/manager 3}]
```

### Attribute Options

Options are specified using a list form:

#### :as (Rename)

```edn
[(:person/name :as "fullName")]
```

#### :default

```edn
[(:person/email :default "no-email@example.com")]
```

#### :limit

For cardinality-many attributes:

```edn
[(:person/tags :limit 5)]

;; nil = unlimited
[(:person/tags :limit nil)]
```

### Combined Pattern Example

```edn
[*
 (:person/email :default "none")
 {:person/friends [:person/name (:person/age :as "years")]}
 {:person/manager 2}]
```

---

## Rules

Rules allow you to define reusable query abstractions. Multiple clauses
for the same rule name define alternatives (like `or`).

### Definition

Rules are defined in the `:rules` clause:

```edn
[:find ?name
 :where
 (ancestor 10001 ?desc)
 [?desc :person/name ?name]
 :rules
 [[(ancestor ?parent ?child)
   [?parent :person/child ?child]]
  [(ancestor ?ancestor ?descendant)
   [?ancestor :person/child ?mid]
   (ancestor ?mid ?descendant)]]]
```

### Rule Structure

```edn
[(rule-name ?arg1 ?arg2 ...)
 <clause1>
 <clause2>
 ...]
```

- The first element is the rule head: `(rule-name ?bound-vars ...)`.
- Remaining elements are where clauses forming the rule body.
- Multiple definitions of the same rule name create alternatives.
- Rule bodies can contain:
  - Data patterns: `[?e :attr ?v]`
  - Predicates: `[(>= ?age 18)]`
  - Arithmetic functions: `[(* ?price 0.9) ?discounted]`
  - Recursive rule invocations: `(rule-name ?x ?y)`

### Recursive Rules

Rules can invoke themselves:

```edn
[[(reachable ?from ?to)
  [?from :link/to ?to]]
 [(reachable ?from ?to)
  [?from :link/to ?mid]
  (reachable ?mid ?to)]]
```

### Rules with Predicates

Rules can filter results using predicates:

```edn
[[(adult ?person)
  [?person :person/age ?age]
  [(>= ?age 18)]]

 [(senior ?person)
  [?person :person/age ?age]
  [(>= ?age 65)]]]
```

### Rules with Arithmetic Functions

Rules can compute derived values:

```edn
[[(discounted-price ?product ?final-price)
  [?product :product/price ?original]
  [(* ?original 0.9) ?final-price]]

 [(total-with-tax ?subtotal ?total)
  [(* ?subtotal 1.08) ?total]]]
```
</invoke>

---

## Ordering and Pagination

### :order

Sort results by one or more variables:

```edn
[:find ?name ?age
 :where [?e :person/name ?name] [?e :person/age ?age]
 :order (asc ?name)]
```

```edn
:order (desc ?age) (asc ?name)
```

Directions:
- `(asc ?var)` -- ascending (default)
- `(desc ?var)` -- descending

### :limit

Maximum number of results:

```edn
[:find ?name
 :where [?e :person/name ?name]
 :limit 10]
```

### :offset

Skip results:

```edn
[:find ?name
 :where [?e :person/name ?name]
 :order (asc ?name)
 :limit 10
 :offset 20]
```

### :with

Include additional variables for distinctness without including them in
results:

```edn
[:find ?name (count ?tag)
 :with ?e
 :where
 [?e :person/name ?name]
 [?e :person/tag ?tag]]
```

Without `:with ?e`, if two entities have the same name and tag, they would
be deduplicated. `:with ?e` ensures each entity is counted separately.

---

## Temporal Queries

Temporal queries are specified via the `inputs` parameter rather than in the
Datalog query itself.

### As-Of

Query the database as it was at transaction `t`:

```sql
SELECT mentat_query(
  '[:find ?name :where [?e :person/name ?name]]',
  '{"asOf": 1000005}'::jsonb
);
```

Only datoms with `tx <= 1000005` are visible.

### Since

Query only datoms added after transaction `t`:

```sql
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{"since": 1000003}'::jsonb
);
```

Only datoms with `tx > 1000003` are visible.

### History

Query the full history including retracted datoms:

```sql
SELECT mentat_query(
  '[:find ?e ?name ?tx ?added
    :where [?e :person/name ?name ?tx ?added]]',
  '{"history": true}'::jsonb
);
```

In history mode, the 4th and 5th pattern positions (`?tx` and `?added`)
become meaningful. `?added` is `true` for assertions and `false` for
retractions.

---

## EDN Value Types in Queries

| EDN Literal             | Type     | Example                     |
|-------------------------|----------|-----------------------------|
| `42`                    | long     | `[?e :age 42]`             |
| `3.14`                  | double   | `[?e :score 3.14]`         |
| `"hello"`               | string   | `[?e :name "hello"]`       |
| `true` / `false`        | boolean  | `[?e :active true]`        |
| `:keyword`              | keyword  | `[?e :type :admin]`        |
| `:ns/keyword`           | keyword  | `[?e :db/ident :person/name]` |
| `#inst "2025-01-15T00:00:00Z"` | instant | `[?e :created-at #inst "..."]` |
| `#uuid "550e8400-..."`  | uuid     | `[?e :id #uuid "..."]`     |

---

## Common Query Patterns

### Find entity by unique attribute

```edn
[:find ?e .
 :where [?e :person/email "alice@example.com"]]
```

### Find all values of an attribute

```edn
[:find [?name ...]
 :where [_ :person/name ?name]]
```

### Count entities

```edn
[:find (count ?e) .
 :where [?e :person/name]]
```

### Join across entities

```edn
[:find ?person-name ?dept-name
 :where
 [?p :person/name ?person-name]
 [?p :person/department ?d]
 [?d :department/name ?dept-name]]
```

### Find entities missing an attribute

```edn
[:find ?name
 :where
 [?e :person/name ?name]
 (not [?e :person/email])]
```

### Parameterized query

```edn
[:find ?name
 :in ?dept-name
 :where
 [?d :department/name ?dept-name]
 [?e :person/department ?d]
 [?e :person/name ?name]]
```

### Self-join (same attribute, different values)

```edn
[:find ?name1 ?name2
 :where
 [?e1 :person/department ?d]
 [?e2 :person/department ?d]
 [?e1 :person/name ?name1]
 [?e2 :person/name ?name2]
 [(!= ?e1 ?e2)]]
```

### Transaction metadata

```edn
[:find ?e ?name ?tx-instant
 :where
 [?e :person/name ?name ?tx]
 [?tx :db/txInstant ?tx-instant]]
```

---

## Differences from Datomic

pg_mentat aims for compatibility with Datomic's query language. Notable
differences:

| Feature                        | pg_mentat                                   | Datomic                         |
|--------------------------------|---------------------------------------------|---------------------------------|
| Storage                        | PostgreSQL tables                           | Custom storage                  |
| Temporal queries               | Via `inputs` JSON parameter                 | Via database value functions    |
| Full-text search               | PostgreSQL tsvector/GIN                     | Lucene                          |
| Multiple databases in query    | Not yet supported                           | Supported via data sources      |
| Excision                       | Not yet supported                           | Supported                       |
| Custom aggregates              | Not yet supported                           | Supported                       |

---

## See Also

- [PostgreSQL Functions Reference](./POSTGRESQL_FUNCTIONS.md) -- `mentat_query` function details
- [mentatd Protocol Reference](./MENTATD_PROTOCOL.md) -- HTTP API for queries
