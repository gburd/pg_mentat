# CI/CD Guide

This document covers how the pg_mentat continuous integration and delivery
pipelines work, how to read their results, and how to debug failures.

## Workflows Overview

| Platform | File | Approach | Status |
|----------|------|----------|--------|
| GitHub Actions | `.github/workflows/nix-test.yml` | Nix flake | Primary |
| GitHub Actions | `.github/workflows/test.yml` | APT + rustup | Legacy / fallback |
| GitLab CI | `.gitlab-ci.yml` | Nix flake | Available |

The **Nix-based workflows** are the recommended default. They use the project's
`flake.nix` to provide every dependency (Rust toolchain, PostgreSQL 16,
LLVM/Clang, system libraries) in a single reproducible environment. No manual
package installation is needed on the runner.

The **traditional workflow** (`test.yml`) installs PostgreSQL and Rust directly
on an Ubuntu runner via APT and `actions-rust-lang/setup-rust-toolchain`. It
serves as a fallback and a compatibility reference.

## GitHub Actions -- Nix Workflow

### Jobs

| Job | Purpose | Depends on |
|-----|---------|------------|
| `test` | Run the 38 pgrx tests against PostgreSQL 16 | -- |
| `build-package` | Build the extension (.so + SQL + control) | -- |
| `lint` | Check formatting (`cargo fmt`) and lints (`cargo clippy`) | -- |

All three jobs run in parallel on `ubuntu-latest`.

### How it works

1. **Checkout** with submodules (the project has workspace path dependencies).
2. **Install Nix** via `cachix/install-nix-action@v27`.
3. **Restore Nix store cache** via `nix-community/cache-nix-action@v6`, keyed
   on `flake.lock`. This avoids re-downloading the entire closure on every run.
4. **Enter the dev shell** (`nix develop --command bash -c '...'`) which exports
   all required environment variables and helper functions.
5. **Run `setup-pgrx`** to install `cargo-pgrx` and initialize it with the
   Nix-provided `pg_config`.
6. **Execute the task** (tests, build, or lint).

### Caching

The Nix store cache is keyed on `flake.lock`. When `flake.lock` changes (e.g.,
after `nix flake update`), the cache is rebuilt from scratch. Otherwise, the
cached store is restored and the dev shell starts almost instantly.

Cargo artifacts inside `target/` are not cached separately because the Nix store
cache already contains the compiled toolchain and all C dependencies. Cargo's
own incremental compilation handles the rest.

### Artifacts

- **`nix-test-failure-logs`** -- uploaded only on test failure; contains the
  pgrx test data directory. Retained for 7 days.
- **`pg_mentat-extension`** -- the built extension package (`.so`, `.sql`,
  `.control`). Retained for 30 days.

### Trigger conditions

- Push to `main`, `claude`, or `develop`
- Pull request targeting `main` or `claude`
- Manual dispatch via the "Run workflow" button

## GitHub Actions -- Traditional Workflow

The traditional workflow in `.github/workflows/test.yml` runs four jobs:

| Job | Purpose |
|-----|---------|
| `test` | Install PG 16 via APT, install pgrx, run tests |
| `lint` | Check `cargo fmt` and `cargo clippy` |
| `build` | Build the extension in release mode, check warnings |
| `integration-check` | Summary job that gates on test + build |

This workflow is more fragile because it depends on the Ubuntu runner having
compatible versions of LLVM, Clang, and system libraries. Use the Nix workflow
when possible.

## GitLab CI

### Pipeline stages

```
check  -->  test  -->  build
 (fmt)      (pg16)     (package)
 (clippy)
```

### Jobs

| Job | Stage | Purpose |
|-----|-------|---------|
| `fmt` | check | `cargo fmt --check` |
| `clippy` | check | `cargo clippy` with deny warnings |
| `test-pg16` | test | Run all 38 pgrx tests |
| `build-extension` | build | Package the extension |

### Image

All jobs use `nixos/nix:latest` as the base image with flakes enabled via the
`NIX_CONFIG` variable.

### Cache

The Nix store (`/nix/store`) is cached and keyed on `flake.lock`. This is a
large cache but significantly reduces pipeline duration after the first run.

### Trigger conditions

