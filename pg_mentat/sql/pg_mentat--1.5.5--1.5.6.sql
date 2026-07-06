-- pg_mentat 1.5.5 -> 1.5.6 upgrade: bound the partition sequences.
--
-- The three entity-id partition sequences shipped with only START WITH and no
-- MAXVALUE, so an exhausted partition silently issued ids that collide with
-- the NEXT partition's space -- a data-integrity hazard. The tx partition band
-- [1000000, 2000000) is also far too small: one tx id is consumed per
-- mentat.t, so a write-heavy store exhausts it in weeks and mentat.t then
-- starts issuing ids in the next partition's range.
--
-- Fresh installs (>= 1.5.6) use a new, disjoint, generous layout:
--   db.part/db   [0,             1e6)   MAXVALUE 999999
--   db.part/user [1e6,           1e12)  MAXVALUE 999999999999
--   db.part/tx   [1e12,          2e12)  MAXVALUE 1999999999999
--
-- EXISTING installs cannot be re-banded in place: their user ids already live
-- in the old [10000, 1000000) band, directly below the old tx band
-- [1000000, 2000000), so user cannot grow upward without hitting live tx ids.
-- What we CAN do safely, and what this migration does:
--
--   1. Bound db and user at their existing band ceilings (fail-loud on
--      exhaustion instead of silent collision). These bands are unchanged;
--      hitting them was already a latent corruption and is now a loud error.
--   2. Raise the tx ceiling far upward (nothing is allocated above tx), which
--      removes the ~weeks-to-exhaustion time bomb without moving any live id.
--
-- All ALTERs are guarded so they are no-ops on a store that is already at the
-- new fresh-install layout (idempotent, and safe to run on either layout).

DO $$
DECLARE
    db_start   BIGINT;
    user_start BIGINT;
    tx_start   BIGINT;
BEGIN
    -- Detect which layout this store is on by the recorded partition starts.
    SELECT start_entid INTO db_start   FROM mentat.partitions WHERE name = 'db.part/db';
    SELECT start_entid INTO user_start FROM mentat.partitions WHERE name = 'db.part/user';
    SELECT start_entid INTO tx_start   FROM mentat.partitions WHERE name = 'db.part/tx';

    IF user_start = 1000000 THEN
        -- New (>= 1.5.6) layout already: bound sequences to the new bands.
        EXECUTE 'ALTER SEQUENCE mentat.partition_db_seq   MINVALUE 100           MAXVALUE 999999';
        EXECUTE 'ALTER SEQUENCE mentat.partition_user_seq  MINVALUE 1000000       MAXVALUE 999999999999';
        EXECUTE 'ALTER SEQUENCE mentat.partition_tx_seq    MINVALUE 1000000000000 MAXVALUE 1999999999999';
    ELSE
        -- Old layout (user band [10000, 1000000), tx band [1000000, 2000000)).
        -- Bound db/user at their existing ceilings; raise the tx ceiling far up
        -- (tx ids stay where they are; only the exhaustion ceiling moves).
        EXECUTE 'ALTER SEQUENCE mentat.partition_db_seq   MINVALUE 1   MAXVALUE 9999';
        EXECUTE 'ALTER SEQUENCE mentat.partition_user_seq MINVALUE 10000 MAXVALUE 999999';
        EXECUTE 'ALTER SEQUENCE mentat.partition_tx_seq   MINVALUE 1000000 MAXVALUE 999999999999';

        -- Record the raised tx ceiling in the metadata table so introspection
        -- and any future retention logic see the true upper bound.
        UPDATE mentat.partitions SET end_entid = 1000000000000 WHERE name = 'db.part/tx';
    END IF;
END $$;
