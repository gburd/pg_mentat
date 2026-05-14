# Property Graph Queries via PG19 SQL/PGQ

PostgreSQL 19 adds [SQL/PGQ][pgq] (ISO/IEC 9075-16, Property Graph
Queries) — `CREATE PROPERTY GRAPH` DDL and `GRAPH_TABLE` SELECT clauses
that let you query relational tables using graph pattern matching
syntax.

[pgq]: https://www.postgresql.org/docs/devel/queries-graph.html

pg_mentat provides helpers that map its narrow EAV storage onto
SQL/PGQ-compatible vertex and edge tables, so a Datalog datom store
can be queried with cypher-style graph patterns alongside Datalog.

**PG19 is in development** at the time of writing. The SQL/PGQ feature
landed on `master` on 2026-03-16 (commit `2f094e7ac69`) and will ship
in PG19. The integration here is **forward-looking**: the DDL
generator produces well-formed text on any PG version, but executing
the result requires PG19+. Detect with `mentat.has_pg19_graph()`.

## Data-model fit

SQL/PGQ expects:

- **Vertex tables**: typed entity tables, one row per entity, with
  named columns becoming vertex properties.
- **Edge tables**: rows representing relationships, with explicit
  SOURCE / DESTINATION foreign-key references.

pg_mentat's narrow datom tables are EAV-shaped (`e`, `a`, `v`, `tx`,
`added`) — there's no built-in entity-typed view. The integration
materializes vertex and edge **views** from named attributes:

| Attribute kind | View shape | Use as |
|:---|:---|:---|
| `:db.type/string` etc. | `(e BIGINT, label TEXT)` | Vertex table |
| `:db.type/ref` | `(id BIGINT, src BIGINT, dst BIGINT, label TEXT)` | Edge table |

Each registered attribute becomes one view; users compose them into a
property graph by name.

## Quick start

```sql
CREATE EXTENSION pg_mentat;

-- Define schema with a ref-type attribute (the edge).
SELECT mentat.t('[
  {:db/ident :person/name     :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :company/name    :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :person/employer :db/valueType :db.type/ref    :db/cardinality :db.cardinality/one}
]');

-- Materialize vertex views (one per entity-attr).
SELECT mentat.create_vertex_view(':person/name');
-- => 'mentat.v__person_name'
SELECT mentat.create_vertex_view(':company/name');
-- => 'mentat.v__company_name'

-- Materialize an edge view (must be ref-typed).
SELECT mentat.create_edge_view(':person/employer');
-- => 'mentat.e__person_employer'

-- Generate the CREATE PROPERTY GRAPH DDL. Returns text — executing
-- it requires PG19+.
SELECT mentat.create_property_graph_ddl(
  'social',
  ARRAY[':person/name', ':company/name'],
  ARRAY[':person/employer']
);
```

Output (well-formed DDL):

```sql
CREATE PROPERTY GRAPH social
    VERTEX TABLES (
        mentat.v__person_name LABEL "person/name",
        mentat.v__company_name LABEL "company/name"
    )
    EDGE TABLES (
        mentat.e__person_employer
            SOURCE mentat.v__person_name
            DESTINATION mentat.v__person_name
            LABEL "person/employer"
    );
```

On PG19+, `EXECUTE` this DDL and then issue graph queries:

```sql
SELECT name FROM GRAPH_TABLE (social
  MATCH (p IS "person/name")-[IS "person/employer"]->(c IS "company/name")
  COLUMNS (c.label AS name));
```

## Helpers

| Function | Purpose |
|:---|:---|
| `mentat.has_pg19_graph()` | True if PG version >= 19 and SQL/PGQ catalog is present. |
| `mentat.create_vertex_view(:attr)` | Create / replace a vertex view for a value attribute. Returns the qualified view name. |
| `mentat.create_edge_view(:attr)` | Create / replace an edge view for a ref-typed attribute. Returns the qualified view name. |
| `mentat.drop_vertex_view(:attr)` | Drop the vertex view. Returns true if it existed. |
| `mentat.drop_edge_view(:attr)` | Drop the edge view. Returns true if it existed. |
| `mentat.create_property_graph_ddl(graph_name, vertex_attrs[], edge_attrs[])` | Generate the CREATE PROPERTY GRAPH text. Does NOT execute. Works on any PG version. |

## Caveats and design notes

1. **Edge SOURCE/DESTINATION resolution.** SQL/PGQ requires explicit
   SOURCE/DESTINATION references for edge tables. The DDL generator
   currently uses the **first** vertex attribute as both source and
   destination labels. For multi-vertex-type graphs (person ->
   company), you'll want to hand-edit the generated DDL or call
   `create_property_graph_ddl` per-edge with the right vertex-attr
   pair. A future revision will accept a per-edge `(src-attr,
   dst-attr)` mapping.

2. **Edge IDs are synthetic.** The edge view exposes `id = e * 100000
   + v` as a synthetic primary key. With realistic entity ids this
   won't collide, but for very large entity-id ranges you may want
   to redefine the view.

3. **Datom type fan-out.** Each vertex view materializes from a
   single typed datom table (string / long / ref / etc.) so a
   "person" with mixed-type attributes has one view per attribute.
   To get multiple property columns in one vertex view, materialize
   a custom view by hand (combining several datom tables on `e`)
   and reference that from the property graph.

4. **Read-only.** SQL/PGQ is a read-only graph view over base
   tables. Updates still go through `mentat.t`. The vertex and
   edge views automatically reflect new transactions because they
   are non-materialized views.

## What this does NOT (yet) give you

- **Native graph storage.** SQL/PGQ is a graph view over relational
  tables. For native graph workloads, integrate Apache AGE or Neo4j.
- **Pre-PG19 support.** SQL/PGQ requires PG19+. On older versions,
  use Datalog patterns directly — `[?p :person/employer ?c] [?c
  :company/name ?cn]` expresses the same graph hop.
- **Heterogeneous edge types.** All edges in the generated DDL share
  the same source and destination labels. Multi-typed graphs (e.g.
  `person -> company` and `person -> person`) need hand-edited DDL.
- **End-to-end CI tests.** The pgrx test cluster is PG16; happy-path
  graph queries can't be exercised until pgrx supports PG19.

## See also

- [PostgreSQL devel docs: Querying Graphs][pgq]
- ISO/IEC 9075-16:2023 (SQL/PGQ standard)
