# Cookbook: JOIN Datalog Results With Regular Postgres Tables

## What this gives you

pg_mentat stores datoms in ordinary PostgreSQL tables (`mentat.datoms_ref_new`,
`mentat.datoms_text_new`, `mentat.datoms_keyword_new`, ...). They are not opaque
blobs, not a foreign data wrapper, and not a separate process you talk to over a
socket. Any other table in the same database can join against them at the SQL
level, in the same transaction, with the same MVCC snapshot. Datomic, XTDB, and
Datalevin require two round trips and an in-application join to mix datalog
results with relational data; pg_mentat does it in a single SQL statement and a
single transaction.

This page shows the pattern. The schemas, queries, and outputs below are run
against a live `cookbook_demo` database (Postgres 16.13, pg_mentat 1.2.1).

## The example domain

Two halves of the same application:

* `app.users` and `app.subscriptions` are normal Postgres tables. They hold
  billing state that the rest of the codebase already reads with plain SQL,
  with foreign keys, sequences, and uniqueness constraints.
* The issue tracker lives in pg_mentat as datoms (`:issue/title`,
  `:issue/state`, `:issue/assignee`, `:issue/created-at`). Issues are an
  append-only event stream with cross-references; the EAV layout fits.

### Setup

```sql
-- pg_mentat side: schema for users and issues
SELECT mentat_transact('[
  {:db/ident :user/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
  {:db/ident :user/display-name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :issue/title
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :issue/state
   :db/valueType :db.type/keyword
   :db/cardinality :db.cardinality/one}
  {:db/ident :issue/assignee
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one}
  {:db/ident :issue/created-at
   :db/valueType :db.type/instant
   :db/cardinality :db.cardinality/one}
]');

-- Relational side: billing tables
CREATE SCHEMA app;

CREATE TABLE app.users (
  id                 BIGSERIAL PRIMARY KEY,
  email              TEXT NOT NULL UNIQUE,
  stripe_customer_id TEXT NOT NULL UNIQUE,
  created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE app.subscriptions (
  stripe_customer_id TEXT PRIMARY KEY,
  plan               TEXT NOT NULL,
  status             TEXT NOT NULL,
  current_period_end TIMESTAMPTZ NOT NULL
);
```

(Skipped here: the obvious `INSERT`s into `app.users`/`app.subscriptions` and
`mentat_transact(...)` calls that create users and issues. The full sample in
`docs/examples/cookbook-postgres-join.sql` populates four users — Alice, Bob,
Carol, Dan — with matching billing rows and five issues.)

## The mixed query

The question: "Of all open issues, which ones are assigned to a user whose
Stripe subscription is currently active?" Half of that lives in datoms, half in
relational tables.

### Pattern A: pure SQL JOIN against the narrow datom tables

```sql
SELECT
  title.v   AS issue_title,
  state.v   AS issue_state,
  uemail.v  AS assignee_email,
  s.plan,
  s.status
FROM mentat.datoms_ref_new      asg
JOIN mentat.datoms_text_new     title  ON title.e  = asg.e
                                       AND title.a  = (SELECT entid FROM mentat.idents
                                                       WHERE ident = ':issue/title')
                                       AND title.added
JOIN mentat.datoms_keyword_new  state  ON state.e  = asg.e
                                       AND state.a  = (SELECT entid FROM mentat.idents
                                                       WHERE ident = ':issue/state')
                                       AND state.added
JOIN mentat.datoms_text_new     uemail ON uemail.e = asg.v
                                       AND uemail.a = (SELECT entid FROM mentat.idents
                                                       WHERE ident = ':user/email')
                                       AND uemail.added
JOIN app.users         u  ON u.email = uemail.v
JOIN app.subscriptions s  ON s.stripe_customer_id = u.stripe_customer_id
WHERE asg.a    = (SELECT entid FROM mentat.idents WHERE ident = ':issue/assignee')
  AND asg.added
  AND s.status = 'active'
  AND state.v  = 'issue.state/open'
ORDER BY issue_title;
```

