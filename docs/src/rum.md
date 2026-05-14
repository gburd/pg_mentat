# BM25-style Ranked Search via rum

PostgreSQL's stock GIN-indexed `tsvector @@ tsquery` is great for
filtering, but to *rank* matches by relevance the planner has to do a
post-fetch scan and call `ts_rank_cd` per row. This is
fine for tens of matches, painful for millions.

[`rum`][rum] is a PostgreSQL-licensed (permissive!) GIN-derived index
access method from PostgresPro that stores positional information
alongside lexemes. Combined with the `<=>` distance operator and
`rum_ts_score()` ranking function, it returns top-K relevance-ordered
documents *directly from the index*. It is the closest permissive
alternative to BM25 indexing in PostgreSQL — pg_mentat's choice over
the AGPL-licensed [`pg_search`][parade] (ParadeDB).

[rum]: https://github.com/postgrespro/rum
[parade]: https://github.com/paradedb/paradedb

`rum` is an **optional** dependency. The `(rum-fulltext ...)`
where-fn produces SQL that uses standard `@@` for filtering and
`rum_ts_score(...)` for ranking — both work without the rum extension
installed (against a sequential scan), but the partial RUM index is
where the speed-up lives.

## When to use this vs other fulltext options

| | `rum-fulltext` (this page) | `(fulltext ...)` (stock GIN) | `(similar-to ...)` ([pg_trgm](./pg-trgm.md)) |
|---|:---|:---|:---|
| Algorithm | Lexeme + position match | Lexeme match only | Trigram overlap |
| Index | rum (partial, per-attr) | gin_tsvector_ops | gin_trgm_ops |
| Ranking | `rum_ts_score` (positional) | `ts_rank_cd` (no positions) | `similarity` (0..1) |
| Top-K from index | yes, via `<=>` | no, post-fetch | no, post-fetch |
| Phrase search | yes | yes (`phraseto_tsquery`) | substring-ish only |
| License | PostgreSQL | core PG | PostgreSQL (contrib) |

If you don't have rum installed and the dataset is < 100k rows, stay
on `(fulltext ...)`. Above that scale, install rum and switch to
`(rum-fulltext ...)`.

## Quick start

```bash
# Build rum from source (one-time, ~30 sec).
git clone https://github.com/postgrespro/rum
cd rum
make USE_PGXS=1 PG_CONFIG=/path/to/your/pg_config
make USE_PGXS=1 PG_CONFIG=/path/to/your/pg_config install
```

```sql
CREATE EXTENSION pg_mentat;
CREATE EXTENSION rum;

-- Define a fulltext-tagged attribute.
SELECT mentat.t('[
  {:db/ident :issue/body
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/fulltext true}
]');

-- Create a partial RUM index on this attribute (idempotent).
SELECT mentat.create_rum_fulltext_index(':issue/body');
-- => 'datoms_text_rum_<entid>_english'

-- Top-K ranked search.
SELECT mentat.q('[
  :find ?body ?score
  :where [(rum-fulltext $ :issue/body "database crash") [[?e ?body ?score]]]
  :order (desc ?score)
]');
```

## The `rum-fulltext` where-fn

```clojure
[(rum-fulltext $ <:attr> <"search-text">) [[?e ?val ?score]]]
```

| Position | Type | Notes |
|:---:|:---|:---|
| 1 | `$` | Source var. Required for symmetry. |
| 2 | keyword | Attribute. Must be `:db.type/string` and ideally `:db/fulltext true`. |
| 3 | string literal | Search text. Wrap in `"..."` for phrase search via `phraseto_tsquery`. |

Binding shape `[[?e ?val ?score]]` (relation):
- `?e` — entid of each matching datom.
- `?val` — the stored text value.
- `?score` — `rum_ts_score(...)` in `[0.0, 1.0]`-ish range. Higher = more relevant.

Plain text uses `plainto_tsquery`; double-quoted text uses
`phraseto_tsquery` for proximity matching.

## Index helpers

