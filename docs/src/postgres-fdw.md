# Cross-Database Datalog with postgres_fdw

`postgres_fdw` is a built-in PostgreSQL contrib extension (PG13+). It
lets one PostgreSQL database run queries against tables on a *different*
PostgreSQL server as if they were local. Combined with pg_mentat,
this gives you cross-database Datalog: one query that joins datoms
from your local store with datoms (or relational rows) on a remote
server.

This is a **cookbook page**, not a new where-fn. Everything here is
plain `postgres_fdw` SQL — pg_mentat just happens to slot in cleanly.

`postgres_fdw` ships with PostgreSQL. No build, no preload.

## When to use this

| Use case | Pattern |
|:---|:---|
| Query datoms across two pg_mentat instances | `IMPORT FOREIGN SCHEMA mentat ...` from each remote, then `[?e :attr ?v]` resolves locally; remote attributes become foreign-table queries. |
| Join datoms to legacy relational tables | Foreign-table the legacy tables, then `(get-else $ ?e :user/email "")` style patterns + raw SQL joins. |
| Read-only replica of a hot pg_mentat for reporting | One central reporting DB foreign-tables N tenant DBs; Datalog queries run on the central side. |

It is **not** a sharding solution. Each remote query goes over the
network with TCP overhead per round-trip. For N>3 remotes joined in
one query, expect minutes-not-milliseconds latency. For a real
sharded multi-store, see Citus integration in `INTEGRATIONS.md`
(Tier 3, currently a stub).

## Setup: one local + one remote pg_mentat

On the **remote** server (we'll call it `tenant_a`):

```sql
-- Standard pg_mentat install on the remote.
CREATE EXTENSION pg_mentat;

-- Some data.
SELECT mentat.t('[
  {:db/ident :issue/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :issue/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
]');
SELECT mentat.t('[
  {:db/id "i1" :issue/title "memory leak in cache"   :issue/status :status/open}
  {:db/id "i2" :issue/title "fix typo in docs"       :issue/status :status/closed}
]');
```

On the **local** (central) server:

```sql
-- Enable the FDW.
CREATE EXTENSION IF NOT EXISTS postgres_fdw;

-- Server pointer.
CREATE SERVER tenant_a_srv
  FOREIGN DATA WRAPPER postgres_fdw
  OPTIONS (host 'tenant_a.example.com', port '5432', dbname 'mydb');

-- User mapping (read-only role recommended).
CREATE USER MAPPING FOR CURRENT_USER
  SERVER tenant_a_srv
  OPTIONS (user 'reporting', password 'secret');

-- Bring the remote mentat schema in as foreign tables.
CREATE SCHEMA tenant_a;
IMPORT FOREIGN SCHEMA mentat
  LIMIT TO (datoms_text_new, datoms_keyword_new, datoms_long_new,
            datoms_ref_new, datoms_double_new, datoms_boolean_new,
            datoms_instant_new, datoms_uuid_new, datoms_bytes_new,
            schema, idents)
  FROM SERVER tenant_a_srv INTO tenant_a;
```

## Cross-store queries

### Approach 1: raw SQL with the foreign tables

Datalog runs on the local store. To pull data from the remote store,
write a SQL view that wraps the foreign datoms:

```sql
CREATE OR REPLACE VIEW tenant_a_open_issue_titles AS
SELECT
  d.e AS entid,
  d.v AS title
FROM tenant_a.datoms_text_new d
WHERE d.a = (SELECT entid FROM tenant_a.schema WHERE ident = ':issue/title')
  AND d.added = true
  AND EXISTS (
    SELECT 1 FROM tenant_a.datoms_keyword_new s
    WHERE s.e = d.e
      AND s.a = (SELECT entid FROM tenant_a.schema WHERE ident = ':issue/status')
      AND s.added = true
      AND s.v = ':status/open'
  );

SELECT * FROM tenant_a_open_issue_titles ORDER BY title;
```

This pushes filtering down to the remote server (postgres_fdw is good
at this — verify with `EXPLAIN`). The local Datalog never touches the
remote; the remote results materialize as ordinary rows.

### Approach 2: union local + remote in one Datalog query

If the local store has its own `:issue/title` attribute and you want
*both* sets in one Datalog answer, materialize the remote as a side
table the local pg_mentat can see:

```sql
-- Materialized cache of remote data (refresh on a cron).
CREATE MATERIALIZED VIEW remote_open_issues AS
SELECT entid, title FROM tenant_a_open_issue_titles;

CREATE INDEX ON remote_open_issues(title);

-- Periodic refresh.
REFRESH MATERIALIZED VIEW CONCURRENTLY remote_open_issues;
```

Then, inside Datalog you can use raw SQL via `(ground ...)` collection:

```clojure
(d/q '[:find ?title
       :where
         [(ground ["t-a-1" "t-a-2" "t-a-3"]) [?title ...]]   ;; ← values from the SQL side
         ;; ... mix with local datoms here ...
       ]
     db)
```

Or — preferable — issue a SQL UNION at the top:

```sql
SELECT title FROM remote_open_issues
UNION
SELECT (mentat.q('[:find ?title :where [?e :issue/title ?title]
                                       [?e :issue/status :status/open]]')
        ->'results') AS title;
```

### Approach 3: Datalog `:in` clause + foreign data

The cleanest pattern. Run the Datalog query locally; pass remote data
in via `:in`:

```sql
WITH remote AS (
  SELECT entid AS r_eid, title AS r_title
  FROM tenant_a_open_issue_titles
)
SELECT mentat.q(
  '[:find ?title ?local-status
    :in $ [[?title ...]]
    :where
      [?e :issue/title ?title]
      [?e :issue/status ?local-status]]',
  jsonb_build_object('?title', (SELECT jsonb_agg(r_title) FROM remote))
) AS result;
```

The `[[?title ...]]` collection binding lets the Datalog query
restrict its search to titles that exist on the remote; the FDW
push-down eliminates remote rows we don't need; and the join happens
inside the local Datalog.

## Multi-tenant pattern: many remotes, one query

```sql
-- Repeat IMPORT FOREIGN SCHEMA for tenant_b, tenant_c, ...
CREATE SCHEMA tenant_b;
IMPORT FOREIGN SCHEMA mentat LIMIT TO (...) FROM SERVER tenant_b_srv INTO tenant_b;

CREATE OR REPLACE VIEW all_open_issues AS
SELECT 'a' AS tenant, * FROM tenant_a_open_issue_titles
UNION ALL
SELECT 'b' AS tenant, * FROM tenant_b_open_issue_titles
UNION ALL
SELECT 'c' AS tenant, entid, title FROM (... tenant_c view ...) v;
```

Each tenant's portion runs in parallel (postgres_fdw uses async
remote execution since PG14). Datalog on the central side joins the
union to local data.

