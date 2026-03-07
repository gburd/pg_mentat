# Contributing to pg_mentat

## Environment Setup

### Recommended: Nix Flake

```bash
nix develop
setup-pgrx
```

This provides Rust 1.90, PostgreSQL 16, LLVM/Clang 18, and all required
libraries. See [NIX_SETUP.md](NIX_SETUP.md) for details.

### Manual Setup

Install the following:

- Rust 1.88+ (stable)
- PostgreSQL 14+ with development headers
- LLVM/Clang development libraries
- cargo-pgrx 0.17.x

```bash
cargo install --locked cargo-pgrx --version '~0.17'
cargo pgrx init --pg16=$(which pg_config)
```

---

## Development Workflow

1. **Create a branch** from `claude`:
   ```bash
   git checkout claude
   git pull
   git checkout -b your-branch-name
   ```

2. **Make changes** in the appropriate crate:
   - `pg_mentat/src/` -- Extension code
   - `mentatd/src/` -- Server code
   - `pg_mentat/sql/` -- Schema DDL

3. **Check your code:**
   ```bash
   cd pg_mentat
   cargo build           # compile
   cargo clippy          # lint (strict -- see below)
   cargo fmt --check     # formatting
   ```

4. **Run tests:**
   ```bash
   cargo pgrx test pg16  # extension tests (requires pgrx setup)
   cd ../mentatd
   cargo test            # server unit tests
   ```

5. **Submit a PR** against the `claude` branch.

---

## Coding Standards

### Clippy Configuration

The project enforces strict clippy lints (defined in `Cargo.toml`):

- `unwrap_used` -- **denied**. Use `?` or `.ok_or()` instead.
- `panic` -- **denied**. Return `Result` types.
- `todo` / `unimplemented` -- **denied**. No placeholder code.
- `dbg_macro` / `print_stdout` / `print_stderr` -- **denied**. Use `pgrx::log!()` or `tracing`.
- `pedantic` -- **warned**. Address where practical.

Test code (`#[cfg(test)]`) relaxes some of these: `unwrap_used`, `expect_used`,
`panic`, and `print_stdout` are allowed in tests.

### SQL in Extension Code

**Always use parameterized queries** via the pgrx SPI API:

```rust
// Correct
Spi::run_with_args(
    "INSERT INTO mentat.datoms (e, a, v, tx, added) VALUES ($1, $2, $3, $4, $5)",
    Some(vec![
        (PgBuiltInOids::INT8OID.oid(), entity_id.into_datum()),
        (PgBuiltInOids::INT8OID.oid(), attr_id.into_datum()),
        // ...
    ]),
)?;

// WRONG -- SQL injection risk
let sql = format!("INSERT INTO mentat.datoms (e, a, v, tx, added) VALUES ({}, {}, ...)", e, a);
Spi::run(&sql)?;
```

### Error Handling

Return `Result` types throughout. Use descriptive error messages:

```rust
fn my_function() -> Result<String, Box<dyn std::error::Error>> {
    let value = Spi::get_one::<String>("SELECT ...")?
        .ok_or("Expected non-NULL result from query")?;
    Ok(value)
}
```

### Documentation

- All public functions require doc comments.
- Use `///` for function docs, `//!` for module docs.
- Include examples in doc comments where helpful.
- Add or update tests when changing functionality.

---

## Testing Requirements

### Before Submitting a PR

1. **Extension compiles** with zero errors: `cargo build`
2. **Clippy passes** with no new warnings: `cargo clippy`
3. **Existing tests pass**: `cargo pgrx test pg16`
4. **New tests added** for new functionality
5. **Formatting**: `cargo fmt`

### Test Structure

Tests live inline in `pg_mentat/src/lib.rs` using the pgrx `#[pg_test]`
attribute. Each test should:

- Call `setup_test_db()` and `bootstrap_schema()` for database setup
- Use descriptive names: `test_pg_query_with_inputs`, not `test1`
- Include assertions that verify specific expected values
- Clean up after itself (pgrx tests run in isolated transactions)

```rust
#[pg_test]
fn test_pg_my_feature() {
    setup_test_db().expect("Failed to setup test db");
    bootstrap_schema().expect("Failed to bootstrap schema");

    // Setup
    Spi::run("INSERT INTO ...").expect("Setup failed");

    // Execute
    let result = Spi::get_one::<String>("SELECT mentat.mentat_query(...)")
        .expect("Query failed");

    // Verify
    let json: serde_json::Value = serde_json::from_str(&result.unwrap())
        .expect("JSON parse failed");
    assert_eq!(json["status"], "ok");
}
```

### mentatd Tests

Server unit tests go in `mentatd/src/` using standard `#[test]` attributes.
Integration tests that require PostgreSQL go in `mentatd/tests/`.

---

## Pull Request Process

1. **Branch naming:** `feature/description`, `fix/description`, or `docs/description`
2. **PR title:** Short, descriptive (under 70 characters)
3. **PR body:** Include what changed, why, and how to test
4. **One concern per PR:** Don't mix unrelated changes
5. **Review:** Get approval before merging to `claude`

---

## Project Resources

- [CURRENT_STATUS.md](CURRENT_STATUS.md) -- What's done, what's pending
- [NIX_SETUP.md](NIX_SETUP.md) -- Environment setup details
- [pg_mentat/README.md](pg_mentat/README.md) -- Extension API reference
- [pgrx documentation](https://github.com/pgcentralfoundation/pgrx)
- [EDN specification](https://github.com/edn-format/edn)

---

## License

By contributing, you agree that your contributions will be licensed under the
Apache License v2.0.
