-- pg_mentat <-> PostgreSQL 19 SQL/PGQ (Property Graph Queries) helpers.
--
-- PG19 introduces ISO SQL/PGQ: the GRAPH_TABLE clause and CREATE
-- PROPERTY GRAPH DDL for graph pattern matching over relational
-- data. pg_mentat treats it as a SOFT dependency (PG19+ only).
--
-- The data model fit is partial: pg_mentat's narrow datom tables are
-- EAV-shaped, while SQL/PGQ expects entity-typed vertex tables and
-- typed edge tables. The helper here lets users declare which
-- attributes are vertex labels and which are edge labels; pg_mentat
-- generates a CREATE PROPERTY GRAPH that maps those onto views over
-- the narrow datom storage.
--
-- This is alpha-level: the API and generated DDL may change as PG19
-- ships and as we learn what graph queries users actually run.
--
-- Reference:
--   https://www.postgresql.org/docs/devel/sql-createpropertygraph.html
--   https://www.postgresql.org/docs/devel/queries-graph.html

CREATE OR REPLACE FUNCTION mentat.has_pg19_graph()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT current_setting('server_version_num')::int >= 190000
       AND EXISTS (SELECT 1 FROM pg_proc WHERE proname = 'pg_get_propertygraphdef');
$$;

-- Build the per-attribute vertex view name. Deterministic so users
-- can refer to it from raw SQL queries that mix Datalog + GRAPH_TABLE.
CREATE OR REPLACE FUNCTION mentat._pg19_vertex_view_name(attr_ident TEXT)
RETURNS TEXT
LANGUAGE sql STABLE
AS $$
    SELECT 'v_' || regexp_replace(attr_ident, '[^a-zA-Z0-9_]', '_', 'g');
$$;

-- Build the per-attribute edge view name.
CREATE OR REPLACE FUNCTION mentat._pg19_edge_view_name(attr_ident TEXT)
RETURNS TEXT
LANGUAGE sql STABLE
AS $$
    SELECT 'e_' || regexp_replace(attr_ident, '[^a-zA-Z0-9_]', '_', 'g');
$$;

-- Materialize a vertex view for a string/long/ref attribute.
-- Vertices = entities that have a value for this attribute. The
-- view exposes (e BIGINT, label TEXT) so SQL/PGQ can use the entid
-- as the vertex key and the value as a vertex property.
--
-- Example: mentat.create_vertex_view(':person/name')
-- creates view mentat.v_person_name AS
--   SELECT e, v AS label FROM mentat.datoms_text_new
--   WHERE a = <entid for :person/name> AND added.
CREATE OR REPLACE FUNCTION mentat.create_vertex_view(attr_ident TEXT)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_view TEXT;
    v_type TEXT;
    v_table TEXT;
BEGIN
    SELECT entid, value_type::text INTO v_entid, v_type
        FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    v_view := mentat._pg19_vertex_view_name(attr_ident);

    -- Map value_type to the narrow storage table.
    v_table := CASE v_type
        WHEN 'string'  THEN 'datoms_text_new'
        WHEN 'keyword' THEN 'datoms_keyword_new'
        WHEN 'long'    THEN 'datoms_long_new'
        WHEN 'ref'     THEN 'datoms_ref_new'
        WHEN 'double'  THEN 'datoms_double_new'
        WHEN 'boolean' THEN 'datoms_boolean_new'
        WHEN 'instant' THEN 'datoms_instant_new'
        WHEN 'uuid'    THEN 'datoms_uuid_new'
        WHEN 'bytes'   THEN 'datoms_bytes_new'
        ELSE NULL
    END;
    IF v_table IS NULL THEN
        RAISE EXCEPTION ':db.error/fn-arg attribute % has unsupported value_type %.',
            attr_ident, v_type;
    END IF;

    EXECUTE format(
        'CREATE OR REPLACE VIEW mentat.%I AS ' ||
        'SELECT e, v::TEXT AS label FROM mentat.%I WHERE a = %s AND added',
        v_view, v_table, v_entid
    );
    RETURN 'mentat.' || v_view;
END;
$$;

-- Materialize an edge view for a ref-type attribute. Edges connect
-- entity (e) to entity (v_ref) via the named attribute. The view
-- exposes (id BIGSERIAL, src BIGINT, dst BIGINT, label TEXT) so
-- SQL/PGQ can use it as an edge table.
CREATE OR REPLACE FUNCTION mentat.create_edge_view(attr_ident TEXT)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_view TEXT;
    v_type TEXT;
