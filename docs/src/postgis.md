# Geospatial Search via PostGIS

`pg_mentat` integrates with [PostGIS](https://postgis.net/), the
canonical geospatial extension for PostgreSQL: WKT/WKB geometry types,
GEOS-backed spatial predicates, GiST spatial indexing, and SRID-aware
coordinate transforms. With PostGIS attached, pg_mentat queries can
filter and rank entities by spatial relationships — distance,
containment, intersection — alongside the rest of Datalog's pattern
matching.

PostGIS is an **optional** dependency. Detect with
`mentat.has_postgis()`. The integration uses a side-table aux
pattern (mirroring pgvector): pg_mentat does not yet add
`:db.type/geometry` to the schema; geometry data lives in
per-attribute auxiliary tables keyed by entid.

## Side-table model

| Step | API |
|---|---|
| Detect availability | `SELECT mentat.has_postgis();` |
| Attach an aux table | `SELECT mentat.attach_geometry_attribute(':place/loc', 4326, 'POINT');` |
| Insert / update | `SELECT mentat.set_geometry(?e, ':place/loc', 'POINT(...)');` |
| Delete | `SELECT mentat.del_geometry(?e, ':place/loc');` |
| Build a GiST index | `SELECT mentat.create_gist_geometry_index(':place/loc');` |
| KNN search | `[(geom-near $ :place/loc "POINT(0 0)" 5) [[?e ?dist]]]` |
| Within-radius filter | `[(geom-within $ :place/loc "POINT(0 0)" 100.0) [[?e ?dist]]]` |
| Containment filter | `[(geom-contains $ :place/loc "POINT(0 0)") [[?e]]]` |
| Intersection filter | `[(geom-intersects $ :place/loc "POLYGON((...))") [[?e]]]` |
| Detach | `SELECT mentat.detach_geometry_attribute(':place/loc');` |

Each `attach_geometry_attribute(:attr, srid, geom_type)` creates:

```sql
CREATE TABLE mentat.attr_<entid>_geom(
    e BIGINT PRIMARY KEY,
    geom geometry(<geom_type>, <srid>) NOT NULL
);
```

`geom_type` defaults to `'GEOMETRY'` (untyped, accepts any subtype);
restrict to `'POINT'`, `'POLYGON'`, etc. when you want PostGIS to
enforce subtype at insert time.

`srid` defaults to 4326 (WGS84). The SRID is read at query-compile
time from `geometry_columns` so input WKT is automatically coerced
to the column's SRID via `ST_GeomFromText(<wkt>, <srid>)`.

## Quick start

```bash
# Install PostGIS (Debian/Ubuntu example).
sudo apt-get install postgresql-16-postgis-3
```

```sql
CREATE EXTENSION pg_mentat;
CREATE EXTENSION postgis;

SELECT mentat.t('[
  {:db/ident :place/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :place/loc  :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
]');
SELECT mentat.t('[
  {:db/id "a" :place/name "Boston" :place/loc "side"}
  {:db/id "b" :place/name "NYC"    :place/loc "side"}
  {:db/id "c" :place/name "Origin" :place/loc "side"}
]');

-- Attach + populate.
SELECT mentat.attach_geometry_attribute(':place/loc', 4326, 'POINT');
SELECT mentat.create_gist_geometry_index(':place/loc');
DO $$
DECLARE eb BIGINT; en BIGINT; eo BIGINT;
BEGIN
  SELECT e INTO eb FROM mentat.datoms_text_new
    WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':place/name')
      AND v = 'Boston';
  SELECT e INTO en FROM mentat.datoms_text_new
    WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':place/name')
      AND v = 'NYC';
  SELECT e INTO eo FROM mentat.datoms_text_new
    WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':place/name')
      AND v = 'Origin';
  PERFORM mentat.set_geometry(eb, ':place/loc', 'POINT(-71.0589 42.3601)');
  PERFORM mentat.set_geometry(en, ':place/loc', 'POINT(-74.0060 40.7128)');
  PERFORM mentat.set_geometry(eo, ':place/loc', 'POINT(0 0)');
END;
$$;

-- 2 nearest cities to (-72, 41).
SELECT mentat.q('[:find ?name ?d
                  :where [(geom-near $ :place/loc "POINT(-72 41)" 2) [[?e ?d]]]
                         [?e :place/name ?name]
                  :order (asc ?d)]');
-- => [["Boston", 1.65...], ["NYC", 2.03...]]
```

## Where-fns

### `[(geom-near $ :attr "WKT" k) [[?e ?dist]]]`

Top-K nearest neighbors by `ST_Distance`, ordered by ascending
distance. Uses PostGIS's `<->` GiST-friendly distance operator inside
the subquery for index-driven retrieval.

### `[(geom-within $ :attr "WKT" radius) [[?e ?dist]]]`

Entities whose geometry is within `radius` of the input WKT, via
`ST_DWithin`. The radius unit matches the column's SRID (degrees for
4326, meters for projected SRIDs). `?dist` is `ST_Distance(geom, wkt)`
for ordering.

### `[(geom-contains $ :attr "WKT") [[?e]]]`

Entities whose geometry **contains** the input WKT, via `ST_Contains`.
Useful for "which polygon does this point fall in?".

