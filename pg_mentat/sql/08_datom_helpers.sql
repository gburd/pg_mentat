-- Datom helper functions for pg_mentat
-- Simplify common datom query patterns over the typed mentat.datoms table.
--
-- These PL/pgSQL functions eliminate verbose WHERE clauses for frequent
-- operations: pattern matching on text, range queries on longs, set
-- membership checks on refs, cardinality-many value collection, and
-- temporal (as-of transaction) lookups.

-- datom_text_like: Find entities whose text attribute matches a LIKE pattern.
--
-- Returns entity IDs where the given attribute's text value matches the
-- supplied SQL LIKE pattern (e.g., 'Alice%', '%@example.com').
-- Only searches the text partition (value_type_tag = 7).
--
-- Example:
--   SELECT * FROM mentat.datom_text_like(':person/name', 'Alice%');
CREATE OR REPLACE FUNCTION mentat.datom_text_like(
    attr_ident TEXT,
    pattern TEXT
)
RETURNS TABLE(entity_id BIGINT, value TEXT, tx BIGINT)
AS $$
DECLARE
    attr_entid BIGINT;
BEGIN
    SELECT mentat.resolve_ident(attr_ident) INTO attr_entid;
    IF attr_entid IS NULL THEN
        RAISE EXCEPTION 'Unknown attribute ident: %', attr_ident;
    END IF;

    RETURN QUERY
    SELECT d.e, d.v_text, d.tx
    FROM mentat.datoms d
    WHERE d.a = attr_entid
      AND d.value_type_tag = 7
      AND d.v_text LIKE pattern
      AND d.added = TRUE;
END;
$$ LANGUAGE plpgsql STABLE;

-- datom_long_between: Find entities whose long attribute falls within a range.
--
-- Returns entity IDs where the given attribute's long value is between
-- low_val and high_val (inclusive). Only searches the long partition
-- (value_type_tag = 2).
--
-- Example:
--   SELECT * FROM mentat.datom_long_between(':person/age', 18, 65);
CREATE OR REPLACE FUNCTION mentat.datom_long_between(
    attr_ident TEXT,
    low_val BIGINT,
    high_val BIGINT
)
RETURNS TABLE(entity_id BIGINT, value BIGINT, tx BIGINT)
AS $$
DECLARE
    attr_entid BIGINT;
BEGIN
    SELECT mentat.resolve_ident(attr_ident) INTO attr_entid;
    IF attr_entid IS NULL THEN
        RAISE EXCEPTION 'Unknown attribute ident: %', attr_ident;
    END IF;

    RETURN QUERY
    SELECT d.e, d.v_long, d.tx
    FROM mentat.datoms d
    WHERE d.a = attr_entid
      AND d.value_type_tag = 2
      AND d.v_long BETWEEN low_val AND high_val
      AND d.added = TRUE;
END;
$$ LANGUAGE plpgsql STABLE;

-- datom_ref_in: Find entities whose ref attribute points to one of a set of targets.
--
-- Returns entity IDs where the given attribute's ref value is in the
-- supplied array of entity IDs. Only searches the ref partition
-- (value_type_tag = 0).
--
-- Example:
--   SELECT * FROM mentat.datom_ref_in(':person/department', ARRAY[100, 101, 102]);
CREATE OR REPLACE FUNCTION mentat.datom_ref_in(
    attr_ident TEXT,
    ref_ids BIGINT[]
)
RETURNS TABLE(entity_id BIGINT, ref_value BIGINT, tx BIGINT)
AS $$
DECLARE
    attr_entid BIGINT;
BEGIN
    SELECT mentat.resolve_ident(attr_ident) INTO attr_entid;
    IF attr_entid IS NULL THEN
        RAISE EXCEPTION 'Unknown attribute ident: %', attr_ident;
    END IF;

    RETURN QUERY
    SELECT d.e, d.v_ref, d.tx
    FROM mentat.datoms d
    WHERE d.a = attr_entid
      AND d.value_type_tag = 0
      AND d.v_ref = ANY(ref_ids)
      AND d.added = TRUE;
END;
$$ LANGUAGE plpgsql STABLE;

-- datom_text_values: Get all current text values for a cardinality-many attribute on an entity.
--
-- Returns all asserted text values for a given entity and attribute.
-- Useful for cardinality-many string attributes (e.g., tags, aliases).
--
-- Example:
--   SELECT * FROM mentat.datom_text_values(100, ':person/alias');
CREATE OR REPLACE FUNCTION mentat.datom_text_values(
    eid BIGINT,
    attr_ident TEXT
)
RETURNS TABLE(value TEXT, tx BIGINT)
AS $$
DECLARE
    attr_entid BIGINT;
