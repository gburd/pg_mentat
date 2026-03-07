# Nix Flake Summary

## Overview

The pg_mentat project uses a Nix flake (`flake.nix`) to provide a reproducible development environment. All build dependencies -- Rust toolchain, PostgreSQL, LLVM/Clang, system libraries -- are declared in a single file and pinned via `flake.lock`.

## Files

| File                | Purpose                                      |
|---------------------|----------------------------------------------|
| `flake.nix`         | Nix flake: dev shell, package, checks        |
| `flake.lock`        | Pinned input revisions                       |
| `.envrc`            | direnv auto-activation (`use flake`)         |
| `verify-nix-env.sh` | Verify tools and env vars inside `nix develop` |
| `NIX_SETUP.md`      | Detailed setup and usage instructions        |
| `TESTING_WITH_NIX.md` | Testing guide with all scenarios           |

## Flake Outputs

```
devShells.default     -- development shell with all tools
packages.default      -- built pg_mentat extension (lib + SQL + control)
checks.build          -- same as packages.default (used by `nix flake check`)
```

## Validation Checklist

Run these on any standard Linux system with Nix installed:

| Step | Command | Expected Result |
|------|---------|-----------------|
| 1. Evaluate flake | `nix flake show` | Lists devShells, packages, checks |
| 2. Enter dev shell | `nix develop` | Prints welcome banner with Rust/PG versions |
| 3. Verify environment | `bash verify-nix-env.sh` | All checks pass (green) |
| 4. Install pgrx | `setup-pgrx` | cargo-pgrx ~0.17 installed, pg16 initialized |
| 5. Build extension | `cd pg_mentat && cargo build` | Compiles without error |
| 6. Run tests | `test-pg16` | 38 tests pass |
| 7. Build package | `nix build` | `result/` contains .so, .sql, .control |
| 8. Flake check | `nix flake check` | Builds successfully |

## Expected Test Output

```
running 38 tests
test test_edn_... ok
test test_pg_query_... ok
test test_time_travel_... ok
test test_rules_... ok
test test_fulltext_... ok

test result: ok. 38 passed; 0 failed; 0 ignored
```

Test categories:
- EDN types: 5 tests
- Query: 11 tests
- Time-travel: 7 tests
- Rules: 8 tests
- Full-text search: 7 tests

## Comparison: Containerfile vs Nix Flake

| Dimension            | Containerfile (Podman/Docker)           | Nix Flake                              |
|----------------------|-----------------------------------------|----------------------------------------|
| **Setup**            | `podman build -f Containerfile .`       | `nix develop`                          |
| **Isolation**        | Full OS-level container                 | User-level Nix store                   |
| **Reproducibility**  | Dockerfile layers (mutable base image)  | Content-addressed, lockfile-pinned     |
| **Startup**          | Container boot overhead                 | Instant (cached shell)                 |
| **Disk usage**       | Full image per project                  | Shared Nix store across projects       |
| **IDE support**      | Requires bind mounts / devcontainers    | Native filesystem, no overhead         |
| **CI integration**   | Needs container runtime on runner       | `cachix/install-nix-action` only       |
| **Offline work**     | After initial pull                      | After initial `nix develop`            |
| **Multi-PG testing** | Rebuild image per version               | Add package to flake, re-enter shell   |
| **macOS support**    | Docker Desktop required                 | Native Nix (x86_64 + aarch64)         |

### When to use the Containerfile

- You need exact OS-level isolation (e.g., testing against specific Fedora packages)
- Your CI/CD system already uses container-based runners and adding Nix is not feasible
- You need to produce a deployable container image

### When to use the Nix flake

- Day-to-day development (recommended default)
- GitHub Actions CI
- Onboarding new contributors
- Reproducible builds across machines

## Bugs Fixed in Flake Revision

The original flake had several issues identified during validation:

1. **`nls` package does not exist** in nixpkgs -- replaced with `gettext`
2. **`PKG_CONFIG_PATH` not defined** in `shellEnv` but referenced in `inherit` -- now properly defined
3. **Missing PostgreSQL** from dev shell `buildInputs` -- added `postgresql_16`
4. **Missing `PGDATA`** environment variable -- added
5. **Helper commands not implemented** -- `setup-pgrx`, `test-pg16`, `build-extension`, `install-extension`, `start-postgres` were documented but not defined in the `shellHook`; now implemented
6. **`checks.cargo-test` used raw `cargo test --workspace`** -- this cannot work for a pgrx extension; replaced with the package build as the check
7. **`apps` section used `toString ./.`** which evaluates to a Nix store path at build time, not the source directory at runtime -- removed in favor of shell helpers
8. **`LLVM_CONFIG_PATH`** pointed to `llvm.dev` but `buildInputs` had `libllvm` -- now consistently uses `libllvm.dev`
9. **Package `buildPhase`** used `cargo build --release` directly which does not produce a pgrx-compatible artifact -- now uses `cargo pgrx package`

## Migration Path from Containerfile

For developers currently using the Containerfile:

1. Install Nix with flakes enabled (see `NIX_SETUP.md`)
2. Run `nix develop` in the project root
3. Run `setup-pgrx` once to install cargo-pgrx
4. Use `test-pg16` instead of running `cargo pgrx test` manually
5. The Containerfile remains available as a fallback

No code changes are required -- only the development environment changes.

## CI/CD

Two GitHub Actions workflows are available:

- **`.github/workflows/test.yml`** -- Traditional workflow: installs PostgreSQL and Rust directly on the runner
- **`.github/workflows/nix-test.yml`** -- Nix workflow: uses the flake for a reproducible environment

The Nix workflow is recommended for new setups. Both can run in parallel during migration.

## Resources

- [Nix Flakes reference](https://nixos.org/manual/nix/stable/command-ref/new-cli/nix3-flake)
- [rust-overlay](https://github.com/oxalica/rust-overlay) -- Rust toolchain management for Nix
- [cachix/install-nix-action](https://github.com/cachix/install-nix-action) -- GitHub Actions Nix installer
- [pgrx](https://github.com/pgcentralfoundation/pgrx) -- PostgreSQL extension framework for Rust