### `[(geom-intersects $ :attr "WKT") [[?e]]]`

Entities whose geometry intersects the input WKT, via `ST_Intersects`.
The most permissive predicate — any kind of overlap, including touching.

All four where-fns:
- Take a leading `$` source-var for parser symmetry.
- Take an attribute keyword and a WKT string literal.
- Bind `?e` to the entity id; `geom-near` and `geom-within` also
  bind `?dist` (a `float8`).
- JOIN cleanly to subsequent EAV patterns by `?e` thanks to the
  FtsJoin entity-binding fix shipped with pgvector.

## Index

```sql
SELECT mentat.create_gist_geometry_index(':place/loc');
-- => 'attr_<entid>_geom_gist'
```

Plain GiST index on `geom`. PostGIS uses it automatically for
`ST_DWithin`, `ST_Distance` (when the `<->` operator drives the
ORDER BY), `ST_Contains`, `ST_Intersects`, and other
spatial-relationship operators. Without the index every spatial
predicate becomes a sequential scan with full GEOS evaluation —
fine for a few hundred rows, painful past ~10k.

## SRID handling

The integration reads each column's SRID from `geometry_columns`
at compile time and emits `ST_GeomFromText(<wkt>, <srid>)` so input
WKT inherits the column's SRID automatically. Without this, mixed
SRID (Point 0 vs Point 4326) raises
`Operation on mixed SRID geometries`.

If your column is SRID 0 (intentionally projection-agnostic), the
emit becomes `ST_GeomFromText(<wkt>, 0)` and PostGIS treats both
sides as "unknown SRID". Spatial predicates still work, but
`ST_Distance` returns Cartesian distance, not geodesic.

For accurate geodesic distance over WGS84, use SRID 4326 and either:
- Cast through `geography` outside this integration (e.g.
  `ST_Distance(g1::geography, g2::geography)`).
- Use a projected SRID that matches your data's region.

## Errors

| Error | Cause | Fix |
|:---|:---|:---|
| `type "geometry" does not exist` | PostGIS not installed. | `CREATE EXTENSION postgis;`. |
| `:db.error/missing-extension PostGIS is not installed` | Calling helper before `CREATE EXTENSION postgis`. | Install PostGIS. |
| `:db.error/unknown-attribute Attribute :foo/bar is not registered` | Attribute missing from `mentat.schema`. | Transact the schema first, then attach. |
| `relation "mentat.attr_<n>_geom" does not exist` | Used `set_geometry` / spatial where-fn before `attach_geometry_attribute`. | Call `attach_geometry_attribute` first. |
| `:db.error/fn-arg geom_type X is not a recognized PostGIS geometry subtype` | Bad `geom_type` arg. | Use one of `GEOMETRY`, `POINT`, `LINESTRING`, `POLYGON`, etc. |
| `:db.error/fn-arity geom-near requires 4 arguments` | Wrong arg count. | Pass `($ :attr "WKT" k)`. |
| `:db.error/fn-arg geom-near k must be > 0` | K not positive. | Pass a positive integer. |
| `:db.error/fn-arg geom-within radius must be > 0` | Bad radius. | Pass a positive float. |
| `Operation on mixed SRID geometries` | Should not happen with this integration; SRID is auto-injected. If you see it, your aux table's SRID is missing from `geometry_columns`. | Check `SELECT * FROM geometry_columns WHERE f_table_schema = 'mentat'`. |

## Worked example: "where are the customers near our new store?"

```clojure
(d/q '[:find ?name ?email ?dist-km
       :where
         ;; New store at given coords.
         [(geom-within $ :customer/address ?store-wkt 10.0) [[?e ?dist]]]
         [?e :customer/name ?name]
         [?e :customer/email ?email]
         [(* ?dist 111.0) ?dist-km]   ;; deg -> km approximation at mid-latitudes
       :order (asc ?dist)
       :limit 50]
     db
     "POINT(-71.05 42.36)")
```

Plan:
1. `geom-within` returns `(?e, ?dist)` for each customer within 10°
   of the store, using the GiST index. With ~1M customers, this is
   typically < 50ms.
2. Two EAV joins follow `?e` to name and email.
3. `(* ?dist 111.0)` converts decimal degrees to kilometers (rough,
   latitude-dependent — for production use a projected SRID instead).
4. Top 50 by ascending distance.

## What this does NOT (yet) give you

- **`:db.type/geometry` schema integration.** Geometries don't transact
  via `mentat.t`. Use `mentat.set_geometry` directly. A future
  schema-side integration is planned; the aux-table representation is
  forward-compatible.
- **Variable WKT in where-fns.** The WKT must be a string literal in
  the EDN. Pass dynamic geometries through the `:in` clause (when
  schema integration ships).
- **Geography type.** Only `geometry` is wrapped today. `geography`
  is reachable via raw SQL alongside Datalog.
- **Raster, topology, tiger geocoder.** PostGIS has many sub-extensions
  this integration doesn't wrap. Use them directly via SQL.
- **3D / 4D coordinates.** Z and M coordinates pass through as part
  of the geometry but the where-fns operate on 2D predicates only.
