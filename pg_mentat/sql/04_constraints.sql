-- Constraints and triggers for pg_mentat
-- Enforces uniqueness constraints and maintains referential integrity

-- Unique value constraint enforcement
-- For attributes with :db/unique :db.unique/value
-- Type-specific unique indexes enforce uniqueness per attribute.
-- We need one per type because values live in different columns.
CREATE UNIQUE INDEX idx_datoms_unique_ref ON mentat.datoms (a, v_ref)
    WHERE added = TRUE AND value_type_tag = 0
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

CREATE UNIQUE INDEX idx_datoms_unique_bool ON mentat.datoms (a, v_bool)
    WHERE added = TRUE AND value_type_tag = 1
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

CREATE UNIQUE INDEX idx_datoms_unique_long ON mentat.datoms (a, v_long)
    WHERE added = TRUE AND value_type_tag = 2
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

CREATE UNIQUE INDEX idx_datoms_unique_double ON mentat.datoms (a, v_double)
    WHERE added = TRUE AND value_type_tag = 3
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

CREATE UNIQUE INDEX idx_datoms_unique_text ON mentat.datoms (a, v_text)
    WHERE added = TRUE AND value_type_tag = 7
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

CREATE UNIQUE INDEX idx_datoms_unique_keyword ON mentat.datoms (a, v_keyword)
    WHERE added = TRUE AND value_type_tag = 8
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

CREATE UNIQUE INDEX idx_datoms_unique_uuid ON mentat.datoms (a, v_uuid)
    WHERE added = TRUE AND value_type_tag = 10
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

-- Function to validate value types match schema
CREATE OR REPLACE FUNCTION mentat.validate_datom_value_type()
RETURNS TRIGGER AS $$
DECLARE
    expected_type mentat.value_type;
    expected_tag SMALLINT;
BEGIN
    -- Get expected value type from schema
    SELECT value_type INTO expected_type
    FROM mentat.schema
    WHERE entid = NEW.a;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attribute % not found in schema', NEW.a;
    END IF;

    -- Map value_type enum to the SMALLINT type tag used in datoms.value_type_tag.
    -- These tags match the encoding in transact.rs encode_value() and the
    -- decoding in query.rs build_value_decode_expr() / pull.rs decode_typed_value().
    expected_tag := CASE expected_type
        WHEN 'ref'::mentat.value_type     THEN 0
        WHEN 'boolean'::mentat.value_type  THEN 1
        WHEN 'long'::mentat.value_type     THEN 2
        WHEN 'double'::mentat.value_type   THEN 3
        WHEN 'instant'::mentat.value_type  THEN 4
        WHEN 'string'::mentat.value_type   THEN 7
        WHEN 'keyword'::mentat.value_type  THEN 8
        WHEN 'uuid'::mentat.value_type     THEN 10
        WHEN 'bytes'::mentat.value_type    THEN 11
    END;

    IF NEW.value_type_tag != expected_tag THEN
        RAISE EXCEPTION 'Value type mismatch for attribute %: expected % (tag %), got tag %',
            NEW.a, expected_type, expected_tag, NEW.value_type_tag;
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to validate value types on insert/update
CREATE TRIGGER validate_datom_value_type_trigger
    BEFORE INSERT OR UPDATE ON mentat.datoms
    FOR EACH ROW
    EXECUTE FUNCTION mentat.validate_datom_value_type();

-- Function to update fulltext search vector
CREATE OR REPLACE FUNCTION mentat.update_fulltext_vector()
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := to_tsvector('english', NEW.text_value);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to automatically update tsvector on insert/update
CREATE TRIGGER update_fulltext_vector_trigger
    BEFORE INSERT OR UPDATE OF text_value ON mentat.fulltext
    FOR EACH ROW
    EXECUTE FUNCTION mentat.update_fulltext_vector();

-- Function to prevent invalid partition modifications
CREATE OR REPLACE FUNCTION mentat.validate_partition()
RETURNS TRIGGER AS $$
BEGIN
    -- Ensure next_entid is within partition bounds
    IF NEW.next_entid < NEW.start_entid OR NEW.next_entid > NEW.end_entid THEN
        RAISE EXCEPTION 'Partition % next_entid (%) must be between start (%) and end (%)',
            NEW.name, NEW.next_entid, NEW.start_entid, NEW.end_entid;
    END IF;

    -- Prevent modifying start/end on existing partitions
    IF TG_OP = 'UPDATE' THEN
        IF OLD.start_entid != NEW.start_entid OR OLD.end_entid != NEW.end_entid THEN
            RAISE EXCEPTION 'Cannot modify partition boundaries for existing partition %', NEW.name;
        END IF;
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to validate partition constraints
CREATE TRIGGER validate_partition_trigger
    BEFORE INSERT OR UPDATE ON mentat.partitions
    FOR EACH ROW
    EXECUTE FUNCTION mentat.validate_partition();
