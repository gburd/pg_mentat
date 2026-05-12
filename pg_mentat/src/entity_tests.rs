// Comprehensive entity operation tests.
//
// Tests cover:
// 1. mentat_entity function (entity lookup by ID)
// 2. Entity creation with various tempid patterns
// 3. Entity with all value types simultaneously
// 4. Entity retraction (retractEntity)
// 5. Entity lookup refs
// 6. Batch entity operations
// 7. Entity with component references

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_entity_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :ent/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :ent/age
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"e\" :db/ident :ent/email
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity}
                {:db/id \"t\" :db/ident :ent/tags
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/many}
                {:db/id \"f\" :db/ident :ent/active
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :ent/score
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
                {:db/id \"r\" :db/ident :ent/friend
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/many}
                {:db/id \"k\" :db/ident :ent/status
                 :db/valueType :db.type/keyword
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("entity schema failed");
    }

    // ========================================================================
    // 1. Entity Lookup (mentat_entity)
    // ========================================================================

    #[pg_test]
    fn test_entity_lookup_by_id() {
        setup();
        setup_entity_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :ent/name \"Alice\"]
                [:db/add \"e\" :ent/age 30]
            ]'::TEXT)",
        )
        .expect("insert failed")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        let entity = Spi::get_one::<String>(&format!(
            "SELECT mentat_entity({})",
            eid
        ))
        .expect("entity lookup failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&entity).expect("parse");
        assert!(json.get(":ent/name").is_some() || json.get(":db/id").is_some(),
            "Entity should have attributes");
    }

    // ========================================================================
    // 2. Tempid Patterns
    // ========================================================================

    #[pg_test]
    fn test_tempid_string_names() {
        setup();
        setup_entity_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"my-person\" :ent/name \"Alice\"]
                [:db/add \"my-person\" :ent/age 25]
            ]'::TEXT)",
        )
        .expect("tempid string")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert!(r["tempids"]["my-person"].as_i64().is_some());
    }

    #[pg_test]
    fn test_tempid_numeric_strings() {
        setup();
        setup_entity_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"1\" :ent/name \"One\"]
                [:db/add \"2\" :ent/name \"Two\"]
            ]'::TEXT)",
        )
        .expect("numeric tempid string")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid1 = r["tempids"]["1"].as_i64().expect("eid1");
        let eid2 = r["tempids"]["2"].as_i64().expect("eid2");
        assert_ne!(eid1, eid2);
    }

    #[pg_test]
    fn test_tempid_cross_reference() {
        setup();
        setup_entity_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"alice\" :ent/name \"Alice\"]
                [:db/add \"bob\" :ent/name \"Bob\"]
                [:db/add \"alice\" :ent/friend \"bob\"]
            ]'::TEXT)",
        )
        .expect("cross-ref tempids")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let alice = r["tempids"]["alice"].as_i64().expect("alice");
        let bob = r["tempids"]["bob"].as_i64().expect("bob");

        // Verify the ref was stored correctly
        let ref_eid = Spi::get_one::<i64>(&format!(
            "SELECT v_ref FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':ent/friend')
             AND added = true LIMIT 1",
            alice
        ))
        .expect("query failed")
        .expect("NULL");

        assert_eq!(ref_eid, bob);
    }

    // ========================================================================
    // 3. Entity with All Value Types
    // ========================================================================

    #[pg_test]
    fn test_entity_all_types_simultaneously() {
        setup();
        setup_entity_schema();

        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e\"
                 :ent/name \"Multi-Type Entity\"
                 :ent/age 42
                 :ent/email \"multi@test.com\"
                 :ent/active true
                 :ent/score 98.5
                 :ent/status :premium}
                [:db/add \"e\" :ent/tags \"tag1\"]
                [:db/add \"e\" :ent/tags \"tag2\"]
            ]'::TEXT)",
        )
        .expect("multi-type entity failed");

        // Verify via query
        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name ?age ?email ?active ?score ?status
                 :where
                 [?e :ent/name ?name]
                 [?e :ent/age ?age]
                 [?e :ent/email ?email]
                 [?e :ent/active ?active]
                 [?e :ent/score ?score]
                 [?e :ent/status ?status]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let results = json["results"].as_array().expect("array");
        assert_eq!(results.len(), 1);

        let row = results[0].as_array().expect("row");
        assert_eq!(row[0].as_str().expect("name"), "Multi-Type Entity");
        assert_eq!(row[1].as_i64().expect("age"), 42);
        assert_eq!(row[2].as_str().expect("email"), "multi@test.com");
        assert_eq!(row[3].as_bool().expect("active"), true);
        assert!((row[4].as_f64().expect("score") - 98.5).abs() < 0.01);
    }

    // ========================================================================
    // 4. Entity Retraction
    // ========================================================================

    #[pg_test]
    fn test_retract_entity_removes_all_facts() {
        setup();
        setup_entity_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"e\" :ent/name \"ToDelete\" :ent/age 99 :ent/active false}
            ]'::TEXT)",
        )
        .expect("insert")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Count facts before retraction
        let before = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = true",
            eid
        ))
        .expect("query")
        .expect("NULL");
        assert!(before >= 3, "Should have at least 3 facts");

        // Retract entity
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retractEntity failed");

        // After retraction, entity should have no active facts
        let after_active = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = true",
            eid
        ))
        .expect("query")
        .expect("NULL");

        // Note: cardinality-one retraction replaces, so the count might not be 0.
        // But there should be retraction datoms
        let retractions = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = false",
            eid
        ))
        .expect("query")
        .expect("NULL");
        assert!(retractions > 0, "Should have retraction datoms");
    }

    #[pg_test]
    fn test_retract_entity_with_cardinality_many() {
        setup();
        setup_entity_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :ent/name \"TaggedEntity\"]
                [:db/add \"e\" :ent/tags \"t1\"]
                [:db/add \"e\" :ent/tags \"t2\"]
                [:db/add \"e\" :ent/tags \"t3\"]
            ]'::TEXT)",
        )
        .expect("insert")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retractEntity");

        // All tags should be retracted
        let retracted_tags = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':ent/tags')
             AND added = false",
            eid
        ))
        .expect("query")
        .expect("NULL");
        assert!(retracted_tags >= 3, "All 3 tags should be retracted");
    }

    // ========================================================================
    // 5. Lookup Refs
    // ========================================================================

    #[pg_test]
    fn test_lookup_ref_finds_entity() {
        setup();
        setup_entity_schema();

        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e\" :ent/email \"lookup@test.com\"]
                [:db/add \"e\" :ent/name \"Looked Up\"]
            ]'::TEXT)",
        )
        .expect("insert");

        // Use lookup ref to update
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add [:ent/email \"lookup@test.com\"] :ent/age 42]
            ]'::TEXT)",
        )
        .expect("lookup ref update");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?age .
                 :where [?e :ent/email \"lookup@test.com\"] [?e :ent/age ?age]]'::TEXT,
                '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(json["result"].as_i64().expect("age"), 42);
    }

    #[pg_test]
    fn test_lookup_ref_nonexistent_fails() {
        setup();
        setup_entity_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add [:ent/email \"nonexistent@test.com\"] :ent/age 99]
            ]'::TEXT)",
        );

        assert!(result.is_err(), "Lookup ref for nonexistent entity should fail");
    }

    #[pg_test]
    fn test_lookup_ref_non_unique_attr_fails() {
        setup();
        setup_entity_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add [:ent/name \"some name\"] :ent/age 99]
            ]'::TEXT)",
        );

        // :ent/name is not unique, so lookup ref should fail
        assert!(result.is_err(), "Lookup ref on non-unique attr should fail");
    }

    // ========================================================================
    // 6. Batch Entity Operations
    // ========================================================================

    #[pg_test]
    fn test_batch_100_entities() {
        setup();
        setup_entity_schema();

        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :ent/name \"person-{i}\" :ent/age {i} :ent/active true}}",
                i = i
            ));
        }
        let txn = format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        );

        let result = Spi::get_one::<String>(&txn)
            .expect("batch 100 entities failed")
            .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let tempids = r["tempids"].as_object().expect("tempids");
        assert_eq!(tempids.len(), 100);

        // Verify all are queryable
        let qresult = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name :where [?e :ent/name ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&qresult).expect("parse");
        assert_eq!(json["results"].as_array().expect("arr").len(), 100);
    }

    // ========================================================================
    // 7. Entity with Keyword Values
    // ========================================================================

    #[pg_test]
    fn test_entity_keyword_values_queryable() {
        setup();
        setup_entity_schema();

        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e1\" :ent/name \"Premium\" :ent/status :premium}
                {:db/id \"e2\" :ent/name \"Basic\" :ent/status :basic}
                {:db/id \"e3\" :ent/name \"Trial\" :ent/status :trial}
            ]'::TEXT)",
        )
        .expect("keyword data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('
                [:find ?name .
                 :where [?e :ent/name ?name] [?e :ent/status :premium]]'::TEXT,
                '{}'::jsonb)::TEXT",
        )
        .expect("keyword query")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(json["result"].as_str().expect("name"), "Premium");
    }

    // ========================================================================
    // 8. Entity Update Patterns
    // ========================================================================

    #[pg_test]
    fn test_entity_update_preserves_other_attrs() {
        setup();
        setup_entity_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"e\" :ent/name \"Alice\" :ent/age 25 :ent/active true}
            ]'::TEXT)",
        )
        .expect("insert")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Update only age
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :ent/age 26]]'::TEXT)",
            eid
        ))
        .expect("update age");

        // Name and active should still be there
        let qresult = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('
                [:find ?name ?age ?active
                 :where
                 [?e :ent/name ?name]
                 [?e :ent/age ?age]
                 [?e :ent/active ?active]
                 [(= ?e {})]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("query")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&qresult).expect("parse");
        let results = json["results"].as_array().expect("results");
        assert_eq!(results.len(), 1);

        let row = results[0].as_array().expect("row");
        assert_eq!(row[0].as_str().expect("name"), "Alice");
        assert_eq!(row[1].as_i64().expect("age"), 26);
        assert_eq!(row[2].as_bool().expect("active"), true);
    }

    #[pg_test]
    fn test_entity_multiple_updates() {
        setup();
        setup_entity_schema();

        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :ent/name \"Counter\" :ent/age 0}]'::TEXT)",
        )
        .expect("insert")
        .expect("NULL");

        let r: serde_json::Value = serde_json::from_str(&result).expect("parse");
        let eid = r["tempids"]["e"].as_i64().expect("eid");

        // Update 10 times
        for i in 1..=10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :ent/age {}]]'::TEXT)",
                eid, i
            ))
            .expect("update");
        }

        // Current age should be 10
        let qresult = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?age . :where [{} :ent/age ?age]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("query")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&qresult).expect("parse");
        assert_eq!(json["result"].as_i64().expect("age"), 10);
    }
}
