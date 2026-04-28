# Test Fixtures

Reusable test data for pg_mentat integration testing.

## Usage

These fixtures are primarily for use with `psql` or integration test harnesses
that operate outside the pgrx test framework. The pgrx `#[pg_test]` tests in
`pg_mentat/src/` each define their own schema setup functions for isolation.

## Fixture Files

### `schema_all_types.edn`

Defines one attribute for each of the 9 Datomic value types, plus cardinality-many
and unique-identity variants.

### `schema_social.edn`

A social-network-style schema (person/name, person/age, person/email,
person/friends) for testing reference graphs, pull patterns, and entity
navigation.

### `data_social.edn`

Sample entities for the social schema: 5 people with cross-references.

### `schema_benchmark.edn`

Schema for performance benchmarks: 8 attributes across 6 value types.