Output:

```text
            issue_title            |   issue_state    |  assignee_email   | plan | status
-----------------------------------+------------------+-------------------+------+--------
 Indexes missing on accounts.email | issue.state/open | bob@example.com   | team | active
 Login is broken on Safari         | issue.state/open | alice@example.com | pro  | active
(2 rows)
```

The query plan (truncated) shows the partial covering indexes doing the work:

```text
Nested Loop
  -> Index Only Scan using idx_datoms_keyword_new_vaet on datoms_keyword_new state
       Index Cond: ((v = 'issue.state/open') AND (a = $1))
  -> Index Only Scan using idx_datoms_ref_new_vaet on datoms_ref_new asg
       Index Cond: ((a = $3) AND (e = state.e))
  -> Index Only Scan using idx_datoms_text_new_eavt on datoms_text_new title
       Index Cond: ((e = asg.e) AND (a = $0))
  -> Index Only Scan using idx_datoms_text_new_eavt on datoms_text_new uemail
       Index Cond: ((e = asg.v) AND (a = $2))
  -> Index Scan using users_email_key on users u
       Index Cond: (email = uemail.v)
  -> Index Scan using subscriptions_pkey on subscriptions s
       Index Cond: (stripe_customer_id = u.stripe_customer_id)
```

Every datom touch is an index-only scan against an EAVT or VAET index; the user
and subscription rows resolve through their existing primary/unique indexes.

### Pattern B: let the datalog engine do the EAV part, then SQL-join the rest

```sql
WITH q AS (
  SELECT mentat_query(
    '[:find ?title ?state ?email
      :where
        [?i :issue/title ?title]
        [?i :issue/state ?state]
        [?i :issue/assignee ?u]
        [?u :user/email ?email]]',
    '{}'::jsonb
  ) AS j
),
issues AS (
  SELECT (r->>0) AS title,
         (r->>1) AS state,
         (r->>2) AS assignee_email
  FROM   q, jsonb_array_elements(q.j -> 'results') r
)
SELECT i.title         AS issue_title,
       i.state         AS issue_state,
       i.assignee_email,
       s.plan,
       s.status
FROM   issues i
JOIN   app.users         u ON u.email = i.assignee_email
JOIN   app.subscriptions s ON s.stripe_customer_id = u.stripe_customer_id
WHERE  i.state = ':issue.state/open'
  AND  s.status = 'active'
ORDER BY i.title;
```

Output is identical to Pattern A.

### Which to use

Pattern A is one query plan. The optimiser sees the whole graph, picks join
order across datoms and relational tables, and chooses index-only scans. Use
it when the predicate against `app.subscriptions` (or any other relational
filter) is selective — pushing the `status = 'active'` filter into the join
graph eliminates entities the datalog engine would have materialised.

Pattern B is two query plans. The datalog engine materialises *all* matching
issues into JSON, then SQL filters and joins. Use it when the EAV side is the
bottleneck and would benefit from datalog features the SQL form can't easily
express — `not`/`or` clauses, rules, recursion, aggregates with `:find`. The
materialised CTE is a coarse-grained boundary; the planner cannot push the
`s.status = 'active'` predicate through it.

Note one wart: `mentat_query` keyword results render as `:issue.state/open`
(with the leading colon), but raw datom storage in `datoms_keyword_new` strips
the colon (`issue.state/open`). Pattern B compares against the JSON form,
Pattern A against the storage form.

## Cross-database scope: datalog meets a foreign-data-wrapper table

The same JOIN works against anything Postgres can see — `postgres_fdw`,
`file_fdw`, `pg_partman` cold partitions, a logical replica, a different
schema. The datalog engine produces a row source; SQL composes it with the
rest of the database.

Setup: a "warehouse" Postgres holds resolved-issue archives, exposed locally
through `postgres_fdw`. (In the working example we point both ends at the
same cluster for reproducibility; substitute a real warehouse host in
production.)

