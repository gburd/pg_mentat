# Getting Started with pg_mentat

pg_mentat is a PostgreSQL extension that embeds a Datomic-compatible Datalog query engine directly inside PostgreSQL. Data is stored as immutable Entity-Attribute-Value-Transaction (EAVT) datoms in nine type-specific narrow tables, queried with Datalog via SQL function calls, and managed through EDN transactions. Every mutation is recorded as a new fact, enabling time-travel queries and full audit history.

This guide takes you from installation to running temporal queries in under 15 minutes.

### Why pg_mentat?

Traditional relational databases require you to define rigid table schemas up front, lose history when you UPDATE rows, and force complex self-joins for entity-attribute-value patterns. pg_mentat gives you:

- **Flexible schema**: Add attributes at any time without ALTER TABLE or migrations.
- **Immutable history**: Every change is recorded as a new fact. Nothing is overwritten.
- **Time travel**: Query your data as it existed at any past transaction.
- **Datalog queries**: Declarative, join-free logic programming replaces multi-table SQL JOINs.
- **Pull API**: Retrieve nested entity graphs in a single call.
- **PostgreSQL foundation**: Full ACID, replication, tooling, and ecosystem compatibility.

---

## Prerequisites

| Requirement | Version | Notes |
|-------------|---------|-------|
| PostgreSQL | 13 -- 18 | Any standard distribution. pg16 is the default build target. |
| Rust toolchain | 1.90+ | Required only when building from source. |
| cargo-pgrx | ~0.17 | Must match the pgrx version used by the project. |
| Git | any | For cloning the repository. |

Optional:
- **Docker / Podman** for containerized deployment (recommended for evaluation).
- **Nix with flakes** for reproducible dev shells.

---

## Installation

### Option A: Docker (fastest)

```bash
docker compose -f docker/docker-compose.yml up -d
psql -h localhost -U postgres -d mentat
```

The container image pre-installs the extension with bootstrap data ready.

### Option B: From Source with pgrx

```bash
# 1. Install cargo-pgrx (must match project version)
cargo install --locked cargo-pgrx --version '~0.17'

# 2. Initialize pgrx for your PostgreSQL installation
cargo pgrx init --pg16=$(which pg_config)

# 3. Clone the repository
git clone https://github.com/qpdb/mentat.git && cd mentat/pg_mentat

# 4. Build and install
cargo pgrx install --release
```

### Option C: Nix

```bash
git clone https://github.com/qpdb/mentat.git && cd mentat
nix develop
cd pg_mentat
cargo pgrx run pg16
```

This drops you into a `psql` session with the extension loaded.

### CREATE EXTENSION

After installation, connect to your database and load the extension:

```sql
CREATE EXTENSION IF NOT EXISTS pg_mentat;
```

Verify it is working:

```sql
SELECT mentat_schema();
```

This returns a JSON array describing the bootstrap schema attributes (`:db/ident`, `:db/valueType`, `:db/cardinality`, etc.). If you see output, the extension is operational.

---

## First Store

pg_mentat supports multiple isolated data stores within a single PostgreSQL database. Each store gets its own PostgreSQL schema (`mentat_<name>`) and its own set of typed datom tables. The `default` store is created automatically during `CREATE EXTENSION`.

```sql
-- List existing stores
SELECT mentat.list_stores();

-- Create a new store
SELECT mentat.create_store('my_project', 'My application data');
```

For most use cases, the default store suffices, and you can ignore store management entirely. All single-argument functions (`mentat_transact`, `mentat_query`, `mentat_pull`) operate on the default store.

---

## First Transaction

Define schema attributes and insert data using `mentat_transact`. Transactions accept EDN (Extensible Data Notation) containing maps or vectors of assertions.

### Define Attributes

```sql
SELECT mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :person/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
]');
```

### Insert Entities

```sql
SELECT mentat_transact('[
  {:person/name "Alice" :person/age 30 :person/email "alice@example.com"}
  {:person/name "Bob" :person/age 25 :person/email "bob@example.com"}
  {:person/name "Carol" :person/age 35 :person/email "carol@example.com"}
]');
```

The return value is a JSON transaction report:

```json
{
  "db-before": {"basis-t": 1000001},
  "db-after": {"basis-t": 1000002},
  "tx-data": [[10001, 65, "Alice", 1000002, true], ...],
  "tempids": {}
}
```

The `tempids` map shows how string temp IDs were resolved to permanent entity IDs. When using map syntax without explicit `:db/id`, temp IDs are generated internally. The `tx-data` array lists every datom asserted as `[entity, attribute-entid, value, tx, added]`.

### Updating Data

To change an existing entity's attribute, assert a new value. For cardinality-one attributes, the old value is automatically retracted:

```sql
-- Update Alice's age from 30 to 31
SELECT mentat_transact('[
  [:db/add [:person/email "alice@example.com"] :person/age 31]
]');
```

The `[:person/email "alice@example.com"]` is a "lookup ref" -- it resolves the entity ID via the unique attribute.

### Retracting Data

Explicitly retract a specific fact:

```sql
SELECT mentat_transact('[
  [:db/retract [:person/email "bob@example.com"] :person/age 25]
]');
```

---

## First Query

Query data with `mentat_query`. The first argument is a Datalog query string; the second is a JSONB object providing input bindings and options.

