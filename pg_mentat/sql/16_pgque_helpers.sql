-- pg_mentat <-> PgQue integration helpers.
--
-- PgQue (https://github.com/NikolayS/PgQue, Apache 2.0) is a pure
-- PL/pgSQL revival of Skype's PgQ queue: snapshot-based batching,
-- TRUNCATE-based event-table rotation, zero-bloat under sustained
-- load, no C extension required, no external daemon (optional
-- pg_cron / pg_timetable for ticking). Works on any PG14+.
--
-- This integration is OPTIONAL. The helpers here detect PgQue at
-- runtime via mentat.has_pgque() and refuse to install triggers if
-- it isn't present. The wire shape is a JSON event per pg_mentat
-- transaction, emitted from a deferred constraint trigger that fires
-- at COMMIT time so the datoms for the tx are fully visible when
-- the payload is assembled.
--
-- Reference: https://github.com/NikolayS/PgQue

-- Detect whether PgQue is installed (it's a schema, not a PG
-- extension, so pg_extension lookup doesn't apply).
CREATE OR REPLACE FUNCTION mentat.has_pgque()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (
        SELECT 1 FROM pg_namespace WHERE nspname = 'pgque'
    ) AND EXISTS (
        SELECT 1 FROM pg_proc p
        JOIN pg_namespace n ON p.pronamespace = n.oid
        WHERE n.nspname = 'pgque' AND p.proname = 'send'
    );
$$;

-- Internal: build the per-tx JSON payload by aggregating datoms
-- across the 9 narrow typed tables. Excluded values for binary
-- (`v_bytes`) which are hex-encoded for transport safety.
CREATE OR REPLACE FUNCTION mentat._pgque_build_tx_payload(tx_id BIGINT)
RETURNS jsonb
LANGUAGE sql STABLE
AS $$
    WITH all_datoms AS (
        SELECT e, a, v::text AS v, 'string' AS vt, tx, added FROM mentat.datoms_text_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v, 'keyword', tx, added FROM mentat.datoms_keyword_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'long', tx, added FROM mentat.datoms_long_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'ref', tx, added FROM mentat.datoms_ref_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'double', tx, added FROM mentat.datoms_double_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'boolean', tx, added FROM mentat.datoms_boolean_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'instant', tx, added FROM mentat.datoms_instant_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, v::text, 'uuid', tx, added FROM mentat.datoms_uuid_new WHERE tx = tx_id
        UNION ALL
        SELECT e, a, encode(v, 'hex'), 'bytes', tx, added FROM mentat.datoms_bytes_new WHERE tx = tx_id
    )
    SELECT jsonb_build_object(
        'tx', tx_id,
        'tx_instant', (SELECT tx_instant FROM mentat.transactions WHERE tx = tx_id),
        'store_id', current_setting('mentat.current_store_id', true),
        'datom_count', (SELECT count(*) FROM all_datoms),
        'datoms', COALESCE(
            (SELECT jsonb_agg(
                jsonb_build_object(
                    'e', e, 'a', a, 'v', v, 'vt', vt, 'tx', tx, 'added', added
                ) ORDER BY e, a
            ) FROM all_datoms),
            '[]'::jsonb
        )
    );
$$;

-- Internal: deferred constraint trigger function. Fires at COMMIT
-- time per AFTER INSERT on mentat.transactions so the datom rows
-- for the tx are fully visible. The queue name is passed via TG_ARGV.
--
-- If PgQue isn't present (e.g. user dropped the schema after
-- installing the trigger), the trigger emits a NOTICE and returns
-- gracefully rather than failing the user's transaction.
CREATE OR REPLACE FUNCTION mentat._pgque_emit_tx_trigger()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    v_queue_name TEXT := TG_ARGV[0];
    v_payload jsonb;
BEGIN
    IF NOT mentat.has_pgque() THEN
        RAISE NOTICE 'mentat: PgQue is not installed; skipping emit for tx %', NEW.tx;
        RETURN NULL;
    END IF;

    v_payload := mentat._pgque_build_tx_payload(NEW.tx);

    -- pgque.insert_event takes ev_type + ev_data; keep ev_type = 'mentat.tx'
    -- so consumers can subscribe to a stable event type. The full payload
    -- goes in ev_data as the JSON text representation.
    PERFORM pgque.insert_event(v_queue_name, 'mentat.tx', v_payload::text);
    RETURN NULL;
EXCEPTION WHEN OTHERS THEN
    -- We deliberately swallow exceptions in the deferred trigger to avoid
    -- rolling back user data because of a queue-side problem. The error
    -- is surfaced as a NOTICE so it's still visible in logs.
    RAISE NOTICE 'mentat: pgque emit for queue % tx % failed: %',
        v_queue_name, NEW.tx, SQLERRM;
    RETURN NULL;
END;
$$;

-- Public: enable per-transaction emit to PgQue. Creates a queue if
-- it doesn't already exist, then attaches a deferred constraint
-- trigger to mentat.transactions that calls insert_event at commit
-- time.
--
-- Idempotent: re-running with the same queue name is a no-op.
-- Returns the queue name for chaining.
CREATE OR REPLACE FUNCTION mentat.pgque_emit_tx(queue_name TEXT)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_trig_name TEXT;
BEGIN
    IF NOT mentat.has_pgque() THEN
        RAISE EXCEPTION ':db.error/missing-extension PgQue is not installed in this database. Run \i sql/pgque.sql from the PgQue source tree first.';
    END IF;

    -- Trigger names must be valid identifiers; sanitize the queue name.
    v_trig_name := 'mentat_pgque_emit_' ||
        regexp_replace(queue_name, '[^a-zA-Z0-9_]', '_', 'g');

    -- Create the queue if missing (idempotent).
    PERFORM pgque.create_queue(queue_name);

    -- Attach the deferred constraint trigger if not already present.
    -- DEFERRABLE INITIALLY DEFERRED means it fires once per inserted
    -- row at COMMIT time, by which point the tx's datoms are visible.
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = v_trig_name
          AND tgrelid = 'mentat.transactions'::regclass
    ) THEN
        EXECUTE format(
            'CREATE CONSTRAINT TRIGGER %I AFTER INSERT ON mentat.transactions ' ||
            'DEFERRABLE INITIALLY DEFERRED FOR EACH ROW ' ||
            'EXECUTE FUNCTION mentat._pgque_emit_tx_trigger(%L)',
            v_trig_name, queue_name
        );
    END IF;

    RETURN queue_name;
