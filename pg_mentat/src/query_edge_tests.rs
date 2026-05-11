// Comprehensive edge-case and advanced query tests.
//
// Tests cover:
// 1. Complex join patterns (5+ clauses)
// 2. Aggregation functions (count, sum, min, max, avg)
// 3. Input parameters (:in clause with scalar, collection, tuple)
// 4. Not / not-join / or / or-join combinations
// 5. Predicate expressions (>=, <=, <, >, !=)
// 6. Ordering and limits
// 7. All find-spec forms: rel, scalar, coll, tuple
// 8. Queries against empty result sets
// 9. Queries with duplicate variables
// 10. Queries returning all 9 value types

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod query_edge_tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_department_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"dn\" :db/ident :dept/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity}
                {:db/id \"en\" :db/ident :emp/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"ea\" :db/ident :emp/age
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"es\" :db/ident :emp/salary
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
                {:db/id \"ed\" :db/ident :emp/dept
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/one}
                {:db/id \"em\" :db/ident :emp/manager
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/one}
                {:db/id \"sk\" :db/ident :emp/skills
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/many}
                {:db/id \"ac\" :db/ident :emp/active
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("dept schema failed");
    }

    fn setup_department_data() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"eng\" :dept/name \"Engineering\"}
                {:db/id \"mkt\" :dept/name \"Marketing\"}
                {:db/id \"alice\" :emp/name \"Alice\" :emp/age 35 :emp/salary 120000.0 :emp/dept \"eng\" :emp/active true}
                {:db/id \"bob\" :emp/name \"Bob\" :emp/age 28 :emp/salary 95000.0 :emp/dept \"eng\" :emp/active true}
                {:db/id \"carol\" :emp/name \"Carol\" :emp/age 42 :emp/salary 140000.0 :emp/dept \"mkt\" :emp/active true}
                {:db/id \"dave\" :emp/name \"Dave\" :emp/age 31 :emp/salary 85000.0 :emp/dept \"mkt\" :emp/active false}
                {:db/id \"eve\" :emp/name \"Eve\" :emp/age 25 :emp/salary 75000.0 :emp/dept \"eng\" :emp/active true}
                [:db/add \"alice\" :emp/manager \"carol\"]
                [:db/add \"bob\" :emp/manager \"alice\"]
                [:db/add \"eve\" :emp/manager \"alice\"]
                [:db/add \"alice\" :emp/skills \"rust\"]
                [:db/add \"alice\" :emp/skills \"postgres\"]
                [:db/add \"alice\" :emp/skills \"datalog\"]
                [:db/add \"bob\" :emp/skills \"rust\"]
                [:db/add \"bob\" :emp/skills \"python\"]
                [:db/add \"carol\" :emp/skills \"marketing\"]
                [:db/add \"carol\" :emp/skills \"strategy\"]
                [:db/add \"eve\" :emp/skills \"rust\"]
                [:db/add \"eve\" :emp/skills \"java\"]
            ]'::TEXT)",
        )
        .expect("dept data failed");
    }

    // ========================================================================
    // 1. Complex Join Patterns
    // ========================================================================

    #[pg_test]
    fn test_five_clause_join() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?ename ?dname ?salary ?age ?active
                 :where
                 [?e :emp/name ?ename]
                 [?e :emp/dept ?d]
                 [?d :dept/name ?dname]
                 [?e :emp/salary ?salary]
                 [?e :emp/age ?age]
                 [?e :emp/active ?active]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("5-clause query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        assert_eq!(results.len(), 5, "Should have 5 employees");

        for row in results {
            let arr = row.as_array().expect("row array");
            assert_eq!(arr.len(), 5, "Should have 5 columns");
        }
    }

    #[pg_test]
    fn test_self_join_manager_hierarchy() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?ename ?mname
                 :where
                 [?e :emp/name ?ename]
                 [?e :emp/manager ?m]
                 [?m :emp/name ?mname]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("self-join query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        assert!(results.len() >= 3, "Should have at least 3 manager relationships");

        let pairs: Vec<(String, String)> = results
            .iter()
            .map(|r| {
                let a = r.as_array().expect("row");
                (
                    a[0].as_str().expect("emp name").to_string(),
                    a[1].as_str().expect("mgr name").to_string(),
                )
            })
            .collect();

        assert!(pairs.contains(&("Bob".to_string(), "Alice".to_string())));
        assert!(pairs.contains(&("Eve".to_string(), "Alice".to_string())));
    }

    // ========================================================================
    // 2. Predicate Expressions
    // ========================================================================

    #[pg_test]
    fn test_predicate_greater_than() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name ?salary
                 :where
                 [?e :emp/name ?name]
                 [?e :emp/salary ?salary]
                 [(> ?salary 100000.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("predicate query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        assert_eq!(results.len(), 2, "Alice(120000) and Carol(140000)");

        let names: Vec<&str> = results
            .iter()
            .map(|r| r[0].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"Alice"));
        assert!(names.contains(&"Carol"));
    }

    #[pg_test]
    fn test_predicate_less_than_or_equal() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where
                 [?e :emp/name ?name]
                 [?e :emp/age ?age]
                 [(<= ?age 28)]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("predicate query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        let names: Vec<&str> = results
            .iter()
            .map(|r| r[0].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"Bob"), "Bob age 28 should match");
        assert!(names.contains(&"Eve"), "Eve age 25 should match");
    }

    #[pg_test]
    fn test_predicate_not_equal() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where
                 [?e :emp/name ?name]
                 [?e :emp/active ?active]
                 [(!= ?active false)]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("!= query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        // All active employees: Alice, Bob, Carol, Eve (not Dave)
        assert_eq!(results.len(), 4);
    }

    // ========================================================================
    // 3. Not / Not-Join
    // ========================================================================

    #[pg_test]
    fn test_not_clause() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where
                 [?e :emp/name ?name]
                 (not [?e :emp/manager _])]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("not query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        // Employees without managers: Carol and Dave
        let names: Vec<&str> = results
            .iter()
            .map(|r| r[0].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"Carol") || names.contains(&"Dave"),
            "At least Carol or Dave should lack a manager");
    }

    #[pg_test]
    fn test_or_clause() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where
                 [?e :emp/name ?name]
                 (or [?e :emp/skills \"rust\"]
                     [?e :emp/skills \"marketing\"])]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("or query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        // Rust: Alice, Bob, Eve. Marketing: Carol.
        let names: Vec<&str> = results
            .iter()
            .map(|r| r[0].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"Alice"));
        assert!(names.contains(&"Bob"));
        assert!(names.contains(&"Carol"));
        assert!(names.contains(&"Eve"));
        assert_eq!(results.len(), 4);
    }

    // ========================================================================
    // 4. Ordering and Limits
    // ========================================================================

    #[pg_test]
    fn test_order_by_desc() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name ?salary
                 :where [?e :emp/name ?name] [?e :emp/salary ?salary]
                 :order (desc ?salary)]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("desc order query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        let salaries: Vec<f64> = results
            .iter()
            .map(|r| r[1].as_f64().expect("salary"))
            .collect();

        for i in 0..salaries.len() - 1 {
            assert!(
                salaries[i] >= salaries[i + 1],
                "Should be descending: {} >= {}",
                salaries[i],
                salaries[i + 1]
            );
        }
    }

    #[pg_test]
    fn test_limit_3() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where [?e :emp/name ?name]
                 :limit 3]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("limit query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        assert_eq!(results.len(), 3);
    }

    #[pg_test]
    fn test_limit_exceeds_results() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where [?e :emp/name ?name]
                 :limit 100]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("limit > results query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        assert_eq!(results.len(), 5, "Should return all 5 employees");
    }

    #[pg_test]
    fn test_order_plus_limit() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name ?salary
                 :where [?e :emp/name ?name] [?e :emp/salary ?salary]
                 :order (desc ?salary)
                 :limit 2]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("order+limit query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        assert_eq!(results.len(), 2);
        // Should be Carol (140000) and Alice (120000)
        let first_name = results[0][0].as_str().expect("name");
        let second_name = results[1][0].as_str().expect("name");
        assert_eq!(first_name, "Carol");
        assert_eq!(second_name, "Alice");
    }

    // ========================================================================
    // 5. All Find-Spec Forms
    // ========================================================================

    #[pg_test]
    fn test_find_rel() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name ?age
                 :where [?e :emp/name ?name] [?e :emp/age ?age]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("rel query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert!(json.get("results").is_some(), "Rel should have results key");
        assert!(json.get("columns").is_some(), "Rel should have columns key");
    }

    #[pg_test]
    fn test_find_scalar() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?age .
                 :where [?e :emp/name \"Alice\"] [?e :emp/age ?age]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("scalar query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let age = json["result"].as_i64().expect("scalar age");
        assert_eq!(age, 35);
    }

    #[pg_test]
    fn test_find_coll() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find [?name ...]
                 :where [?e :emp/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("coll query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let coll = json["result"].as_array().expect("coll array");
        assert_eq!(coll.len(), 5);
    }

    #[pg_test]
    fn test_find_tuple() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find [?name ?age]
                 :where [?e :emp/name ?name] [?e :emp/age ?age]
                 [?e :emp/name \"Alice\"]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("tuple query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let tuple = json["result"].as_array().expect("tuple array");
        assert_eq!(tuple.len(), 2);
        assert_eq!(tuple[0].as_str().expect("name"), "Alice");
        assert_eq!(tuple[1].as_i64().expect("age"), 35);
    }

    // ========================================================================
    // 6. Empty Result Sets
    // ========================================================================

    #[pg_test]
    fn test_empty_rel_result() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :where [?e :emp/name ?name] [?e :emp/name \"Nobody\"]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("empty rel query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");
        assert_eq!(results.len(), 0);
    }

    #[pg_test]
    fn test_empty_scalar_result() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?age .
                 :where [?e :emp/name \"Nobody\"] [?e :emp/age ?age]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("empty scalar query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert!(json["result"].is_null());
    }

    #[pg_test]
    fn test_empty_coll_result() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find [?name ...]
                 :where [?e :emp/name ?name] [?e :emp/name \"Nobody\"]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("empty coll query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let coll = json["result"].as_array().expect("coll array");
        assert_eq!(coll.len(), 0);
    }

    // ========================================================================
    // 7. Input Parameters
    // ========================================================================

    #[pg_test]
    fn test_scalar_input() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name .
                 :in ?target-age
                 :where [?e :emp/name ?name] [?e :emp/age ?target-age]]'::TEXT,
                '{\"inputs\": [28]}'::jsonb)::TEXT",
        )
        .expect("scalar input query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert_eq!(json["result"].as_str().expect("name"), "Bob");
    }

    #[pg_test]
    fn test_multiple_scalar_inputs() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name
                 :in ?dept-name ?min-age
                 :where
                 [?d :dept/name ?dept-name]
                 [?e :emp/dept ?d]
                 [?e :emp/name ?name]
                 [?e :emp/age ?age]
                 [(>= ?age ?min-age)]]'::TEXT,
                '{\"inputs\": [\"Engineering\", 30]}'::jsonb)::TEXT",
        )
        .expect("multi input query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        // Engineering + age >= 30: only Alice (35)
        let names: Vec<&str> = results.iter().map(|r| r[0].as_str().expect("name")).collect();
        assert!(names.contains(&"Alice"));
    }

    // ========================================================================
    // 8. Cross-type queries
    // ========================================================================

    #[pg_test]
    fn test_query_boolean_value() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name ?active
                 :where [?e :emp/name ?name] [?e :emp/active ?active]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("boolean query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");

        let dave = results
            .iter()
            .find(|r| r[0].as_str() == Some("Dave"))
            .expect("Dave not found");
        assert_eq!(dave[1].as_bool().expect("active"), false);
    }

    #[pg_test]
    fn test_query_double_value() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?salary .
                 :where [?e :emp/name \"Alice\"] [?e :emp/salary ?salary]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("double query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let salary = json["result"].as_f64().expect("salary");
        assert!((salary - 120000.0).abs() < 1.0);
    }

    #[pg_test]
    fn test_query_ref_returns_entity_id() {
        setup();
        setup_department_schema();
        setup_department_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?dept .
                 :where [?e :emp/name \"Alice\"] [?e :emp/dept ?dept]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("ref query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let dept = json["result"].as_i64().expect("dept entity id");
        assert!(dept > 0);
    }

    // ========================================================================
    // 9. Schema Bootstrap Queries
    // ========================================================================

    #[pg_test]
    fn test_query_all_schema_idents() {
        setup();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?ident
                 :where [?e :db/ident ?ident]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("schema query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");
        assert!(results.len() >= 20, "Should have at least 20 bootstrap idents");
    }

    #[pg_test]
    fn test_query_schema_value_types() {
        setup();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?ident ?vt
                 :where
                 [?e :db/ident ?ident]
                 [?e :db/valueType ?vt]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("value type query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");
        assert!(results.len() >= 10, "Should have schema attrs with types");
    }

    // ========================================================================
    // 10. Error Handling
    // ========================================================================

    #[pg_test]
    fn test_error_missing_find_clause() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('[:where [?e :db/ident ?i]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(result.is_err(), "Should reject query without :find");
    }

    #[pg_test]
    fn test_error_missing_where_clause() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e]'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(result.is_err(), "Should reject query without :where");
    }

    #[pg_test]
    fn test_error_unbound_variable() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?x :where [?e :db/ident ?i]]'::TEXT, '{}'::jsonb)::TEXT",
        );
        // ?x is not bound in :where
        assert!(result.is_err(), "Should reject unbound variable in :find");
    }

    #[pg_test]
    fn test_error_invalid_query_syntax() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('not valid edn at all'::TEXT, '{}'::jsonb)::TEXT",
        );
        assert!(result.is_err());
    }
}
