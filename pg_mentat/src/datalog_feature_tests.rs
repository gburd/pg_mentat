// Datalog feature verification tests: covers in-transaction tempid merging,
// upsert conflict detection, transaction function dispatch (retractEntity, CAS),
// and deduplication after tempid remapping.
//
// These tests target the specific code paths added in Tasks #4 and #5:
// - Phase A (DB-level upsert) and Phase B (in-transaction tempid merging)
// - recognize_tx_fn() dispatch framework
// - execute_retract_entity_fn() and execute_cas_fn()

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
             $$"
        ).expect("create helper");
    }

    fn raises_error(sql: &str) -> bool {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<bool>(&format!(
            "SELECT mentat._test_raises_error('{}')", escaped
        )).expect("raises_error call").unwrap_or(false)
    }

    fn setup_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"uid\" :db/ident :df/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"email\" :db/ident :df/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"code\" :db/ident :df/code :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/value}
                {:db/id \"name\" :db/ident :df/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"val\" :db/ident :df/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"tags\" :db/ident :df/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"ref\" :db/ident :df/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("schema");
    }

    // ========================================================================
    // Phase B: In-transaction tempid merging
    //
    // When two tempids in the same transaction assert the same value for a
    // :db.unique/identity attribute, they should be merged into a single entity.
    // ========================================================================

    #[pg_test]
    fn test_df_intx_tempid_merge_same_uid() {
        setup(); setup_schema();
        // Two tempids, same :df/uid value, same transaction.
        // They should resolve to the same entity.
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"a\" :df/uid \"MERGE1\" :df/name \"Alice\"}
                {:db/id \"b\" :df/uid \"MERGE1\" :df/val 42}
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a_id = j["tempids"]["a"].as_i64().expect("a");
        let b_id = j["tempids"]["b"].as_i64().expect("b");
        assert_eq!(a_id, b_id, "Both tempids should resolve to the same entity");

        // Verify both attributes landed on the merged entity
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :df/uid \"MERGE1\"] [?e :df/name ?n] [?e :df/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(results.len(), 1, "Should be exactly one entity");
        assert_eq!(results[0][0].as_str().expect("name"), "Alice");
        assert_eq!(results[0][1].as_i64().expect("val"), 42);
    }

    #[pg_test]
    fn test_df_intx_merge_three_tempids() {
        setup(); setup_schema();
        // Three tempids all referencing the same unique/identity value
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"a\" :df/uid \"MERGE3\" :df/name \"Alice\"}
                {:db/id \"b\" :df/uid \"MERGE3\" :df/val 10}
                [:db/add \"c\" :df/uid \"MERGE3\"]
                [:db/add \"c\" :df/tags \"tag1\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a_id = j["tempids"]["a"].as_i64().expect("a");
        let b_id = j["tempids"]["b"].as_i64().expect("b");
        let c_id = j["tempids"]["c"].as_i64().expect("c");
        assert_eq!(a_id, b_id, "a and b merged");
        assert_eq!(b_id, c_id, "b and c merged");
    }

    #[pg_test]
    fn test_df_intx_merge_with_existing_entity() {
        setup(); setup_schema();
        // First: create an entity in the DB
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :df/uid \"EXISTING1\" :df/name \"Original\"}]'::TEXT)").expect("create");

        // Second tx: two new tempids reference the same uid as the existing entity
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"x\" :df/uid \"EXISTING1\" :df/val 100}
                {:db/id \"y\" :df/uid \"EXISTING1\" :df/tags \"updated\"}
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let x_id = j["tempids"]["x"].as_i64().expect("x");
        let y_id = j["tempids"]["y"].as_i64().expect("y");
        assert_eq!(x_id, y_id, "Both tempids should resolve to the existing entity");

        // Verify the original name is preserved and new attrs added
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :df/uid \"EXISTING1\"] [?e :df/name ?n] [?e :df/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0][0].as_str().expect("name"), "Original");
        assert_eq!(results[0][1].as_i64().expect("val"), 100);
    }

    // ========================================================================
    // Deduplication after tempid remapping
    //
    // When two tempids merge into one entity, identical assertions (same E, A, V)
    // should be deduplicated to avoid constraint violations.
    // ========================================================================

    #[pg_test]
    fn test_df_dedup_identical_assertions_after_merge() {
        setup(); setup_schema();
        // Both tempids assert the same uid value; after merging they produce
        // duplicate [e, :df/uid, "DEDUP1", true] datoms that must be deduped.
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"a\" :df/uid \"DEDUP1\" :df/name \"Alice\"}
                {:db/id \"b\" :df/uid \"DEDUP1\" :df/name \"Alice\"}
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a_id = j["tempids"]["a"].as_i64().expect("a");
        let b_id = j["tempids"]["b"].as_i64().expect("b");
        assert_eq!(a_id, b_id, "Should merge to same entity");

        // Verify only one name datom exists (not duplicated)
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :df/uid \"DEDUP1\"] [?e :df/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("name"), "Alice");
    }

    // ========================================================================
    // Conflict detection: tempid resolves to multiple entities
    //
    // If a single tempid asserts two different :db.unique/identity attributes
    // that each resolve to a different existing entity, the transaction
    // should fail with a conflict error.
    // ========================================================================

    #[pg_test]
    fn test_df_conflict_two_unique_attrs_different_entities() {
        setup(); setup_schema();
        // Create two separate entities with different unique attrs
        Spi::run("SELECT mentat_transact('[
            {:db/id \"e1\" :df/uid \"CONFLICT-UID\" :df/name \"Entity1\"}
            {:db/id \"e2\" :df/email \"conflict@test.com\" :df/name \"Entity2\"}
        ]'::TEXT)").expect("create");

        // Now try to assert a single tempid with both unique values.
        // This should fail because :df/uid resolves to e1 and :df/email resolves to e2.
        assert!(
            raises_error("SELECT mentat_transact('[{:db/id \"x\" :df/uid \"CONFLICT-UID\" :df/email \"conflict@test.com\" :df/name \"Merged\"}]'::TEXT)"),
            "Should fail: tempid resolves to two different entities"
        );
    }

    // ========================================================================
    // unique/value still errors (not upserts)
    // ========================================================================

    #[pg_test]
    fn test_df_unique_value_no_upsert() {
        setup(); setup_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :df/code \"UNIQUE-CODE\"]]'::TEXT)").expect("first");
        assert!(
            raises_error("SELECT mentat_transact('[[:db/add \"e2\" :df/code \"UNIQUE-CODE\"]]'::TEXT)"),
            "unique/value should error, not upsert"
        );
    }

    #[pg_test]
    fn test_df_unique_value_intx_no_merge() {
        setup(); setup_schema();
        // Two tempids with same unique/value in same tx should error
        assert!(
            raises_error("SELECT mentat_transact('[{:db/id \"a\" :df/code \"SAME-CODE\" :df/name \"A\"} {:db/id \"b\" :df/code \"SAME-CODE\" :df/name \"B\"}]'::TEXT)"),
            "unique/value should not merge tempids"
        );
    }

    // ========================================================================
    // Transaction function: retractEntity
    //
    // Tests the execute_retract_entity_fn() dispatch via recognize_tx_fn()
    // ========================================================================

    #[pg_test]
    fn test_df_retract_entity_basic() {
        setup(); setup_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :df/name \"ToDelete\" :df/val 42 :df/tags \"t1\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Retract the entity
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/retractEntity {}]]'::TEXT)", eid
        )).expect("retractEntity");

        // All attributes should be gone
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?a ?v :where [{} ?a ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(results.len(), 0, "All attributes should be retracted");
    }

    #[pg_test]
    fn test_df_retract_entity_preserves_others() {
        setup(); setup_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"keep\" :df/name \"Keep\" :df/val 1}
                {:db/id \"del\" :df/name \"Delete\" :df/val 2}
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let keep_id = j["tempids"]["keep"].as_i64().expect("keep");
        let del_id = j["tempids"]["del"].as_i64().expect("del");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/retractEntity {}]]'::TEXT)", del_id
        )).expect("retractEntity");

        // "keep" entity should still have its attributes
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :df/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", keep_id
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("name"), "Keep");
    }

    #[pg_test]
    fn test_df_retract_entity_with_many_attrs() {
        setup(); setup_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"e\" :df/name \"Multi\" :df/val 99}
                [:db/add \"e\" :df/tags \"alpha\"]
                [:db/add \"e\" :df/tags \"beta\"]
                [:db/add \"e\" :df/tags \"gamma\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/retractEntity {}]]'::TEXT)", eid
        )).expect("retractEntity");

        // Verify tags are all gone
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :df/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tags = v["result"].as_array().expect("arr");
        assert_eq!(tags.len(), 0, "All cardinality-many values retracted");
    }

    #[pg_test]
    fn test_df_retract_entity_alt_keyword() {
        setup(); setup_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :df/name \"AltKW\" :df/val 7}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Use :db/retractEntity (alternative keyword form)
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid
        )).expect("retractEntity via :db/retractEntity");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :df/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null(), "Entity should be fully retracted");
    }

    // ========================================================================
    // Transaction function: CAS with upsert interaction
    //
    // Tests CAS combined with unique/identity upsert resolution
    // ========================================================================

    #[pg_test]
    fn test_df_cas_after_upsert() {
        setup(); setup_schema();
        // Create entity via upsert
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :df/uid \"CAS-UP1\" :df/val 10}]'::TEXT)").expect("create");

        // Upsert to get the entity ID, then CAS
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :df/uid \"CAS-UP1\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let eid = v["result"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :df/val 10 20]]'::TEXT)", eid
        )).expect("cas");

        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :df/uid \"CAS-UP1\"] [?e :df/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_i64().expect("v"), 20);
    }

    #[pg_test]
    fn test_df_retract_entity_then_recreate() {
        setup(); setup_schema();
        // Create, retract, then create again with same uid
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :df/uid \"RECREATE1\" :df/name \"V1\" :df/val 1}]'::TEXT)").expect("create");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :df/uid \"RECREATE1\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let eid = v["result"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db.fn/retractEntity {}]]'::TEXT)", eid
        )).expect("retract");

        // Recreate with same uid - should create a new entity (old one is retracted)
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e2\" :df/uid \"RECREATE1\" :df/name \"V2\" :df/val 2}]'::TEXT)",
        ).expect("recreate").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        // The new entity should exist with the new values
        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :df/uid \"RECREATE1\"] [?e :df/name ?n] [?e :df/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        let results = v2["results"].as_array().expect("arr");
        assert!(results.len() >= 1, "Recreated entity should exist");
    }

    // ========================================================================
    // recognize_tx_fn dispatch: unknown function keyword
    // ========================================================================

    #[pg_test]
    fn test_df_unknown_tx_fn_errors() {
        setup(); setup_schema();
        assert!(
            raises_error("SELECT mentat_transact('[[:db.fn/nonexistent 12345]]'::TEXT)"),
            "Unknown transaction function should error"
        );
    }
}