BEGIN
    SELECT mentat.resolve_ident(attr_ident) INTO attr_entid;
    IF attr_entid IS NULL THEN
        RAISE EXCEPTION 'Unknown attribute ident: %', attr_ident;
    END IF;

    RETURN QUERY
    SELECT d.v_text, d.tx
    FROM mentat.datoms d
    WHERE d.e = eid
      AND d.a = attr_entid
      AND d.value_type_tag = 7
      AND d.added = TRUE
    ORDER BY d.tx;
END;
$$ LANGUAGE plpgsql STABLE;

-- datom_ref_values: Get all current ref values for a cardinality-many attribute on an entity.
--
-- Returns all asserted ref values for a given entity and attribute.
-- Useful for cardinality-many ref attributes (e.g., parent, friend).
--
-- Example:
--   SELECT * FROM mentat.datom_ref_values(100, ':person/friend');
CREATE OR REPLACE FUNCTION mentat.datom_ref_values(
    eid BIGINT,
    attr_ident TEXT
)
RETURNS TABLE(ref_value BIGINT, tx BIGINT)
AS $$
DECLARE
    attr_entid BIGINT;
BEGIN
    SELECT mentat.resolve_ident(attr_ident) INTO attr_entid;
    IF attr_entid IS NULL THEN
        RAISE EXCEPTION 'Unknown attribute ident: %', attr_ident;
    END IF;

    RETURN QUERY
    SELECT d.v_ref, d.tx
    FROM mentat.datoms d
    WHERE d.e = eid
      AND d.a = attr_entid
      AND d.value_type_tag = 0
      AND d.added = TRUE
    ORDER BY d.tx;
END;
$$ LANGUAGE plpgsql STABLE;

-- datom_value_at_tx: Get the value of an attribute for an entity as of a specific transaction.
--
-- Performs a temporal lookup: finds the most recent assertion of the attribute
-- on the entity at or before the given transaction ID. Returns NULL columns
-- when no value existed at that point.
--
-- The returned row contains all typed value columns; exactly one will be non-NULL
-- (matching the attribute's value type).
--
-- Example:
--   SELECT * FROM mentat.datom_value_at_tx(100, ':person/name', 1000005);
CREATE OR REPLACE FUNCTION mentat.datom_value_at_tx(
    eid BIGINT,
    attr_ident TEXT,
    as_of_tx BIGINT
)
RETURNS TABLE(
    value_type_tag SMALLINT,
    v_ref BIGINT,
    v_bool BOOLEAN,
    v_long BIGINT,
    v_double DOUBLE PRECISION,
    v_text TEXT,
    v_keyword TEXT,
    v_instant TIMESTAMPTZ,
    v_uuid UUID,
    v_bytes BYTEA,
    tx BIGINT
)
AS $$
DECLARE
    attr_entid BIGINT;
BEGIN
    SELECT mentat.resolve_ident(attr_ident) INTO attr_entid;
    IF attr_entid IS NULL THEN
        RAISE EXCEPTION 'Unknown attribute ident: %', attr_ident;
    END IF;

    -- Find the most recent assertion at or before as_of_tx that has not
    -- been retracted by a later transaction (also at or before as_of_tx).
    RETURN QUERY
    SELECT d.value_type_tag,
           d.v_ref, d.v_bool, d.v_long, d.v_double,
           d.v_text, d.v_keyword, d.v_instant, d.v_uuid, d.v_bytes,
           d.tx
    FROM mentat.datoms d
    WHERE d.e = eid
      AND d.a = attr_entid
      AND d.tx <= as_of_tx
      AND d.added = TRUE
      -- Exclude values that were retracted before or at as_of_tx
      AND NOT EXISTS (
          SELECT 1
          FROM mentat.datoms r
          WHERE r.e = d.e
            AND r.a = d.a
            AND r.value_type_tag = d.value_type_tag
            AND r.tx <= as_of_tx
            AND r.tx > d.tx
            AND r.added = FALSE
            -- Match the specific value being retracted
            AND r.v_ref IS NOT DISTINCT FROM d.v_ref
            AND r.v_bool IS NOT DISTINCT FROM d.v_bool
            AND r.v_long IS NOT DISTINCT FROM d.v_long
            AND r.v_double IS NOT DISTINCT FROM d.v_double
            AND r.v_text IS NOT DISTINCT FROM d.v_text
            AND r.v_keyword IS NOT DISTINCT FROM d.v_keyword
            AND r.v_instant IS NOT DISTINCT FROM d.v_instant
            AND r.v_uuid IS NOT DISTINCT FROM d.v_uuid
            AND r.v_bytes IS NOT DISTINCT FROM d.v_bytes
      )
    ORDER BY d.tx DESC
    LIMIT 1;
END;
$$ LANGUAGE plpgsql STABLE;
