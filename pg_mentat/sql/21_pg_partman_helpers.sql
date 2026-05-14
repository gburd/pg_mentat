-- pg_mentat <-> pg_partman integration helpers.
--
-- pg_partman (https://github.com/pgpartman/pg_partman, PostgreSQL
-- license) is the canonical declarative partition-management
-- extension. It builds on PostgreSQL's native partitioning to
-- automate creation, retention, and rotation of time-/range-based
-- partitions.
--
-- pg_mentat treats it as a SOFT dependency.
--
-- The natural fit is partitioning mentat.transactions on tx_instant
-- — pg_partman creates and rotates monthly/weekly partitions
-- automatically, so old tx history can be dropped or archived
-- without manual maintenance.
--
-- Reference: https://github.com/pgpartman/pg_partman/blob/master/doc/pg_partman.md

CREATE OR REPLACE FUNCTION mentat.has_pg_partman()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_partman');
$$;

-- Bootstrap declarative partitioning on mentat.transactions.
--
-- This is destructive: mentat.transactions is renamed to
-- mentat.transactions_template, and a new partitioned mentat.transactions
-- is created with the same shape, partitioned by tx_instant on the
-- given interval. Existing rows are migrated by pg_partman during
-- create_parent.
--
-- Returns the partman config row's parent_table.
--
-- WARNING: this rewrites the transactions table layout. Run during
-- a maintenance window. The Datalog query path is unaffected
-- because mentat.transactions is queried by tx (the PK), not
-- scanned in bulk.
CREATE OR REPLACE FUNCTION mentat.partman_attach_transactions(
    interval_str TEXT DEFAULT '1 month',
    premake INT DEFAULT 4
)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_count BIGINT;
    v_existing TEXT;
BEGIN
    IF NOT mentat.has_pg_partman() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_partman is not installed in this database. CREATE EXTENSION pg_partman SCHEMA partman;';
    END IF;

    -- Detect whether already partman-managed.
    SELECT parent_table INTO v_existing FROM partman.part_config
        WHERE parent_table = 'mentat.transactions';
    IF v_existing IS NOT NULL THEN
        RAISE NOTICE 'mentat.transactions is already managed by pg_partman; skipping.';
        RETURN v_existing;
    END IF;

    -- Detect whether mentat.transactions is already a partitioned
    -- table (i.e. someone ran this before but registration was
    -- removed). If not, this transformation must convert plain
    -- table to partitioned root.
    -- pg_partman 5.x can manage existing native PARTITION BY
    -- tables; if mentat.transactions is plain, we need a manual
    -- conversion which is more invasive than this helper provides.
    -- Refuse with a clear message in that case.
    IF NOT EXISTS (
        SELECT 1 FROM pg_partitioned_table pt
        JOIN pg_class c ON pt.partrelid = c.oid
        JOIN pg_namespace n ON c.relnamespace = n.oid
        WHERE n.nspname = 'mentat' AND c.relname = 'transactions'
    ) THEN
        RAISE EXCEPTION ':db.error/manual-step mentat.transactions is not a partitioned table. Convert it manually first by recreating it as PARTITION BY RANGE (tx_instant) and migrating rows; then re-run partman_attach_transactions. See docs/src/pg_partman.md.';
    END IF;

    -- Register with pg_partman as a native-partitioned parent.
    PERFORM partman.create_parent(
        p_parent_table := 'mentat.transactions',
        p_control      := 'tx_instant',
        p_type         := 'native',
        p_interval     := interval_str,
        p_premake      := premake
    );
    RETURN 'mentat.transactions';
END;
$$;

-- Set retention on the partman-managed transactions parent.
-- `keep_for` is a string like '90 days' or '6 months'.
CREATE OR REPLACE FUNCTION mentat.partman_set_transaction_retention(keep_for TEXT)
RETURNS void
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT mentat.has_pg_partman() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_partman is not installed.';
    END IF;
    UPDATE partman.part_config
       SET retention = keep_for, retention_keep_table = false
     WHERE parent_table = 'mentat.transactions';
    IF NOT FOUND THEN
        RAISE EXCEPTION ':db.error/missing-config mentat.transactions is not registered with pg_partman. Run mentat.partman_attach_transactions first.';
    END IF;
END;
$$;

-- Run partman maintenance on mentat-managed parents. Typically
-- scheduled via pg_cron; can also be invoked manually.
CREATE OR REPLACE FUNCTION mentat.partman_run_maintenance()
RETURNS void
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT mentat.has_pg_partman() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_partman is not installed.';
    END IF;
    -- Limit to mentat-managed parents.
    PERFORM partman.run_maintenance(
        p_parent_table := pc.parent_table,
        p_analyze      := true
    )
    FROM partman.part_config pc
    WHERE pc.parent_table LIKE 'mentat.%';
END;
$$;