```sql
CREATE EXTENSION postgres_fdw;
CREATE SERVER warehouse_server
  FOREIGN DATA WRAPPER postgres_fdw
  OPTIONS (host 'warehouse.internal', port '5432', dbname 'warehouse');
CREATE USER MAPPING FOR CURRENT_USER
  SERVER warehouse_server OPTIONS (user 'app_ro');
IMPORT FOREIGN SCHEMA warehouse_local
  FROM SERVER warehouse_server INTO public;
-- public.issue_archive(archived_issue_eid bigint, resolution text,
--                      resolved_at timestamptz, hours_open numeric)
```

Then the report — "for every issue I have closed in the EAV store, fetch the
post-resolution metrics from the warehouse":

```sql
WITH closed_issues AS (
  SELECT (r->>0)::bigint AS issue_eid,
         (r->>1)         AS title,
         (r->>2)         AS assignee_email
  FROM   jsonb_array_elements(
           (mentat_query(
             '[:find ?i ?title ?email
               :where
                 [?i :issue/title ?title]
                 [?i :issue/state :issue.state/closed]
                 [?i :issue/assignee ?u]
                 [?u :user/email ?email]]',
             '{}'::jsonb
           ) -> 'results')
         ) r
)
SELECT c.title,
       c.assignee_email,
       a.resolution,
       a.resolved_at,
       a.hours_open
FROM   closed_issues c
JOIN   public.issue_archive a ON a.archived_issue_eid = c.issue_eid
ORDER BY a.resolved_at;
```

Output:

```text
           title           |  assignee_email   | resolution |      resolved_at       | hours_open
---------------------------+-------------------+------------+------------------------+------------
 OAuth token refresh fails | carol@example.com | fixed      | 2025-10-12 14:00:00-04 |      50.00
(1 row)
```

The same structure works with `pg_partman`-managed time-tiered storage:
substitute `public.issue_archive` with the partitioned parent table and let
partition pruning skip cold partitions when the datalog side has already
narrowed the entity-id set.

## Single-transaction guarantee

A datalog write and a relational write share one MVCC snapshot:

```sql
BEGIN;
INSERT INTO app.users (email, stripe_customer_id)
  VALUES ('eve@example.com', 'cus_eve');
INSERT INTO app.subscriptions
  VALUES ('cus_eve', 'pro', 'active', now() + interval '14 days');
SELECT mentat_transact('[
  {:db/id "u-eve" :user/email "eve@example.com" :user/display-name "Eve"}
  {:issue/title "Slack integration is silently failing"
   :issue/state :issue.state/open
   :issue/assignee "u-eve"}
]');
-- A mixed JOIN here sees the freshly inserted user *and* the freshly
-- transacted issue. ROLLBACK undoes both; COMMIT publishes both.
COMMIT;
```

There is no second connection, no two-phase commit, no compensating
transaction. `mentat_transact` runs inside the surrounding `BEGIN`/`COMMIT`
exactly like any other function call; the narrow tables participate in
WAL, replication, and logical decoding the same way every other Postgres
table does.

## Mechanics: the table layout you are joining against

Each datom value type lives in its own narrow table. All have the same
column shape:

```text
mentat.datoms_<type>_new (
  store_id BIGINT NOT NULL DEFAULT 0,
  e        BIGINT NOT NULL,        -- entity id
  a        BIGINT NOT NULL,        -- attribute entid (resolve via mentat.idents)
  v        <type> NOT NULL,        -- bigint / text / boolean / timestamptz / ...
  tx       BIGINT NOT NULL,        -- transaction id
  added    BOOLEAN NOT NULL DEFAULT true
)
```

The nine type tables are `datoms_ref_new`, `datoms_long_new`,
`datoms_double_new`, `datoms_text_new`, `datoms_keyword_new`,
`datoms_boolean_new`, `datoms_instant_new`, `datoms_uuid_new`,
`datoms_bytes_new`. Picking the correct table is the only piece of schema
awareness Pattern A requires — you choose it from the attribute's
`:db/valueType`.