```sql
-- Find all person names
SELECT mentat_query(
  '[:find ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);

-- Find name where age = 30
SELECT mentat_query(
  '[:find ?name :where [?e :person/name ?name] [?e :person/age 30]]',
  '{}'::jsonb
);

-- Parameterized query using :in binding
SELECT mentat_query(
  '[:find ?name :in ?target-age :where [?e :person/name ?name] [?e :person/age ?target-age]]',
  '{"?target-age": 25}'::jsonb
);

-- Aggregate: count all persons
SELECT mentat_query(
  '[:find (count ?e) :where [?e :person/name _]]',
  '{}'::jsonb
);
```

Results are returned as JSONB. The response shape depends on the `:find` specification:

**FindRel** (default -- multiple variables):
```json
{"columns": ["?name", "?age"], "results": [["Alice", 30], ["Bob", 25], ["Carol", 35]]}
```

**FindScalar** (single value with `.` suffix):
```sql
SELECT mentat_query('[:find ?name . :where [?e :person/name ?name] [?e :person/age 30]]', '{}'::jsonb);
-- Returns: {"result": "Alice"}
```

**FindColl** (collection with `...` suffix):
```sql
SELECT mentat_query('[:find [?name ...] :where [?e :person/name ?name]]', '{}'::jsonb);
-- Returns: {"result": ["Alice", "Bob", "Carol"]}
```

### Predicates and Ordering

```sql
-- Find people aged >= 30, ordered by name
SELECT mentat_query(
  '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] [(>= ?age 30)] :order (asc ?name)]',
  '{}'::jsonb
);
```

---

## First Pull

The Pull API retrieves structured entity data by ID, selecting specific attributes.

```sql
-- First, find an entity ID
SELECT mentat_query(
  '[:find ?e :where [?e :person/email "alice@example.com"]]',
  '{}'::jsonb
);
-- Returns: {"result": 10001}

-- Pull specific attributes
SELECT mentat_pull('[:person/name :person/age :person/email]', 10001);

-- Pull all attributes with wildcard
SELECT mentat_pull('[*]', 10001);

-- Pull multiple entities at once
SELECT mentat_pull_many('[:person/name :person/age]', ARRAY[10001, 10002, 10003]);
```

Pull returns a JSON object:
```json
{":db/id": 10001, ":person/name": "Alice", ":person/age": 30, ":person/email": "alice@example.com"}
```

Pull patterns support nested references, reverse lookups, recursion, limits, defaults, and renames. See the Datalog Reference for the full pull expression syntax.

---

## Temporal Queries (as-of / since)

Every datum records the transaction in which it was asserted. This enables time-travel queries without any extra tables or configuration.

### as-of: Query the database at a past point in time

```sql
-- Query as it was at transaction 1000005
SELECT mentat_query(
  '[:find ?name :where [?e :person/name ?name]]',
  '{"asOf": 1000005}'::jsonb
);
```

### since: Query only changes after a transaction

```sql
-- Find entities modified after tx 1000003
SELECT mentat_query(
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{"since": 1000003}'::jsonb
);
```

### Full temporal function (4-argument form)

```sql
SELECT mentat_q_full('default',
  '[:find ?name :where [?e :person/name ?name]]',
  '{}'::jsonb,
  1000005  -- as_of_tx
);
```

### Diff between two transactions

Compare query results at two different points in time:

```sql
SELECT mentat.diff('default', 1000001, 1000005,
  '[:find ?e ?name :where [?e :person/name ?name]]',
  '{}'::jsonb
);
```

Returns a JSON object with `added` (rows in the new result but not the old) and `removed` (rows in the old result but not the new).

### History mode

Retrieve all historical versions of datoms (including retractions):

```sql
SELECT mentat_query(
  '[:find ?name ?tx :where [?e :person/name ?name]]',
  '{"history": true}'::jsonb
);
```

---

## Speculative Transactions (mentat_with)

Preview the effect of a transaction without committing:

```sql
SELECT mentat_with('[
  {:person/name "New Person" :person/age 40 :person/email "new@example.com"}
]');
```

This executes the full transaction pipeline inside a SAVEPOINT, returns the transaction report (identical to what `mentat_transact` would produce), then rolls back. No data is persisted. Use this for:

- Validating complex transactions before committing
- Building "what-if" UIs that preview changes
- Testing transaction logic in automated tests

---

## Entity Lookup

Retrieve all attributes for an entity as a flat JSON map:

```sql
SELECT mentat_entity(10001);
```

Returns:
```json
{":db/id": 10001, ":person/name": "Alice", ":person/age": 30, ":person/email": "alice@example.com"}
```

This is simpler than `mentat_pull('[*]', eid)` when you want all attributes without nested reference expansion.

---

## Inspecting Queries

Use `mentat_explain` to see the generated SQL and PostgreSQL query plan without executing:

```sql
SELECT mentat_explain(
  '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age] [(>= ?age 30)]]',
  '{}'::jsonb
);
```

This returns a JSON object with keys `generated_sql`, `datalog_plan`, and `pg_explain`. It is invaluable for understanding performance characteristics and debugging unexpected results.

---

## Next Steps

- **[Datalog Reference](DATALOG_REFERENCE.md)** -- Complete query syntax: predicates, OR/NOT/rules, aggregates, pull expressions, binding forms.
- **[Schema Reference](SCHEMA_REFERENCE.md)** -- Value types, cardinality, uniqueness constraints, transaction functions (CAS, retractEntity), tempid resolution.
- **[Operations Guide](OPERATIONS.md)** -- GUC configuration, monitoring, performance tuning, mentatd deployment, backup/restore.
- **`mentat_explain`** -- Inspect the generated SQL and query plan for any Datalog query:
  ```sql
  SELECT mentat_explain('[:find ?name :where [?e :person/name ?name]]', '{}'::jsonb);
  ```
