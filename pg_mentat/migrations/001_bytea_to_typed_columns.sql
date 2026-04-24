-- Migration 001: BYTEA to Typed Columns
--
-- Converts the mentat.datoms table from a single `v BYTEA` column
-- to type-specific columns (v_ref, v_long, v_text, etc.) for correct
-- native PostgreSQL comparisons.
--
-- PROBLEM: The old BYTEA column stored all values as binary blobs.
-- Binary comparison produced wrong results for range queries:
--   "2" > "10" in BYTEA (because 0x32 > 0x31)
-- but "2" < "10" in TEXT (correct lexicographic ordering)
-- and 2 < 10 in BIGINT (correct numeric ordering).
--
-- SOLUTION: Store each value type in its native PostgreSQL column so that
-- comparison operators (=, <, >, BETWEEN) use the correct type semantics.
--
-- USAGE:
--   psql -d your_database -f 001_bytea_to_typed_columns.sql
--
-- This script is idempotent: it checks whether the migration has already
-- been applied before making changes.

BEGIN;

-- Check if migration is needed (old schema has 'v' column)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'mentat'
          AND table_name = 'datoms'
          AND column_name = 'v'
          AND data_type = 'bytea'
    ) THEN
        RAISE NOTICE 'Migration already applied or old v BYTEA column not found. Skipping.';
        RETURN;
    END IF;

    RAISE NOTICE 'Starting BYTEA to typed columns migration...';

    -- Step 1: Add new typed columns
    RAISE NOTICE 'Adding typed value columns...';
    ALTER TABLE mentat.datoms ADD COLUMN IF NOT EXISTS v_ref BIGINT;
    ALTER TABLE mentat.datoms ADD COLUMN IF NOT EXISTS v_bool BOOLEAN;
    ALTER TABLE mentat.datoms ADD COLUMN IF NOT EXISTS v_long BIGINT;
    ALTER TABLE mentat.datoms ADD COLUMN IF NOT EXISTS v_double DOUBLE PRECISION;
    ALTER TABLE mentat.datoms ADD COLUMN IF NOT EXISTS v_text TEXT;
    ALTER TABLE mentat.datoms ADD COLUMN IF NOT EXISTS v_keyword TEXT;
    ALTER TABLE mentat.datoms ADD COLUMN IF NOT EXISTS v_instant TIMESTAMPTZ;
    ALTER TABLE mentat.datoms ADD COLUMN IF NOT EXISTS v_uuid UUID;
    ALTER TABLE mentat.datoms ADD COLUMN IF NOT EXISTS v_bytes BYTEA;

    -- Step 2: Migrate data from BYTEA to typed columns
    -- Type tags: 0=ref, 1=boolean, 2=long, 3=double, 4=instant,
    --            7=string, 8=keyword, 10=uuid, 11=bytes
    --
    -- BYTEA encoding (little-endian):
    --   ref/long: 8-byte i64
    --   boolean:  1 byte (0 or 1)
    --   double:   8-byte f64
    --   instant:  8-byte i64 (microseconds since Unix epoch)
    --   string:   UTF-8 bytes
    --   keyword:  UTF-8 bytes (without leading colon)
    --   uuid:     16 raw bytes
    --   bytes:    raw bytes

    -- Ref values (type_tag = 0): i64 little-endian -> BIGINT
    RAISE NOTICE 'Migrating ref values...';
    UPDATE mentat.datoms
    SET v_ref = (
        get_byte(v, 0)::BIGINT
      | (get_byte(v, 1)::BIGINT << 8)
      | (get_byte(v, 2)::BIGINT << 16)
      | (get_byte(v, 3)::BIGINT << 24)
      | (get_byte(v, 4)::BIGINT << 32)
      | (get_byte(v, 5)::BIGINT << 40)
      | (get_byte(v, 6)::BIGINT << 48)
      | (get_byte(v, 7)::BIGINT << 56)
    )
    WHERE value_type_tag = 0 AND v_ref IS NULL;

    -- Boolean values (type_tag = 1): single byte
    RAISE NOTICE 'Migrating boolean values...';
    UPDATE mentat.datoms
    SET v_bool = (get_byte(v, 0) != 0)
    WHERE value_type_tag = 1 AND v_bool IS NULL;

    -- Long values (type_tag = 2): i64 little-endian -> BIGINT
    RAISE NOTICE 'Migrating long values...';
    UPDATE mentat.datoms
    SET v_long = (
        get_byte(v, 0)::BIGINT
      | (get_byte(v, 1)::BIGINT << 8)
      | (get_byte(v, 2)::BIGINT << 16)
      | (get_byte(v, 3)::BIGINT << 24)
      | (get_byte(v, 4)::BIGINT << 32)
      | (get_byte(v, 5)::BIGINT << 40)
      | (get_byte(v, 6)::BIGINT << 48)
      | (get_byte(v, 7)::BIGINT << 56)
    )
    WHERE value_type_tag = 2 AND v_long IS NULL;

    -- Double values (type_tag = 3): f64 little-endian
    -- PostgreSQL doesn't have a direct BYTEA-to-float cast for LE bytes,
    -- so we use the int64 bit pattern and reinterpret via int8send/float8recv.
    RAISE NOTICE 'Migrating double values...';
    UPDATE mentat.datoms
    SET v_double = float8recv(
        int8send(
            get_byte(v, 0)::BIGINT
          | (get_byte(v, 1)::BIGINT << 8)
          | (get_byte(v, 2)::BIGINT << 16)
          | (get_byte(v, 3)::BIGINT << 24)
          | (get_byte(v, 4)::BIGINT << 32)
          | (get_byte(v, 5)::BIGINT << 40)
          | (get_byte(v, 6)::BIGINT << 48)
          | (get_byte(v, 7)::BIGINT << 56)
        )
    )
    WHERE value_type_tag = 3 AND v_double IS NULL;

    -- Instant values (type_tag = 4): i64 microseconds since Unix epoch -> TIMESTAMPTZ
    RAISE NOTICE 'Migrating instant values...';
    UPDATE mentat.datoms
    SET v_instant = to_timestamp(
        (
            get_byte(v, 0)::BIGINT
          | (get_byte(v, 1)::BIGINT << 8)
          | (get_byte(v, 2)::BIGINT << 16)
          | (get_byte(v, 3)::BIGINT << 24)
          | (get_byte(v, 4)::BIGINT << 32)
          | (get_byte(v, 5)::BIGINT << 40)
          | (get_byte(v, 6)::BIGINT << 48)
          | (get_byte(v, 7)::BIGINT << 56)
        )::DOUBLE PRECISION / 1000000.0
    )
    WHERE value_type_tag = 4 AND v_instant IS NULL;

    -- String values (type_tag = 7): UTF-8 bytes -> TEXT
    RAISE NOTICE 'Migrating string values...';
    UPDATE mentat.datoms
    SET v_text = convert_from(v, 'UTF8')
    WHERE value_type_tag = 7 AND v_text IS NULL;

    -- Keyword values (type_tag = 8): UTF-8 bytes (without leading colon) -> TEXT
    RAISE NOTICE 'Migrating keyword values...';
    UPDATE mentat.datoms
    SET v_keyword = convert_from(v, 'UTF8')
    WHERE value_type_tag = 8 AND v_keyword IS NULL;

    -- UUID values (type_tag = 10): 16 raw bytes -> UUID
    RAISE NOTICE 'Migrating UUID values...';
    UPDATE mentat.datoms
    SET v_uuid = encode(v, 'hex')::UUID
    WHERE value_type_tag = 10 AND v_uuid IS NULL;

    -- Bytes values (type_tag = 11): raw bytes -> BYTEA (just copy)
    RAISE NOTICE 'Migrating raw bytes values...';
    UPDATE mentat.datoms
    SET v_bytes = v
    WHERE value_type_tag = 11 AND v_bytes IS NULL;

    -- Step 3: Verify migration completeness
    RAISE NOTICE 'Verifying migration...';
    IF EXISTS (
        SELECT 1 FROM mentat.datoms
        WHERE v_ref IS NULL AND v_bool IS NULL AND v_long IS NULL
          AND v_double IS NULL AND v_text IS NULL AND v_keyword IS NULL
          AND v_instant IS NULL AND v_uuid IS NULL AND v_bytes IS NULL
    ) THEN
        RAISE EXCEPTION 'Migration incomplete: some rows have no typed value column populated. Check for unknown value_type_tag values.';
    END IF;

    -- Step 4: Drop old indexes that reference 'v' column
    RAISE NOTICE 'Dropping old BYTEA-based indexes...';
    DROP INDEX IF EXISTS mentat.idx_datoms_eavt;
    DROP INDEX IF EXISTS mentat.idx_datoms_aevt;
    DROP INDEX IF EXISTS mentat.idx_datoms_avet;
    DROP INDEX IF EXISTS mentat.idx_datoms_vaet;
    DROP INDEX IF EXISTS mentat.idx_datoms_cardinality;
    DROP INDEX IF EXISTS mentat.idx_datoms_unique_value;

    -- Step 5: Drop old 'v' column
    RAISE NOTICE 'Dropping old v BYTEA column...';
    ALTER TABLE mentat.datoms DROP COLUMN v;

    -- Step 6: Add CHECK constraint
    RAISE NOTICE 'Adding CHECK constraint...';
    ALTER TABLE mentat.datoms ADD CONSTRAINT chk_datom_value CHECK (
        (CASE WHEN v_ref IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_bool IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_long IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_double IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_text IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_keyword IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_instant IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_uuid IS NOT NULL THEN 1 ELSE 0 END
       + CASE WHEN v_bytes IS NOT NULL THEN 1 ELSE 0 END) = 1
    );

    -- Step 7: Create new type-specific indexes
    RAISE NOTICE 'Creating type-specific indexes...';

    CREATE INDEX idx_datoms_eavt ON mentat.datoms (e, a, value_type_tag, tx) WHERE added = TRUE;
    CREATE INDEX idx_datoms_aevt ON mentat.datoms (a, e, value_type_tag, tx) WHERE added = TRUE;

    CREATE INDEX idx_datoms_vaet ON mentat.datoms (v_ref, a, e, tx) WHERE added = TRUE AND value_type_tag = 0;

    CREATE INDEX idx_datoms_avet_ref ON mentat.datoms (a, v_ref, e, tx) WHERE added = TRUE AND value_type_tag = 0;
    CREATE INDEX idx_datoms_avet_long ON mentat.datoms (a, v_long, e, tx) WHERE added = TRUE AND value_type_tag = 2;
    CREATE INDEX idx_datoms_avet_double ON mentat.datoms (a, v_double, e, tx) WHERE added = TRUE AND value_type_tag = 3;
    CREATE INDEX idx_datoms_avet_instant ON mentat.datoms (a, v_instant, e, tx) WHERE added = TRUE AND value_type_tag = 4;
    CREATE INDEX idx_datoms_avet_text ON mentat.datoms (a, v_text, e, tx) WHERE added = TRUE AND value_type_tag = 7;
    CREATE INDEX idx_datoms_avet_keyword ON mentat.datoms (a, v_keyword, e, tx) WHERE added = TRUE AND value_type_tag = 8;
    CREATE INDEX idx_datoms_avet_uuid ON mentat.datoms (a, v_uuid, e, tx) WHERE added = TRUE AND value_type_tag = 10;

    CREATE INDEX idx_datoms_cardinality ON mentat.datoms (e, a, added) INCLUDE (value_type_tag, tx);

    -- Step 8: Create type-specific unique indexes for :db/unique attributes
    CREATE UNIQUE INDEX idx_datoms_unique_ref ON mentat.datoms (a, v_ref)
        WHERE added = TRUE AND value_type_tag = 0
        AND a IN (SELECT entid FROM mentat.schema WHERE unique_constraint IS NOT NULL);
    CREATE UNIQUE INDEX idx_datoms_unique_long ON mentat.datoms (a, v_long)
        WHERE added = TRUE AND value_type_tag = 2
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

    RAISE NOTICE 'Migration complete. Typed columns are now active.';

END $$;

COMMIT;
