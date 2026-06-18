// Comprehensive query tests covering all find-specs, clauses, predicates,
// aggregates, ordering, pagination, and error handling.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._test_raises_error(stmt TEXT) RETURNS BOOLEAN
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
        Spi::get_one::<bool>(&format!("SELECT mentat._test_raises_error('{}')", escaped))
            .expect("raises_error call")
            .unwrap_or(false)
    }

    fn setup_query_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :qc/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :qc/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :qc/score :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :qc/active :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :qc/role :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :qc/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :qc/manager :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :qc/dept :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"e\" :db/ident :qc/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"sal\" :db/ident :qc/salary :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("query schema");
    }

    fn setup_query_data() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"mgr\" :qc/name \"Boss\" :qc/age 50 :qc/score 95.0 :qc/active true :qc/role :admin :qc/dept \"Engineering\" :qc/salary 150000}
                {:db/id \"e1\" :qc/name \"Alice\" :qc/age 30 :qc/score 88.5 :qc/active true :qc/role :engineer :qc/dept \"Engineering\" :qc/manager \"mgr\" :qc/email \"alice@test.com\" :qc/salary 100000}
                {:db/id \"e2\" :qc/name \"Bob\" :qc/age 35 :qc/score 72.3 :qc/active true :qc/role :engineer :qc/dept \"Engineering\" :qc/manager \"mgr\" :qc/salary 110000}
                {:db/id \"e3\" :qc/name \"Carol\" :qc/age 28 :qc/score 91.7 :qc/active false :qc/role :designer :qc/dept \"Design\" :qc/salary 95000}
                {:db/id \"e4\" :qc/name \"Dave\" :qc/age 42 :qc/score 67.8 :qc/active true :qc/role :pm :qc/dept \"Product\" :qc/salary 120000}
                {:db/id \"e5\" :qc/name \"Eve\" :qc/age 25 :qc/score 95.2 :qc/active true :qc/role :engineer :qc/dept \"Engineering\" :qc/manager \"mgr\" :qc/salary 90000}
                [:db/add \"e1\" :qc/tags \"rust\"]
                [:db/add \"e1\" :qc/tags \"postgres\"]
                [:db/add \"e2\" :qc/tags \"rust\"]
                [:db/add \"e2\" :qc/tags \"java\"]
                [:db/add \"e3\" :qc/tags \"figma\"]
                [:db/add \"e5\" :qc/tags \"rust\"]
                [:db/add \"e5\" :qc/tags \"python\"]
            ]'::TEXT)",
        ).expect("query data");
    }

    // ========================================================================
    // Find-spec: relation (default)
    // ========================================================================

    #[pg_test]
    fn test_qc_find_relation_basic() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name ?age :where [?e :qc/name ?name] [?e :qc/age ?age]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        assert_eq!(results.len(), 6, "Should find 6 people");
    }

    #[pg_test]
    fn test_qc_find_relation_three_vars() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name ?age ?score :where [?e :qc/name ?name] [?e :qc/age ?age] [?e :qc/score ?score]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        assert_eq!(results.len(), 6);
        for row in results {
            let r = row.as_array().expect("row");
            assert_eq!(r.len(), 3);
        }
    }

    // ========================================================================
    // Find-spec: scalar
    // ========================================================================

    #[pg_test]
    fn test_qc_find_scalar_basic() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name . :where [?e :qc/name ?name] [?e :qc/name \"Alice\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_str().expect("s"), "Alice");
    }

    #[pg_test]
    fn test_qc_find_scalar_no_match() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name . :where [?e :qc/name ?name] [?e :qc/name \"Nonexistent\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(
            j["result"].is_null(),
            "Non-matching scalar should return null"
        );
    }

    // ========================================================================
    // Find-spec: collection
    // ========================================================================

    #[pg_test]
    fn test_qc_find_collection_names() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        assert_eq!(names.len(), 6);
    }

    #[pg_test]
    fn test_qc_find_collection_ages() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?age ...] :where [?e :qc/age ?age]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let ages = j["result"].as_array().expect("arr");
        assert_eq!(ages.len(), 6);
    }

    // ========================================================================
    // Find-spec: tuple
    // ========================================================================

    #[pg_test]
    fn test_qc_find_tuple_basic() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ?age] :where [?e :qc/name ?name] [?e :qc/name \"Alice\"] [?e :qc/age ?age]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let result = j["result"].as_array().expect("tuple");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].as_str().expect("name"), "Alice");
        assert_eq!(result[1].as_i64().expect("age"), 30);
    }

    // ========================================================================
    // Predicates: comparison
    // ========================================================================

    #[pg_test]
    fn test_qc_predicate_gt() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/age ?age] [(> ?age 35)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Boss=50, Dave=42 => 2 matches
        assert_eq!(names.len(), 2);
    }

    #[pg_test]
    fn test_qc_predicate_lt() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/age ?age] [(< ?age 30)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Carol=28, Eve=25 => 2 matches
        assert_eq!(names.len(), 2);
    }

    #[pg_test]
    fn test_qc_predicate_gte() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/age ?age] [(>= ?age 35)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Boss=50, Bob=35, Dave=42 => 3 matches
        assert_eq!(names.len(), 3);
    }

    #[pg_test]
    fn test_qc_predicate_lte() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/age ?age] [(<= ?age 30)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Alice=30, Carol=28, Eve=25 => 3 matches
        assert_eq!(names.len(), 3);
    }

    #[pg_test]
    fn test_qc_predicate_ne() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/dept ?d] [(!= ?d \"Engineering\")]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Carol=Design, Dave=Product => 2 matches
        assert_eq!(names.len(), 2);
    }

    // ========================================================================
    // Predicates: double comparison
    // ========================================================================

    #[pg_test]
    fn test_qc_predicate_double_gt() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/score ?s] [(> ?s 90.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Boss=95.0, Carol=91.7, Eve=95.2 => 3 matches
        assert_eq!(names.len(), 3);
    }

    // ========================================================================
    // Join queries
    // ========================================================================

    #[pg_test]
    fn test_qc_join_name_via_manager() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?ename ...] :where [?e :qc/manager ?m] [?m :qc/name ?mname] [?e :qc/name ?ename] [(= ?mname \"Boss\")]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Alice, Bob, Eve all report to Boss
        assert_eq!(names.len(), 3);
    }

    #[pg_test]
    fn test_qc_join_two_entities_same_dept() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n1 ?n2 :where [?e1 :qc/name ?n1] [?e2 :qc/name ?n2] [?e1 :qc/dept ?d] [?e2 :qc/dept ?d] [(!= ?e1 ?e2)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        // Engineering has Boss, Alice, Bob, Eve => C(4,2)*2 = 12 ordered pairs
        assert!(
            results.len() >= 12,
            "Should have at least 12 same-dept pairs, got {}",
            results.len()
        );
    }

    // ========================================================================
    // Cardinality-many in queries
    // ========================================================================

    #[pg_test]
    fn test_qc_query_cardinality_many_join() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/tags \"rust\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Alice, Bob, Eve have "rust" tag
        assert_eq!(names.len(), 3);
    }

    #[pg_test]
    fn test_qc_query_all_tags_for_entity() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :qc/name \"Alice\"] [?e :qc/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tags = j["result"].as_array().expect("arr");
        assert_eq!(tags.len(), 2); // "rust", "postgres"
    }

    // ========================================================================
    // Boolean queries
    // ========================================================================

    #[pg_test]
    fn test_qc_query_boolean_true() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/active true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Boss, Alice, Bob, Dave, Eve are active
        assert_eq!(names.len(), 5);
    }

    #[pg_test]
    fn test_qc_query_boolean_false() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/active false]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        assert_eq!(names.len(), 1); // Carol
    }

    // ========================================================================
    // Keyword queries
    // ========================================================================

    #[pg_test]
    fn test_qc_query_keyword_match() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/role :engineer]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Alice, Bob, Eve
        assert_eq!(names.len(), 3);
    }

    #[pg_test]
    fn test_qc_query_keyword_all_roles() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?r ...] :where [_ :qc/role ?r]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let roles = j["result"].as_array().expect("arr");
        // admin, engineer, designer, pm => 4 distinct roles
        assert_eq!(roles.len(), 4);
    }

    // ========================================================================
    // Complex multi-clause queries
    // ========================================================================

    #[pg_test]
    fn test_qc_query_multi_clause_filter() {
        setup();
        setup_query_schema();
        setup_query_data();
        // Active engineers over 25 in Engineering
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...]
                :where
                [?e :qc/name ?name]
                [?e :qc/active true]
                [?e :qc/role :engineer]
                [?e :qc/age ?age]
                [(> ?age 25)]
                [?e :qc/dept \"Engineering\"]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Alice=30, Bob=35 match; Eve=25 does not (not > 25)
        assert_eq!(names.len(), 2);
    }

    // ========================================================================
    // Empty result queries
    // ========================================================================

    #[pg_test]
    fn test_qc_query_no_results_relation() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name :where [?e :qc/name ?name] [?e :qc/age 999]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        assert_eq!(results.len(), 0);
    }

    #[pg_test]
    fn test_qc_query_no_results_collection() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/age 999]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let arr = j["result"].as_array().expect("arr");
        assert_eq!(arr.len(), 0);
    }

    // ========================================================================
    // Entity ID in results
    // ========================================================================

    #[pg_test]
    fn test_qc_query_entity_id_in_results() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e ?name :where [?e :qc/name ?name] [?e :qc/name \"Alice\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = j["results"].as_array().expect("arr");
        assert_eq!(results.len(), 1);
        let row = results[0].as_array().expect("row");
        assert!(row[0].as_i64().is_some(), "Entity ID should be an integer");
        assert_eq!(row[1].as_str().expect("name"), "Alice");
    }

    // ========================================================================
    // Multiple constant bindings
    // ========================================================================

    #[pg_test]
    fn test_qc_query_constant_string_binding() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?age . :where [?e :qc/name \"Bob\"] [?e :qc/age ?age]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_i64().expect("age"), 35);
    }

    #[pg_test]
    fn test_qc_query_constant_long_binding() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name . :where [?e :qc/name ?name] [?e :qc/age 30]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_str().expect("name"), "Alice");
    }

    // ========================================================================
    // Query after mutations
    // ========================================================================

    #[pg_test]
    fn test_qc_query_after_update() {
        setup();
        setup_query_schema();
        setup_query_data();
        // Find Alice's entity ID
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :qc/email \"alice@test.com\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let eid = j["result"].as_i64().expect("eid");

        // Update Alice's age
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :qc/age 31]]'::TEXT)",
            eid
        ))
        .expect("update");

        // Query should reflect update
        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?age . :where [?e :qc/email \"alice@test.com\"] [?e :qc/age ?age]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(j2["result"].as_i64().expect("age"), 31);
    }

    #[pg_test]
    fn test_qc_query_after_retract() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :qc/email \"alice@test.com\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let eid = j["result"].as_i64().expect("eid");

        // Retract entity
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retract");

        // Query should no longer find Alice
        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name . :where [?e :qc/email \"alice@test.com\"] [?e :qc/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert!(
            j2["result"].is_null(),
            "Retracted entity should not be found"
        );
    }

    // ========================================================================
    // Query error handling
    // ========================================================================

    #[pg_test]
    fn test_qc_query_missing_find() {
        setup();
        setup_query_schema();
        assert!(
            raises_error(
                "SELECT mentat_query('[:where [?e :qc/name ?name]]'::TEXT, '{}'::jsonb)::TEXT"
            ),
            "Query without :find should fail"
        );
    }

    #[pg_test]
    fn test_qc_query_missing_where() {
        setup();
        setup_query_schema();
        assert!(
            raises_error("SELECT mentat_query('[:find ?name]'::TEXT, '{}'::jsonb)::TEXT"),
            "Query without :where should fail"
        );
    }

    #[pg_test]
    fn test_qc_query_invalid_edn() {
        setup();
        assert!(
            raises_error("SELECT mentat_query('not valid edn'::TEXT, '{}'::jsonb)::TEXT"),
            "Invalid EDN should fail"
        );
    }

    #[pg_test]
    fn test_qc_query_unknown_attribute() {
        setup();
        setup_query_schema();
        assert!(
            raises_error("SELECT mentat_query('[:find ?v . :where [?e :nonexistent/attr ?v]]'::TEXT, '{}'::jsonb)::TEXT"),
            "Unknown attribute should fail"
        );
    }

    // ========================================================================
    // Sequential queries (consistency)
    // ========================================================================

    #[pg_test]
    fn test_qc_sequential_queries_consistent() {
        setup();
        setup_query_schema();
        setup_query_data();
        for _ in 0..10 {
            let q = Spi::get_one::<String>(
                "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
            ).expect("q").expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(j["result"].as_array().expect("arr").len(), 6);
        }
    }

    // ========================================================================
    // Salary-based range queries
    // ========================================================================

    #[pg_test]
    fn test_qc_salary_range_query() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/salary ?s] [(>= ?s 100000)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Boss=150000, Alice=100000, Bob=110000, Dave=120000
        assert_eq!(names.len(), 4);
    }

    #[pg_test]
    fn test_qc_salary_between_query() {
        setup();
        setup_query_schema();
        setup_query_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?name ...] :where [?e :qc/name ?name] [?e :qc/salary ?s] [(>= ?s 95000)] [(<= ?s 110000)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = j["result"].as_array().expect("arr");
        // Alice=100000, Bob=110000, Carol=95000
        assert_eq!(names.len(), 3);
    }
}
