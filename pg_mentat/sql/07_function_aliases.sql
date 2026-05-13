-- Short-name SQL function aliases for pg_mentat
-- Following Datomic conventions: q, t, pull, entity, schema, etc.
--
-- These thin wrappers delegate to the full mentat_* functions so that
-- users can write concise queries such as:
--
--   SELECT mentat.q('[:find ?e :where [?e :person/name "Alice"]]', '{}'::jsonb);
--   SELECT mentat.t('[[:db/add "p" :person/name "Bob"]]');
--   SELECT mentat.pull('[*]', 42);

-- q: query the database with Datalog
CREATE OR REPLACE FUNCTION mentat.q(query TEXT, inputs JSONB DEFAULT '{}'::JSONB)
RETURNS JSONB
LANGUAGE SQL STABLE
AS $$ SELECT public.mentat_query(query, inputs); $$;

-- t: transact EDN data
CREATE OR REPLACE FUNCTION mentat.t(edn_tx TEXT)
RETURNS TEXT
LANGUAGE SQL VOLATILE
AS $$ SELECT public.mentat_transact(edn_tx); $$;

-- pull: pull an attribute pattern for a single entity
CREATE OR REPLACE FUNCTION mentat.pull(pattern TEXT, entity_id BIGINT)
RETURNS JSONB
LANGUAGE SQL STABLE
AS $$ SELECT public.mentat_pull(pattern, entity_id); $$;

-- pull_many: pull an attribute pattern for multiple entities
CREATE OR REPLACE FUNCTION mentat.pull_many(pattern TEXT, entity_ids BIGINT[])
RETURNS JSONB
LANGUAGE SQL STABLE
AS $$ SELECT public.mentat_pull_many(pattern, entity_ids); $$;

-- entity: get all attributes for an entity as JSON
CREATE OR REPLACE FUNCTION mentat.entity(entity_id BIGINT)
RETURNS JSONB
LANGUAGE SQL STABLE
AS $$ SELECT public.mentat_entity(entity_id); $$;

-- schema: inspect the current schema
CREATE OR REPLACE FUNCTION mentat.schema()
RETURNS JSONB
LANGUAGE SQL STABLE
AS $$ SELECT public.mentat_schema(); $$;

-- explain: show the query plan for a Datalog query
CREATE OR REPLACE FUNCTION mentat.explain(query TEXT, inputs JSONB DEFAULT '{}'::JSONB)
RETURNS JSONB
LANGUAGE SQL STABLE
AS $$ SELECT public.mentat_explain(query, inputs); $$;

-- stats: query execution statistics
CREATE OR REPLACE FUNCTION mentat.stats()
RETURNS JSONB
LANGUAGE SQL STABLE
AS $$ SELECT public.mentat_query_stats(); $$;

-- slow_queries: find slow mentat functions
CREATE OR REPLACE FUNCTION mentat.slow_queries(threshold_ms FLOAT8 DEFAULT 100.0)
RETURNS JSONB
LANGUAGE SQL STABLE
AS $$ SELECT public.mentat_slow_queries(threshold_ms); $$;

-- storage: storage and index statistics
CREATE OR REPLACE FUNCTION mentat.storage()
RETURNS JSONB
LANGUAGE SQL STABLE
AS $$ SELECT public.mentat_storage_stats(); $$;

-- cache_stats: prepared statement cache statistics
CREATE OR REPLACE FUNCTION mentat.cache_stats()
RETURNS JSONB
LANGUAGE SQL STABLE
AS $$ SELECT public.mentat_stmt_cache_stats(); $$;

-- cache_clear: clear the prepared statement cache
CREATE OR REPLACE FUNCTION mentat.cache_clear()
RETURNS TEXT
LANGUAGE SQL VOLATILE
AS $$ SELECT public.mentat_stmt_cache_clear(); $$;

-- edn_pretty: backwards compatibility alias (moved to public schema)
CREATE OR REPLACE FUNCTION mentat.edn_pretty(edn_input TEXT, width INT DEFAULT NULL)
RETURNS TEXT
LANGUAGE SQL IMMUTABLE PARALLEL SAFE
AS $$ SELECT public.edn_pretty(edn_input, width); $$;
