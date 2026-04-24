# pg_mentat / mentatd -- Datomic Compatibility Report

## Overview

This document records the compatibility status between the **official
Datomic Peer API** (`datomic.api`) and the **pg_mentat** PostgreSQL
extension exposed through the **mentatd** daemon.  The goal is to let
existing Datomic application code run against pg_mentat with minimal or
no changes.

All tests reference the files under
`mentatd/tests/datomic_client/test/datomic_compat/`.

---

## Test environment

| Component       | Version / Notes                                   |
|-----------------|---------------------------------------------------|
| pg_mentat       | 0.1.0 (pgrx PostgreSQL extension)                 |
| mentatd         | 0.1.0 (Axum-based HTTP daemon)                    |
| Datomic client  | `com.datomic/datomic-free 0.9.5697`               |
| Clojure         | 1.11.1                                            |
| Java            | Temurin 17                                        |
| PostgreSQL      | 15 / 16                                           |
| Test runner     | Leiningen 2.11.2 (`lein test`)                    |
| Protocol        | EDN-over-HTTP (POST to `http://<host>:8080/`)     |

---

## API coverage matrix

### Fully supported

| API function           | Status | Notes                                           |
|------------------------|--------|-------------------------------------------------|
| `d/create-database`    | PASS   | Creates a PostgreSQL database                   |
| `d/delete-database`    | PASS   | Drops a PostgreSQL database                     |
| `d/connect`            | PASS   | Returns connection map with UUID                |
| `d/db`                 | PASS   | Returns database value from connection          |
| `d/transact`           | PASS   | Map form, list form (:db/add, :db/retract)      |
| `d/q`                  | PASS   | Basic queries, :in params, aggregates           |
| `:db/retractEntity`    | PASS   | Removes all datoms for entity                   |

### Partially supported

| API function           | Status  | Notes / Limitations                             |
|------------------------|---------|--------------------------------------------------|
| `d/pull`               | PARTIAL | Wildcard, specific attrs, nested refs work.  :limit, :default, :as modifiers depend on pg_mentat pull implementation. |
| `d/pull-many`          | PARTIAL | Works when `d/pull` works; same limitations      |
| `d/entity`             | PARTIAL | Lazy entity map. `keys` and `touch` depend on protocol completeness. |
| `d/history`            | PARTIAL | History DB works for 5-tuple queries; mentatd must forward history flag. |
| `d/as-of`              | PARTIAL | Works when mentatd passes `as-of` timestamp to pg_mentat. |
| `d/since`              | PARTIAL | Works when mentatd passes `since` timestamp.    |
| `d/basis-t`            | PARTIAL | Returns transaction T; depends on DbId struct.  |
| Query with rules (`%`) | PARTIAL | Basic rules work; recursive rules untested.     |
| Collection inputs      | PARTIAL | `[?x ...]` binding form needs mentatd to expand collection args. |
| Lookup refs             | PARTIAL | `[:attr val]` works in queries and transactions when pg_mentat resolves them. |
| Upsert via unique identity | PARTIAL | `:db.unique/identity` upsert semantics depend on pg_mentat implementation. |

### Not yet supported

| API function / Feature        | Status      | Notes                                      |
|-------------------------------|-------------|--------------------------------------------|
| `d/with`                      | UNSUPPORTED | Speculative transactions not implemented   |
| `d/filter`                    | UNSUPPORTED | Filtered databases not implemented         |
| `d/index-range`               | UNSUPPORTED | Index range scan not exposed               |
| `d/seek-datoms` / `d/datoms`  | UNSUPPORTED | Raw datom iteration not exposed            |
| `d/entid`                     | UNSUPPORTED | Entity-id resolution by ident              |
| `d/ident`                     | UNSUPPORTED | Ident resolution by entity-id              |
| `d/invoke` (tx functions)     | UNSUPPORTED | Server-side transaction functions           |
| `d/squuid`                    | N/A         | Client-side; works without server support  |
| `d/tempid`                    | N/A         | Client-side; works without server support  |
| Excision                      | UNSUPPORTED | Permanent data removal not implemented     |
| Log API (`d/log`, `d/tx-range`) | UNSUPPORTED | Transaction log not exposed              |
| Attribute predicates / specs  | UNSUPPORTED | Not implemented in pg_mentat               |
| Entity specs                  | UNSUPPORTED | Not implemented in pg_mentat               |
| Transit serialization         | IN PROGRESS | Phase 2.2; currently EDN-only              |

---

## Protocol details

mentatd uses an EDN-over-HTTP protocol.  Requests are POSTed to the root
endpoint as EDN maps with an `:op` keyword.  The Datomic Free peer
client (`datomic.api`) communicates with the transactor via a different
binary protocol, so **a shim layer is required** to make the official
client talk to mentatd.  The tests in this repository use `datomic.api`
functions but rely on the Datomic Free in-process transactor mode
connecting through the SQL storage back-end that pg_mentat provides.

### Supported operations (mentatd HTTP)

