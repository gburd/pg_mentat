-- pg_mentat <-> pg_cron integration helpers.
--
-- pg_cron (https://github.com/citusdata/pg_cron, PostgreSQL license)
-- is the standard scheduler extension: cron-style schedules stored
-- in cron.job, executed by a background worker. Treat as a SOFT
-- dependency.
--
-- pg_cron requires shared_preload_libraries = 'pg_cron' (or
-- 'pg_cron,...') and a cluster restart, plus
-- cron.database_name = '<your-db>' to point the scheduler at the
-- right database. Without those, CREATE EXTENSION pg_cron fails;
-- the helpers here detect via has_pg_cron() and refuse to install
-- jobs if pg_cron is missing.
--
-- Common pg_mentat use cases:
--   * Periodic mentat.partman_run_maintenance() — partition
--     creation/retention.
--   * Periodic CLUSTER / VACUUM ANALYZE on the narrow datom tables.
--   * Periodic refresh of materialized views (FDW caches, vertex
--     views, etc).
--
-- Reference: https://github.com/citusdata/pg_cron

CREATE OR REPLACE FUNCTION mentat.has_pg_cron()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_cron');
$$;

-- Schedule a one-line SQL command on a cron schedule. Returns the
-- pg_cron job id. The job_name is required (pg_cron uses it for
-- updates / deletes).
--
-- Example:
--   SELECT mentat.cron_schedule(
--       'mentat-partman-maint',
--       '0 3 * * *',  -- every day at 03:00 UTC
--       'SELECT mentat.partman_run_maintenance();'
--   );
CREATE OR REPLACE FUNCTION mentat.cron_schedule(
    job_name TEXT,
    schedule TEXT,
    command TEXT
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
DECLARE
    v_id bigint;
BEGIN
    IF NOT mentat.has_pg_cron() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_cron is not installed (or not in shared_preload_libraries). See docs/src/pg_cron.md.';
    END IF;
    -- cron.schedule(name, schedule, command) returns the job id.
    EXECUTE format('SELECT cron.schedule(%L, %L, %L)', job_name, schedule, command)
        INTO v_id;
    RETURN v_id;
END;
$$;

-- Cancel a scheduled job by name.
CREATE OR REPLACE FUNCTION mentat.cron_unschedule(job_name TEXT)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_existed boolean;
BEGIN
    IF NOT mentat.has_pg_cron() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_cron is not installed.';
    END IF;
    EXECUTE format('SELECT cron.unschedule(%L)', job_name) INTO v_existed;
    RETURN v_existed;
END;
$$;

-- Convenience: schedule a daily 03:00 UTC partman maintenance job.
-- Idempotent: removes any existing job with the same name first.
CREATE OR REPLACE FUNCTION mentat.cron_schedule_partman_maintenance(
    schedule TEXT DEFAULT '0 3 * * *'
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT mentat.has_pg_cron() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_cron is not installed.';
    END IF;
    -- Best-effort unschedule; silently ignore if not present.
    BEGIN
        PERFORM mentat.cron_unschedule('mentat-partman-maintenance');
    EXCEPTION WHEN OTHERS THEN NULL;
    END;
    RETURN mentat.cron_schedule(
        'mentat-partman-maintenance',
        schedule,
        'SELECT mentat.partman_run_maintenance();'
    );
END;
$$;

-- Convenience: schedule periodic VACUUM ANALYZE on the narrow
-- datom tables. Useful in append-mostly workloads where autovacuum
-- doesn't keep up with planner statistics.
CREATE OR REPLACE FUNCTION mentat.cron_schedule_vacuum_datoms(
    schedule TEXT DEFAULT '0 4 * * *'
)
RETURNS bigint
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT mentat.has_pg_cron() THEN
        RAISE EXCEPTION ':db.error/missing-extension pg_cron is not installed.';
    END IF;
    BEGIN
        PERFORM mentat.cron_unschedule('mentat-vacuum-datoms');
    EXCEPTION WHEN OTHERS THEN NULL;
    END;
    RETURN mentat.cron_schedule(
        'mentat-vacuum-datoms',
        schedule,
        $vac$
        VACUUM (ANALYZE) mentat.datoms_text_new;
        VACUUM (ANALYZE) mentat.datoms_keyword_new;
        VACUUM (ANALYZE) mentat.datoms_long_new;
        VACUUM (ANALYZE) mentat.datoms_ref_new;
        VACUUM (ANALYZE) mentat.datoms_double_new;
        VACUUM (ANALYZE) mentat.datoms_boolean_new;
        VACUUM (ANALYZE) mentat.datoms_instant_new;
        VACUUM (ANALYZE) mentat.datoms_uuid_new;
        VACUUM (ANALYZE) mentat.datoms_bytes_new;
        $vac$
    );
END;
$$;
