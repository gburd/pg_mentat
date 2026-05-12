// Comprehensive temporal (time-travel) tests.
//
// Tests cover:
// 1. as-of queries (point-in-time snapshots)
// 2. since queries (changes after a point)
// 3. history queries (all datoms including retractions)
// 4. Interaction of temporal queries with different find-specs
// 5. Temporal queries with predicates
// 6. Multi-entity temporal tracking
// 7. Schema changes visible at correct points

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_temporal_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :tt/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :tt/age
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :tt/status
                 :db/valueType :db.type/keyword
                 :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :tt/tags
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        )
        .expect("temporal schema failed");
    }

    /// Creates three transactions and returns (tx1, tx2, tx3, alice_eid).
    fn create_temporal_chain() -> (i64, i64, i64, i64) {
        // TX1: Create Alice age 25
        let result1 = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"alice\" :tt/name \"Alice\"]
                [:db/add \"alice\" :tt/age 25]
                [:db/add \"alice\" :tt/status :active]
            ]'::TEXT)",
        )
        .expect("tx1 failed")
        .expect("NULL");

        let r1: serde_json::Value = serde_json::from_str(&result1).expect("parse");
        let alice_eid = r1["tempids"]["alice"].as_i64().expect("alice eid");
        let tx1 = r1["db-after"]["basis-t"].as_i64().expect("tx1");

        // TX2: Update Alice age to 26
        let result2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add {} :tt/age 26]]'::TEXT)",
            alice_eid
        ))
        .expect("tx2 failed")
        .expect("NULL");

        let r2: serde_json::Value = serde_json::from_str(&result2).expect("parse");
        let tx2 = r2["db-after"]["basis-t"].as_i64().expect("tx2");

        // TX3: Update Alice age to 27 and change status
        let result3 = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[
                [:db/add {} :tt/age 27]
                [:db/add {} :tt/status :senior]
            ]'::TEXT)",
            alice_eid, alice_eid
        ))
        .expect("tx3 failed")
        .expect("NULL");

        let r3: serde_json::Value = serde_json::from_str(&result3).expect("parse");
        let tx3 = r3["db-after"]["basis-t"].as_i64().expect("tx3");

        (tx1, tx2, tx3, alice_eid)
    }

    // ========================================================================
    // 1. As-Of Queries
    // ========================================================================

    #[pg_test]
    fn test_as_of_tx1_sees_original() {
        setup();
        setup_temporal_schema();
        let (tx1, _, _, _) = create_temporal_chain();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?age .
                 :where [?p :tt/name \"Alice\"] [?p :tt/age ?age]]'::TEXT,
                '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("as-of tx1 failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(json["result"].as_i64().expect("age"), 25);
    }

    #[pg_test]
    fn test_as_of_tx2_sees_update() {
        setup();
        setup_temporal_schema();
        let (_, tx2, _, _) = create_temporal_chain();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?age .
                 :where [?p :tt/name \"Alice\"] [?p :tt/age ?age]]'::TEXT,
                '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx2
        ))
        .expect("as-of tx2 failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(json["result"].as_i64().expect("age"), 26);
    }

    #[pg_test]
    fn test_as_of_tx3_sees_latest() {
        setup();
        setup_temporal_schema();
        let (_, _, tx3, _) = create_temporal_chain();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?age .
                 :where [?p :tt/name \"Alice\"] [?p :tt/age ?age]]'::TEXT,
                '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx3
        ))
        .expect("as-of tx3 failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(json["result"].as_i64().expect("age"), 27);
    }

    #[pg_test]
    fn test_as_of_entity_not_yet_created() {
        setup();
        setup_temporal_schema();
        let (tx1, _, _, _) = create_temporal_chain();

        // Create another entity in a later transaction
        Spi::run(
            "SELECT mentat_transact('[[:db/add \"bob\" :tt/name \"Bob\"] [:db/add \"bob\" :tt/age 20]]'::TEXT)",
        )
        .expect("bob failed");

        // As-of tx1, Bob should not exist
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?age .
                 :where [?p :tt/name \"Bob\"] [?p :tt/age ?age]]'::TEXT,
                '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("as-of query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert!(json["result"].is_null(), "Bob should not exist at tx1");
    }

    #[pg_test]
    fn test_as_of_with_collection_result() {
        setup();
        setup_temporal_schema();
        let (tx1, _, _, _) = create_temporal_chain();

        // At tx1, only Alice exists
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find [?name ...]
                 :where [?p :tt/name ?name]]'::TEXT,
                '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("as-of coll failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let names = json["result"].as_array().expect("names array");
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].as_str().expect("name"), "Alice");
    }

    // ========================================================================
    // 2. Since Queries
    // ========================================================================

    #[pg_test]
    fn test_since_tx1_sees_changes() {
        setup();
        setup_temporal_schema();
        let (tx1, _, _, _) = create_temporal_chain();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?e ?a ?v ?tx ?added
                 :where [?e ?a ?v ?tx ?added]]'::TEXT,
                '{{\"since\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("since query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let results = json["results"].as_array().expect("results array");

        assert!(results.len() > 0, "Should have datoms since tx1");

        for row in results {
            let tx = row[3].as_i64().expect("tx");
            assert!(tx > tx1, "All txs should be after tx1");
        }
    }

    #[pg_test]
    fn test_since_tx2_sees_only_tx3() {
        setup();
        setup_temporal_schema();
        let (_, tx2, tx3, _) = create_temporal_chain();

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?e ?a ?v ?tx ?added
                 :where [?e ?a ?v ?tx ?added]]'::TEXT,
                '{{\"since\": {}}}'::jsonb)::TEXT",
            tx2
        ))
        .expect("since tx2 query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let results = json["results"].as_array().expect("results array");

        for row in results {
            let tx = row[3].as_i64().expect("tx");
            assert!(tx > tx2, "All txs should be after tx2");
        }
    }

    // ========================================================================
    // 3. History Queries
    // ========================================================================

    #[pg_test]
    fn test_history_shows_all_age_values() {
        setup();
        setup_temporal_schema();
        let (_, _, _, _) = create_temporal_chain();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?age ?tx ?added
                 :where
                 [?p :tt/name \"Alice\"]
                 [?p :tt/age ?age ?tx ?added]]'::TEXT,
                '{\"history\": true}'::jsonb)::TEXT",
        )
        .expect("history query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let results = json["results"].as_array().expect("results array");

        let ages: Vec<i64> = results
            .iter()
            .map(|r| r[0].as_i64().expect("age"))
            .collect();

        assert!(ages.contains(&25), "Should contain age 25");
        assert!(ages.contains(&26), "Should contain age 26");
        assert!(ages.contains(&27), "Should contain age 27");
    }

    #[pg_test]
    fn test_history_shows_retractions() {
        setup();
        setup_temporal_schema();
        create_temporal_chain();

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?age ?added
                 :where
                 [?p :tt/name \"Alice\"]
                 [?p :tt/age ?age ?tx ?added]]'::TEXT,
                '{\"history\": true}'::jsonb)::TEXT",
        )
        .expect("history query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let results = json["results"].as_array().expect("results array");

        let retractions: Vec<&serde_json::Value> = results
            .iter()
            .filter(|r| r[1].as_bool() == Some(false))
            .collect();

        // Age was updated twice (25->26, 26->27), so there should be retractions
        assert!(retractions.len() >= 2, "Should have retractions for old ages");
    }

    #[pg_test]
    fn test_history_with_cardinality_many_retract() {
        setup();
        setup_temporal_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :tt/name \"TagEntity\"]
                [:db/add \"e\" :tt/tags \"tag1\"]
                [:db/add \"e\" :tt/tags \"tag2\"]
                [:db/add \"e\" :tt/tags \"tag3\"]
            ]'::TEXT)",
        )
        .expect("initial tags failed")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Retract one tag
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :tt/tags \"tag2\"]]'::TEXT)",
            eid
        ))
        .expect("retract tag failed");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?tag ?added
                 :where
                 [?e :tt/name \"TagEntity\"]
                 [?e :tt/tags ?tag ?tx ?added]]'::TEXT,
                '{\"history\": true}'::jsonb)::TEXT",
        )
        .expect("history query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let results = json["results"].as_array().expect("results array");

        // Should have: tag1(true), tag2(true), tag2(false), tag3(true) = at least 4
        assert!(results.len() >= 4);

        let tag2_retraction = results
            .iter()
            .find(|r| r[0].as_str() == Some("tag2") && r[1].as_bool() == Some(false));
        assert!(tag2_retraction.is_some(), "Should have tag2 retraction");
    }

    // ========================================================================
    // 4. Multi-Entity Temporal
    // ========================================================================

    #[pg_test]
    fn test_temporal_multiple_entities() {
        setup();
        setup_temporal_schema();

        let r1 = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"a\" :tt/name \"Alice\"]
                [:db/add \"a\" :tt/age 20]
            ]'::TEXT)",
        )
        .expect("tx1")
        .expect("NULL");
        let j1: serde_json::Value = serde_json::from_str(&r1).unwrap();
        let tx1 = j1["db-after"]["basis-t"].as_i64().unwrap();
        let alice = j1["tempids"]["a"].as_i64().unwrap();

        let r2 = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"b\" :tt/name \"Bob\"]
                [:db/add \"b\" :tt/age 22]
            ]'::TEXT)",
        )
        .expect("tx2")
        .expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).unwrap();
        let tx2 = j2["db-after"]["basis-t"].as_i64().unwrap();

        // Update Alice's age
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :tt/age 21]]'::TEXT)",
            alice
        ))
        .expect("tx3");

        // As-of tx1: only Alice exists
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?name ...] :where [?p :tt/name ?name]]'::TEXT,
             '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("as-of tx1")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let names = json["result"].as_array().unwrap();
        assert_eq!(names.len(), 1);

        // As-of tx2: both exist
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?name ...] :where [?p :tt/name ?name]]'::TEXT,
             '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx2
        ))
        .expect("as-of tx2")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let names = json["result"].as_array().unwrap();
        assert_eq!(names.len(), 2);
    }

    // ========================================================================
    // 5. Temporal with Predicates
    // ========================================================================

    #[pg_test]
    fn test_as_of_with_predicate() {
        setup();
        setup_temporal_schema();
        let (tx1, _, _, _) = create_temporal_chain();

        // At tx1, Alice is 25. Predicate age > 20 should match.
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?name .
                 :where
                 [?p :tt/name ?name]
                 [?p :tt/age ?age]
                 [(> ?age 20)]]'::TEXT,
                '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("as-of predicate failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(json["result"].as_str().expect("name"), "Alice");
    }

    // ========================================================================
    // 6. Status Keyword Changes Over Time
    // ========================================================================

    #[pg_test]
    fn test_keyword_history() {
        setup();
        setup_temporal_schema();
        let (tx1, _, _, _) = create_temporal_chain();

        // At tx1, status should be :active
        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?status .
                 :where [?p :tt/name \"Alice\"] [?p :tt/status ?status]]'::TEXT,
                '{{\"asOf\": {}}}'::jsonb)::TEXT",
            tx1
        ))
        .expect("status at tx1 failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let status = json["result"].as_str().expect("status");
        assert!(
            status.contains("active"),
            "Status at tx1 should be :active, got {}",
            status
        );
    }
}
