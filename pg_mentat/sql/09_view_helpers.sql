-- VIEW helpers for pg_mentat
-- Functions that enable creating SQL VIEWs backed by Datalog queries.
--
-- These helpers bridge the gap between Datalog's relational query model
-- and PostgreSQL's VIEW system, allowing Datalog queries to appear as
-- standard SQL tables.

-- Create a SQL VIEW backed by a Datalog query.
--
-- The VIEW is created in the specified schema (default: public) and
-- executes the Datalog query whenever the VIEW is queried via SQL.
--
-- Parameters:
--   view_name  - Name for the VIEW (optionally schema-qualified)
--   datalog    - The Datalog query string
--   inputs     - JSONB inputs (temporal options, input bindings)
--
-- Example:
--   SELECT mentat.create_datalog_view(
--       'people',
--       '[:find ?e ?name :where [?e :person/name ?name]]',
--       '{}'::jsonb
--   );
--   -- Then: SELECT * FROM people;
--
CREATE OR REPLACE FUNCTION mentat.create_datalog_view(
    view_name TEXT,
    datalog TEXT,
    inputs JSONB DEFAULT '{}'::jsonb
)
RETURNS TEXT AS $$
DECLARE
    query_info JSONB;
    generated_sql TEXT;
    columns JSONB;
    col_count INTEGER;
    col_defs TEXT;
    view_sql TEXT;
    safe_view_name TEXT;
    i INTEGER;
    col_name TEXT;
BEGIN
    -- Validate the Datalog query by generating SQL (this also checks for parse errors)
    query_info := mentat.mentat_query_sql(datalog, inputs);
    generated_sql := query_info->>'sql';
    columns := query_info->'columns';
    col_count := jsonb_array_length(columns);

    IF col_count = 0 THEN
        RAISE EXCEPTION 'Datalog query has no :find columns';
    END IF;

    -- Sanitize the view name: only allow valid SQL identifiers
    -- Allow schema-qualified names like "myschema.myview"
    IF view_name !~ '^[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)?$' THEN
        RAISE EXCEPTION 'Invalid view name: %. Use only alphanumeric characters and underscores.', view_name;
    END IF;
    safe_view_name := view_name;

    -- Build the VIEW definition using mentat_query_view() for up to 8 columns
    IF col_count <= 8 THEN
        -- Use mentat_query_view() which returns typed columns
        col_defs := '';
        FOR i IN 0..col_count - 1 LOOP
            col_name := columns->>i;
            IF i > 0 THEN
                col_defs := col_defs || ', ';
            END IF;
            col_defs := col_defs || format('col%s AS %I', i + 1, col_name);
        END LOOP;

        view_sql := format(
            'CREATE OR REPLACE VIEW %s AS SELECT %s FROM mentat.mentat_query_view(%L, %L::jsonb)',
            safe_view_name,
            col_defs,
            datalog,
            inputs::text
        );
    ELSE
        RAISE EXCEPTION 'Datalog queries with more than 8 columns are not supported for VIEWs. '
            'This query has % columns.', col_count;
    END IF;

    -- Execute the CREATE VIEW
    EXECUTE view_sql;

    RETURN format('VIEW %s created with %s columns: %s',
        safe_view_name,
        col_count,
        array_to_string(ARRAY(SELECT jsonb_array_elements_text(columns)), ', ')
    );
END;
$$ LANGUAGE plpgsql;

-- Create a MATERIALIZED VIEW backed by a Datalog query.
--
-- Materialized views cache query results for better performance on
-- expensive queries. Use REFRESH MATERIALIZED VIEW to update the data.
--
-- Parameters:
--   view_name  - Name for the materialized view
--   datalog    - The Datalog query string
--   inputs     - JSONB inputs (temporal options, input bindings)
--
-- Example:
--   SELECT mentat.create_datalog_materialized_view(
--       'people_cache',
--       '[:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]',
--       '{}'::jsonb
--   );
--   -- Then: SELECT * FROM people_cache;
--   -- Refresh: REFRESH MATERIALIZED VIEW people_cache;
--
CREATE OR REPLACE FUNCTION mentat.create_datalog_materialized_view(
    view_name TEXT,
    datalog TEXT,
    inputs JSONB DEFAULT '{}'::jsonb
)
RETURNS TEXT AS $$
DECLARE
    query_info JSONB;
    generated_sql TEXT;
    columns JSONB;
    col_count INTEGER;
    col_defs TEXT;
    view_sql TEXT;
    safe_view_name TEXT;
    i INTEGER;
    col_name TEXT;
