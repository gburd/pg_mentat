// Stress and scale tests: large entity counts, high transaction volume,
// large attribute counts, and data-intensive queries.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod stress_scale_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_stress_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :ss/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :ss/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :ss/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :ss/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :ss/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :ss/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"rm\" :db/ident :ss/refs :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"k\" :db/ident :ss/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("stress schema");
    }

    // ========================================================================
    // Large batch entity creation
    // ========================================================================

    #[pg_test]
    fn test_ss_batch_500_entities() {
        setup(); setup_stress_schema();
        let mut ops = Vec::new();
        for i in 0..500 {
            ops.push(format!("{{:db/id \"e{i}\" :ss/name \"ent-{i}\" :ss/val {i}}}", i = i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch 500");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':ss/name') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 500);
    }

    #[pg_test]
    fn test_ss_batch_1000_simple_entities() {
        setup(); setup_stress_schema();
        let mut ops = Vec::new();
        for i in 0..1000 {
            ops.push(format!("[:db/add \"e{i}\" :ss/name \"entity-{i}\"]", i = i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch 1000");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':ss/name') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 1000);
    }

    #[pg_test]
    fn test_ss_batch_200_multi_attr_entities() {
        setup(); setup_stress_schema();
        let mut ops = Vec::new();
        for i in 0..200 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :ss/name \"ent-{i}\" :ss/val {i} :ss/dbl {d} :ss/flag {f} :ss/kw :type-{k}}}",
                i = i, d = (i as f64) * 0.7, f = if i % 2 == 0 { "true" } else { "false" }, k = i % 5
            ));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 200);
    }

    // ========================================================================
    // Sequential transaction throughput
    // ========================================================================

    #[pg_test]
    fn test_ss_100_sequential_transactions() {
        setup(); setup_stress_schema();
        for i in 0..100 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{i}\" :ss/name \"seq-{i}\"]]'::TEXT)", i = i
            )).expect("seq tx");
        }
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':ss/name') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 100);
    }

    #[pg_test]
    fn test_ss_100_sequential_updates_same_entity() {
        setup(); setup_stress_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :ss/val 0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for i in 1..=100 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :ss/val {}]]'::TEXT)", eid, i)).expect("update");
        }

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :ss/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 100);
    }

    // ========================================================================
    // Large cardinality-many
    // ========================================================================

    #[pg_test]
    fn test_ss_200_tags_one_entity() {
        setup(); setup_stress_schema();
        let mut ops = vec!["[:db/add \"e\" :ss/name \"tagged\"]".to_string()];
        for i in 0..200 {
            ops.push(format!("[:db/add \"e\" :ss/tags \"tag-{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("many tags");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :ss/name \"tagged\"] [?e :ss/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 200);
    }

    #[pg_test]
    fn test_ss_100_refs_one_entity() {
        setup(); setup_stress_schema();
        let mut ops = vec!["[:db/add \"hub\" :ss/name \"hub\"]".to_string()];
        for i in 0..100 {
            ops.push(format!("[:db/add \"s{}\" :ss/name \"spoke-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :ss/refs \"s{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("many refs");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?r ...] :where [?h :ss/name \"hub\"] [?h :ss/refs ?r]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 100);
    }

    // ========================================================================
    // Query performance under data volume
    // ========================================================================

    #[pg_test]
    fn test_ss_query_500_entities_filter() {
        setup(); setup_stress_schema();
        let mut ops = Vec::new();
        for i in 0..500 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :ss/name \"ent-{i}\" :ss/val {i} :ss/flag {f}}}",
                i = i, f = if i % 2 == 0 { "true" } else { "false" }
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("data");

        // Filter: active entities with val > 250
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :ss/name ?n] [?e :ss/flag true] [?e :ss/val ?v] [(> ?v 250)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let names = v["result"].as_array().expect("arr");
        // Even values > 250: 252, 254, ..., 498 => 124 values
        assert!(names.len() > 100, "Should find >100 matches, got {}", names.len());
    }

    #[pg_test]
    fn test_ss_query_1000_entities_collection() {
        setup(); setup_stress_schema();
        let mut ops = Vec::new();
        for i in 0..1000 {
            ops.push(format!("[:db/add \"e{i}\" :ss/val {i}]", i = i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("data");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :ss/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1000);
    }

    // ========================================================================
    // Sequential query throughput
    // ========================================================================

    #[pg_test]
    fn test_ss_50_sequential_queries() {
        setup(); setup_stress_schema();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e1\" :ss/name \"Alice\" :ss/val 100}
                {:db/id \"e2\" :ss/name \"Bob\" :ss/val 200}
                {:db/id \"e3\" :ss/name \"Carol\" :ss/val 300}
            ]'::TEXT)",
        ).expect("data");

        for _ in 0..50 {
            let q = Spi::get_one::<String>(
                "SELECT mentat_query('[:find [?n ...] :where [?e :ss/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
            ).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_array().expect("arr").len(), 3);
        }
    }

    // ========================================================================
    // Schema with many attributes
    // ========================================================================

    #[pg_test]
    fn test_ss_define_50_attrs() {
        setup();
        let mut ops = Vec::new();
        for i in 0..50 {
            ops.push(format!(
                "{{:db/id \"a{i}\" :db/ident :ss.gen/attr-{i} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}",
                i = i
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("50 attrs");

        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        for i in 0..50 {
            assert!(result.contains(&format!("ss.gen/attr-{}", i)), "attr-{} missing", i);
        }
    }

    // ========================================================================
    // Batch retraction under scale
    // ========================================================================

    #[pg_test]
    fn test_ss_batch_retract_100_entities() {
        setup(); setup_stress_schema();
        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!("{{:db/id \"e{i}\" :ss/name \"doomed-{i}\" :ss/val {i}}}", i = i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");

        let mut retract_ops = Vec::new();
        for i in 0..100 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            retract_ops.push(format!("[:db/retractEntity {}]", eid));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", retract_ops.join("\n"))).expect("batch retract");

        // All should be retracted
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':ss/name') AND v_text LIKE 'doomed-%' AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 0, "All 100 entities should be retracted");
    }

    // ========================================================================
    // Mixed create/update/query workflow
    // ========================================================================

    #[pg_test]
    fn test_ss_create_update_query_cycle() {
        setup(); setup_stress_schema();

        // Create 50 entities
        let mut ops = Vec::new();
        for i in 0..50 {
            ops.push(format!("{{:db/id \"e{i}\" :ss/name \"cycle-{i}\" :ss/val 0}}", i = i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");

        // Update all 50 entities 5 times each
        for round in 1..=5 {
            let mut updates = Vec::new();
            for i in 0..50 {
                let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
                updates.push(format!("[:db/add {} :ss/val {}]", eid, round * 10 + i));
            }
            Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", updates.join("\n"))).expect("update batch");
        }

        // Query to verify final state
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :ss/val ?v] [(> ?v 40)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0, "Should find some values > 40");
    }
}
