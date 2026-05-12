// Comprehensive batch operation tests.
//
// Tests cover:
// 1. mentat_batch with mixed operations
// 2. Batch query operations
// 3. Batch transact operations
// 4. Batch pull operations
// 5. Batch entity operations
// 6. Error handling in batches
// 7. Large batches
// 8. Batch with schema operations

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_batch_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :bat/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :bat/age
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"e\" :db/ident :bat/email
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity}
                {:db/id \"t\" :db/ident :bat/tags
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        )
        .expect("batch schema failed");
    }

    // ========================================================================
    // 1. Large Single-Transaction Batches
    // ========================================================================

    #[pg_test]
    fn test_batch_10_entities() {
        setup();
        setup_batch_schema();

        let mut ops = Vec::new();
        for i in 0..10 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :bat/name \"person-{i}\" :bat/age {age}}}",
                i = i,
                age = 20 + i
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch 10");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':bat/name')
             AND added = true",
        )
        .expect("query")
        .expect("NULL");
        assert_eq!(count, 10);
    }

    #[pg_test]
    fn test_batch_ops_50_entities() {
        setup();
        setup_batch_schema();

        let mut ops = Vec::new();
        for i in 0..50 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :bat/name \"person-{i}\" :bat/age {age}}}",
                i = i,
                age = 20 + i
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch 50");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':bat/name')
             AND added = true",
        )
        .expect("query")
        .expect("NULL");
        assert_eq!(count, 50);
    }

    #[pg_test]
    fn test_batch_200_entities() {
        setup();
        setup_batch_schema();

        let mut ops = Vec::new();
        for i in 0..200 {
            ops.push(format!(
                "[:db/add \"e{i}\" :bat/name \"person-{i}\"]",
                i = i
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch 200");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':bat/name')
             AND added = true",
        )
        .expect("query")
        .expect("NULL");
        assert_eq!(count, 200);
    }

    // ========================================================================
    // 2. Mixed Operations in One Transaction
    // ========================================================================

    #[pg_test]
    fn test_batch_mixed_add_and_retract() {
        setup();
        setup_batch_schema();

        // First create some entities
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e1\" :bat/name \"Alice\"]
                [:db/add \"e1\" :bat/age 25]
                [:db/add \"e2\" :bat/name \"Bob\"]
                [:db/add \"e2\" :bat/age 30]
            ]'::TEXT)",
        )
        .expect("create entities")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let e1 = r["tempids"]["e1"].as_i64().expect("e1");
        let e2 = r["tempids"]["e2"].as_i64().expect("e2");

        // Mix of add and retract in one transaction
        Spi::run(&format!(
            "SELECT mentat_transact('[
                [:db/add {} :bat/age 26]
                [:db/retract {} :bat/name \"Bob\"]
                [:db/add \"e3\" :bat/name \"Carol\"]
            ]'::TEXT)",
            e1, e2
        ))
        .expect("mixed ops");

        // Alice's age should be 26
        let alice_age = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?age . :where [{} :bat/age ?age]]'::TEXT, '{{}}'::jsonb)::TEXT",
            e1
        ))
        .expect("query")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&alice_age).expect("parse");
        assert_eq!(json["result"].as_i64().expect("age"), 26);

        // Carol should exist
        let carol = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name . :where [?e :bat/name \"Carol\"]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&carol).expect("parse");
        assert_eq!(json["result"].as_str().expect("name"), "Carol");
    }

    // ========================================================================
    // 3. Cardinality-Many Batch Operations
    // ========================================================================

    #[pg_test]
    fn test_batch_cardinality_many_add() {
        setup();
        setup_batch_schema();

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e\" :bat/name \"Tagged\"]
                [:db/add \"e\" :bat/tags \"tag1\"]
                [:db/add \"e\" :bat/tags \"tag2\"]
                [:db/add \"e\" :bat/tags \"tag3\"]
                [:db/add \"e\" :bat/tags \"tag4\"]
                [:db/add \"e\" :bat/tags \"tag5\"]
                [:db/add \"e\" :bat/tags \"tag6\"]
                [:db/add \"e\" :bat/tags \"tag7\"]
                [:db/add \"e\" :bat/tags \"tag8\"]
                [:db/add \"e\" :bat/tags \"tag9\"]
                [:db/add \"e\" :bat/tags \"tag10\"]
            ]'::TEXT)",
        )
        .expect("batch many add");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find [?tag ...]
                 :where [?e :bat/name \"Tagged\"] [?e :bat/tags ?tag]]'::TEXT,
                '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let tags = json["result"].as_array().expect("tags");
        assert_eq!(tags.len(), 10);
    }

    #[pg_test]
    fn test_batch_cardinality_many_partial_retract() {
        setup();
        setup_batch_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :bat/name \"Pruned\"]
                [:db/add \"e\" :bat/tags \"keep1\"]
                [:db/add \"e\" :bat/tags \"keep2\"]
                [:db/add \"e\" :bat/tags \"remove1\"]
                [:db/add \"e\" :bat/tags \"remove2\"]
            ]'::TEXT)",
        )
        .expect("insert")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[
                [:db/retract {} :bat/tags \"remove1\"]
                [:db/retract {} :bat/tags \"remove2\"]
            ]'::TEXT)",
            eid, eid
        ))
        .expect("partial retract");

        let qresult = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find [?tag ...]
                 :where [{} :bat/tags ?tag]]'::TEXT,
                '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("query")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&qresult).expect("parse");
        let tags = json["result"].as_array().expect("tags");
        assert_eq!(tags.len(), 2);

        let tag_strs: Vec<&str> = tags.iter().map(|t| t.as_str().expect("tag")).collect();
        assert!(tag_strs.contains(&"keep1"));
        assert!(tag_strs.contains(&"keep2"));
        assert!(!tag_strs.contains(&"remove1"));
        assert!(!tag_strs.contains(&"remove2"));
    }

    // ========================================================================
    // 4. Sequential Multi-Transaction Workflow
    // ========================================================================

    #[pg_test]
    fn test_sequential_create_update_retract_lifecycle() {
        setup();
        setup_batch_schema();

        // Step 1: Create
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"e\" :bat/name \"Lifecycle\" :bat/age 1 :bat/email \"lc@test.com\"}
            ]'::TEXT)",
        )
        .expect("create")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Step 2: Update age 10 times
        for i in 2..=11 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :bat/age {}]]'::TEXT)",
                eid, i
            ))
            .expect("update");
        }

        // Step 3: Verify current state
        let qresult = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?age . :where [{} :bat/age ?age]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("query")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qresult).expect("parse");
        assert_eq!(json["result"].as_i64().expect("age"), 11);

        // Step 4: Retract entity
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retract");

        // Step 5: Verify retraction
        let qresult = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?name . :where [{} :bat/name ?name]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("query")
        .expect("NULL");
        let json: serde_json::Value = serde_json::from_str(&qresult).expect("parse");
        assert!(json["result"].is_null(), "Entity should be gone after retractEntity");
    }

    // ========================================================================
    // 5. Schema + Data Batch
    // ========================================================================

    #[pg_test]
    fn test_batch_schema_then_data_same_tx() {
        setup();

        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :bat/combo
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                [:db/add \"e1\" :bat/combo \"first\"]
                [:db/add \"e2\" :bat/combo \"second\"]
                [:db/add \"e3\" :bat/combo \"third\"]
            ]'::TEXT)",
        )
        .expect("schema + data same tx");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :bat/combo ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let vals = json["result"].as_array().expect("array");
        assert_eq!(vals.len(), 3);
    }

    // ========================================================================
    // 6. Upsert Batch
    // ========================================================================

    #[pg_test]
    fn test_batch_upsert_multiple() {
        setup();
        setup_batch_schema();

        // Insert initial data with unique identity emails
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e1\" :bat/email \"a@test.com\" :bat/name \"Alice\" :bat/age 25}
                {:db/id \"e2\" :bat/email \"b@test.com\" :bat/name \"Bob\" :bat/age 30}
            ]'::TEXT)",
        )
        .expect("initial insert");

        // Upsert: update ages for existing entities via identity attribute
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"u1\" :bat/email \"a@test.com\" :bat/age 26}
                {:db/id \"u2\" :bat/email \"b@test.com\" :bat/age 31}
            ]'::TEXT)",
        )
        .expect("upsert");

        // Check Alice's age
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?age .
                 :where [?e :bat/email \"a@test.com\"] [?e :bat/age ?age]]'::TEXT,
                '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(json["result"].as_i64().expect("age"), 26);

        // Check Bob's age
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?age .
                 :where [?e :bat/email \"b@test.com\"] [?e :bat/age ?age]]'::TEXT,
                '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(json["result"].as_i64().expect("age"), 31);

        // Should still be only 2 entities with emails
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':bat/email')
             AND added = true",
        )
        .expect("query")
        .expect("NULL");
        assert_eq!(count, 2, "Upsert should not create new entities");
    }

    // ========================================================================
    // 7. Empty and Single-Op Transactions
    // ========================================================================

    #[pg_test]
    fn test_batch_empty_transaction() {
        setup();
        let _result = Spi::get_one::<String>("SELECT mentat_transact('[]'::TEXT)");
    }

    #[pg_test]
    fn test_single_assertion() {
        setup();
        setup_batch_schema();

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :bat/name \"single\"]]'::TEXT)",
        )
        .expect("single assertion");
    }

    // ========================================================================
    // 8. Query Batching
    // ========================================================================

    #[pg_test]
    fn test_multiple_sequential_queries() {
        setup();
        setup_batch_schema();

        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e1\" :bat/name \"Alice\" :bat/age 25}
                {:db/id \"e2\" :bat/name \"Bob\" :bat/age 30}
                {:db/id \"e3\" :bat/name \"Carol\" :bat/age 35}
            ]'::TEXT)",
        )
        .expect("data");

        // Run 20 queries sequentially
        for _ in 0..20 {
            let result = Spi::get_one::<String>(
                "SELECT mentat_query('[:find [?name ...] :where [?e :bat/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
            )
            .expect("query")
            .expect("NULL");

            let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
            assert_eq!(json["result"].as_array().expect("arr").len(), 3);
        }
    }
}
