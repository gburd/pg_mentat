# Datomic Client Compatibility Tests

This directory contains automated tests that validate mentatd against the
official Datomic Peer API (`datomic.api`).  The tests use
[Leiningen](https://leiningen.org/) as the test runner and
`clojure.test` assertions.

## Directory layout

```
mentatd/tests/datomic_client/
  project.clj                          Leiningen project (deps, test paths)
  test/
    datomic_compat/
      core_test.clj                    Core compatibility suite
      real_client_test.clj             Extended real-client tests
  test_queries.clj                     Legacy REPL-oriented manual tests
  test_client.sh                       Shell-based protocol tests (curl/EDN)
  compatibility_report.md              API coverage matrix and known limitations
  README.md                            This file
```

## Prerequisites

- Java 17+ (Temurin recommended)
- [Leiningen](https://leiningen.org/) 2.11+
- A running mentatd server connected to a PostgreSQL instance with pg_mentat
  installed

## Running locally

### 1. Start mentatd

```bash
cd mentatd
export DATABASE_URL="postgresql://localhost:5432/mentat"
cargo run
```

### 2. Run the Clojure tests

```bash
cd mentatd/tests/datomic_client

# All tests
lein test

# Only the core suite
lein test datomic-compat.core-test

# Only the extended real-client suite
lein test datomic-compat.real-client-test
```

The default mentatd URI is `datomic:free://localhost:8080/test-db`.  Override
it by setting the `MENTATD_URI` environment variable:

```bash
export MENTATD_URI="datomic:free://myhost:9090/my-db"
lein test
```

### 3. Run the shell protocol tests

These tests use `curl` to exercise the mentatd HTTP/EDN protocol directly
without a Datomic client library:

```bash
cd mentatd/tests/datomic_client
export MENTATD_URL="http://localhost:8080"
./test_client.sh
```

## CI/CD

The GitHub Actions workflow `.github/workflows/datomic_compat_test.yml` runs
both the shell and Clojure tests automatically:

- **Trigger**: Push to `main`, `claude`, or `develop` branches; PRs targeting
  `main` or `claude`; manual dispatch.
- **Matrix**: PostgreSQL 15 and 16.
- **Steps**:
  1. Build mentatd (`cargo build --release`).
  2. Start a PostgreSQL service container.
  3. Start mentatd with a test config.
  4. Run `test_client.sh` (shell protocol tests).
  5. Run `lein test` (Clojure compatibility tests).
  6. Generate a GitHub step summary with results.

## Test suites

### `core_test.clj`

Validates the most critical Datomic API operations using a single shared
database (`:once` fixture):

| Category     | Tests                                                |
|--------------|------------------------------------------------------|
| Connection   | connect, create/delete database                      |
| Schema       | attribute installation, queryable schema             |
| Transactions | map-form, :db/add, :db/retract, retractEntity       |
| Queries      | find-all, input params, aggregates                   |
| Pull API     | wildcard, specific attributes                        |
| Entity API   | lazy entity map, attribute access                    |
| Time-travel  | history, as-of                                       |

### `real_client_test.clj`

Extended tests where each `deftest` creates and destroys its own database
(`with-fresh-db`) for full isolation.  Covers:

1. Connection lifecycle (create, connect, delete, nonexistent DB)
2. Schema definition (string attrs, unique identity, cardinality-many)
3. Transactions (map form, list form, retract, retractEntity, tempids, multi-entity)
4. Queries (basic, single binding, input params, multi-input, collection input, aggregates, min/max, rules)
5. Pull API (wildcard, specific attrs, missing entity, :default, :limit, :as, nested refs, reverse refs, pull-many)
6. Lookup refs (in query, in pull, in transaction)
7. Entity API (basic, keys, touch)
8. Time-travel (as-of, since, history, basis-t)
9. Error handling (invalid attribute, syntax error, empty tx, empty results, duplicate unique identity / upsert)

### `test_client.sh`

Shell-based protocol tests that send EDN requests via `curl`:

- Health check, list databases, connect, query, transact
- Error cases (invalid op, missing fields, bad UUID, nonexistent DB)
- Create/delete database round-trip

### `test_queries.clj`

Legacy REPL-oriented test script.  Retained for ad-hoc manual testing in a
Datomic REPL:

```clojure
;; In a Datomic REPL:
(load-file "test_queries.clj")
(run-all-tests)
```

## Compatibility report

See [compatibility_report.md](compatibility_report.md) for:

- Full API coverage matrix (supported / partial / unsupported)
- Protocol details and response format differences
- Known limitations and workarounds

## Troubleshooting

### Connection refused

Ensure mentatd is running and listening on the expected port:

```bash
curl http://localhost:8080/health
```

### Invalid response format

Enable debug logging in mentatd:

```bash
RUST_LOG=debug cargo run
```

### Leiningen dependency resolution failures

Clear the local Maven cache and re-fetch:

```bash
rm -rf ~/.m2/repository/com/datomic
lein deps
```

### Transaction failures

Verify the PostgreSQL connection and that pg_mentat is installed:

```bash
psql "$DATABASE_URL" -c "SELECT mentat.mentat_query('[:find ?e :where [?e :db/ident :db/ident]]', '{}'::jsonb);"
```
