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
| `d/basis-t`            | PASS   | Returns MAX(tx) from mentat.transactions (Task #7) |
| `d/with`               | PASS   | Speculative transactions via BEGIN/ROLLBACK (Task #7) |
| `d/filter`             | PASS   | Filtered database views via savepoint + view pattern (Task #7) |
| `d/datoms`             | PASS   | Direct index access: EAVT, AEVT, AVET, VAET (Task #7) |

### Partially supported

| API function           | Status  | Notes / Limitations                             |
|------------------------|---------|--------------------------------------------------|
| `d/pull`               | PARTIAL | Wildcard, specific attrs, nested refs work.  :limit, :default, :as modifiers depend on pg_mentat pull implementation. |
| `d/pull-many`          | PARTIAL | Works when `d/pull` works; same limitations      |
| `d/entity`             | PARTIAL | Lazy entity map. `keys` and `touch` depend on protocol completeness. |
| `d/history`            | PARTIAL | History DB works for 5-tuple queries; mentatd must forward history flag. |
| `d/as-of`              | PARTIAL | Works when mentatd passes `as-of` timestamp to pg_mentat. |
| `d/since`              | PARTIAL | Works when mentatd passes `since` timestamp.    |
| Query with rules (`%`) | PARTIAL | Basic rules work; recursive rules untested.     |
| Collection inputs      | PARTIAL | `[?x ...]` binding form needs mentatd to expand collection args. |
| Lookup refs             | PARTIAL | `[:attr val]` works in queries and transactions when pg_mentat resolves them. |
| Upsert via unique identity | PARTIAL | `:db.unique/identity` upsert semantics depend on pg_mentat implementation. |

### Not yet supported

| API function / Feature        | Status      | Notes                                      |
|-------------------------------|-------------|--------------------------------------------|
| `d/index-range`               | UNSUPPORTED | Index range scan not exposed               |
| `d/seek-datoms`               | UNSUPPORTED | Seek-based datom iteration not exposed     |
| `d/entid`                     | UNSUPPORTED | Entity-id resolution by ident              |
| `d/ident`                     | UNSUPPORTED | Ident resolution by entity-id              |
| `d/invoke` (tx functions)     | UNSUPPORTED | Server-side transaction functions           |
| `d/squuid`                    | N/A         | Client-side; works without server support  |
| `d/tempid`                    | N/A         | Client-side; works without server support  |
| Excision                      | UNSUPPORTED | Permanent data removal not implemented     |
| Log API (`d/log`, `d/tx-range`) | UNSUPPORTED | Transaction log not exposed              |
| Attribute predicates / specs  | UNSUPPORTED | Not implemented in pg_mentat               |
| Entity specs                  | UNSUPPORTED | Not implemented in pg_mentat               |
| Transit serialization         | PASS        | Transit+JSON and Transit+MessagePack supported (Task #6) |

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
:transact          -> {:result {:db-before {:basis-t N} :db-after {:basis-t M} :tx-data [[e a v tx added] ...] :tempids {"temp" entity-id}}}
:basis-t           -> {:result N}
:with              -> {:result {:db-before {...} :db-after {...} :tx-data [...] :tempids {...}}}
:filter            -> {:result [[val1 val2] ...]}
:datoms            -> {:result [[e a v tx added] ...]}
```

Operations also accept the `datomic.catalog/` namespace prefix for
`:list-dbs`, `:create-db`, and `:delete-db`.

### Response format differences from Datomic

1. **Query results** are returned as a vector of vectors, not a set of
   vectors.  The Clojure client coerces them to sets.
2. **Transaction reports** now use Datomic-compatible format (Task #5):
   `:db-before`, `:db-after` (with `:basis-t`), `:tx-data` (5-element
   datom vectors), and `:tempids` (string keys to entity IDs).
3. **Error responses** use the Cognitect anomalies format
   (`:cognitect.anomalies/category`, `:cognitect.anomalies/message`).
4. **Transit wire format** is fully supported (Task #6): both
   Transit+JSON (`application/transit+json`) and Transit+MessagePack
   (`application/transit+msgpack`) are accepted for request and response.
   Content-Type negotiation works across all format combinations.

---

## Known limitations

1. **Transaction reports now use Datomic-compatible format (Task #5).**
   As of Task #5, mentatd returns `:db-before`, `:db-after`, `:tx-data`,
   and `:tempids` in transaction reports, matching Datomic's format.
   `:db-before` and `:db-after` include `:basis-t`.  `:tx-data` contains
   5-element datom vectors `[e a v tx added]`.  `:tempids` uses string keys.

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

6. **No `d/seek-datoms` or `d/index-range`.**
   `d/datoms` is supported (Task #7), but `d/seek-datoms` and `d/index-range`
   are not yet exposed.

7. **No transaction log API (`d/log`, `d/tx-range`).**

8. **No server-side transaction functions (`d/invoke`, `:db/fn`).**

---

## Workarounds

| Limitation                      | Workaround                                      |
|---------------------------------|-------------------------------------------------|
| No `d/index-range`             | Use `d/q` with range predicates or `d/datoms`    |
| No `d/seek-datoms`             | Use `d/datoms` with component filters             |
| No `d/log`                     | Query the history database                       |
| No tx functions                | Perform multi-step logic client-side              |

---

## Test file inventory

| File | Description |
|------|-------------|
| `test/datomic_compat/core_test.clj` | Core compatibility suite (connection, schema, CRUD, pull, entity, time-travel, d/with, d/filter, d/datoms) |
| `test/datomic_compat/real_client_test.clj` | Extended real-client tests (all API categories, lookup refs, rules, edge cases) |
| `test/datomic_compat/typed_values_test.clj` | Typed value round-trips, range queries, UUID/timestamp ordering (BYTEA fix validation) |
| `test/datomic_compat/http_integration_test.clj` | HTTP-based EDN API tests (db lifecycle, tx reports, pull, time-travel, errors, d/with, d/filter, d/datoms) |
| `test/datomic_compat/transit_test.clj` | Transit+JSON and Transit+MessagePack wire format tests (content negotiation, value fidelity) |
| `test_queries.clj` | REPL-oriented manual test script |
| `test_client.sh` | Shell-based EDN protocol tests (including range query regression tests) |
| `test_transit.sh` | Shell-based Transit wire format tests (content-type negotiation, msgpack) |
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
| 2026-04-24 | CI     | Task #7: Added d/with, d/filter, d/datoms, d/basis-t as fully supported |
| 2026-04-24 | CI     | Updated report for Task #5 (tx report format) and Task #6 (Transit support) |
| 2026-04-24 | CI     | Added transit_test.clj, http_integration_test.clj   |
| 2026-04-24 | CI     | Added typed_values_test.clj (BYTEA fix validation)  |
| 2026-04-24 | CI     | CI workflow: added timeouts, artifact upload, perf smoke test |
| 2026-04-22 | CI     | Initial compatibility report with full API matrix    |
