// Tests for the collection / tuple / relation forms of the `ground`
// where-function, e.g. `[(ground [10 20 30]) [?x ...]]`,
// `[(ground [1 "Alice" 30]) [?id ?name ?age]]`,
// `[(ground [[1 "a"] [2 "b"]]) [[?id ?label]]]`.
//
// These build on the same SQL plumbing used by `:in` collection bindings
// (`build_collection_in_clause`, `build_relation_values_join`, per-tuple
// `bind_input_value`).  Each successful test verifies an exact result set —
// silently-wrong is worse than unimplemented.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._gc_test_raises_error(stmt TEXT) RETURNS BOOLEAN
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN false;
             EXCEPTION WHEN OTHERS THEN
                 RETURN true;
             END;
             $$",
        )
        .expect("create helper");
    }

    fn raises_error(sql: &str) -> bool {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<bool>(&format!(
            "SELECT mentat._gc_test_raises_error('{}')",
            escaped
        ))
        .expect("raises_error call")
        .unwrap_or(false)
    }

    fn error_message(sql: &str) -> String {
        let escaped = sql.replace('\'', "''");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._gc_test_error_msg(stmt TEXT) RETURNS TEXT
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN ''::TEXT;
             EXCEPTION WHEN OTHERS THEN
                 RETURN SQLERRM;
             END;
             $$",
        )
        .expect("create error_msg helper");
        Spi::get_one::<String>(&format!(
            "SELECT mentat._gc_test_error_msg('{}')",
            escaped
        ))
        .expect("error_msg call")
        .unwrap_or_default()
    }

    fn setup_gc_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"id\"   :db/ident :gc/id   :db/valueType :db.type/long   :db/cardinality :db.cardinality/one}
                {:db/id \"name\" :db/ident :gc/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"age\"  :db/ident :gc/age  :db/valueType :db.type/long   :db/cardinality :db.cardinality/one}
                {:db/id \"city\" :db/ident :gc/city :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("gc schema");

        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :gc/id 1 :gc/name \"Alice\"   :gc/age 30 :gc/city \"Paris\"}
                {:db/id \"b\" :gc/id 2 :gc/name \"Bob\"     :gc/age 25 :gc/city \"Berlin\"}
                {:db/id \"c\" :gc/id 3 :gc/name \"Carol\"   :gc/age 40 :gc/city \"Paris\"}
                {:db/id \"d\" :gc/id 4 :gc/name \"Dave\"    :gc/age 35 :gc/city \"Lisbon\"}
                {:db/id \"e\" :gc/id 5 :gc/name \"Eve\"     :gc/age 22 :gc/city \"Berlin\"}
            ]'::TEXT)",
        )
        .expect("gc data");
    }

    // ========================================================================
    // Collection form: [(ground [v1 v2 v3]) [?x ...]]
    // Binds ?x to each value in turn (?x ∈ {v1, v2, v3}).
    // ========================================================================

    #[pg_test]
    fn pg_test_ground_collection_basic() {
        setup();
        setup_gc_schema();
        // ages 22, 25, 35 -> Eve, Bob, Dave
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where \
             [?e :gc/age ?a] \
             [(ground [22 25 35]) [?a ...]] \
             [?e :gc/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let mut names: Vec<String> = j["result"]
            .as_array()
            .expect("arr")
            .iter()
            .map(|v| v.as_str().expect("str").to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["Bob", "Dave", "Eve"]);
    }

    #[pg_test]
    fn pg_test_ground_collection_empty_intersection() {
        // Ground'd values that don't match any datom => empty result.
        setup();
        setup_gc_schema();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where \
             [?e :gc/age ?a] \
             [(ground [99 100 101]) [?a ...]] \
             [?e :gc/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        assert_eq!(names.len(), 0);
    }

    // ========================================================================
    // Tuple form: [(ground [v1 v2 v3]) [?x ?y ?z]]
    // Binds ?x, ?y, ?z to v1, v2, v3 simultaneously (one row, three columns).
    // ========================================================================

    #[pg_test]
    fn pg_test_ground_tuple_basic() {
        setup();
        setup_gc_schema();
        // The tuple (1, "Alice", 30) selects exactly entity "a".
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?id ?name ?age :where \
             [?e :gc/id ?id] \
             [?e :gc/name ?name] \
             [?e :gc/age ?age] \
             [(ground [1 \"Alice\" 30]) [?id ?name ?age]]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let rows = j["results"].as_array().expect("arr");
        assert_eq!(rows.len(), 1, "expected exactly one row, got {:?}", rows);
        assert_eq!(rows[0][0].as_i64().expect("id"), 1);
        assert_eq!(rows[0][1].as_str().expect("name"), "Alice");
        assert_eq!(rows[0][2].as_i64().expect("age"), 30);
    }

    #[pg_test]
    fn pg_test_ground_tuple_no_match() {
        // Tuple where one column doesn't agree with the entity in the others
        // produces an empty result set.
        setup();
        setup_gc_schema();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?id ?name :where \
             [?e :gc/id ?id] \
             [?e :gc/name ?name] \
             [(ground [1 \"Bob\"]) [?id ?name]]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let rows = j["results"].as_array().expect("arr");
        assert_eq!(rows.len(), 0);
    }

    // ========================================================================
    // Collection text typing.
    // ========================================================================

    #[pg_test]
    fn pg_test_ground_collection_text() {
        setup();
        setup_gc_schema();
        // Cities "Paris" or "Lisbon" -> Alice, Carol, Dave
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where \
             [?e :gc/city ?c] \
             [(ground [\"Paris\" \"Lisbon\"]) [?c ...]] \
             [?e :gc/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let mut names: Vec<String> = j["result"]
            .as_array()
            .expect("arr")
            .iter()
            .map(|v| v.as_str().expect("str").to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["Alice", "Carol", "Dave"]);
    }

    // ========================================================================
    // Relation form: [(ground [[v1 v2] [v3 v4]]) [[?x ?y]]]
    // Each inner vector is a row; bind both columns simultaneously.
    // ========================================================================

    #[pg_test]
    fn pg_test_ground_relation_basic() {
        setup();
        setup_gc_schema();
        // Rows (1,"Alice"), (2,"Bob"), (99,"Mismatch") -> Alice, Bob match
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?id ?name :where \
             [?e :gc/id ?id] \
             [?e :gc/name ?name] \
             [(ground [[1 \"Alice\"] [2 \"Bob\"] [99 \"Mismatch\"]]) [[?id ?name]]]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let mut rows: Vec<(i64, String)> = j["results"]
            .as_array()
            .expect("arr")
            .iter()
            .map(|r| {
                (
                    r[0].as_i64().expect("id"),
                    r[1].as_str().expect("name").to_string(),
                )
            })
            .collect();
        rows.sort();
        assert_eq!(
            rows,
            vec![(1, "Alice".to_string()), (2, "Bob".to_string())]
        );
    }

    // ========================================================================
    // Error cases.
    // ========================================================================

    #[pg_test]
    fn pg_test_ground_mixed_type_rejected() {
        setup();
        setup_gc_schema();
        let sql = "SELECT mentat_query('[:find ?n :where \
             [?e :gc/name ?n] \
             [(ground [1 \"two\"]) [?n ...]]]'::TEXT, '{}'::jsonb)::TEXT";
        assert!(
            raises_error(sql),
            "mixed-type ground collection should raise an error"
        );
        let msg = error_message(sql);
        assert!(
            msg.contains(":db.error/fn-arg") && msg.contains("mixed-type"),
            "error message should call out :db.error/fn-arg and mixed-type, got: {}",
            msg
        );
    }

    #[pg_test]
    fn pg_test_ground_tuple_arity_mismatch() {
        setup();
        setup_gc_schema();
        let sql = "SELECT mentat_query('[:find ?id ?name ?age :where \
             [?e :gc/id ?id] \
             [?e :gc/name ?name] \
             [?e :gc/age ?age] \
             [(ground [1 2]) [?id ?name ?age]]]'::TEXT, '{}'::jsonb)::TEXT";
        assert!(
            raises_error(sql),
            "tuple arity mismatch should raise an error"
        );
        let msg = error_message(sql);
        assert!(
            msg.contains(":db.error/fn-arg") && msg.contains("tuple"),
            "error message should call out :db.error/fn-arg and tuple, got: {}",
            msg
        );
    }

    #[pg_test]
    fn pg_test_ground_collection_non_vector_arg() {
        setup();
        setup_gc_schema();
        // Scalar arg with collection binding -> error.
        let sql = "SELECT mentat_query('[:find ?n :where \
             [?e :gc/name ?n] \
             [(ground 42) [?n ...]]]'::TEXT, '{}'::jsonb)::TEXT";
        assert!(
            raises_error(sql),
            "scalar arg with collection binding should raise an error"
        );
        let msg = error_message(sql);
        assert!(
            msg.contains(":db.error/fn-arg") && msg.contains("collection"),
            "error message should call out :db.error/fn-arg and collection, got: {}",
            msg
        );
    }

    #[pg_test]
    fn pg_test_ground_relation_row_arity_mismatch() {
        setup();
        setup_gc_schema();
        let sql = "SELECT mentat_query('[:find ?id ?name :where \
             [?e :gc/id ?id] \
             [?e :gc/name ?name] \
             [(ground [[1 \"Alice\"] [2]]) [[?id ?name]]]]'::TEXT, '{}'::jsonb)::TEXT";
        assert!(
            raises_error(sql),
            "relation row arity mismatch should raise an error"
        );
        let msg = error_message(sql);
        assert!(
            msg.contains(":db.error/fn-arg") && msg.contains("relation"),
            "error message should call out :db.error/fn-arg and relation, got: {}",
            msg
        );
    }
}