BEGIN
    -- Validate the Datalog query
    query_info := mentat.mentat_query_sql(datalog, inputs);
    generated_sql := query_info->>'sql';
    columns := query_info->'columns';
    col_count := jsonb_array_length(columns);

    IF col_count = 0 THEN
        RAISE EXCEPTION 'Datalog query has no :find columns';
    END IF;

    -- Sanitize the view name
    IF view_name !~ '^[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)?$' THEN
        RAISE EXCEPTION 'Invalid view name: %. Use only alphanumeric characters and underscores.', view_name;
    END IF;
    safe_view_name := view_name;

    IF col_count <= 8 THEN
        col_defs := '';
        FOR i IN 0..col_count - 1 LOOP
            col_name := columns->>i;
            IF i > 0 THEN
                col_defs := col_defs || ', ';
            END IF;
            col_defs := col_defs || format('col%s AS %I', i + 1, col_name);
        END LOOP;

        view_sql := format(
            'CREATE MATERIALIZED VIEW %s AS SELECT %s FROM mentat.mentat_query_view(%L, %L::jsonb)',
            safe_view_name,
            col_defs,
            datalog,
            inputs::text
        );
    ELSE
        RAISE EXCEPTION 'Datalog queries with more than 8 columns are not supported for materialized VIEWs. '
            'This query has % columns.', col_count;
    END IF;

    EXECUTE view_sql;

    RETURN format('MATERIALIZED VIEW %s created with %s columns: %s',
        safe_view_name,
        col_count,
        array_to_string(ARRAY(SELECT jsonb_array_elements_text(columns)), ', ')
    );
END;
$$ LANGUAGE plpgsql;

-- Drop a Datalog-backed VIEW.
--
-- Convenience wrapper around DROP VIEW with CASCADE option.
--
-- Parameters:
--   view_name    - Name of the VIEW to drop
--   cascade      - Whether to cascade the drop (default: false)
--   materialized - Whether this is a materialized view (default: false)
--
CREATE OR REPLACE FUNCTION mentat.drop_datalog_view(
    view_name TEXT,
    cascade BOOLEAN DEFAULT FALSE,
    materialized BOOLEAN DEFAULT FALSE
)
RETURNS TEXT AS $$
DECLARE
    drop_sql TEXT;
    safe_view_name TEXT;
BEGIN
    IF view_name !~ '^[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)?$' THEN
        RAISE EXCEPTION 'Invalid view name: %. Use only alphanumeric characters and underscores.', view_name;
    END IF;
    safe_view_name := view_name;

    IF materialized THEN
        drop_sql := format('DROP MATERIALIZED VIEW IF EXISTS %s', safe_view_name);
    ELSE
        drop_sql := format('DROP VIEW IF EXISTS %s', safe_view_name);
    END IF;

    IF cascade THEN
        drop_sql := drop_sql || ' CASCADE';
    END IF;

    EXECUTE drop_sql;

    IF materialized THEN
        RETURN format('MATERIALIZED VIEW %s dropped', safe_view_name);
    ELSE
        RETURN format('VIEW %s dropped', safe_view_name);
    END IF;
END;
$$ LANGUAGE plpgsql;

-- Refresh a Datalog-backed materialized VIEW.
--
-- Parameters:
--   view_name   - Name of the materialized view to refresh
--   concurrently - Whether to refresh concurrently (requires unique index, default: false)
--
CREATE OR REPLACE FUNCTION mentat.refresh_datalog_view(
    view_name TEXT,
    concurrently BOOLEAN DEFAULT FALSE
)
RETURNS TEXT AS $$
DECLARE
    refresh_sql TEXT;
    safe_view_name TEXT;
BEGIN
    IF view_name !~ '^[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)?$' THEN
        RAISE EXCEPTION 'Invalid view name: %. Use only alphanumeric characters and underscores.', view_name;
    END IF;
    safe_view_name := view_name;

    IF concurrently THEN
        refresh_sql := format('REFRESH MATERIALIZED VIEW CONCURRENTLY %s', safe_view_name);
    ELSE
        refresh_sql := format('REFRESH MATERIALIZED VIEW %s', safe_view_name);
    END IF;

    EXECUTE refresh_sql;

    RETURN format('MATERIALIZED VIEW %s refreshed', safe_view_name);
END;
$$ LANGUAGE plpgsql;
