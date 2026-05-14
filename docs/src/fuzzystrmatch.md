# Phonetic and Edit-Distance Functions via fuzzystrmatch

`pg_mentat` integrates with the built-in PostgreSQL contrib extension
[`fuzzystrmatch`][fz] to expose four scalar phonetic and edit-distance
functions as Datalog where-fns. Unlike the more powerful pg_tre
integration (which requires PG18+ and `shared_preload_libraries`),
fuzzystrmatch is **`contrib`** — already on disk in any standard
PostgreSQL build from PG13 onward, with no preload required and no
restart needed.

[fz]: https://www.postgresql.org/docs/current/fuzzystrmatch.html

`fuzzystrmatch` is an **optional** dependency. If it is not installed
in the database, queries that use these where-fns fail at execution
with the standard PostgreSQL error
"function levenshtein(...) does not exist". Use
`mentat.has_fuzzystrmatch()` to detect availability.

## When to use this vs (fuzzy-match ...)

| | `fuzzystrmatch` (this page) | `pg_tre` ([fuzzy-search](./fuzzy-search.md)) |
|---|:---|:---|
| PostgreSQL versions | 13+ | 18+ |
| `shared_preload_libraries` | not required | required |
| Restart to enable | no | yes |
| Index-backed | no — scalar function per row | yes — three-tier filter funnel |
| Use case | Per-row name matching, predicates over distance | Bulk approximate-regex search |
| Result quality | Levenshtein distance is exact | Edit-distance regex with TRE semantics |

For a thousand-row table, fuzzystrmatch is fine. For a million-row
table where you want "rows whose body matches this regex with up to k
typos," use pg_tre.

## Quick start

```sql
CREATE EXTENSION pg_mentat;
CREATE EXTENSION fuzzystrmatch;          -- contrib, ships with PG

-- Define a string attribute as usual.
SELECT mentat.t('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

SELECT mentat.t('[
  {:db/id "a" :person/name "Alice"}
  {:db/id "b" :person/name "Alyce"}
  {:db/id "c" :person/name "Robert"}
]');

-- Levenshtein: edit distance from each name to "Alice".
SELECT mentat.q('[:find ?n ?d
                  :where [?e :person/name ?n]
                         [(levenshtein ?n "Alice") ?d]]');
-- [["Alice", 0], ["Alyce", 1], ["Robert", 6]]

-- Soundex: groups homophones. "Alice" and "Alyce" both hash to "A420".
SELECT mentat.q('[:find ?n ?h
                  :where [?e :person/name ?n]
                         [(soundex ?n) ?h]]');
-- [["Alice", "A420"], ["Alyce", "A420"], ["Robert", "R163"]]
```

## Where-fns

### `[(levenshtein ?a ?b) ?d]`

Edit distance between two text values. Both args may be variables or
text constants. Result is an integer.

```clojure
[(levenshtein ?name "Alice") ?d]   ;; ?d = distance from ?name to "Alice"
[(levenshtein ?a ?b) ?d]           ;; ?d = distance between two bound vars
```

### `[(soundex ?s) ?code]`

The classic 4-character American Soundex code for the input. Useful
for grouping name homophones.

```clojure
[(soundex ?surname) ?sx]
```

### `[(metaphone ?s ?max) ?code]`

Metaphone phonetic encoding, truncated to `?max` characters. Better
than Soundex for non-Anglo names. `?max` must be an integer literal.

```clojure
[(metaphone ?surname 5) ?mp]
```

### `[(daitch-mokotoff ?s) ?codes]`

Daitch-Mokotoff Soundex — better for Slavic and Yiddish surnames.
Returns a `text[]` of codes; the binding receives a Postgres text
representation like `"{367460}"` or `"{394000,367460}"`.

```clojure
[(daitch-mokotoff ?surname) ?dm]
```

## Helper

`mentat.has_fuzzystrmatch() → boolean` returns true when the contrib
extension is loaded. Use it as a guard in test suites and
application code that needs to fall back gracefully when the extension
isn't available.

## Composing with predicates and other where-fns

Where-fn output variables can flow into other where-fns:

```clojure
:where [?e :person/name ?n]
       [(levenshtein ?n "Alice") ?d]
       [(* ?d 2) ?da]               ;; arithmetic chain works
```

There is a known limitation: where-fn output variables (`?d`, `?da`,
etc.) are **not yet** usable inside `(< ...)`-style predicates. This
is a pre-existing gap in the predicate compiler that affects all
where-fn bindings (arithmetic too), not just fuzzystrmatch's. As a
workaround, use a wrapping query that filters at the SQL level, or
do the comparison entirely with arithmetic where-fns:

```clojure
;; Instead of [(<= ?d 2)], do:
[(- 3 ?d) ?diff]                    ;; ?diff > 0 iff ?d < 3
```

Tracked as a future query-compiler improvement; pull requests welcome.

## Errors

| Error | Cause | Fix |
|:---|:---|:---|
| `function levenshtein(...) does not exist` | fuzzystrmatch not installed in this database. | `CREATE EXTENSION fuzzystrmatch;` |
| `:db.error/fn-arity levenshtein requires exactly 2 text arguments` | Wrong argument count. | Pass exactly the documented arity. |
| `:db.error/fn-arg fuzzystrmatch 'soundex' arguments must be text variables or string constants.` | Passed a non-text constant (e.g. an int). | Use a string. |
| `:db.error/fn-binding fuzzystrmatch function 'levenshtein' requires a scalar binding.` | Used `[[?d]]` or similar relation/tuple form. | Bind to a single variable: `?d`. |

## What this does NOT give you

- **Soundex / Metaphone variants other than the four above.** The
  contrib extension itself only ships those.
- **Trigram similarity.** That's `pg_trgm` — see [pg_trgm
  integration](./pg-trgm.md).
- **Edit-distance with regex semantics.** That's `pg_tre` — see
  [fuzzy-search](./fuzzy-search.md).
- **Index acceleration.** These are scalar functions; there is no
  index for "names within 2 edits of X." For that workload use
  pg_tre or a precomputed Soundex column.