```
:health            -> {:result "healthy"}
:list-dbs          -> {:result ["db1" "db2" ...]}
:create-db         -> {:result true}
:delete-db         -> {:result true}
:connect           -> {:result {:connection-id "uuid" :db-name "..." :status "connected"}}
:db                -> {:result {:connection-id "uuid" :status "active"}}
:q                 -> {:result [[val1 val2] ...]}
:transact          -> {:result {:tx-id N :tx-instant ... :tempids {...} :datoms-inserted N :status "committed"}}
```

Operations also accept the `datomic.catalog/` namespace prefix for
`:list-dbs`, `:create-db`, and `:delete-db`.

### Response format differences from Datomic

1. **Query results** are returned as a vector of vectors, not a set of
   vectors.  The Clojure client coerces them to sets.
2. **Transaction reports** use `:tx-id` / `:datoms-inserted` instead of
   Datomic's `:db-before` / `:db-after` / `:tx-data` datom vectors.
   Client code relying on `:tx-data` as a vector of `Datom` objects will
   need adaptation.
3. **Error responses** use the Cognitect anomalies format
   (`:cognitect.anomalies/category`, `:cognitect.anomalies/message`).

---

## Known limitations

1. **No `:db-before` / `:db-after` in transaction reports.**
   Datomic returns full database values before and after each
   transaction.  mentatd returns a simplified report.  Code that
   inspects `:db-before` or `:db-after` will need changes.

2. **`:tx-data` format differs.**
   Datomic returns a list of `Datom` objects (entity, attribute, value,
   tx, added?).  mentatd returns a count (`:datoms-inserted`) instead.
   Code that iterates over `:tx-data` datoms will not work as-is.

3. **Pull API modifiers.**
   `:limit`, `:default`, and `:as` are supported by pg_mentat but may
   behave slightly differently in edge cases (e.g., `:limit` on a
   cardinality-one attribute is a no-op in Datomic but may behave
   differently here).

4. **Entity API completeness.**
   `d/entity` returns a lazy map.  Whether all operations on that map
   (e.g., `seq`, `into {}`, `assoc`) work depends on the shim
   implementation.

5. **Time-travel granularity.**
   `d/as-of` and `d/since` accept a transaction T value.  Passing an
   `Instant` or `Date` is not yet supported.

6. **No speculative transactions (`d/with`).**
   There is no way to get a hypothetical database value without actually
   committing.

7. **No filtered databases (`d/filter`).**

8. **No raw index access (`d/datoms`, `d/seek-datoms`, `d/index-range`).**

9. **No transaction log API (`d/log`, `d/tx-range`).**

10. **No server-side transaction functions (`d/invoke`, `:db/fn`).**

---

## Workarounds

| Limitation                      | Workaround                                      |
|---------------------------------|-------------------------------------------------|
| No `:db-before` / `:db-after`  | Snapshot `(d/db conn)` before transacting        |
| No `:tx-data` datom list       | Query for recently-changed datoms after tx       |
| No `d/with`                    | Create a throwaway database, test, then delete   |
| No `d/filter`                  | Add filtering predicates to your Datalog queries |
| No `d/datoms`                  | Use `d/q` with appropriate where clauses         |
| No `d/index-range`             | Use `d/q` with range predicates                  |
| No `d/log`                     | Query the history database                       |
| No tx functions                | Perform multi-step logic client-side              |

---

## Test file inventory

| File | Description |
|------|-------------|
| `test/datomic_compat/core_test.clj` | Core compatibility suite (connection, schema, CRUD, pull, entity, time-travel) |
| `test/datomic_compat/real_client_test.clj` | Extended real-client tests (all API categories, lookup refs, rules, edge cases) |
| `test/datomic_compat/typed_values_test.clj` | Typed value round-trips, range queries, UUID/timestamp ordering (BYTEA fix validation) |
| `test_queries.clj` | REPL-oriented manual test script |
| `test_client.sh` | Shell-based protocol-level tests (including range query regression tests) |
| `project.clj` | Leiningen project config with `datomic-free` dependency |

---

## Running the tests

### Prerequisites

- Running mentatd server (connected to a PostgreSQL instance with pg_mentat installed)
- Java 17+
- Leiningen 2.11+

### Commands

```bash
# Set the mentatd URI (default: datomic:free://localhost:8080/test-db)
export MENTATD_URI="datomic:free://localhost:8080/test-db"

# Run all Clojure tests
cd mentatd/tests/datomic_client
lein test

# Run only the real-client tests
lein test datomic-compat.real-client-test

# Run shell protocol tests
./test_client.sh
```

### CI

The GitHub Actions workflow `.github/workflows/datomic_compat_test.yml`
runs these tests automatically on push to `main`, `claude`, or `develop`
branches, and on PRs targeting `main` or `claude`.

---

## Revision history

| Date       | Author | Change                                              |
|------------|--------|-----------------------------------------------------|
| 2026-04-24 | CI     | Added typed_values_test.clj (BYTEA fix validation)  |
| 2026-04-22 | CI     | Initial compatibility report with full API matrix    |
