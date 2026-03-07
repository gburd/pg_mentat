# Next Steps

**Updated:** 2026-03-07

This document lists the immediate priorities for bringing pg_mentat from its
current state (~65% complete) to a working, testable system.

---

## Priority 1: Validate the Build Environment

The single biggest blocker is that no one has yet run `cargo pgrx test` against
a live PostgreSQL instance for the pg_mentat extension.

### Using Nix (recommended)

```bash
nix develop
setup-pgrx
test-pg16
```

### Using a container

```bash
podman build -t pg_mentat_build -f Containerfile .
podman run --rm -v $(pwd):/workspace:Z -w /workspace/pg_mentat \
    pg_mentat_build cargo pgrx test pg16
```

### Using GitHub Actions

Push to GitHub and enable the workflow at `.github/workflows/test.yml`.

**Success criteria:** `cargo pgrx test pg16` runs and reports pass/fail counts.

---

## Priority 2: Fix Test Failures

Based on results from Priority 1, fix failures in this order:

1. **Schema initialization** -- `setup_test_db()` and `bootstrap_schema()`
   must succeed for all other tests to run.
2. **EDN roundtrip tests** (5 tests) -- validates the custom type works.
3. **Basic query** (`test_pg_rel`) -- validates the query path end-to-end.
4. **Basic transact** -- validates datom persistence.
5. **Remaining tests** -- time-travel, rules, full-text, pull.

**Success criteria:** At least 70% of the 38 tests pass (27+/38).

---

## Priority 3: Fix SQL Injection Issues

Several functions build SQL strings using `format!()` instead of parameterized
queries. This must be fixed before any production use.

**Files to audit:**

- `pg_mentat/src/functions/transact.rs`
- `pg_mentat/src/functions/query.rs`
- `pg_mentat/src/functions/entity.rs`
- `pg_mentat/src/storage.rs`

**Fix pattern:** Replace `format!("... {}", value)` with
`Spi::run_with_args("... $1", Some(vec![(oid, value.into_datum())]))`.

---

## Priority 4: Validate mentatd Integration

Once the extension works, verify the HTTP data path:

```bash
# Start PostgreSQL with the extension
cd pg_mentat
cargo pgrx run pg16
# In psql: CREATE EXTENSION pg_mentat; SELECT mentat.initialize_schema();

# In another terminal, start mentatd
cd mentatd
cargo run

# Test the data path
curl http://localhost:8080/health
curl -X POST http://localhost:8080 \
    -H "Content-Type: application/edn" \
    -d '[:transact {:tx-data [[:db/add "t1" :db/ident :test/attr]]}]'
curl -X POST http://localhost:8080 \
    -H "Content-Type: application/edn" \
    -d '[:q {:query [:find ?e :where [?e :db/ident]]}]'
```

**Success criteria:** Transact persists data, query retrieves it.

---

## Priority 5: Add Missing Type Support

Currently only 4 of 9 value types are handled: boolean, long, string, keyword.

Missing types and affected files:

| Type | Encode (transact.rs) | Decode (entity.rs) | Query (query.rs) |
|------|---------------------|---------------------|-------------------|
| `:db.type/ref` | Needed | Needed | Needed |
| `:db.type/double` | Needed | Needed | Needed |
| `:db.type/instant` | Needed | Needed | Needed |
| `:db.type/uuid` | Needed | Needed | Needed |
| `:db.type/bytes` | Needed | Needed | Needed |

---

## Priority 6: CI/CD

Validate and enable the GitHub Actions workflows in `.github/workflows/`:

- `test.yml` -- Main test pipeline
- `pg_mentat_test.yml` -- Extension-specific tests
- `mentatd_test.yml` -- Server tests

---

## Longer-Term

These items are not immediate blockers:

- **WASM support** -- Architecture designed, implementation not started
- **Performance benchmarking** -- No benchmarks exist yet
- **CBOR serialization** -- Dependency included, not wired into EdnValue storage
- **Remaining ~150 test ports** -- Only 38 of ~200 original tests have been migrated
- **Query planner hooks** -- Planned but not implemented
- **Containment operators** (`@>`, `<@`, `?`, `?|`, `?&`) -- Not implemented

---

## Reference

- [CURRENT_STATUS.md](CURRENT_STATUS.md) -- Detailed component status
- [NIX_SETUP.md](NIX_SETUP.md) -- Nix environment setup
- [TEST_MIGRATION_COMPLETE.md](TEST_MIGRATION_COMPLETE.md) -- Test migration details
- [HONEST_STATUS.md](HONEST_STATUS.md) -- Validator audit (2026-03-05)
