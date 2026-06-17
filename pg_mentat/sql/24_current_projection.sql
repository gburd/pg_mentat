-- pg_mentat current-state projection (item 3 of the 1.5.0 append-only work).
--
-- Nine mentat.current_<type> tables hold ONLY the live datoms -- the
-- "current value" of each (e, a) for cardinality-one attributes, and each
-- live (e, a, v) for cardinality-many. They mirror the nine
-- datoms_<type>_new narrow tables but carry no history: a retraction
-- DELETEs the projection row, an assertion upserts it.
--
-- Purpose:
--   * Cheap current-time reads: a current-time Datalog query reads the
--     projection (a small, cache-dense table) instead of resolving
--     latest-tx-wins over the full append-only log.
--   * Makes append-only safe: once current values live here, the
--     datoms_<type>_new tables are pure history and (in 1.6.0) old
--     history partitions can be dropped wholesale.
--
-- Maintenance is done by the transact path (functions/transact.rs), not by
-- triggers, so it participates in the same transaction and the same
-- store_id / cardinality logic that produces the datoms.
--
-- PK is (store_id, e, a, v): unique per value. Cardinality-one attributes
-- never have more than one v per (e, a) because the transact path retracts
-- the prior value first; cardinality-many legitimately has several.
--
-- fillfactor = 75: these tables take in-place upserts (cardinality-one
-- replace updates the row's v + tx in place), so reserved page space keeps
-- updates HOT and avoids index churn -- the opposite tuning from the
-- append-only history tables (which are fillfactor 100).

CREATE TABLE IF NOT EXISTS mentat.current_ref (
    store_id BIGINT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL, a BIGINT NOT NULL, v BIGINT NOT NULL, tx BIGINT NOT NULL,
    PRIMARY KEY (store_id, e, a, v)
) WITH (fillfactor = 75);

CREATE TABLE IF NOT EXISTS mentat.current_long (
    store_id BIGINT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL, a BIGINT NOT NULL, v BIGINT NOT NULL, tx BIGINT NOT NULL,
    PRIMARY KEY (store_id, e, a, v)
) WITH (fillfactor = 75);

CREATE TABLE IF NOT EXISTS mentat.current_text (
    store_id BIGINT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL, a BIGINT NOT NULL, v TEXT NOT NULL, tx BIGINT NOT NULL,
    PRIMARY KEY (store_id, e, a, v)
) WITH (fillfactor = 75);

CREATE TABLE IF NOT EXISTS mentat.current_double (
    store_id BIGINT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL, a BIGINT NOT NULL, v DOUBLE PRECISION NOT NULL, tx BIGINT NOT NULL,
    PRIMARY KEY (store_id, e, a, v)
) WITH (fillfactor = 75);

CREATE TABLE IF NOT EXISTS mentat.current_instant (
    store_id BIGINT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL, a BIGINT NOT NULL, v TIMESTAMPTZ NOT NULL, tx BIGINT NOT NULL,
    PRIMARY KEY (store_id, e, a, v)
) WITH (fillfactor = 75);

CREATE TABLE IF NOT EXISTS mentat.current_keyword (
    store_id BIGINT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL, a BIGINT NOT NULL, v TEXT NOT NULL, tx BIGINT NOT NULL,
    PRIMARY KEY (store_id, e, a, v)
) WITH (fillfactor = 75);

CREATE TABLE IF NOT EXISTS mentat.current_uuid (
    store_id BIGINT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL, a BIGINT NOT NULL, v UUID NOT NULL, tx BIGINT NOT NULL,
    PRIMARY KEY (store_id, e, a, v)
) WITH (fillfactor = 75);

CREATE TABLE IF NOT EXISTS mentat.current_bytes (
    store_id BIGINT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL, a BIGINT NOT NULL, v BYTEA NOT NULL, tx BIGINT NOT NULL,
    PRIMARY KEY (store_id, e, a, v)
) WITH (fillfactor = 75);

CREATE TABLE IF NOT EXISTS mentat.current_boolean (
    store_id BIGINT NOT NULL DEFAULT 0,
    e BIGINT NOT NULL, a BIGINT NOT NULL, v BOOLEAN NOT NULL, tx BIGINT NOT NULL,
    PRIMARY KEY (store_id, e, a, v)
) WITH (fillfactor = 75);

-- AEVT lookup index per projection table: current-time Datalog patterns
-- of the form [?e :attr ?v] scan by (store_id, a) and return (e, v). The PK
-- already covers (store_id, e, a, v); add (store_id, a, e) INCLUDE (v) so
-- attribute-leading scans are index-only too.
CREATE INDEX IF NOT EXISTS idx_current_ref_aev     ON mentat.current_ref     (store_id, a, e) INCLUDE (v, tx);
CREATE INDEX IF NOT EXISTS idx_current_long_aev    ON mentat.current_long    (store_id, a, e) INCLUDE (v, tx);
CREATE INDEX IF NOT EXISTS idx_current_text_aev    ON mentat.current_text    (store_id, a, e) INCLUDE (v, tx);
CREATE INDEX IF NOT EXISTS idx_current_double_aev  ON mentat.current_double  (store_id, a, e) INCLUDE (v, tx);
CREATE INDEX IF NOT EXISTS idx_current_instant_aev ON mentat.current_instant (store_id, a, e) INCLUDE (v, tx);
CREATE INDEX IF NOT EXISTS idx_current_keyword_aev ON mentat.current_keyword (store_id, a, e) INCLUDE (v, tx);
CREATE INDEX IF NOT EXISTS idx_current_uuid_aev    ON mentat.current_uuid    (store_id, a, e) INCLUDE (v, tx);
CREATE INDEX IF NOT EXISTS idx_current_bytes_aev   ON mentat.current_bytes   (store_id, a, e) INCLUDE (v, tx);
CREATE INDEX IF NOT EXISTS idx_current_boolean_aev ON mentat.current_boolean (store_id, a, e) INCLUDE (v, tx);

-- VAET index on the ref projection: reverse-reference traversal
-- [?e :attr <target>] -> find entities pointing at target. Mirrors the
-- datoms_ref_new VAET index but over current state only.
CREATE INDEX IF NOT EXISTS idx_current_ref_vae ON mentat.current_ref (store_id, v, a, e);

-- These tables take in-place upserts; keep autovacuum aggressive so the
-- HOT-pruning + dead-tuple reclaim keeps pace with churn.
DO $$
DECLARE t TEXT;
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'current_ref', 'current_long', 'current_text', 'current_double',
        'current_instant', 'current_keyword', 'current_uuid', 'current_bytes',
        'current_boolean'
    ] LOOP
        EXECUTE format(
            'ALTER TABLE mentat.%I SET ('
            || 'autovacuum_vacuum_scale_factor = 0, '
            || 'autovacuum_vacuum_threshold = 10000, '
            || 'autovacuum_analyze_scale_factor = 0, '
            || 'autovacuum_analyze_threshold = 10000)',
            t
        );
    END LOOP;