END;
$$;

-- Public: disable per-transaction emit. Drops the trigger; the
-- queue itself is left intact (consumers may still want to drain it).
-- Returns true if a trigger existed and was dropped.
CREATE OR REPLACE FUNCTION mentat.pgque_disable_tx(queue_name TEXT)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_trig_name TEXT;
    v_existed boolean;
BEGIN
    v_trig_name := 'mentat_pgque_emit_' ||
        regexp_replace(queue_name, '[^a-zA-Z0-9_]', '_', 'g');

    SELECT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = v_trig_name
          AND tgrelid = 'mentat.transactions'::regclass
    ) INTO v_existed;

    IF v_existed THEN
        EXECUTE format(
            'DROP TRIGGER IF EXISTS %I ON mentat.transactions',
            v_trig_name
        );
    END IF;
    RETURN v_existed;
END;
$$;

-- Convenience wrapper: register a consumer on the queue. Pure
-- forwarding to pgque.register_consumer so users don't have to
-- juggle two namespaces in application code that already imports
-- mentat.*.
CREATE OR REPLACE FUNCTION mentat.pgque_register_consumer(
    queue_name TEXT,
    consumer_name TEXT
)
RETURNS integer
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT mentat.has_pgque() THEN
        RAISE EXCEPTION ':db.error/missing-extension PgQue is not installed.';
    END IF;
    RETURN pgque.register_consumer(queue_name, consumer_name);
END;
$$;
