-- Constraints and triggers for pg_mentat
-- Enforces uniqueness constraints and maintains referential integrity

-- Unique value constraint enforcement
-- For attributes with :db/unique :db.unique/value
-- Creates a unique index that enforces uniqueness per attribute
CREATE UNIQUE INDEX idx_datoms_unique_value ON mentat.datoms (a, value_type_tag, v)
    WHERE added = TRUE
    AND a IN (
        SELECT entid FROM mentat.schema
        WHERE unique_constraint IS NOT NULL
    );

-- Function to validate value types match schema
CREATE OR REPLACE FUNCTION mentat.validate_datom_value_type()
RETURNS TRIGGER AS $$
DECLARE
    expected_type mentat.value_type;
    type_tag_map CONSTANT INTEGER[] := ARRAY[0, 1, 5, 4, 3, 10, 13, 11, 12];
BEGIN
    -- Get expected value type from schema
    SELECT value_type INTO expected_type
    FROM mentat.schema
    WHERE entid = NEW.a;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attribute % not found in schema', NEW.a;
    END IF;

    -- Map value_type enum to tag (ref=0, boolean=1, instant=5, long=4, etc.)
    -- Validate that value_type_tag matches expected type
    -- This mapping corresponds to mentat's ValueTypeTag constants
    IF NEW.value_type_tag != type_tag_map[(expected_type::TEXT)::INTEGER + 1] THEN
        RAISE EXCEPTION 'Value type mismatch for attribute %: expected %, got tag %',
            NEW.a, expected_type, NEW.value_type_tag;
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
