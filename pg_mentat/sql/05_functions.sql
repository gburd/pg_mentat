-- Helper functions for pg_mentat
-- Entity ID allocation, value encoding/decoding, and utility functions

-- Allocate a new entity ID from a partition
-- Returns the next available entid and increments the partition counter
CREATE OR REPLACE FUNCTION mentat.allocate_entid(partition_name TEXT)
RETURNS BIGINT AS $$
DECLARE
    new_entid BIGINT;
BEGIN
    UPDATE mentat.partitions
    SET next_entid = next_entid + 1
    WHERE name = partition_name
    RETURNING next_entid - 1 INTO new_entid;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Partition % does not exist', partition_name;
    END IF;

    RETURN new_entid;
END;
$$ LANGUAGE plpgsql;

-- Allocate multiple entity IDs from a partition
-- Returns an array of new entids
CREATE OR REPLACE FUNCTION mentat.allocate_entids(partition_name TEXT, count INTEGER)
RETURNS BIGINT[] AS $$
DECLARE
    start_entid BIGINT;
    entids BIGINT[];
    i INTEGER;
BEGIN
    IF count <= 0 THEN
        RAISE EXCEPTION 'Count must be positive';
    END IF;

    UPDATE mentat.partitions
    SET next_entid = next_entid + count
    WHERE name = partition_name
    RETURNING next_entid - count INTO start_entid;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Partition % does not exist', partition_name;
    END IF;

    -- Build array of allocated entids
    entids := ARRAY[]::BIGINT[];
    FOR i IN 0..(count - 1) LOOP
        entids := array_append(entids, start_entid + i);
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
    -- Allocate from :db.part/tx partition
    tx_id := mentat.allocate_entid('db.part/tx');

    -- Create transaction record
    INSERT INTO mentat.transactions (tx_id, instant)
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
        ts_rank(f.search_vector, websearch_to_tsquery('english', query))::REAL as rank
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