### Indexes

Every narrow table carries four covering indexes, all `WHERE added`
(partial):

| Index | Column order | When it helps in a JOIN |
|-------|-------------|--------------------------|
| EAVT  | `(store_id, e, a, tx) INCLUDE (v)` | "given an entity, give me this attribute's value" — the common shape after an `?i` is bound |
| AEVT  | `(store_id, a, e, tx)`              | "every entity that has attribute A" — full-attribute scans |
| AVET  | (PK: `store_id, e, a, v, tx`)       | "find the entity whose attribute A equals V" — value-driven entry into the graph |
| VAET  | `(store_id, v, a, e, tx)`           | "find every entity that points at V" — reverse-ref traversal (only meaningful on `datoms_ref_new`) |

Because every index has a `WHERE added` predicate, **always include `<alias>.added`
in your JOIN/WHERE conditions**. Without it the planner falls back to the
primary key, which lacks `added` as a leading column, and you lose the
index-only scan.

### Resolving keywords to entids

`mentat.idents(ident TEXT, entid BIGINT)` is a regular two-column lookup
table with a unique index on each column. The pattern

```sql
(SELECT entid FROM mentat.idents WHERE ident = ':issue/title')
```

is a single index probe. The planner runs it once as an InitPlan and reuses
the constant. If you write the same query many times, you can hard-code the
entid once you know it (`a = 10002`), but the lookup is cheap enough that
hard-coding is rarely worth the loss of legibility.

`mentat.schema` carries the rest of the attribute metadata
(`value_type`, `cardinality`, `unique_constraint`, `indexed`, `fulltext`,
`component`, `no_history`). Join it with `mentat.idents` if you need to
choose the narrow table dynamically rather than statically.

## Caveats

* **Use the narrow tables, not `mentat.datoms`.** The `mentat.datoms` view is a
  `UNION ALL` over all nine narrow tables, kept for compatibility with code
  that pre-dates the per-type split. A query against the view fans out to
  nine `Append` branches and nine index probes; the same query against
  `datoms_text_new` is one. We measured a single-attribute count: the view
  takes 41 plan rows and visits eight empty tables; the narrow form is 9 rows.
  Always know the value type and use the matching table.

* **Always include `WHERE added` (or `AND <alias>.added`).** All four
  covering indexes are partial on `added = true`. Predicates without it
  fall back to the primary key and pay for a full row fetch.

* **The datalog engine does not see your `app.*` tables.** `mentat_query`
  compiles to SQL that references the narrow tables and `mentat.idents` only;
  it has no awareness of `app.users` or `app.subscriptions`. If you want
  bidirectional queries — e.g., "find me datoms whose `:issue/assignee`
  is in `app.users WHERE created_at > now() - interval '7 days'`" — you have
  two paths:

  1. **Project the relational data into datoms.** Maintain
     `:user/stripe-customer-id`, `:user/created-at`, etc. as datoms (via a
     trigger on `app.users` or a periodic sync). The datalog engine can then
     filter on it natively.

  2. **JOIN at the SQL layer.** This is the path described on this page. The
     datalog engine produces an entity set; SQL filters it against the
     relational table. Use it when the relational data is large, mutable, or
     authoritative elsewhere — copying it into datoms would create a
     consistency burden you do not want.

* **Keyword rendering differs by surface.** Datoms in `datoms_keyword_new`
  store the value without a leading colon (`issue.state/open`); the JSON
  produced by `mentat_query` reattaches it (`:issue.state/open`); EDN input
  to `mentat_transact` of course uses the colon form. Match the surface in
  whichever predicate you are writing.

* **The compatibility view's `INSTEAD OF` triggers do not apply to JOINs.**
  Reading from the view works (slowly); inserting through it routes to the
  correct narrow table. JOINing through it gets you the slow `Append` plan
  with no upside.
