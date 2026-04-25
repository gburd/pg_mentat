-- Constraints and triggers for pg_mentat
-- Enforces uniqueness constraints and maintains referential integrity

-- ==========================================================================
-- Unique value constraint enforcement
-- ==========================================================================
-- For attributes with :db/unique :db.unique/value or :db.unique/identity
-- Uniqueness is enforced in Rust code (transact.rs lines 1121-1164) using:
--   1. In-transaction duplicate checking
--   2. Advisory locks (pg_advisory_xact_lock) to prevent race conditions
--   3. Database lookups for existing values
--
-- Database-level unique indexes cannot be used here because PostgreSQL does not
-- support subqueries in index predicates (we would need to filter by attributes
-- marked with :db/unique, which requires a subquery against mentat.schema).
--
-- The Rust implementation provides complete enforcement, so no database-level
-- indexes are needed.

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