## Performance notes

- **Use FDW pushdown.** Always start with `EXPLAIN VERBOSE` and look
  for `Foreign Scan` with `Remote SQL`. Joins, sorts, aggregates, and
  WHERE-clauses pushdown when types match exactly.
- **Pin `use_remote_estimate = true`** on the foreign server for any
  query the planner gets wrong:

  ```sql
  ALTER SERVER tenant_a_srv OPTIONS (SET use_remote_estimate 'true');
  ```

- **Network is always the bottleneck.** 1 ms of round-trip × N rows
  pulled is the floor. Pull aggregates, not rows, when you can.
- **Foreign tables are not indexable from the local side.** Do
  index work on the remote.
- **Read-only by default.** pg_mentat's transact path doesn't
  cross FDW boundaries; `mentat.t` only writes locally. If you need
  to write to a remote pg_mentat, send the EDN over via plain SQL
  (`SELECT mentat.t(...) ON tenant_a_srv` is not possible — you
  need a separate connection).

## Errors

| Error | Cause | Fix |
|:---|:---|:---|
| `permission denied for foreign server` | User mapping missing or pg_hba.conf rejects. | Add user mapping; check `pg_hba.conf` on remote. |
| `relation "mentat.foo" does not exist` | Wrong schema name in IMPORT FOREIGN SCHEMA. | Check remote pg_mentat installed; schema is `mentat`. |
| Slow planning, hash joins on huge remote tables | `use_remote_estimate = false` (default). | Set to `true`; ANALYZE foreign tables. |

## See also

- [Joining mentat to legacy SQL tables](./cookbook-postgres-join.md) —
  same FDW techniques applied to non-pg_mentat tables.
- [`INTEGRATIONS.md`][int] — Citus and pglogical entries for true
  sharding and CDC instead of pull-based federation.

[int]: https://codeberg.org/gregburd/pg_mentat/src/branch/main/docs/INTEGRATIONS.md
