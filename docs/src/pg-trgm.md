# Trigram Similarity via pg_trgm

`pg_mentat` integrates with [`pg_trgm`][trgm], a built-in PostgreSQL
contrib extension that provides trigram-based similarity matching.
Where `fuzzystrmatch`'s Levenshtein answers "how many edits apart?",
`pg_trgm` answers "how many overlapping 3-grams?" — better at
matching reordered tokens, partial substrings, and typos that
preserve trigrams.

[trgm]: https://www.postgresql.org/docs/current/pgtrgm.html

`pg_trgm` is an **optional** dependency. Detect with
`mentat.has_pg_trgm()`. Without it, queries that use `(similar-to ...)`
fail at execution with the standard PostgreSQL error
"function similarity(...) does not exist".

## When to use this vs other fuzzy options

| | `pg_trgm` (this page) | `fuzzystrmatch` ([page](./fuzzystrmatch.md)) | `pg_tre` ([page](./fuzzy-search.md)) |
|---|:---|:---|:---|
| Algorithm | Trigram overlap | Levenshtein / phonetic | Edit-distance regex |
| PG version | 13+ | 13+ | 18+ |
| `shared_preload_libraries` | no | no | yes |
| Score | yes (0..=1 real) | distance only | none |
| Index | GIN/GiST gin_trgm_ops | none | TRE custom AM |
| Best for | Free-text search ranking | Name homophones, dedup | Bulk regex with typos |

## Quick start

```sql
CREATE EXTENSION pg_mentat;
CREATE EXTENSION pg_trgm;          -- contrib

SELECT mentat.t('[
  {:db/ident :issue/title
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Index recommendation: partial GIN on the attribute.
SELECT mentat.create_trgm_index(':issue/title');

-- Find titles similar to "databse" with score >= 0.3.
SELECT mentat.q('[
  :find ?title ?score
  :where [(similar-to $ :issue/title "databse" 0.3) [[?e ?title ?score]]]
  :order [?score :desc]
]');
```

## The `similar-to` where-fn

```clojure
[(similar-to $ <:attr> <"needle"> <threshold>) [[?e ?val ?score]]]
```

Arguments:

| Position | Type | Notes |
|:---:|:---|:---|
| 1 | `$` | Source var. Required for parser symmetry; not used. |
| 2 | keyword | Datalog attribute (e.g. `:issue/title`). Must be `:db.type/string`. |
| 3 | string literal | The "needle" to match against. |
| 4 | float literal | Threshold in `(0.0, 1.0]`. pg_trgm's default is `0.3`. |

Binding shape `[[?e ?val ?score]]` (relation):
- `?e` — entid of each matching datom.
- `?val` — the actual stored value.
- `?score` — pg_trgm `similarity(stored, needle)` in [0.0, 1.0].

## Indexing — `mentat.create_trgm_index`

```sql
SELECT mentat.create_trgm_index(':issue/title');
-- => 'datoms_text_trgm_<entid>'
```

Creates a partial GIN trigram index keyed by the attribute's entid:

```sql
CREATE INDEX datoms_text_trgm_<entid>
    ON mentat.datoms_text_new USING GIN (v gin_trgm_ops)
    WHERE a = <entid> AND added = true;
```

The partial WHERE keeps the index small even in workspaces with
hundreds of string attributes. The function is idempotent — calling
it again with the same attribute is a no-op. To remove:

```sql
SELECT mentat.drop_trgm_index(':issue/title');
-- => true if the index existed and was dropped, false otherwise
```

The compiled SQL filters with `similarity(v, needle) >= threshold`.
PostgreSQL's planner uses the GIN index for the equivalent `v %
needle` filter when the trigram-similarity GUC matches; otherwise it
falls back to a partial-index scan plus recheck. Verify with
`EXPLAIN ANALYZE` on a representative query.

## Errors

| Error | Cause | Fix |
|:---|:---|:---|
| `function similarity(...) does not exist` | pg_trgm not installed in this database. | `CREATE EXTENSION pg_trgm;` |
| `:db.error/fn-arity similar-to requires exactly 4 arguments` | Wrong arg count. | Pass `($ :attr "needle" threshold)`. |
| `:db.error/fn-arg similar-to second argument must be a keyword attribute` | Passed a string or var as attr. | Use `:attr/ident` form. |
| `:db.error/fn-arg similar-to threshold must be in (0.0, 1.0]` | Threshold ≤ 0 or > 1. | Pass a float in `(0.0, 1.0]`. pg_trgm default is `0.3`. |
| `:db.error/missing-extension pg_trgm is not installed in this database` | Calling `mentat.create_trgm_index` without pg_trgm. | `CREATE EXTENSION pg_trgm;` |
| `:db.error/unknown-attribute Attribute :foo/bar is not registered` | `mentat.create_trgm_index(':foo/bar')` for an unregistered attr. | Transact the schema first. |

## Worked example: dedupe near-duplicate names

```clojure
(def people
  [{:db/id "a" :p/name "Alice Henderson"}
   {:db/id "b" :p/name "Alyce Henderson"}
   {:db/id "c" :p/name "Alice Hendersn"}
   {:db/id "d" :p/name "Bob Smith"}])

(d/q '[:find ?name ?score
       :where [(similar-to $ :p/name "Alice Henderson" 0.5) [[?e ?name ?score]]]
       :order [?score :desc]]
     db)
;; =>
;; [["Alice Henderson"  1.0]
;;  ["Alyce Henderson"  0.6875]
;;  ["Alice Hendersn"   0.6363...]]
;; "Bob Smith" filtered out.
```

## Composing with the rest of Datalog

`similar-to` plays the same role as `fulltext` and `fuzzy-match` in a
clause: the bound `?e` joins back into the rest of the where-graph.

```clojure
:where
  [(similar-to $ :issue/title "databse" 0.3) [[?issue ?title ?score]]]
  [?issue :issue/status :status/open]            ; restrict to open
  [?issue :issue/assignee ?dev]
  [?dev :user/name ?dev-name]
:find ?title ?dev-name ?score
:order [?score :desc]
```

## What this does NOT give you

- **Tokenization or stop-word handling.** That's `to_tsvector` /
  `tsquery` territory — see [Postgres full-text search](./fulltext.md).
- **Phonetic matching.** That's [fuzzystrmatch](./fuzzystrmatch.md).
- **Approximate-regex with bounded edits.** That's [pg_tre](./fuzzy-search.md).
- **Cross-attribute similarity in one call.** Each `similar-to`
  binds one attribute; combine with OR clauses to search across
  attributes.