END $$;

-- ---------------------------------------------------------------------------
-- mentat.rebuild_current_projection(store BIGINT DEFAULT 0)
--
-- Repopulate all nine current_<type> tables from the append-only log for one
-- store. Used by the 1.4.0->1.5.0 upgrade migration and as a
-- disaster-recovery / consistency-repair tool.
--
-- Definition of "current": for each (e, a, v) the latest tx wins; the value
-- is live iff that latest row is an assertion (added = true). This is the
-- same latest-tx-wins rule the query engine uses for as-of resolution, so
-- the projection is by construction equal to the current-time query result.
--
-- Runs in one transaction: TRUNCATE + INSERT per type table. Returns the
-- total number of live datoms written.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION mentat.rebuild_current_projection(store BIGINT DEFAULT 0)
RETURNS BIGINT
LANGUAGE plpgsql
AS $$
DECLARE
    total BIGINT := 0;
    n BIGINT;
    rec RECORD;
BEGIN
    FOR rec IN
        SELECT * FROM (VALUES
            ('ref'), ('long'), ('text'), ('double'), ('instant'),
            ('keyword'), ('uuid'), ('bytes'), ('boolean')
        ) AS t(ty)
    LOOP
        -- Clear this store's rows in the projection (not the whole table,
        -- which may hold other stores).
        EXECUTE format('DELETE FROM mentat.current_%s WHERE store_id = $1', rec.ty)
            USING store;

        -- Insert the live current value per (e, a, v): the latest tx row,
        -- kept only when it is an assertion.
        EXECUTE format($q$
            INSERT INTO mentat.current_%1$s (store_id, e, a, v, tx)
            SELECT store_id, e, a, v, tx
            FROM (
                SELECT DISTINCT ON (store_id, e, a, v)
                       store_id, e, a, v, tx, added
                FROM mentat.datoms_%1$s_new
                WHERE store_id = $1
                ORDER BY store_id, e, a, v, tx DESC
            ) latest
            WHERE latest.added
        $q$, rec.ty) USING store;

        GET DIAGNOSTICS n = ROW_COUNT;
        total := total + n;
    END LOOP;

    RETURN total;
END;
$$;

-- ---------------------------------------------------------------------------
-- mentat.verify_current_projection(store BIGINT DEFAULT 0)
--
-- Safety-belt check used before the append-only cutover (item 1) and in
-- tests: confirm the projection matches the latest-tx-wins resolution over
-- the log exactly. Returns the number of (e, a, v) rows that DISAGREE
-- between the projection and a fresh resolution of the log. 0 = consistent.
--
-- A non-zero result is a bug: the transact-path maintenance has drifted
-- from the log. This must be 0 before the in-place flip is removed.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION mentat.verify_current_projection(store BIGINT DEFAULT 0)
RETURNS BIGINT
LANGUAGE plpgsql
AS $$
DECLARE
    mismatches BIGINT := 0;
    n BIGINT;
    rec RECORD;
BEGIN
    FOR rec IN
        SELECT * FROM (VALUES
            ('ref'), ('long'), ('text'), ('double'), ('instant'),
            ('keyword'), ('uuid'), ('bytes'), ('boolean')
        ) AS t(ty)
    LOOP
        -- Symmetric difference between the projection and a freshly-resolved
        -- "current from log" set, on (store_id, e, a, v).
        EXECUTE format($q$
            WITH resolved AS (
                SELECT store_id, e, a, v
                FROM (
                    SELECT DISTINCT ON (store_id, e, a, v)
                           store_id, e, a, v, added
                    FROM mentat.datoms_%1$s_new
                    WHERE store_id = $1
                    ORDER BY store_id, e, a, v, tx DESC
                ) latest
                WHERE latest.added
            ),
            proj AS (
                SELECT store_id, e, a, v FROM mentat.current_%1$s WHERE store_id = $1
            )
            SELECT count(*) FROM (
                (SELECT store_id, e, a, v FROM resolved
                 EXCEPT SELECT store_id, e, a, v FROM proj)
                UNION ALL
                (SELECT store_id, e, a, v FROM proj
                 EXCEPT SELECT store_id, e, a, v FROM resolved)
            ) diff
        $q$, rec.ty) INTO n USING store;
        mismatches := mismatches + n;
    END LOOP;

    RETURN mismatches;
END;
$$;
