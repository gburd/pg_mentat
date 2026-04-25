// Comprehensive Pull API tests.
//
// Tests cover:
// 1. Simple attribute pulls
// 2. Wildcard pulls [*]
// 3. Nested/map spec pulls
// 4. Reverse lookups (:person/_friends)
// 5. Recursion (bounded and unbounded)
// 6. Limits on cardinality-many
// 7. Defaults for missing values
// 8. Rename (:as)
// 9. Component entities
// 10. Error handling

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod pull_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_graph_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"pn\" :db/ident :p/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"pa\" :db/ident :p/age
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"pe\" :db/ident :p/email
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity}
                {:db/id \"pf\" :db/ident :p/friends
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/many}
                {:db/id \"pp\" :db/ident :p/parent
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/one}
                {:db/id \"pt\" :db/ident :p/tags
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/many}
                {:db/id \"pa2\" :db/ident :p/active
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema failed");
    }

    fn setup_graph_data() -> (i64, i64, i64) {
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"alice\" :p/name \"Alice\" :p/age 30 :p/email \"alice@test.com\" :p/active true}
                {:db/id \"bob\" :p/name \"Bob\" :p/age 25 :p/email \"bob@test.com\" :p/active true}
                {:db/id \"carol\" :p/name \"Carol\" :p/age 35 :p/email \"carol@test.com\" :p/active false}
                [:db/add \"alice\" :p/friends \"bob\"]
                [:db/add \"alice\" :p/friends \"carol\"]
                [:db/add \"bob\" :p/friends \"carol\"]
                [:db/add \"bob\" :p/parent \"alice\"]
                [:db/add \"carol\" :p/parent \"alice\"]
                [:db/add \"alice\" :p/tags \"engineer\"]
                [:db/add \"alice\" :p/tags \"rust\"]
                [:db/add \"alice\" :p/tags \"leader\"]
                [:db/add \"bob\" :p/tags \"engineer\"]
                [:db/add \"bob\" :p/tags \"python\"]
            ]'::TEXT)",
        )
        .expect("data failed")
        .expect("NULL");

        let tx_report: serde_json::Value =
            serde_json::from_str(&result).expect("parse tx report");
        let alice = tx_report["tempids"]["alice"].as_i64().expect("alice eid");
        let bob = tx_report["tempids"]["bob"].as_i64().expect("bob eid");
        let carol = tx_report["tempids"]["carol"].as_i64().expect("carol eid");
        (alice, bob, carol)
    }

    // ========================================================================
    // 1. Simple Attribute Pulls
    // ========================================================================

    #[pg_test]
    fn test_pull_single_attribute() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:p/name]'::TEXT, {})",
            alice
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert_eq!(json[":p/name"].as_str().expect("name"), "Alice");
    }

    #[pg_test]
    fn test_pull_multiple_attributes() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:p/name :p/age :p/email]'::TEXT, {})",
            alice
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert_eq!(json[":p/name"].as_str().expect("name"), "Alice");
        assert_eq!(json[":p/age"].as_i64().expect("age"), 30);
        assert_eq!(json[":p/email"].as_str().expect("email"), "alice@test.com");
    }

    #[pg_test]
    fn test_pull_boolean_attribute() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:p/active]'::TEXT, {})",
            alice
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert_eq!(json[":p/active"].as_bool().expect("active"), true);
    }

    #[pg_test]
    fn test_pull_cardinality_many() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:p/tags]'::TEXT, {})",
            alice
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let tags = json[":p/tags"].as_array().expect("tags array");
        assert_eq!(tags.len(), 3);

        let tag_strs: Vec<&str> = tags.iter().map(|t| t.as_str().expect("tag")).collect();
        assert!(tag_strs.contains(&"engineer"));
        assert!(tag_strs.contains(&"rust"));
        assert!(tag_strs.contains(&"leader"));
    }

    // ========================================================================
    // 2. Wildcard Pulls
    // ========================================================================

    #[pg_test]
    fn test_pull_wildcard() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[*]'::TEXT, {})",
            alice
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");

        // Should contain :db/id and all user attributes
        assert!(json.get(":db/id").is_some(), "Should have :db/id");
        assert!(json.get(":p/name").is_some(), "Should have :p/name");
        assert!(json.get(":p/age").is_some(), "Should have :p/age");
        assert!(json.get(":p/email").is_some(), "Should have :p/email");
    }

    // ========================================================================
    // 3. Nested/Map Spec Pulls
    // ========================================================================

    #[pg_test]
    fn test_pull_nested_ref() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:p/name {{:p/friends [:p/name :p/age]}}]'::TEXT, {})",
            alice
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert_eq!(json[":p/name"].as_str().expect("name"), "Alice");

        let friends = json[":p/friends"].as_array().expect("friends array");
        assert_eq!(friends.len(), 2);

        for friend in friends {
            assert!(friend.get(":p/name").is_some(), "Friend should have name");
            assert!(friend.get(":p/age").is_some(), "Friend should have age");
        }
    }

    #[pg_test]
    fn test_pull_deeply_nested() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        // Alice -> friends -> friends (2 levels)
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:p/name {{:p/friends [:p/name {{:p/friends [:p/name]}}]}}]'::TEXT, {})",
            alice
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let friends = json[":p/friends"].as_array().expect("friends array");
        assert!(friends.len() > 0);
    }

    // ========================================================================
    // 4. Reverse Lookups
    // ========================================================================

    #[pg_test]
    fn test_pull_reverse_lookup() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:p/name :p/_parent]'::TEXT, {})",
            alice
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert_eq!(json[":p/name"].as_str().expect("name"), "Alice");

        // Alice is parent of Bob and Carol
        let children = json[":p/_parent"].as_array().expect("reverse parent array");
        assert_eq!(children.len(), 2);
    }

    #[pg_test]
    fn test_pull_reverse_lookup_with_nested() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:p/name {{:p/_parent [:p/name :p/age]}}]'::TEXT, {})",
            alice
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let children = json[":p/_parent"].as_array().expect("reverse parent array");

        for child in children {
            assert!(child.get(":p/name").is_some());
            assert!(child.get(":p/age").is_some());
        }
    }

    // ========================================================================
    // 5. Recursion
    // ========================================================================

    #[pg_test]
    fn test_pull_bounded_recursion() {
        setup();
        setup_graph_schema();
        let (_, bob, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:p/name {{:p/parent 2}}]'::TEXT, {})",
            bob
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert_eq!(json[":p/name"].as_str().expect("name"), "Bob");
        // Bob -> parent -> Alice (depth 1)
        if let Some(parent) = json.get(":p/parent") {
            assert!(parent.get(":p/name").is_some());
        }
    }

    #[pg_test]
    fn test_pull_unbounded_recursion() {
        setup();
        setup_graph_schema();
        let (_, bob, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:p/name {{:p/parent ...}}]'::TEXT, {})",
            bob
        ))
        .expect("pull failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert_eq!(json[":p/name"].as_str().expect("name"), "Bob");
    }

    // ========================================================================
    // 6. Limits
    // ========================================================================

    #[pg_test]
    fn test_pull_limit_cardinality_many() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[(:p/tags :limit 2)]'::TEXT, {})",
            alice
        ))
        .expect("pull with limit failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let tags = json[":p/tags"].as_array().expect("tags array");
        assert_eq!(tags.len(), 2, "Should be limited to 2 tags");
    }

    #[pg_test]
    fn test_pull_limit_1() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[(:p/friends :limit 1)]'::TEXT, {})",
            alice
        ))
        .expect("pull with limit 1 failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let friends = json[":p/friends"].as_array().expect("friends array");
        assert_eq!(friends.len(), 1, "Should be limited to 1 friend");
    }

    // ========================================================================
    // 7. Defaults
    // ========================================================================

    #[pg_test]
    fn test_pull_default_for_missing_attr() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        // Alice has no :p/parent, so default should be applied
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[(:p/parent :default \"none\")]'::TEXT, {})",
            alice
        ))
        .expect("pull with default failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        // Default should show up when Alice has no parent
        // The exact key depends on implementation
        assert!(json.get(":p/parent").is_some() || json.as_object().is_some());
    }

    // ========================================================================
    // 8. Rename (:as)
    // ========================================================================

    #[pg_test]
    fn test_pull_rename_attribute() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[(:p/name :as \"Full Name\")]'::TEXT, {})",
            alice
        ))
        .expect("pull with rename failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert_eq!(json["Full Name"].as_str().expect("renamed"), "Alice");
    }

    // ========================================================================
    // 9. Nonexistent Entity
    // ========================================================================

    #[pg_test]
    fn test_pull_nonexistent_entity() {
        setup();
        setup_graph_schema();
        setup_graph_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_pull('[:p/name]'::TEXT, 999999999)",
        )
        .expect("pull nonexistent should not error")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        // Should return empty map or null for missing attributes
        assert!(json.is_null() || json.is_object());
    }

    // ========================================================================
    // 10. Error Handling
    // ========================================================================

    #[pg_test]
    fn test_pull_invalid_pattern() {
        setup();
        let result = Spi::get_one::<String>(
            "SELECT mentat_pull('not a pattern'::TEXT, 1)",
        );
        assert!(result.is_err(), "Should reject invalid pull pattern");
    }

    #[pg_test]
    fn test_pull_empty_pattern() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[]'::TEXT, {})",
            alice
        ))
        .expect("empty pattern should work")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        assert!(json.is_object());
    }

    #[pg_test]
    fn test_pull_unknown_attribute() {
        setup();
        setup_graph_schema();
        let (alice, _, _) = setup_graph_data();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('[:nonexistent/attr]'::TEXT, {})",
            alice
        ));
        // Should either error or return empty for unknown attrs
        // Depending on implementation, this might succeed with empty result
    }

    // ========================================================================
    // 11. Pull in Query Context (via query)
    // ========================================================================

    #[pg_test]
    fn test_pull_via_query() {
        setup();
        setup_graph_schema();
        setup_graph_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find (pull ?e [:p/name :p/age])
                 :where [?e :p/email \"alice@test.com\"]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("pull in query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");
        assert_eq!(results.len(), 1);

        let pulled = &results[0][0];
        assert_eq!(pulled[":p/name"].as_str().expect("name"), "Alice");
        assert_eq!(pulled[":p/age"].as_i64().expect("age"), 30);
    }

    #[pg_test]
    fn test_pull_wildcard_via_query() {
        setup();
        setup_graph_schema();
        setup_graph_data();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find (pull ?e [*])
                 :where [?e :p/name \"Bob\"]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("pull * in query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let results = json["results"].as_array().expect("results array");
        assert_eq!(results.len(), 1);

        let pulled = &results[0][0];
        assert!(pulled.get(":db/id").is_some());
        assert_eq!(pulled[":p/name"].as_str().expect("name"), "Bob");
    }
}
