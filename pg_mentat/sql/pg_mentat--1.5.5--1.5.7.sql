-- pg_mentat 1.5.5 -> 1.5.7 upgrade (DIRECT path, bypasses the unsafe 1.5.6
-- migration).
--
-- WHY THIS EDGE EXISTS
-- -------------------
-- The 1.5.5->1.5.6 migration bounded the partition sequences by ALTERing their
-- MAXVALUE to fixed band ceilings (e.g. partition_user_seq MAXVALUE 999999).
-- That is UNSAFE for any store whose sequences had already run past those
-- ceilings: `ALTER SEQUENCE ... MAXVALUE m` errors with
--   "RESTART value (N) cannot be greater than MAXVALUE (m)"
-- when the sequence's last_value N > m, aborting ALTER EXTENSION. Stores that
-- were created before the band bounds existed ran their sequences UNBOUNDED
-- (to bigint max), so a long-lived, write-heavy store overflows the old
-- [1e6,2e6) tx band and [1e4,1e6) user band -- exactly the situation 1.5.6 was
-- meant to describe, now realized. Such a store cannot traverse 1.5.6 at all.
--
-- PostgreSQL picks the SHORTEST update path, so shipping this direct
-- 1.5.5->1.5.7 edge lets a 1.5.5 store jump straight here, skipping the broken
-- 1.5.6 step. (Stores already on 1.5.6 take 1.5.6->1.5.7.)
--
-- WHAT THIS DOES (safe on any sequence position)
-- ---------------------------------------------
-- Bound each partition sequence to GREATEST(intended_ceiling, current head).
-- Never lower a MAXVALUE below the live last_value. Result:
--   * A store still within its bands gets the fail-loud ceiling (as 1.5.6
--     intended).
--   * A store that already overflowed keeps an effectively-unbounded ceiling
--     (bigint max), so the migration NEVER breaks mentat.t. Its historical
--     partition overlap is a separate data-repair concern surfaced by
--     mentat.entid_collision_report() (added in 1.5.7); this migration does
--     not attempt the renumbering (it is destructive and opt-in).
--
-- No id is moved; no data is rewritten.

DO $$
DECLARE
    seqname   TEXT;
    ceil      BIGINT;
    head      BIGINT;
    newmax    BIGINT;
    r         RECORD;
BEGIN
    FOR r IN
        SELECT * FROM (VALUES
            ('mentat.partition_db_seq',   999999::BIGINT),
            ('mentat.partition_user_seq', 999999999999::BIGINT),
            ('mentat.partition_tx_seq',   1999999999999::BIGINT)
        ) AS t(seqname, ceil)
    LOOP
        -- Current head (last_value). last_value is NULL until the first
        -- nextval; treat that as the sequence's start/min.
        EXECUTE format('SELECT last_value FROM %s', r.seqname) INTO head;
        IF head IS NULL THEN
            head := 0;
        END IF;

        -- Never set MAXVALUE below the live head.
        newmax := GREATEST(r.ceil, head);

        EXECUTE format('ALTER SEQUENCE %s MAXVALUE %s', r.seqname, newmax);

        IF newmax > r.ceil THEN
            RAISE NOTICE 'pg_mentat: % has overflowed its intended band (head=%, intended ceiling=%); left MAXVALUE at % (fail-loud disabled for this partition). Run SELECT * FROM mentat.entid_collision_report() to assess historical partition overlap.',
                r.seqname, head, r.ceil, newmax;
        END IF;
    END LOOP;
END $$;

-- === entid-collision diagnostics + repair (also in sql/25 for fresh installs) ===
-- Included here so stores reaching 1.5.7 via upgrade get these functions
-- (extension_sql_file! blocks run only on CREATE EXTENSION, not upgrades).
-- Entity-id partition-collision diagnostics and repair.
--
-- Background: before the partition sequences were bounded (< 1.5.6), a
-- long-lived write-heavy store could run partition_tx_seq / partition_user_seq
-- past their intended bands (old tx band [1e6,2e6), user band [1e4,1e6)) into
-- one another's id space. Because the sequences were unbounded to bigint max,
-- this never failed loud; it silently produced entids that are used as BOTH a
-- transaction id and a user/schema entity id -- one integer, two logical
-- entities. mentat.entity(E) / mentat.q then return the UNION of both.
--
-- These functions let an operator (a) measure the damage and (b) repair it by
-- renumbering the colliding NON-tx entities into a fresh high id band. tx
-- entities keep their ids (their id is woven through every datom's tx column
-- and anchors basis-t / :as-of monotonicity; renumbering them is not safe).
-- The non-tx side is contained: an entity id appears as `e` (all datom
-- tables), as `a` when it is an attribute, and as `v` in the ref tables.
--
-- The repair is DESTRUCTIVE and OPT-IN. It defaults to a dry run.

