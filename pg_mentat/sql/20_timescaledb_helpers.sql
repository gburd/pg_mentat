-- pg_mentat <-> TimescaleDB integration helpers.
--
-- TimescaleDB (https://github.com/timescale/timescaledb, Apache 2.0
-- for the OSS edition) is a time-series extension that converts
-- regular tables into "hypertables" — automatically partitioned by
-- a time column. pg_mentat's transaction log
-- (mentat.transactions) and instant-typed datoms
-- (mentat.datoms_instant_new) are natural fits.
--
-- Treat as a SOFT dependency.
--
-- Reference: https://docs.timescale.com/

CREATE OR REPLACE FUNCTION mentat.has_timescaledb()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'timescaledb');
$$;

-- Convert mentat.transactions into a hypertable partitioned by
-- tx_instant. The chunk_time_interval defaults to 1 month — adjust
-- via the argument for high-volume stores. Idempotent: if the
-- table is already a hypertable, this is a no-op.
--
-- Returns the hypertable id from create_hypertable's output.
CREATE OR REPLACE FUNCTION mentat.timescale_attach_transactions(
    chunk_time_interval INTERVAL DEFAULT INTERVAL '1 month'
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
DECLARE
    v_hyper_id bigint;
    v_already boolean;
BEGIN
    IF NOT mentat.has_timescaledb() THEN
        RAISE EXCEPTION ':db.error/missing-extension TimescaleDB is not installed in this database. CREATE EXTENSION timescaledb;';
    END IF;

    -- Skip if already a hypertable.
    SELECT EXISTS (
        SELECT 1 FROM timescaledb_information.hypertables
        WHERE hypertable_schema = 'mentat' AND hypertable_name = 'transactions'
    ) INTO v_already;
    IF v_already THEN
        RAISE NOTICE 'mentat.transactions is already a hypertable; skipping.';
        SELECT id INTO v_hyper_id
            FROM _timescaledb_catalog.hypertable
            WHERE schema_name = 'mentat' AND table_name = 'transactions';
        RETURN v_hyper_id;
    END IF;

    -- create_hypertable returns a SETOF (hypertable_id, schema_name,
    -- table_name, created); call as `(create_hypertable(...)).hypertable_id`.
    EXECUTE format(
        'SELECT (create_hypertable(%L, by_range(%L, %L::INTERVAL), if_not_exists => true)).hypertable_id',
        'mentat.transactions', 'tx_instant', chunk_time_interval
    ) INTO v_hyper_id;

    RETURN v_hyper_id;
END;
$$;

-- Convert mentat.datoms_instant_new into a hypertable partitioned by
-- the v (instant) column. Useful when datom-stream queries are
-- time-window-heavy. Optional; most workloads do fine with the
-- transactions hypertable alone.
CREATE OR REPLACE FUNCTION mentat.timescale_attach_instant_datoms(
    chunk_time_interval INTERVAL DEFAULT INTERVAL '1 month'
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
DECLARE
    v_hyper_id bigint;
    v_already boolean;
BEGIN
    IF NOT mentat.has_timescaledb() THEN
        RAISE EXCEPTION ':db.error/missing-extension TimescaleDB is not installed.';
    END IF;
    SELECT EXISTS (
        SELECT 1 FROM timescaledb_information.hypertables
        WHERE hypertable_schema = 'mentat' AND hypertable_name = 'datoms_instant_new'
    ) INTO v_already;
    IF v_already THEN
        SELECT id INTO v_hyper_id
            FROM _timescaledb_catalog.hypertable
            WHERE schema_name = 'mentat' AND table_name = 'datoms_instant_new';
        RETURN v_hyper_id;
    END IF;
    EXECUTE format(
        'SELECT (create_hypertable(%L, by_range(%L, %L::INTERVAL), if_not_exists => true)).hypertable_id',
        'mentat.datoms_instant_new', 'v', chunk_time_interval
    ) INTO v_hyper_id;
    RETURN v_hyper_id;
END;
$$;

-- Add a retention policy: drop transaction history older than the
-- given interval. WARNING: this drops underlying datoms transactionally.
-- Datalog queries with `:as-of` will fail for txs older than this.
CREATE OR REPLACE FUNCTION mentat.timescale_set_transaction_retention(
    keep_for INTERVAL
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
DECLARE
    v_job_id bigint;
BEGIN
    IF NOT mentat.has_timescaledb() THEN
        RAISE EXCEPTION ':db.error/missing-extension TimescaleDB is not installed.';
    END IF;
    EXECUTE format(
        'SELECT add_retention_policy(%L, INTERVAL %L)',
        'mentat.transactions', keep_for
    ) INTO v_job_id;
    RETURN v_job_id;
END;
$$;