- Merge request events
- Pushes to `main`, `claude`, or `develop`

## Debugging Failures

### Test failures

1. Check the job log for the specific test that failed. pgrx test output
   includes the test name and a backtrace (since `RUST_BACKTRACE=1` is set in
   the flake's dev shell).

2. Download the failure artifacts. For GitHub Actions:
   ```
   gh run download <run-id> -n nix-test-failure-logs
   ```
   The artifacts contain the pgrx test data directory with PostgreSQL server
   logs.

3. Reproduce locally:
   ```bash
   nix develop
   setup-pgrx
   test-pg16 --nocapture
   ```

### Build failures

The most common build failure is a missing or incompatible system library.
Inside the Nix dev shell this should not happen because all dependencies are
pinned. If it does:

1. Check that `flake.lock` is committed and up to date.
2. Run `nix flake metadata` to verify the inputs resolve correctly.
3. Try `nix develop --command bash -c 'bash verify-nix-env.sh'` to run the
   environment validation script.

### Cache issues

If you suspect stale cache:

- **GitHub Actions**: delete the cache entry from the Actions tab, or push a
  change to `flake.lock` (`nix flake update`) to rotate the cache key.
- **GitLab CI**: clear the cache from the CI/CD settings page, or change the
  cache key by updating `flake.lock`.

### Clippy failures

Clippy is configured with strict lints in `pg_mentat/Cargo.toml`:

- `unwrap_used = "deny"` -- use `Result` types instead
- `panic = "deny"` -- no panicking in production code
- `todo = "deny"`, `dbg_macro = "deny"` -- no debug leftovers

If clippy fails, fix the lint locally before pushing. Run:
```bash
nix develop --command bash -c 'setup-pgrx && cargo clippy --all-targets -- -D warnings'
```

## Adding New Tests

Tests live inline in `pg_mentat/src/lib.rs` under `#[cfg(any(test, feature = "pg_test"))]`.
They use the pgrx test framework:

```rust
#[pg_test]
fn test_my_feature() -> Result<(), pgrx::spi::Error> {
    // SPI calls here
    Ok(())
}
```

After adding a test, the CI will pick it up automatically. No workflow changes
needed.

## Adding Support for More PostgreSQL Versions

The `pg_mentat/Cargo.toml` already declares features for pg13 through pg18. To
test against another version in CI:

1. Add the PostgreSQL package to `flake.nix` (e.g., `postgresql_17`).
2. Add a corresponding `setup-pgrx` and `test-pgNN` helper in the shell hook.
3. Add a new job (or matrix entry) in the workflow that calls `test-pgNN`.

## Badges

Add these to the project README to show CI status at a glance.

### GitHub Actions

```markdown
[![Tests (Nix)](https://github.com/gburd/pg_mentat/actions/workflows/nix-test.yml/badge.svg)](https://github.com/gburd/pg_mentat/actions/workflows/nix-test.yml)
[![Tests (Traditional)](https://github.com/gburd/pg_mentat/actions/workflows/test.yml/badge.svg)](https://github.com/gburd/pg_mentat/actions/workflows/test.yml)
```

### Static badges

```markdown
[![PostgreSQL 16](https://img.shields.io/badge/PostgreSQL-16-blue)](https://www.postgresql.org/)
[![Rust 1.90](https://img.shields.io/badge/Rust-1.90-orange)](https://www.rust-lang.org/)
[![Nix Flake](https://img.shields.io/badge/Nix-flake-5277C3?logo=nixos)](./flake.nix)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-green)](./LICENSE)
```

## File Reference

| File | Purpose |
|------|---------|
| `.github/workflows/nix-test.yml` | GitHub Actions: Nix-based test, build, lint |
| `.github/workflows/test.yml` | GitHub Actions: traditional APT-based pipeline |
| `.gitlab-ci.yml` | GitLab CI: Nix-based pipeline |
| `flake.nix` | Nix flake defining the dev shell and package |
| `flake.lock` | Pinned Nix input revisions |
| `verify-nix-env.sh` | Script to validate the Nix dev shell environment |
| `NIX_SETUP.md` | How to install and use the Nix environment |
| `NIX_FLAKE_SUMMARY.md` | Flake validation results and architecture notes |