```sql
-- Idempotent. Returns the deterministic index name.
SELECT mentat.create_rum_fulltext_index(':issue/body');
-- => 'datoms_text_rum_<entid>_english'

-- Same attribute, different language config.
SELECT mentat.create_rum_fulltext_index(':issue/body', 'spanish');
-- => 'datoms_text_rum_<entid>_spanish'

-- Drop. Returns true if the index existed.
SELECT mentat.drop_rum_fulltext_index(':issue/body');
SELECT mentat.drop_rum_fulltext_index(':issue/body', 'spanish');
```

The index DDL is partial:

```sql
CREATE INDEX datoms_text_rum_<entid>_<lang>
    ON mentat.datoms_text_new
    USING rum (to_tsvector('<lang>', v) rum_tsvector_ops)
    WHERE a = <entid> AND added = true;
```

The `WHERE a = <entid> AND added = true` clause keeps the index
small even in workspaces with many string attributes, and excludes
retracted datoms automatically.

## How rum's ranking differs from `ts_rank_cd`

`ts_rank_cd` (cover density) only knows lexeme presence and document
length. `rum_ts_score` *also* knows lexeme positions, so it ranks:

- Phrase matches higher than disjoint matches of the same lexemes.
- Closer co-occurrences higher than distant ones.
- Documents where query lexemes appear early higher than late.

This is closer in spirit to BM25's term-frequency × inverse-document-
frequency × proximity scoring than `ts_rank_cd` is, though it is not
algorithmically identical to BM25.

## Errors

| Error | Cause | Fix |
|:---|:---|:---|
| `function rum_ts_score(...) does not exist` | rum not installed in this database. | Install rum (see Quick start), then `CREATE EXTENSION rum;` |
| `:db.error/fn-arity rum-fulltext requires at least 3 arguments` | Wrong arg count. | Pass `($ :attr "text")`. |
| `:db.error/fn-arg rum-fulltext second argument must be a keyword attribute` | Attribute is missing or not a keyword. | Use `:attr/ident` form. |
| `:db.error/missing-extension rum is not installed in this database` | Calling `create_rum_fulltext_index` before installing rum. | `CREATE EXTENSION rum;` |
| `:db.error/unknown-attribute Attribute :foo/bar is not registered` | Index helper for an unregistered attribute. | Transact the schema first. |

## Worked example: bug-tracker search

```clojure
(d/q '[:find ?title ?score
       :where
         [(rum-fulltext $ :issue/title "memory leak") [[?e ?title ?score]]]
         [?e :issue/status :status/open]
         [?e :issue/priority :priority/high]
       :order (desc ?score)
       :limit 20]
     db)
```

Compiles to a query plan that:

1. Hits the partial RUM index on `:issue/title` for ranked top matches.
2. Joins the result back to the `:issue/status` and `:issue/priority`
   attribute datoms via the entid.
3. Returns the top 20 by descending `rum_ts_score`.

If the RUM index is sized appropriately and the status/priority datoms
are also indexed, the entire pipeline is index-driven — no sequential
scan over the issue body text.

## License caveat

ParadeDB's `pg_search` is **AGPL-3.0**, which makes it unsuitable for
many commercial deployments without source-disclosure. `rum` is
**PostgreSQL** license — same terms as PostgreSQL itself. If you ship
a SaaS or proprietary product, rum is the only credible permissive
choice for index-backed BM25-style ranking in PostgreSQL today.

## What this does NOT give you

- **True BM25.** rum's score is positional + length-aware but is
  not the algorithm Lucene/Tantivy implement. Close, not identical.
- **Faceted search, aggregations, learned ranking.** That's
  Elasticsearch/Tantivy territory.
- **Cross-attribute ranking in one call.** Each `rum-fulltext` binds
  one attribute. Combine with OR for multi-attribute search; the
  resulting plan does index lookups per attribute then unions.
- **Fuzzy / typo-tolerant matching.** Use [pg_trgm's `(similar-to)`](./pg-trgm.md)
  or [pg_tre's `(fuzzy-match)`](./fuzzy-search.md) for that.
