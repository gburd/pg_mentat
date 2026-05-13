# Approximate-Regex Search via pg_tre (optional)

`pg_mentat` integrates with [pg_tre], a PostgreSQL 18+ index access method
that adds approximate-regex matching with configurable edit distance to
PostgreSQL. Once pg_tre is installed and you have built a TRE index on a
`:db.type/string` attribute, you can run Datalog queries that find values
matching a regex with up to *k* typos per sub-expression in
sub-millisecond time.

[pg_tre]: https://codeberg.org/gregburd/pg_tre

`pg_tre` is an **optional** dependency. Nothing in `pg_mentat` requires it
to be installed. If you do not need approximate-regex search, ignore this
page; the `(fulltext ...)` where-fn (backed by `tsvector`/GIN) and
`LIKE`/`ILIKE` predicates remain available without any extra setup.

## When to use this

Use `(fuzzy-match ...)`:

- Your text values come from typo-prone sources: user names, log lines,
  scanned-document OCR, manual data entry.
- You want **edit-distance** semantics, not just trigram similarity.
  `pg_trgm` scores `database` and `databse` as similar; pg_tre asserts
  the edit distance is exactly 1.
- You need `{~k}` per-phrase edit budgets, e.g.
  `(error){~1}.*(42[0-9]){~0}` — "the word *error* with up to 1 typo
  followed by a 3-digit number starting with 42, exactly".

Use `(fulltext ...)` instead if you want exact-token full-text search
with stemming and ranking.

## Prerequisites

- PostgreSQL 18 or newer.
- pg_tre built and installed against the same `pg_config`:
  ```sh
  git clone --recurse-submodules https://codeberg.org/gregburd/pg_tre
  cd pg_tre
  make PG_CONFIG=/path/to/pg18/bin/pg_config
  make PG_CONFIG=/path/to/pg18/bin/pg_config install
  ```
- `shared_preload_libraries = 'pg_tre'` in `postgresql.conf`. Restart the
  server after editing.
- `CREATE EXTENSION pg_tre;` in the database where pg_mentat lives.

## Step-by-step example

```sql
CREATE EXTENSION pg_mentat;
CREATE EXTENSION pg_tre;

-- Define a string attribute as usual.
SELECT mentat.t('[
  {:db/ident :doc/body
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Insert some data with deliberate typos.
SELECT mentat.t('[
  {:db/id "d1" :doc/body "the database error happens at scale"}
  {:db/id "d2" :doc/body "the databse error happens at scale"}
  {:db/id "d3" :doc/body "an unrelated row about cats"}
  {:db/id "d4" :doc/body "datbase error in production"}
]');

-- Build a TRE index on this attribute. Idempotent; safe to re-run.
SELECT mentat.create_tre_index(':doc/body');
-- -> "TRE index idx_datoms_text_new_tre_a10000 created on
--    mentat.datoms_text_new for attribute :doc/body (entid 10000).
--    Use (fuzzy-match $ :doc/body \"pattern\" k) to query."

-- Exact match (k = 0) returns only "database".
SELECT mentat.q('[:find ?val
                  :where [(fuzzy-match $ :doc/body "database" 0) [[?e ?val]]]]');
-- -> [["the database error happens at scale"]]

-- One typo (k = 1) returns the canonical row plus "databse" and "datbase".
SELECT mentat.q('[:find ?val
                  :where [(fuzzy-match $ :doc/body "database" 1) [[?e ?val]]]]');
-- -> [["the database error happens at scale"],
--     ["the databse error happens at scale"],
--     ["datbase error in production"]]
```

## The `(fuzzy-match ...)` where-fn

Syntax:

```
(fuzzy-match $ :attr "pattern" k)
```

| Position | Meaning |
|---:|:---|
| `$` | source database; reserved as in `(fulltext ...)`. |
| `:attr` | a `:db.type/string` attribute keyword. |
| `"pattern"` | a TRE regex string. Plain literals work; you can also use TRE's `{~k}` per-sub-expression edit budgets. |
| `k` | overall edit budget, 0–8. Larger values are rejected to bound regex compilation cost. |

Binding shape (relation):

```
[(fuzzy-match $ :attr "pat" k) [[?e ?val]]]
```

- `?e` is the entity id (cast to text in the result row).
- `?val` is the matched string value.

The compiled SQL filters on attribute, store, and `added = TRUE`, then
applies pg_tre's `%~~` operator. If the attribute has a TRE index,
the planner picks it; otherwise pg_tre's heap-recheck path runs.

## Helper functions

| Function | Purpose |
|:---|:---|
| `mentat.has_pg_tre()` → `boolean` | True if pg_tre is installed in this database. |
| `mentat.create_tre_index('<:attr>')` | Build a TRE index on the named string attribute. Idempotent. |
| `mentat.drop_tre_index('<:attr>')` | Remove the TRE index for the named attribute. Idempotent. |

The index is partial: `WHERE store_id = 0 AND a = <attr_entid> AND added`.
That keeps it small (only live datoms of the chosen attribute) and lets
the planner use it for `mentat_query` calls automatically.

## Errors and how to fix them

| Error | Cause | Fix |
|:---|:---|:---|
| `:db.error/missing-extension pg_tre is not installed in this database.` | `mentat.create_tre_index()` called without pg_tre. | Install pg_tre and `CREATE EXTENSION pg_tre;`. |
| `:db.error/attribute-not-found Attribute ':foo/bar' is not defined in mentat.schema.` | Attribute does not exist yet. | Define it first with a `mentat.t([...])` schema transaction. |
| `:db.error/wrong-type-for-tre-index Attribute ':foo/age' has value type 'long' but pg_tre indexes only :db.type/string.` | Attribute is not a string. | Either change the attribute type, or use `(< ... )` predicates / a `:db/index` for non-text attrs. |
| `:db.error/fn-arity fuzzy-match requires exactly 4 arguments` | Wrong arg count. | Pass `($ :attr "pattern" k)` exactly. |
| `:db.error/fn-arg fuzzy-match k must be in [0, 8], got N` | k out of range. | Use a smaller k, or pre-filter with `(fulltext ...)` first. |

## What this does NOT give you

- **Relevance scoring.** pg_tre's `%~~` is a boolean operator. Use
  `(fulltext ...)` if you need ranked results.
- **PG13–17 support.** pg_tre is PG18-only. If you target older
  PostgreSQL versions, `(fuzzy-match ...)` will fail with
  `tre_pattern does not exist` even at runtime.
- **Multi-store scoping.** `mentat.create_tre_index` builds the partial
  index against `store_id = 0`. Other stores work via the heap-recheck
  fallback (correct, but no index speedup). Multi-store TRE indexes
  are tracked as future work.

## Caveats

- The TRE index is a partial index per-attribute. If you index N
  attributes, you get N indexes and N corresponding storage costs.
  Index only what you actually query.
- `pg_tre` requires `shared_preload_libraries`, which means a server
  restart to enable. Plan accordingly.
- pg_tre and pg_mentat are independently versioned. The integration in
  this version of pg_mentat targets pg_tre 1.0.0+. Newer pg_tre
  versions may add operators (e.g. score-returning variants) that
  pg_mentat does not yet expose; please open an issue.
