-- Phase 2 EAV baseline schema.
--
-- A plain Postgres EAV layout with the same indexes pg_mentat would put
-- on equivalent narrow tables. This exists so we can measure what
-- pg_mentat costs ON TOP OF an equivalent storage layout, by running
-- the same workload against both and diffing.
--
-- Design choices:
--   * Mirror pg_mentat's narrow-table storage: one table per value type.
--     This makes it a fair comparison — both engines work against the
--     same index shapes. The pg_mentat extension adds Datalog
--     compilation, schema resolution, and JSON encoding on top.
--   * `store_id = 0` omitted: single-store baseline.
--   * Attribute identifiers are ints (like pg_mentat's entids for
--     attrs), not strings. No cache is needed.
--
-- NOT present in the baseline:
--   * Temporal (`tx`, `added`): out of scope for this comparison.
--     A stricter apples-to-apples would include them; tracked.
--   * History / retraction semantics.
--
-- This schema is created in a separate PostgreSQL schema called `eav`
-- to live alongside the `mentat` schema in the same database.

CREATE SCHEMA IF NOT EXISTS eav;

-- Type-specific value tables
CREATE TABLE IF NOT EXISTS eav.long (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BIGINT NOT NULL,
    PRIMARY KEY (e, a)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS eav.text (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v TEXT   NOT NULL,
    PRIMARY KEY (e, a)
) WITH (fillfactor = 85);

CREATE TABLE IF NOT EXISTS eav.keyword (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v TEXT   NOT NULL,
    PRIMARY KEY (e, a)
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS eav.ref (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v BIGINT NOT NULL,
    PRIMARY KEY (e, a, v)  -- cardinality-many refs (labels) need v in PK
) WITH (fillfactor = 90);

CREATE TABLE IF NOT EXISTS eav.instant (
    e BIGINT NOT NULL,
    a BIGINT NOT NULL,
    v TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (e, a)
) WITH (fillfactor = 90);

-- Indexes mirroring the pg_mentat narrow tables
CREATE INDEX IF NOT EXISTS eav_long_aevt    ON eav.long    (a, e) INCLUDE (v);
CREATE INDEX IF NOT EXISTS eav_text_aevt    ON eav.text    (a, e);
CREATE INDEX IF NOT EXISTS eav_keyword_aevt ON eav.keyword (a, e) INCLUDE (v);
CREATE INDEX IF NOT EXISTS eav_keyword_vaet ON eav.keyword (v, a, e);
CREATE INDEX IF NOT EXISTS eav_ref_aevt     ON eav.ref     (a, e, v);
CREATE INDEX IF NOT EXISTS eav_ref_vaet     ON eav.ref     (v, a, e);
CREATE INDEX IF NOT EXISTS eav_instant_aevt ON eav.instant (a, e) INCLUDE (v);

-- Attribute id -> name (so queries can use meaningful names)
CREATE TABLE IF NOT EXISTS eav.attrs (
    a    BIGINT PRIMARY KEY,
    name TEXT   UNIQUE NOT NULL
);

-- Populate the attribute ids. Keep in sync with gen_dataset.py ATTR_IDS.
INSERT INTO eav.attrs (a, name) VALUES
    (1000, ':user/email'),
    (1001, ':user/name'),
    (1002, ':issue/title'),
    (1003, ':issue/state'),
    (1004, ':issue/priority'),
    (1005, ':issue/assignee'),
    (1006, ':issue/reporter'),
    (1007, ':issue/created-at'),
    (1008, ':issue/label'),
    (1009, ':label/name')
ON CONFLICT (a) DO NOTHING;
