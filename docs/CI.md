# CI and local pre-push checks

This project runs two GitHub Actions workflows on every push:

| workflow              | file                                  | purpose                                                                                           | target wallclock |
|-----------------------|---------------------------------------|---------------------------------------------------------------------------------------------------|------------------|
| `installcheck`        | `.github/workflows/installcheck.yml`  | Fast feedback: `cargo build`, `cargo pgrx install`, `CREATE EXTENSION pg_mentat`, `smoke.sql`.    | < 5 min warm     |
| `ci`                  | `.github/workflows/ci.yml`            | Full gate: build + installcheck, `cargo clippy -- -D warnings`, `cargo pgrx test pg16`.           | < 15 min warm    |

Both workflows target PostgreSQL 16 only. Other `pgNN` feature flags compile,
but the build-farm matrix has not been widened yet. Do not add rows to the
matrix without first verifying `make smoke` locally on that version.

## Bug classes these checks catch

- **Duplicate-symbol link errors.** Caught by `cargo build` and again by
  `cargo pgrx install` (which re-links the cdylib).
- **`CREATE EXTENSION pg_mentat` failures.** Caught by `smoke.sql` step 1.
  This is the regression test for the schema-ownership and bootstrap-order
  bug fixed in commit `bf0f6e0`.
- **Install-time SQL errors** (e.g. `ROUND(float, int)`, invalid cast,
  missing function). Caught when pgrx loads `pg_mentat--1.0.0.sql`.
- **Rust/PG type mismatches** (i16 where i32 was expected, etc.). Caught by
  `cargo pgrx install` — pgrx validates the SQL wrapper against the Rust
  signature and refuses to emit a broken extension script.
- **Bootstrap-entid drift.** Caught by `smoke.sql` steps 7–10: if the Rust
  bootstrap entid constants and the `06_bootstrap_data.sql` rows disagree,
  `mentat_transact` will fail when it tries to resolve `:db/ident`.
- **Narrow-storage regressions.** Caught by `smoke.sql` steps 5–6: the nine
  `datoms_*_new` tables and the `dual_write_datoms_trigger` must exist and
  be enabled.

## Running the same checks locally

Everything CI does is reproducible with one command:

```bash
make smoke
```

That is a thin wrapper around `scripts/smoke.sh`, which by default uses the
pgrx-managed cluster at `~/.pgrx/data-16` on port `28816`. The script:

1. `cargo pgrx install --no-default-features --features pg16`
2. Recreates a `pg_mentat_smoke` database.
3. Runs `pg_mentat/tests/smoke.sql` with `ON_ERROR_STOP=on`.
4. Drops the scratch database and prints `smoke: PASS` or `smoke: FAIL`.

If the pgrx cluster is not running, start it first:

```bash
cargo pgrx start pg16
```

To reproduce the full `ci` workflow locally:

```bash
# 1. Build with the same feature set CI uses
(cd pg_mentat && cargo build --no-default-features --features pg16)

# 2. Zero-warnings policy (fails on any clippy warning)
(cd pg_mentat && cargo clippy --no-default-features --features pg16 -- -D warnings)

# 3. Install + SQL smoke test
make smoke

# 4. In-tree #[pg_test] tests
(cd pg_mentat && cargo pgrx test --no-default-features --features pg16 pg16)
```

## Recommended pre-push hook

Drop the following into `.git/hooks/pre-push` and `chmod +x` it so pushes
that would fail CI fail locally first:

```bash
#!/usr/bin/env bash
set -euo pipefail

echo "pre-push: cargo build -p pg_mentat (pg16)"
(cd pg_mentat && cargo build --no-default-features --features pg16)

echo "pre-push: cargo clippy -p pg_mentat -- -D warnings"
(cd pg_mentat && cargo clippy --no-default-features --features pg16 -- -D warnings)

echo "pre-push: make smoke"
make smoke
```

## Environment variables the smoke script understands

| variable  | default (local)                | default (CI)    | meaning                                    |
|-----------|--------------------------------|-----------------|--------------------------------------------|
| `PG_VERSION` | `pg16`                      | `pg16`          | Which pgrx feature flag / install dir.     |
| `PGHOST`  | `$HOME/.pgrx`                  | `localhost`     | Postgres host (or Unix socket directory).  |
| `PGPORT`  | `28816`                        | `5432`          | Postgres port.                             |
| `PGUSER`  | current user                   | `postgres`      | Postgres role.                             |
| `DB_NAME` | `pg_mentat_smoke`              | `pg_mentat_smoke` | Scratch database name.                   |
| `CI`      | unset                          | `1`             | When set, skips pgrx install (uses system PG). |

## When CI fails and local passes

Almost always one of:

1. **Cargo cache skew.** Clear `target/` and `~/.cargo_pg_mentat/registry`.
2. **pgrx init points at a different PG.** Compare `pg_config --version`
   in CI (`$(which pg_config)`) with your local pgrx install.
3. **Uncommitted SQL migration.** `cargo pgrx install` regenerates
   `pg_mentat--1.0.0.sql` from Rust + the `sql/*.sql` includes. If you
   changed a Rust `#[pg_extern]` or a `sql/*.sql` file, re-run
   `make smoke` before pushing.
