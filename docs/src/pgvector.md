# Vector Search via pgvector

`pg_mentat` integrates with [`pgvector`][pgv], the standard PostgreSQL
vector-similarity extension. Use this for semantic search, embedding-based
recommendations, or any workload where you need K-nearest-neighbor
lookups by cosine / L2 / inner-product distance.

[pgv]: https://github.com/pgvector/pgvector

`pgvector` is an **optional** dependency. Detect with
`mentat.has_pgvector()`. The integration is a soft, side-table design:
vectors live in per-attribute auxiliary tables — `pg_mentat` does
**not** (yet) register `:db.type/vector` in the schema or accept
vectors through `mentat.t`. That bigger schema-side integration is
tracked in `docs/INTEGRATIONS.md`.

## How the side-table integration works

| Step | API |
|---|---|
| Detect availability | `SELECT mentat.has_pgvector();` |
| Attach an aux table | `SELECT mentat.attach_vector_attribute(':doc/embedding', 384);` |
| Insert / update a vector | `SELECT mentat.set_vector(?e, ':doc/embedding', '[v1,v2,...]');` |
| Delete a vector | `SELECT mentat.del_vector(?e, ':doc/embedding');` |
| KNN search | `[(vector-near $ :doc/embedding "[1,0,0]" 5) [[?e ?dist]]]` |
| Build an HNSW index | `SELECT mentat.create_hnsw_vector_index(':doc/embedding', 'cosine');` |

Each `attach_vector_attribute(:attr, dim)` call creates:

```sql
CREATE TABLE mentat.attr_<entid>_vector(
    e BIGINT PRIMARY KEY,
    v vector(<dim>) NOT NULL
);
```

Vectors are keyed by entid only. The corresponding Datalog attribute
must already be registered in the schema (any value type works — the
attribute exists in `mentat.schema` for entid lookup, but the vector
data lives separately).

## Quick start

```bash
# Build pgvector from source (one-time).
git clone --depth 1 --branch v0.7.4 https://github.com/pgvector/pgvector
cd pgvector
make USE_PGXS=1 PG_CONFIG=/path/to/pg_config
make USE_PGXS=1 PG_CONFIG=/path/to/pg_config install
```

```sql
CREATE EXTENSION pg_mentat;
CREATE EXTENSION vector;

-- Define the attribute (any string-or-long type — the data lives in the
-- aux table).
SELECT mentat.t('[
  {:db/ident :doc/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :doc/embedding :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
]');

-- Insert documents (no vectors yet).
SELECT mentat.t('[
  {:db/id "a" :doc/title "How to install Postgres"}
  {:db/id "b" :doc/title "Datalog query patterns"}
  {:db/id "c" :doc/title "Cookies are good"}
]');

-- Attach the aux table for embeddings of dimension 3.
SELECT mentat.attach_vector_attribute(':doc/embedding', 3);

-- Populate vectors via the helper (use real entids in production).
DO $do$
DECLARE e_a BIGINT; e_b BIGINT; e_c BIGINT;
BEGIN
  SELECT e INTO e_a FROM mentat.datoms_text_new
    WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':doc/title')
      AND v = 'How to install Postgres';
  SELECT e INTO e_b FROM mentat.datoms_text_new
    WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':doc/title')
      AND v = 'Datalog query patterns';
  SELECT e INTO e_c FROM mentat.datoms_text_new
    WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':doc/title')
      AND v = 'Cookies are good';
  PERFORM mentat.set_vector(e_a, ':doc/embedding', '[0.9, 0.1, 0.0]');
  PERFORM mentat.set_vector(e_b, ':doc/embedding', '[0.0, 0.9, 0.1]');
  PERFORM mentat.set_vector(e_c, ':doc/embedding', '[0.0, 0.0, 1.0]');
END;
$do$;

-- Top-2 nearest by cosine distance, joined to the title attribute.
SELECT mentat.q('[
  :find ?title ?dist
  :where [(vector-near $ :doc/embedding "[1,0,0]" 2) [[?e ?dist]]]
         [?e :doc/title ?title]
  :order (asc ?dist)
]');
-- => [["How to install Postgres", 0.0061], ["Datalog query patterns", 1.0]]
```

## The `vector-near` where-fn

```clojure
[(vector-near $ <:attr> <"[v1,v2,...]"> <k> [<distance-op>]) [[?e ?dist]]]
```

