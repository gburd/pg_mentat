-- pg_mentat <-> fuzzystrmatch integration helper.
--
-- fuzzystrmatch is a built-in PostgreSQL contrib extension (PG13+) that
-- provides Levenshtein edit distance, Soundex, Metaphone, and
-- Daitch-Mokotoff phonetic matching as scalar SQL functions. pg_mentat
-- treats it as a SOFT dependency: nothing in pg_mentat requires it, and
-- the Datalog where-fns that compile to its functions only succeed when
-- the extension is installed in the current database.
--
-- Reference: https://www.postgresql.org/docs/current/fuzzystrmatch.html

-- mentat.has_fuzzystrmatch(): true if the contrib extension is installed
-- in this database. Use as a guard in application code or test setup.
CREATE OR REPLACE FUNCTION mentat.has_fuzzystrmatch()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (
        SELECT 1 FROM pg_extension WHERE extname = 'fuzzystrmatch'
    );
$$;