-- ---------------------------------------------------------------------------
-- mentat.entid_collision_report(store BIGINT DEFAULT 0)
--
-- One row per colliding entid: an entid that is a transaction (present in
-- mentat.transactions) AND also carries at least one non-txInstant datom as an
-- entity. Reports how many non-tx datoms it carries and whether it is an
-- attribute (appears as `a`) or referenced (appears as ref `v`).
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION mentat.entid_collision_report(store BIGINT DEFAULT 0)
RETURNS TABLE (
    entid          BIGINT,
    non_tx_datoms  BIGINT,
    is_attribute   BOOLEAN,
    ref_incoming   BIGINT
)
LANGUAGE sql STABLE
AS $$
    WITH tx_entids AS (
        SELECT tx AS e FROM mentat.transactions
    ),
    non_tx AS (
        -- entids that carry a real (non-txInstant) datom as an entity
        SELECT d.e, count(*) AS n
        FROM mentat.datoms d
        WHERE d.added AND d.a <> 50   -- 50 = :db/txInstant
        GROUP BY d.e
    )
    SELECT
        t.e AS entid,
        nt.n AS non_tx_datoms,
        EXISTS (SELECT 1 FROM mentat.datoms a WHERE a.a = t.e) AS is_attribute,
        (SELECT count(*) FROM mentat.datoms r
          WHERE r.value_type_tag = 0 AND r.v_ref = t.e AND r.added) AS ref_incoming
    FROM tx_entids t
    JOIN non_tx nt ON nt.e = t.e
    ORDER BY t.e
$$;

-- ---------------------------------------------------------------------------
-- mentat.entid_collision_count(store BIGINT DEFAULT 0) -> BIGINT
-- Convenience: number of colliding entids (0 == healthy).
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION mentat.entid_collision_count(store BIGINT DEFAULT 0)
RETURNS BIGINT
LANGUAGE sql STABLE
AS $$
    SELECT count(*) FROM mentat.entid_collision_report(store)
$$;

-- ---------------------------------------------------------------------------
-- mentat.repair_entid_collisions(dry_run BOOLEAN DEFAULT true,
--                                store BIGINT DEFAULT 0)
--
-- Renumber the colliding NON-tx entities into a fresh block at the top of the
-- user partition (allocated from partition_user_seq), rewriting every
-- reference: `e` and `a` in all nine datoms_<type>_new + nine current_<type>
-- tables, and ref `v` (value_type_tag=0) in datoms_ref_new + current_ref.
-- The transaction id keeps its value, so the tx column and basis-t are
-- untouched; only the user/schema entity moves off the shared integer.
--
-- Returns the number of colliding entids remapped. With dry_run=true (default)
-- it makes NO changes and just returns the count that WOULD be remapped.
--
-- SAFETY: run inside your own transaction, take a backup first, and expect a
-- full rewrite of every datom belonging to a colliding entity. Verify with
-- mentat.entid_collision_count() = 0 afterwards.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION mentat.repair_entid_collisions(
    dry_run BOOLEAN DEFAULT true,
    store   BIGINT  DEFAULT 0
)
RETURNS BIGINT
LANGUAGE plpgsql
AS $$
DECLARE
    rec        RECORD;
    new_e      BIGINT;
    remapped   BIGINT := 0;
    ty         TEXT;
    types      TEXT[] := ARRAY['ref','long','text','double','instant',
                               'keyword','uuid','bytes','boolean'];
BEGIN
    FOR rec IN SELECT entid FROM mentat.entid_collision_report(store) LOOP
        remapped := remapped + 1;
        IF dry_run THEN
            CONTINUE;
        END IF;

        -- Allocate a fresh id from the user partition for the NON-tx entity.
        new_e := nextval('mentat.partition_user_seq');

        -- Rewrite `e` and `a` across all typed log tables, but ONLY the
        -- non-tx datoms (a <> 50); the txInstant datom (a = 50) belongs to the
        -- transaction and must keep e = old id.
        FOREACH ty IN ARRAY types LOOP
            EXECUTE format(
                'UPDATE mentat.datoms_%s_new SET e = $1 WHERE store_id = $2 AND e = $3 AND a <> 50',
                ty) USING new_e, store, rec.entid;
            EXECUTE format(
                'UPDATE mentat.datoms_%s_new SET a = $1 WHERE store_id = $2 AND a = $3',
                ty) USING new_e, store, rec.entid;
            EXECUTE format(
                'UPDATE mentat.current_%s SET e = $1 WHERE store_id = $2 AND e = $3',
                ty) USING new_e, store, rec.entid;
            EXECUTE format(
                'UPDATE mentat.current_%s SET a = $1 WHERE store_id = $2 AND a = $3',
                ty) USING new_e, store, rec.entid;
        END LOOP;

        -- Rewrite incoming ref values that point at the moved entity.
        UPDATE mentat.datoms_ref_new SET v = new_e
            WHERE store_id = store AND v = rec.entid AND added;
        UPDATE mentat.current_ref SET v = new_e
            WHERE store_id = store AND v = rec.entid;

        -- Keep the schema / idents catalogs in sync if the moved entity was an
        -- attribute (registered by entid).
        UPDATE mentat.schema SET entid = new_e WHERE entid = rec.entid;
        UPDATE mentat.idents  SET entid = new_e WHERE entid = rec.entid;
    END LOOP;

    RETURN remapped;
END;
$$;
