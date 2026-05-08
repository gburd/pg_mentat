# Historical SQL migrations

The files in this directory are historical migration scripts that were
drafted during early refactors of the pg_mentat storage layer. **None of
them have been applied to any shipping database.** They are kept here
for reference only and are not wired into `CREATE EXTENSION` or
`ALTER EXTENSION ... UPDATE`.

The current schema is produced by the numbered files in
`pg_mentat/sql/00_schema.sql` through `pg_mentat/sql/10_narrow_storage.sql`,
combined by the pgrx build and installed when `CREATE EXTENSION pg_mentat`
runs.

## Contents

- `migrate_bytea_to_typed.sql` — early sketch of moving from a single
  `bytea` value column to typed columns (`v_long`, `v_text`, ...). The
  current schema was built with typed columns from the start; no live
  data was ever converted.
- `migrate_partition_datoms.sql` — exploration of partitioning
  `mentat.datoms` by transaction range. Not adopted.
- `migrate_reduce_indexes.sql` — a proposal to drop some of the covering
  indexes. Not adopted; the current schema keeps EAVT / AEVT / AVET / VAET.
- `migrate_storage_redesign_phase1.sql` /
  `migrate_storage_redesign_phase2_backfill.sql` — the first cut of the
  narrow-table storage redesign. Superseded by
  `pg_mentat/sql/10_narrow_storage.sql`, which is the version actually
  installed by `CREATE EXTENSION`.

## Why keep them?

Two reasons:

1. They contain design notes and comments that are useful context for
   anyone picking up Phase 1 (storage unification) in the roadmap
   (`docs/ROADMAP.md`).
2. If someone runs an old development cluster that was manually fed one
   of these scripts, having the original text available makes recovery
   easier.

If you are starting from a clean database, ignore this directory.
