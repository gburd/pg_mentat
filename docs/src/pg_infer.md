# Model-Knowledge Search via pg_infer

`pg_mentat` integrates with [`pg_infer`][pginfer], an experimental
PostgreSQL extension that exposes transformer model knowledge as
SQL relations. With pg_infer installed and a model registered,
pg_mentat queries can rank text by what the model "knows" — without
running inference at query time and without precomputed embeddings.

[pginfer]: https://codeberg.org/gregburd/pg_infer

`pg_infer` is **experimental** (alpha, PG18+, may break between
releases). Treat this integration as the same kind of contract:
expect breakage when pg_infer's SQL surface evolves.

It is an **optional** dependency. Detect with
`mentat.has_pg_infer()`. The Datalog where-fns produce SQL that
calls pg_infer's operators directly; without pg_infer installed,
queries fail at execution with the standard PG "operator/function
does not exist" error.

## How it differs from pgvector / pg_trgm / rum

| | `pg_infer` (this page) | `pgvector` | `pg_trgm` | `rum` |
|---|:---|:---|:---|:---|
| What's similar? | Model "knowledge" | Vector arithmetic | Trigram overlap | Lexeme + position |
| Precompute step | One-time vindex extraction | Per-row embedding | None | None |
| Runtime cost | mmap'd weights | Dot product | Trigram set ops | Index lookup |
| Discovers semantic links text doesn't expose | yes | yes | no | no |
| PG version | 18+ | 13+ | 13+ | 13+ |

Use pg_infer when you need to find that
"AutoML for Deep Networks" matches the query
"neural architecture search" because the model learned that
relationship — even though no keywords overlap and you haven't
embedded anything.

## Quick start

```sh
# Build pg_infer (requires Rust nightly + pgrx 0.17 + PG18+; see
# https://codeberg.org/gregburd/pg_infer for instructions).
git clone https://codeberg.org/gregburd/pg_infer
cd pg_infer
cargo pgrx install --release
```

```sql
CREATE EXTENSION pg_mentat;
CREATE EXTENSION pg_infer;

-- Register a model from a vindex artifact.
SELECT infer_create_model('qwen05b', '/data/qwen-0.5b.vindex');

-- (Optional) make it the default for queries that don't pass a model.
SET infer.default_model = 'qwen05b';

-- Define a string attribute.
SELECT mentat.t('[
  {:db/ident :paper/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
]');

SELECT mentat.t('[
  {:db/id "p1" :paper/title "Efficient Neural Architecture Search"}
  {:db/id "p2" :paper/title "AutoML for Deep Networks"}
  {:db/id "p3" :paper/title "Cookies are good"}
]');

-- Build the partial pg_infer index keyed by attribute.
SELECT mentat.create_infer_index(':paper/title', 'qwen05b');

-- Top-2 nearest by model-knowledge distance.
SELECT mentat.q('[
  :find ?title ?dist
  :where [(infer-near $ :paper/title "neural architecture search" 2) [[?e ?title-shadow ?dist]]]
         [?e :paper/title ?title]
  :order (asc ?dist)
]');
-- => Both AutoML and ENAS papers, even though "AutoML" shares no
-- keywords with "neural architecture search".
```

## Where-fns

### `[(infer-near $ <:attr> <"text"> <k> [<:model>]) [[?e ?dist]]]`

Top-K nearest neighbors by model-knowledge distance, using
pg_infer's `<~>` operator + `ORDER BY ... LIMIT k` for
index-driven retrieval.

| Position | Type | Notes |
|:---:|:---|:---|
| 1 | `$` | Source var. |
| 2 | keyword | Attribute (must be `:db.type/string`). |
| 3 | string literal | Query text. |
| 4 | int literal | Top-K. Must be > 0. |
| 5 (optional) | keyword | Model name as keyword (e.g. `:qwen05b`). Today this is **accepted but not yet routed** \u2014 the SQL emit uses pg_infer's session GUC `infer.default_model`. To pin a model per query, set the GUC before the query. |

Binding `[[?e ?dist]]`:
- `?e` \u2014 entid of each near neighbor.
- `?dist` \u2014 model-knowledge distance (lower = more similar).

The `infer-near` subquery applies LIMIT inside, so exactly K rows
are returned before joining to subsequent patterns. `?e` is also
exposed for downstream EAV joins via the FTS-join entity-binding
fix landed alongside the pgvector integration.

### `[(infer-similar a b [<:model>]) ?score]`

Scalar similarity between two text values via pg_infer's
`infer_similarity(text, text)` function. Higher = more similar.

```clojure
[(infer-similar ?title "France") ?s]
[(infer-similar "Paris" "France") ?s]
```