BEGIN
    SELECT entid, value_type::text INTO v_entid, v_type
        FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    IF v_type <> 'ref' THEN
        RAISE EXCEPTION ':db.error/fn-arg attribute % must be :db.type/ref to be used as an edge (got type %).',
            attr_ident, v_type;
    END IF;
    v_view := mentat._pg19_edge_view_name(attr_ident);

    EXECUTE format(
        'CREATE OR REPLACE VIEW mentat.%I AS ' ||
        'SELECT (e * 100000 + v) AS id, e AS src, v AS dst, %L AS label ' ||
        'FROM mentat.datoms_ref_new WHERE a = %s AND added',
        v_view, attr_ident, v_entid
    );
    RETURN 'mentat.' || v_view;
END;
$$;

-- Drop the vertex/edge view for an attribute. Returns true if dropped.
CREATE OR REPLACE FUNCTION mentat.drop_vertex_view(attr_ident TEXT)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_view TEXT;
    v_existed boolean;
BEGIN
    v_view := mentat._pg19_vertex_view_name(attr_ident);
    SELECT EXISTS (SELECT 1 FROM pg_views WHERE schemaname='mentat' AND viewname=v_view)
        INTO v_existed;
    EXECUTE format('DROP VIEW IF EXISTS mentat.%I', v_view);
    RETURN v_existed;
END;
$$;

CREATE OR REPLACE FUNCTION mentat.drop_edge_view(attr_ident TEXT)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_view TEXT;
    v_existed boolean;
BEGIN
    v_view := mentat._pg19_edge_view_name(attr_ident);
    SELECT EXISTS (SELECT 1 FROM pg_views WHERE schemaname='mentat' AND viewname=v_view)
        INTO v_existed;
    EXECUTE format('DROP VIEW IF EXISTS mentat.%I', v_view);
    RETURN v_existed;
END;
$$;

-- Generate (but do NOT execute) the CREATE PROPERTY GRAPH DDL for a
-- given graph name, vertex-attribute list, and edge-attribute list.
-- Returns the DDL as text so users can review and execute it
-- themselves. The vertex and edge views must already exist (call
-- create_vertex_view / create_edge_view first).
--
-- Example:
--   SELECT mentat.create_property_graph_ddl(
--       'social',
--       ARRAY[':person/name', ':company/name'],
--       ARRAY[':person/employer', ':person/friend']
--   );
CREATE OR REPLACE FUNCTION mentat.create_property_graph_ddl(
    graph_name TEXT,
    vertex_attrs TEXT[],
    edge_attrs TEXT[]
)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    -- Per-attribute clauses
    v_attr TEXT;
    v_vertex_clauses TEXT[] := ARRAY[]::TEXT[];
    v_edge_clauses TEXT[] := ARRAY[]::TEXT[];
    v_src_attr TEXT;
    v_dst_attr TEXT;
BEGIN
    -- DDL generation does not require SQL/PGQ to be available; the
    -- caller decides whether to EXECUTE the returned text on a
    -- PG19+ cluster. The detection helper is exposed separately as
    -- mentat.has_pg19_graph().

    FOREACH v_attr IN ARRAY vertex_attrs LOOP
        v_vertex_clauses := array_append(
            v_vertex_clauses,
            format('mentat.%I LABEL %I', mentat._pg19_vertex_view_name(v_attr),
                   regexp_replace(v_attr, '^:', ''))
        );
    END LOOP;

    FOREACH v_attr IN ARRAY edge_attrs LOOP
        -- For edges we use the first vertex_attr as src/dst label by
        -- default; users can override by hand-editing the DDL.
        v_src_attr := vertex_attrs[1];
        v_dst_attr := vertex_attrs[1];
        v_edge_clauses := array_append(
            v_edge_clauses,
            format('mentat.%I SOURCE mentat.%I DESTINATION mentat.%I LABEL %I',
                mentat._pg19_edge_view_name(v_attr),
                mentat._pg19_vertex_view_name(v_src_attr),
                mentat._pg19_vertex_view_name(v_dst_attr),
                regexp_replace(v_attr, '^:', ''))
        );
    END LOOP;

    RETURN format(
        E'CREATE PROPERTY GRAPH %I\n    VERTEX TABLES (\n        %s\n    )\n    EDGE TABLES (\n        %s\n    );',
        graph_name,
        array_to_string(v_vertex_clauses, E',\n        '),
        array_to_string(v_edge_clauses, E',\n        ')
    );
END;
$$;
