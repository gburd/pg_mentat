-- pg_mentat 1.5.4 -> 1.5.5 upgrade: VAET indexes on the remaining value tables.
--
-- The transact / lookup-ref resolution probe
--
--     SELECT e FROM mentat.datoms_<type>_new
--      WHERE store_id = $1 AND a = $2 AND v = $3 AND added = true LIMIT 1
--
-- resolves an entity id from a known (attribute, value). It fires once per
-- resolvable ref/upsert value inside mentat.t, on EVERY value type. Only
-- datoms_ref_new and datoms_keyword_new shipped a VAET index
-- (store_id, v, a, e, tx); the other seven value tables resolved this by
-- scanning the AEVT index on (store_id, a) and FILTERing by v. On a
-- high-fanout attribute (millions of rows per (store_id, a)) that scan walks
-- ~all of them per lookup -- a production operator measured ~30x slower on a
-- 1.1M-row attribute, dominating write-path latency.
--
-- This adds the VAET index to the seven tables that were missing it. The
-- value column is already part of each table's primary key
-- (store_id, e, a, v, tx), so a value that fits the PK btree fits this index
-- too -- no new index-row-width risk for text/bytes.
--
-- LOCKING NOTE FOR OPERATORS WITH LARGE EXISTING TABLES
-- -----------------------------------------------------
-- `ALTER EXTENSION pg_mentat UPDATE TO '1.5.5'` runs inside a transaction, so
-- these CREATE INDEX statements CANNOT be CONCURRENTLY (Postgres forbids it in
-- a transaction block). A plain CREATE INDEX takes a SHARE lock that blocks
-- writes to that table for the duration of the build. On tables with tens of
-- millions of live rows under heavy ingest, build the indexes CONCURRENTLY
-- out-of-band FIRST, then run the extension update (these CREATE INDEX
-- IF NOT EXISTS become no-ops):
--
--     CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_datoms_text_new_vaet
--         ON mentat.datoms_text_new (store_id, v, a, e, tx) WHERE added;
--     -- ...repeat for long/double/instant/uuid/bytes/boolean as needed...
--     ALTER EXTENSION pg_mentat UPDATE TO '1.5.5';
--
-- On a fresh or lightly-loaded install just run the update; the in-transaction
-- builds are quick on small/empty tables.

CREATE INDEX IF NOT EXISTS idx_datoms_long_new_vaet
    ON mentat.datoms_long_new (store_id, v, a, e, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_text_new_vaet
    ON mentat.datoms_text_new (store_id, v, a, e, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_double_new_vaet
    ON mentat.datoms_double_new (store_id, v, a, e, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_instant_new_vaet
    ON mentat.datoms_instant_new (store_id, v, a, e, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_uuid_new_vaet
    ON mentat.datoms_uuid_new (store_id, v, a, e, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_bytes_new_vaet
    ON mentat.datoms_bytes_new (store_id, v, a, e, tx) WHERE added;
CREATE INDEX IF NOT EXISTS idx_datoms_boolean_new_vaet
    ON mentat.datoms_boolean_new (store_id, v, a, e, tx) WHERE added;