Today the optional model arg is accepted syntactically but ignored
\u2014 pg_infer's `infer_similarity` is documented as 2-arg only, with
model selected via the `infer.default_model` GUC. A future pg_infer
release that adds a 3-arg form will be picked up here without an
API change.

### `[(infer-implies a b [<:model>]) ?bool]`

Test whether the model's knowledge supports a directional
relationship from subject `a` to object `b`. Returns 0 or 1
(not boolean \u2014 pg_mentat's scalar-binding return path needs an
integer-shaped value).

```clojure
[(infer-implies "France" "Paris") ?ok]    ;; ?ok = 1 if implies holds
[(infer-implies ?title "AI") ?ok]
```

## Index helpers

```sql
SELECT mentat.create_infer_index(':paper/title', 'qwen05b');
-- => 'datoms_text_infer_<entid>_qwen05b'
```

Idempotent. Creates a partial index using the default `infer_text_ops`
opclass:

```sql
CREATE INDEX datoms_text_infer_<entid>_<model>
    ON mentat.datoms_text_new
    USING infer (v) WITH (model = '<model>')
    WHERE a = <entid> AND added = true;
```

Drop with:

```sql
SELECT mentat.drop_infer_index(':paper/title', 'qwen05b');
-- => true if dropped, false otherwise
```

## Combined-search pattern

pg_infer composes with pg_trgm, pgvector, and rum in a single
Datalog query. The classic multi-signal ranking pattern:

```clojure
:find ?title ?score
:in $ ?query
:where
  [(infer-near $ :paper/title ?query 100) [[?e ?title-shadow ?infer-d]]]
  [?e :paper/title ?title]
  [(similar-to $ :paper/title ?query 0.2) [[?e ?title-shadow2 ?trgm]]]
  [(rum-fulltext $ :paper/body ?query) [[?e ?body ?ts-rank]]]
  [(* (- 1 ?infer-d) 0.4) ?part1]      ;; via where-fn arithmetic
  [(* ?trgm 0.2) ?part2]
  [(* ?ts-rank 0.4) ?part3]
  [(+ ?part1 ?part2) ?p12]
  [(+ ?p12 ?part3) ?score]
:order (desc ?score)
:limit 20
```

Each signal contributes orthogonal information: pg_infer finds
semantic relationships the other tools can't discover; pg_trgm
catches typos; rum ranks by lexeme position. The final score is
just a weighted sum of the three.

## Errors

| Error | Cause | Fix |
|:---|:---|:---|
| `function infer_similarity(...) does not exist` | pg_infer not installed. | `CREATE EXTENSION pg_infer;` (PG18+). |
| `operator does not exist: text <~> text` | pg_infer not installed. | Same. |
| `:db.error/missing-extension pg_infer is not installed in this database` | Calling helper before `CREATE EXTENSION pg_infer`. | Install pg_infer. |
| `:db.error/unknown-attribute Attribute :foo/bar is not registered` | Index helper for an unregistered attribute. | Transact the schema first. |
| `:db.error/fn-arity infer-near requires 4 or 5 arguments` | Wrong arg count. | Pass `($ :attr "text" k)` with optional `:model`. |
| `:db.error/fn-arg infer-near k must be > 0, got 0` | K not positive. | Pass a positive integer. |
| `:db.error/fn-arg pg_infer 'infer-similar' arguments must be text variables or string constants` | Numeric or boolean arg. | Use only text variables / string constants. |
| `model "..." not found` (from pg_infer) | Used a model name that wasn't registered via `infer_create_model`. | Register first, or use a model that's already registered. |

## What this does NOT (yet) give you

- **Per-query model override threading.** The `:model` keyword arg
  is parsed but doesn't yet rewrite the SQL to inline the model.
  Set `infer.default_model` GUC at session/transaction scope.
- **The `walk()` / `describe()` tabular outputs.** Those return
  multi-column tables; pg_mentat where-fns are scalar / relation
  shaped today. To use them, drop into raw SQL alongside Datalog.
- **Index-only `(infer-implies)`.** The implies operator `@>` is
  not in `infer_text_ops`'s default opclass; queries using
  `(infer-implies)` are sequential scans regardless of index.
- **CI happy-path tests.** pg_infer is experimental and not in any
  managed-Postgres apt repo today. The pg_mentat test suite
  exercises every negative path (arity, arg type, missing
  extension, unknown attribute) on every CI run, but the e2e
  happy path tests skip unless pg_infer is installed in the test
  cluster.

## See also

- [pg_infer README](https://codeberg.org/gregburd/pg_infer) for the
  underlying SQL surface, model registration, and vindex extraction.
- [Vector Search via pgvector](./pgvector.md) \u2014 the embedding-based
  cousin of `infer-near`.
- [Trigram Similarity via pg_trgm](./pg-trgm.md) and
  [BM25-style Ranked Search via rum](./rum.md) for the
  text-side complements.
