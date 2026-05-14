-- pg_mentat <-> PostGIS integration helpers.
--
-- PostGIS (https://postgis.net, GPL-2.0+) is the industry-standard
-- geospatial extension for PostgreSQL: WKT/WKB geometry types,
-- GEOS-backed spatial predicates, GiST indexing, and SRID-aware
-- coordinate transforms. pg_mentat treats it as a SOFT dependency
-- via a side-table aux pattern (mirroring pgvector): pg_mentat does
-- not yet add :db.type/geometry to the schema; geometry data lives
-- in per-attribute aux tables keyed by entid.
--
-- The (geom-near ...), (geom-within ...), (geom-contains ...),
-- (geom-intersects ...) Datalog where-fns produce SQL that calls
-- PostGIS's ST_* operators directly. Without PostGIS installed
-- queries compile cleanly and fail at execution with the standard
-- PG "type/operator does not exist" error.
--
-- Reference: https://postgis.net/docs/

CREATE OR REPLACE FUNCTION mentat.has_postgis()
RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'postgis');
$$;

-- Attach a per-attribute geometry aux table.
--
-- `srid` is the EPSG code (4326 for WGS84 lat/long, 3857 for
-- Web Mercator, 0 for unknown). `geom_type` is the PostGIS subtype:
-- 'POINT', 'POLYGON', 'LINESTRING', 'MULTIPOLYGON', 'GEOMETRY' (the
-- generic untyped variant), etc.
--
-- Idempotent. Returns the qualified table name.
CREATE OR REPLACE FUNCTION mentat.attach_geometry_attribute(
    attr_ident TEXT,
    srid INTEGER DEFAULT 4326,
    geom_type TEXT DEFAULT 'GEOMETRY'
)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
BEGIN
    IF NOT mentat.has_postgis() THEN
        RAISE EXCEPTION ':db.error/missing-extension PostGIS is not installed in this database. CREATE EXTENSION postgis;';
    END IF;

    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;

    -- PostGIS rejects unknown geom_type; whitelist common values for
    -- a clearer error than "geometry type X is unknown".
    IF upper(geom_type) NOT IN (
        'GEOMETRY', 'POINT', 'LINESTRING', 'POLYGON',
        'MULTIPOINT', 'MULTILINESTRING', 'MULTIPOLYGON',
        'GEOMETRYCOLLECTION', 'CIRCULARSTRING', 'COMPOUNDCURVE',
        'CURVEPOLYGON', 'MULTICURVE', 'MULTISURFACE'
    ) THEN
        RAISE EXCEPTION ':db.error/fn-arg geom_type % is not a recognized PostGIS geometry subtype.', geom_type;
    END IF;

    v_table := format('attr_%s_geom', v_entid);
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS mentat.%I (' ||
        '    e BIGINT PRIMARY KEY,' ||
        '    geom geometry(%s, %s) NOT NULL' ||
        ')',
        v_table, upper(geom_type), srid
    );
    RETURN 'mentat.' || v_table;
END;
$$;

-- Set or replace the geometry for an entity.
CREATE OR REPLACE FUNCTION mentat.set_geometry(
    e BIGINT,
    attr_ident TEXT,
    wkt_text TEXT,
    srid INTEGER DEFAULT 4326
)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
BEGIN
    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    v_table := format('attr_%s_geom', v_entid);
    EXECUTE format(
        'INSERT INTO mentat.%I (e, geom) VALUES ($1, ST_GeomFromText($2, $3)) ' ||
        'ON CONFLICT (e) DO UPDATE SET geom = EXCLUDED.geom',
        v_table
    ) USING e, wkt_text, srid;
END;
$$;

-- Remove the geometry for an entity. Returns true if a row was deleted.
CREATE OR REPLACE FUNCTION mentat.del_geometry(
    e BIGINT,
    attr_ident TEXT
)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
    v_existed boolean;
BEGIN
    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    v_table := format('attr_%s_geom', v_entid);
    EXECUTE format('DELETE FROM mentat.%I WHERE e = $1 RETURNING true', v_table)
        INTO v_existed USING e;
    RETURN COALESCE(v_existed, false);
END;
$$;

-- Create a GiST spatial index on the aux table. PostGIS's GIST index
-- on geometry columns is what backs every ST_DWithin / ST_Intersects /
-- ST_Contains / ST_Distance KNN query plan. Idempotent.
CREATE OR REPLACE FUNCTION mentat.create_gist_geometry_index(attr_ident TEXT)
RETURNS TEXT
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
    v_idx TEXT;
BEGIN
    IF NOT mentat.has_postgis() THEN
        RAISE EXCEPTION ':db.error/missing-extension PostGIS is not installed.';
    END IF;
    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    v_table := format('attr_%s_geom', v_entid);
    v_idx := format('attr_%s_geom_gist', v_entid);
    EXECUTE format(
        'CREATE INDEX IF NOT EXISTS %I ON mentat.%I USING GIST (geom)',
        v_idx, v_table
    );
    RETURN v_idx;
END;
$$;

-- Detach (drop) a geometry attribute's aux table and any indexes on
-- it. Returns true if the table existed.
CREATE OR REPLACE FUNCTION mentat.detach_geometry_attribute(attr_ident TEXT)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_entid BIGINT;
    v_table TEXT;
    v_existed boolean;
BEGIN
    SELECT entid INTO v_entid FROM mentat.schema WHERE ident = attr_ident;
    IF v_entid IS NULL THEN
        RAISE EXCEPTION ':db.error/unknown-attribute Attribute % is not registered.', attr_ident;
    END IF;
    v_table := format('attr_%s_geom', v_entid);
    SELECT EXISTS (
        SELECT 1 FROM pg_class c
        JOIN pg_namespace n ON c.relnamespace = n.oid
        WHERE n.nspname = 'mentat' AND c.relname = v_table
    ) INTO v_existed;
    EXECUTE format('DROP TABLE IF EXISTS mentat.%I', v_table);
    RETURN v_existed;
END;
$$;
