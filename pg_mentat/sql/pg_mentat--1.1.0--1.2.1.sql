-- pg_mentat upgrade script: 1.1.0 -> 1.2.1
-- Release packaging, no schema changes.
--
-- Apply with: ALTER EXTENSION pg_mentat UPDATE TO '1.2.1';

INSERT INTO mentat.extension_version (version, description)
VALUES ('1.2.1', 'Release packaging: version unification, documentation, CI');
