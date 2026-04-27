# pg_mentat Migration Guide

This guide covers migrating from earlier versions of pg_mentat to the current release, which adds store management, virtual tables, materialized views, time-travel queries, subscriptions, and recursive query support.

## Table of Contents

- [Breaking Changes](#breaking-changes)
- [New Schema Objects](#new-schema-objects)
- [Upgrading from Pre-Store Versions](#upgrading-from-pre-store-versions)
- [Migrating to Multi-Store Architecture](#migrating-to-multi-store-architecture)
- [Function Signature Changes](#function-signature-changes)
- [Configuration Changes](#configuration-changes)
- [Step-by-Step Upgrade](#step-by-step-upgrade)

---

## Breaking Changes

### EdnValue renamed to Edn

The PostgreSQL type name changed from `ednvalue` to `edn`. If you have columns using the old type name, they need to be updated.

**Before:**
```sql
CREATE TABLE my_data (id SERIAL, payload ednvalue);
```

**After:**
```sql
CREATE TABLE my_data (id SERIAL, payload mentat.edn);
```

**Migration:**
```sql
-- The type is automatically renamed during extension upgrade.
-- If you have tables in custom schemas referencing the old type:
ALTER TABLE my_data ALTER COLUMN payload TYPE mentat.edn;
```

### edn_pretty() moved to public schema

The `edn_pretty()` function is now in the `public` schema for convenience. A backwards-compatible alias remains in the `mentat` schema.

**Before:**
```sql
SELECT mentat.edn_pretty('{:a 1}');
```

**After (both work):**
```sql
SELECT edn_pretty('{:a 1}');           -- public schema (preferred)
SELECT mentat.edn_pretty('{:a 1}');    -- alias still works
```

---

## New Schema Objects

The upgrade creates several new tables and views in the `mentat` schema.

### New Tables

| Table | Purpose |
|-------|---------|
| `mentat.stores` | Store registry (name, schema, description, created_at) |
| `mentat.subscriptions` | Active subscription definitions |
| `mentat.matviews` | Materialized view registry |

### New Views (Virtual Tables)

These views are created automatically for each store:

| View | Purpose |
|------|---------|
| `entities` | Entity ID listing with metadata |
| `attributes` | Schema attribute listing |
| `facts` | Human-readable fact triples |
| `text_values` | String-typed values |
| `numeric_values` | Integer/long-typed values |
| `double_values` | Float/double-typed values |
| `boolean_values` | Boolean-typed values |
| `keyword_values` | Keyword-typed values |
| `ref_values` | Reference-typed values |
| `searchable_text` | Full-text search view |

### New Functions

| Category | Functions |
|----------|-----------|
| Store Management | `mentat_create_store`, `mentat_drop_store`, `mentat_list_stores`, `mentat_rename_store` |
| Store-Aware Core | `mentat_transact_in_store`, `mentat_query_in_store`, `mentat_pull_in_store`, `mentat_pull_many_in_store`, `mentat_entity_in_store`, `mentat_schema_in_store` |
| Virtual Tables | `mentat_create_virtual_tables` |
| Materialized Views | `mentat_create_matview`, `mentat_refresh_matview`, `mentat_drop_matview`, `mentat_list_matviews` |
| Time-Travel | `mentat_as_of`, `mentat_since`, `mentat_history` |
| Subscriptions | `mentat_subscribe`, `mentat_unsubscribe`, `mentat_list_subscriptions`, `mentat_notify_subscribers` |
| Recursive Queries | `mentat_recursive_query`, `mentat_ancestors`, `mentat_descendants` |

---

## Upgrading from Pre-Store Versions

### Existing Data

Your existing data in the `mentat` schema is preserved. The existing `mentat` schema becomes the "default" store. No data migration is needed.

```sql
-- Verify existing data is accessible
SELECT mentat_query('[:find ?e :where [?e :person/name _]]', '{}');

-- This is equivalent to:
SELECT mentat_query_in_store('default', '[:find ?e :where [?e :person/name _]]', '{}');
```

### Existing Functions

All existing function signatures remain unchanged. The non-suffixed functions continue to operate on the default store:

```sql
-- These still work exactly as before:
SELECT mentat_transact('[{:db/id "t" :person/name "Test"}]');
SELECT mentat_query('[:find ?name :where [?e :person/name ?name]]', '{}');
SELECT mentat_pull('[*]', 12345);
SELECT mentat_entity(12345);
SELECT mentat_schema();
```

### Extension Upgrade

```sql
-- Upgrade the extension to the latest version
ALTER EXTENSION pg_mentat UPDATE;

-- Verify the upgrade
SELECT extversion FROM pg_extension WHERE extname = 'pg_mentat';
```

---

## Migrating to Multi-Store Architecture

If you want to split existing data into multiple stores (e.g., for multi-tenancy), follow these steps.

### Step 1: Create Target Stores

```sql
SELECT mentat_create_store('tenant_a', 'Tenant A data');
SELECT mentat_create_store('tenant_b', 'Tenant B data');
```

### Step 2: Export Data from Default Store

```sql
-- Export entities for tenant A
SELECT mentat.export_edn(ARRAY(
    SELECT DISTINCT (elem->>0)::BIGINT
    FROM mentat_query('[:find ?e :where [?e :tenant/id "A"]]', '{}') q,
         jsonb_array_elements(q->'results') elem
));
```

### Step 3: Import into Target Store

```sql
-- Import schema first
SELECT mentat_transact_in_store('tenant_a', '<schema EDN>');

-- Import data
SELECT mentat_transact_in_store('tenant_a', '<exported EDN>');
```

### Step 4: Verify and Clean Up

```sql
-- Verify data in new store
SELECT mentat_query_in_store('tenant_a',
    '[:find (count ?e) :where [?e :person/name _]]', '{}');

-- Optionally retract from default store after verification
```

---

## Function Signature Changes

### Temporal Query Inputs

The `inputs` JSONB parameter for `mentat_query` now accepts additional keys:

| Key | Type | Description |
|-----|------|-------------|
| `asOf` | integer | Transaction ID for point-in-time query |
| `since` | integer | Transaction ID for "since" query |
| `history` | boolean | Enable full history mode |
| `limit` | integer | Maximum result rows |
| `offset` | integer | Skip N result rows |

**Example:**
```sql
-- Before: temporal queries not available
-- After: temporal queries via inputs
SELECT mentat_query('[:find ?name :where [?e :person/name ?name]]',
    '{"asOf": 1000005}');
```

### New Dedicated Temporal Functions

For clarity, dedicated functions are also available:

```sql
-- These are equivalent:
SELECT mentat_query('[:find ?name :where [?e :person/name ?name]]', '{"asOf": 1000005}');
SELECT mentat_as_of(1000005, '[:find ?name :where [?e :person/name ?name]]', '{}');
```

---

## Configuration Changes

### New GUC Parameters

No new GUC parameters were added in this release. Existing parameters continue to work:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `mentat.enable_optimizer_hints` | `true` | Enable optimizer hints |
| `mentat.default_work_mem` | `64MB` | Work memory for queries |
| `mentat.max_result_rows` | `0` | Maximum result rows (0 = unlimited) |

---

## Step-by-Step Upgrade

### 1. Back Up

```bash
pg_dump -Fc -d mydb -f mydb_backup.dump
```

### 2. Upgrade Extension

```sql
-- In psql:
ALTER EXTENSION pg_mentat UPDATE;
```

### 3. Verify Core Functions

```sql
-- Test basic operations
SELECT mentat_transact('[{:db/id "test" :person/name "Migration Test"}]');
SELECT mentat_query('[:find ?name :where [?e :person/name ?name]]', '{}');
```

### 4. Generate Virtual Tables

```sql
-- Regenerate virtual tables for the default store
SELECT mentat_create_virtual_tables('default');
```

### 5. Verify Virtual Tables

```sql
SELECT COUNT(*) FROM mentat.entities;
SELECT COUNT(*) FROM mentat.facts;
SELECT COUNT(*) FROM mentat.attributes;
```

### 6. Test New Features

```sql
-- Store management
SELECT mentat_create_store('test_upgrade', 'Upgrade verification');
SELECT mentat_list_stores();
SELECT mentat_drop_store('test_upgrade');

-- Time-travel
SELECT mentat_history(
    '[:find ?name ?tx ?added :where [?e :person/name ?name ?tx ?added]]',
    '{}');

-- Materialized views
SELECT mentat_create_matview('test_mv',
    '[:find ?name :where [?e :person/name ?name]]', '{}');
SELECT * FROM mentat.matview_test_mv;
SELECT mentat_drop_matview('test_mv');
```

### 7. Update Application Code

If your application uses pg_mentat, update it to take advantage of new features:

- Replace ad-hoc temporal logic with `mentat_as_of`/`mentat_since`/`mentat_history`
- Consider using materialized views for dashboard queries
- Use virtual table views for simple SQL-based reporting
- Consider multi-store architecture for tenant isolation

---

## Compatibility Matrix

| Feature | Pre-Store | Current | Notes |
|---------|-----------|---------|-------|
| `mentat_transact` | Supported | Supported | Unchanged |
| `mentat_query` | Supported | Supported | New input keys: asOf, since, history |
| `mentat_pull` | Supported | Supported | Unchanged |
| `mentat_entity` | Supported | Supported | Unchanged |
| `mentat_schema` | Supported | Supported | Unchanged |
| `edn_pretty` | `mentat.edn_pretty` | `public.edn_pretty` | Alias in mentat schema preserved |
| Multi-store | N/A | New | Default store is backwards-compatible |
| Virtual tables | N/A | New | Auto-generated views |
| Materialized views | N/A | New | Opt-in feature |
| Time-travel | Via raw SQL | Built-in functions | mentat_as_of, mentat_since, mentat_history |
| Subscriptions | N/A | New | LISTEN/NOTIFY based |
| Recursive queries | Via Datalog rules | Optimized CTE translation | mentat_ancestors, mentat_descendants |

---

## Getting Help

- File issues at the project repository
- Check `docs/SQL_INTEGRATION.md` for complete function reference
- Use `mentat_explain(query, inputs)` to debug query plans
- Use `mentat_query_stats()` for performance diagnostics
