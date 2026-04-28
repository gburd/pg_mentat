-- Helper functions for pg_mentat
-- Entity ID allocation, value encoding/decoding, and utility functions

-- Allocate a new entity ID from a partition using lock-free sequences.
-- Returns the next available entid without acquiring row-level locks.
CREATE OR REPLACE FUNCTION mentat.allocate_entid(partition_name TEXT)
RETURNS BIGINT AS $$
BEGIN
    CASE partition_name
        WHEN 'db.part/db' THEN RETURN nextval('mentat.partition_db_seq');
        WHEN 'db.part/user' THEN RETURN nextval('mentat.partition_user_seq');
        WHEN 'db.part/tx' THEN RETURN nextval('mentat.partition_tx_seq');
        ELSE RAISE EXCEPTION 'Partition % does not exist', partition_name;
    END CASE;
END;
$$ LANGUAGE plpgsql;

-- Allocate multiple entity IDs from a partition using lock-free sequences.
-- Returns an array of new entids without acquiring row-level locks.
CREATE OR REPLACE FUNCTION mentat.allocate_entids(partition_name TEXT, count INTEGER)
RETURNS BIGINT[] AS $$
DECLARE
    entids BIGINT[];
    i INTEGER;
BEGIN
    IF count <= 0 THEN
        RAISE EXCEPTION 'Count must be positive';
    END IF;

    entids := ARRAY[]::BIGINT[];
    FOR i IN 1..count LOOP
        entids := array_append(entids, mentat.allocate_entid(partition_name));
    END LOOP;

    RETURN entids;
END;
$$ LANGUAGE plpgsql;

-- Get the current transaction ID
-- Creates a new transaction record if needed
CREATE OR REPLACE FUNCTION mentat.current_tx()
RETURNS BIGINT AS $$
DECLARE
    tx_id BIGINT;
BEGIN
    -- Allocate from :db.part/tx partition using lock-free sequence
    tx_id := nextval('mentat.partition_tx_seq');

    -- Create transaction record
    INSERT INTO mentat.transactions (tx, tx_instant)
    VALUES (tx_id, CURRENT_TIMESTAMP);

    RETURN tx_id;
END;
$$ LANGUAGE plpgsql;

-- Resolve a keyword ident to its entid
CREATE OR REPLACE FUNCTION mentat.resolve_ident(keyword TEXT)
RETURNS BIGINT AS $$
DECLARE
    result BIGINT;
BEGIN
    SELECT entid INTO result
    FROM mentat.idents
    WHERE ident = keyword;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Unknown keyword ident: %', keyword;
    END IF;

    RETURN result;
END;
$$ LANGUAGE plpgsql;

-- Lookup entity by unique attribute value
CREATE OR REPLACE FUNCTION mentat.lookup_ref(attr_ident TEXT, value BYTEA, vtype_tag SMALLINT)
RETURNS BIGINT AS $$
DECLARE
    attr_entid BIGINT;
    entity_id BIGINT;
BEGIN
    -- Resolve attribute ident to entid
    attr_entid := mentat.resolve_ident(attr_ident);

    -- Find entity with this unique value
    SELECT e INTO entity_id
    FROM mentat.datoms
    WHERE a = attr_entid
      AND v = value
      AND value_type_tag = vtype_tag
      AND added = TRUE
    LIMIT 1;

    RETURN entity_id;
END;
$$ LANGUAGE plpgsql;

-- Get all datoms for an entity (current state, not history)
CREATE OR REPLACE FUNCTION mentat.entity_datoms(entity_id BIGINT)
RETURNS TABLE(
    attribute BIGINT,
    value BYTEA,
    value_type SMALLINT,
    transaction BIGINT
) AS $$
BEGIN
    RETURN QUERY
    SELECT a, v, value_type_tag, tx
    FROM mentat.datoms
    WHERE e = entity_id
      AND added = TRUE
    ORDER BY a, tx DESC;
END;
$$ LANGUAGE plpgsql;

-- Fulltext search helper
CREATE OR REPLACE FUNCTION mentat.fulltext_search(query TEXT)
RETURNS TABLE(
    rowid BIGINT,
    text_value TEXT,
    rank REAL
) AS $$
BEGIN
    RETURN QUERY
    SELECT
        f.rowid,
        f.text_value,
        ts_rank_cd(f.search_vector, websearch_to_tsquery('english', query))::REAL as rank
    FROM mentat.fulltext f
    WHERE f.search_vector @@ websearch_to_tsquery('english', query)
    ORDER BY rank DESC;
END;
$$ LANGUAGE plpgsql;

-- Check if an attribute has the :db/index property
CREATE OR REPLACE FUNCTION mentat.is_indexed(attr_entid BIGINT)
RETURNS BOOLEAN AS $$
DECLARE
    result BOOLEAN;
BEGIN
    SELECT indexed INTO result
    FROM mentat.schema
    WHERE entid = attr_entid;

    RETURN COALESCE(result, FALSE);
END;
$$ LANGUAGE plpgsql;

-- Check if an attribute has the :db/unique property
CREATE OR REPLACE FUNCTION mentat.is_unique(attr_entid BIGINT)
RETURNS BOOLEAN AS $$
DECLARE
    result mentat.unique_type;
BEGIN
    SELECT unique_constraint INTO result
    FROM mentat.schema
    WHERE entid = attr_entid;

    RETURN result IS NOT NULL;
END;
$$ LANGUAGE plpgsql;

-- Get the value type for an attribute
CREATE OR REPLACE FUNCTION mentat.attribute_value_type(attr_entid BIGINT)
RETURNS mentat.value_type AS $$
DECLARE
    result mentat.value_type;
BEGIN
    SELECT value_type INTO result
    FROM mentat.schema
    WHERE entid = attr_entid;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attribute % not found', attr_entid;
    END IF;

    RETURN result;
END;
$$ LANGUAGE plpgsql;