| Position | Type | Notes |
|:---:|:---|:---|
| 1 | `$` | Source var. Required for symmetry. |
| 2 | keyword | Vector-attached attribute (must call `attach_vector_attribute` first). |
| 3 | string literal | pgvector textual representation: `"[1.0, 2.0, 3.0]"`. |
| 4 | int literal | K — top-K neighbors to return. |
| 5 (optional) | keyword | Distance op: `:cosine` (default), `:l2`, `:inner`. |

Binding shape `[[?e ?dist]]` (relation):
- `?e` — entid of each near neighbor.
- `?dist` — distance (lower = closer for `:cosine` / `:l2`; lower = closer for `:inner`'s negative inner product convention).

The compiled SQL uses pgvector's distance operators directly:

| `:cosine` | `<=>` | Cosine distance, in `[0, 2]`. |
| `:l2` | `<->` | Euclidean / L2 distance. |
| `:inner` | `<#>` | Negative inner product (lower = more similar). |

The K-limit is applied **inside** the subquery, so `vector-near` returns
exactly K rows before joining to the rest of the where-clause graph.
Subsequent patterns (e.g. `[?e :doc/title ?title]`) JOIN by entid — no
cartesian-product workarounds required.

## HNSW index

```sql
SELECT mentat.create_hnsw_vector_index(':doc/embedding', 'cosine');
-- => 'attr_<entid>_vector_hnsw_cosine'
```

Idempotent. `dist_op` must be `'cosine'`, `'l2'`, or `'inner'`; the
function chooses the right pgvector opclass (`vector_cosine_ops`,
`vector_l2_ops`, `vector_ip_ops`).

The index is keyed on the aux table only — there's no partial-WHERE
trick because each attribute already has its own table. Tune
`hnsw.m` / `hnsw.ef_construction` via session GUCs in the standard
pgvector way; pg_mentat doesn't wrap those.

## Errors

| Error | Cause | Fix |
|:---|:---|:---|
| `function vector_send(...) does not exist` (or similar) | pgvector not installed in this database. | Build pgvector and `CREATE EXTENSION vector;` |
| `:db.error/missing-extension pgvector is not installed` | Calling helper before `CREATE EXTENSION vector`. | Install pgvector. |
| `:db.error/unknown-attribute vector-near attribute :foo/bar is not registered` | Attribute missing from `mentat.schema`. | Transact the schema first, then attach. |
| `relation "mentat.attr_<n>_vector" does not exist` | Attempted `set_vector` / `del_vector` / `vector-near` before `attach_vector_attribute`. | Call `attach_vector_attribute` first. |
| `:db.error/fn-arity vector-near requires 4 or 5 arguments` | Wrong arg count. | Pass `($ :attr "[...]" k)` with optional `:cosine`/`:l2`/`:inner`. |
| `:db.error/fn-arg vector-near distance op must be one of :cosine, :l2, :inner` | Unknown distance keyword. | Use one of the three. |
| `:db.error/fn-arg vector dimensionality must be in (0, 16000]` | Bad `dim` argument to `attach_vector_attribute`. | pgvector caps at 16000 dimensions. |

## Worked example: semantic document search

```clojure
;; Application populates :doc/embedding via mentat.set_vector after
;; running each document through a sentence-transformer.

(d/q '[:find ?title ?author ?dist
       :where
         [(vector-near $ :doc/embedding ?query-embedding 10) [[?d ?dist]]]
         [?d :doc/title ?title]
         [?d :doc/author ?author-eid]
         [?author-eid :user/name ?author]
       :order (asc ?dist)]
     db
     query-embedding)  ;; passed in via :in
```

Plan:

1. `vector-near` returns top-10 doc entids by cosine distance, JOINed
   from the per-attribute aux table directly via the HNSW index.
2. Three EAV joins follow the entid back to title, author entid, and
   author name.
3. Result is 10 rows ordered by ascending distance.

## What this does NOT (yet) give you

- **`:db.type/vector` schema integration.** Vectors don't transact via
  `mentat.t`. Use `mentat.set_vector` directly. A future session
  will add the schema-side integration; the aux-table representation
  this integration uses is forward-compatible with that design.
- **Variable-length vector args to `vector-near`.** The vector is a
  string literal in the EDN; passing a Datalog variable bound elsewhere
  is not supported (parameterized embedding values can be threaded via
  the `:in` clause once schema integration ships).
- **IVFFlat indexes.** Only HNSW is exposed today; `vector_*_ops`
  with IVFFlat are accessible through plain SQL `CREATE INDEX`.
- **Quantization (BIT, halfvec, sparsevec).** pgvector 0.7+ supports
  these; pg_mentat doesn't wrap them yet.
