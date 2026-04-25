-- Constraints and triggers for pg_mentat
-- Enforces uniqueness constraints and maintains referential integrity

-- ==========================================================================
-- Unique value constraint enforcement
-- ==========================================================================
-- For attributes with :db/unique :db.unique/value or :db.unique/identity
-- Type-specific unique indexes enforce uniqueness per attribute.
-- We need one per type because values live in different columns.
--
-- With partitioned datoms table, unique indexes are created on the specific
-- partition for each type (PostgreSQL requires unique indexes on partitions,
-- not the parent table, unless they include the partition key).
--
-- NOTE: Uniqueness is also enforced in Rust (transact.rs validate_datom_constraints)
-- using advisory locks. These indexes serve as a database-level safety net.
-- Only the most common unique-value types are indexed; rare types (bool, double,
-- uuid) rely on the Rust-level enforcement.

-- Unique ref values (on ref partition)
CREATE UNIQUE INDEX idx_datoms_unique_ref ON mentat.datoms_ref (a, v_ref)
    WHERE added = TRUE
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

-- Unique long values (on long partition)
CREATE UNIQUE INDEX idx_datoms_unique_long ON mentat.datoms_long (a, v_long)
    WHERE added = TRUE
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

-- Unique text values (on text partition)
CREATE UNIQUE INDEX idx_datoms_unique_text ON mentat.datoms_text (a, v_text)
    WHERE added = TRUE
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

-- Unique keyword values (on keyword partition)
CREATE UNIQUE INDEX idx_datoms_unique_keyword ON mentat.datoms_keyword (a, v_keyword)
    WHERE added = TRUE
    AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);

-- ==========================================================================
-- Validation Triggers
-- ==========================================================================

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

-- Trigger on the partitioned parent table (inherited by all partitions)
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
