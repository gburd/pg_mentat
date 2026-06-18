// Regression tests for the PostGIS integration:
//   (geom-near $ :attr "WKT" k)        [[?e ?dist]]
//   (geom-within $ :attr "WKT" radius) [[?e ?dist]]
//   (geom-contains $ :attr "WKT")      [[?e]]
//   (geom-intersects $ :attr "WKT")    [[?e]]
//   mentat.attach_geometry_attribute / set_geometry / del_geometry /
//   create_gist_geometry_index / detach_geometry_attribute
//
// PostGIS is OPTIONAL. Tests skip the happy path when it isn't
// installed; negative-path tests run unconditionally.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._postgis_capture(stmt TEXT) RETURNS TEXT
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN '';
             EXCEPTION WHEN OTHERS THEN
                 RETURN SQLERRM;
             END;
             $$",
        )
        .expect("error-capture helper");
    }

    fn capture_error(sql: &str) -> String {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<String>(&format!("SELECT mentat._postgis_capture('{}')", escaped))
            .expect("capture")
            .unwrap_or_default()
    }

    fn has_postgis() -> bool {
        Spi::get_one::<bool>("SELECT mentat.has_postgis()")
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    fn install_places_with_geometry() -> (i64, i64, i64) {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :place/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/ident :place/loc  :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :place/name \"Origin\" :place/loc \"side\"}
                {:db/id \"b\" :place/name \"Boston\" :place/loc \"side\"}
                {:db/id \"c\" :place/name \"NYC\"    :place/loc \"side\"}
            ]'::TEXT)",
        )
        .expect("data tx");
        let lookup = |name: &str| -> i64 {
            Spi::get_one::<i64>(&format!(
                "SELECT e FROM mentat.datoms_text_new \
                 WHERE a = (SELECT entid FROM mentat.schema WHERE ident = ':place/name') \
                   AND v = '{}'",
                name
            ))
            .expect("lookup")
            .expect("NULL")
        };
        let e_a = lookup("Origin");
        let e_b = lookup("Boston");
        let e_c = lookup("NYC");

        Spi::run("SELECT mentat.attach_geometry_attribute(':place/loc', 4326, 'POINT')")
            .expect("attach");
        Spi::run(&format!(
            "SELECT mentat.set_geometry({}, ':place/loc', 'POINT(0 0)')",
            e_a
        ))
        .expect("set a");
        Spi::run(&format!(
            "SELECT mentat.set_geometry({}, ':place/loc', 'POINT(-71.0589 42.3601)')",
            e_b
        ))
        .expect("set b");
        Spi::run(&format!(
            "SELECT mentat.set_geometry({}, ':place/loc', 'POINT(-74.0060 40.7128)')",
            e_c
        ))
        .expect("set c");

        (e_a, e_b, e_c)
    }

    #[pg_test]
    fn pg_test_postgis_has_postgis_returns_bool() {
        setup();
        let _ = Spi::get_one::<bool>("SELECT mentat.has_postgis()")
            .expect("call")
            .unwrap_or(false);
    }

    /// geom-near returns the top-K rows in ascending distance order.
    #[pg_test]
    fn pg_test_postgis_geom_near_top_k() {
        setup();
        if !has_postgis() {
            return;
        }
        install_places_with_geometry();
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name ?d :where \
             [(geom-near $ :place/loc \"POINT(-72 41)\" 2) [[?e ?d]]] \
             [?e :place/name ?name] :order (asc ?d)]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        assert_eq!(results.len(), 2, "K=2");
        // Boston is closer to (-72, 41) than NYC.
        let names: Vec<String> = results
            .iter()
            .map(|r| r[0].as_str().expect("name").to_string())
            .collect();
        assert_eq!(names[0], "Boston");
        assert_eq!(names[1], "NYC");
    }

    /// geom-within with a moderate radius captures both Boston and NYC,
    /// excludes Origin (which is at (0, 0), well outside).
    #[pg_test]
    fn pg_test_postgis_geom_within() {
        setup();
        if !has_postgis() {
            return;
        }
        install_places_with_geometry();
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name :where \
             [(geom-within $ :place/loc \"POINT(-72 41)\" 5.0) [[?e ?d]]] \
             [?e :place/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let names: std::collections::HashSet<String> = j["results"]
            .as_array()
            .expect("array")
            .iter()
            .map(|r| r[0].as_str().expect("name").to_string())
            .collect();
        assert!(names.contains("Boston"));
        assert!(names.contains("NYC"));
        assert!(!names.contains("Origin"), "Origin (0,0) is too far");
    }

    /// geom-intersects with a polygon containing both Boston and NYC.
    #[pg_test]
    fn pg_test_postgis_geom_intersects_polygon() {
        setup();
        if !has_postgis() {
            return;
        }
        install_places_with_geometry();
        let raw = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name :where \
             [(geom-intersects $ :place/loc \"POLYGON((-75 40, -70 40, -70 43, -75 43, -75 40))\") [[?e]]] \
             [?e :place/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let names: std::collections::HashSet<String> = j["results"]
            .as_array()
            .expect("array")
            .iter()
            .map(|r| r[0].as_str().expect("name").to_string())
            .collect();
        assert!(names.contains("Boston"));
        assert!(names.contains("NYC"));
        assert!(!names.contains("Origin"));
    }

    /// Idempotent attach_geometry_attribute.
    #[pg_test]
    fn pg_test_postgis_attach_idempotent() {
        setup();
        if !has_postgis() {
            return;
        }
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/loc :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        let n1 = Spi::get_one::<String>(
            "SELECT mentat.attach_geometry_attribute(':p/loc', 4326, 'POINT')",
        )
        .expect("attach 1")
        .expect("NULL");
        let n2 = Spi::get_one::<String>(
            "SELECT mentat.attach_geometry_attribute(':p/loc', 4326, 'POINT')",
        )
        .expect("attach 2")
        .expect("NULL");
        assert_eq!(n1, n2);
        assert!(n1.starts_with("mentat.attr_"));
    }

    /// set then del cycle.
    #[pg_test]
    fn pg_test_postgis_set_then_del() {
        setup();
        if !has_postgis() {
            return;
        }
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/loc :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run("SELECT mentat.attach_geometry_attribute(':p/loc', 4326, 'POINT')")
            .expect("attach");
        Spi::run("SELECT mentat.set_geometry(99999, ':p/loc', 'POINT(1 2)', 4326)").expect("set");
        let dropped = Spi::get_one::<bool>("SELECT mentat.del_geometry(99999, ':p/loc')")
            .expect("del 1")
            .expect("NULL");
        assert!(dropped);
        let dropped2 = Spi::get_one::<bool>("SELECT mentat.del_geometry(99999, ':p/loc')")
            .expect("del 2")
            .expect("NULL");
        assert!(!dropped2);
    }

    /// GiST index helper succeeds.
    #[pg_test]
    fn pg_test_postgis_gist_index() {
        setup();
        if !has_postgis() {
            return;
        }
        install_places_with_geometry();
        let n = Spi::get_one::<String>("SELECT mentat.create_gist_geometry_index(':place/loc')")
            .expect("create")
            .expect("NULL");
        assert!(n.starts_with("attr_"));
        assert!(n.ends_with("_geom_gist"));
    }

    /// Bad arity for geom-near surfaces fn-arity.
    #[pg_test]
    fn pg_test_postgis_arity_error() {
        setup();
        let err = capture_error(
            "SELECT mentat_query('[:find ?e :where [(geom-near $ :p/loc \"POINT(0 0)\") [[?e ?d]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/fn-arity") && err.contains("geom-near"),
            "expected fn-arity, got: {}",
            err,
        );
    }

    /// Unknown attribute compile-time error.
    #[pg_test]
    fn pg_test_postgis_unknown_attr() {
        setup();
        let err = capture_error(
            "SELECT mentat_query('[:find ?e :where [(geom-near $ :no/such \"POINT(0 0)\" 1) [[?e ?d]]]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(
            err.contains(":db.error/unknown-attribute"),
            "expected unknown-attribute, got: {}",
            err,
        );
    }

    /// Bad geom_type in attach surfaces fn-arg error.
    #[pg_test]
    fn pg_test_postgis_attach_bad_geom_type() {
        setup();
        if !has_postgis() {
            return;
        }
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/g :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        let err = capture_error("SELECT mentat.attach_geometry_attribute(':p/g', 4326, 'BANANA')");
        assert!(
            err.contains(":db.error/fn-arg") && err.contains("BANANA"),
            "expected fn-arg geom_type error, got: {}",
            err,
        );
    }
}
