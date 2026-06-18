// Exhaustive retraction tests covering db/retract, db/retractEntity,
// partial retractions, and retraction edge cases.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_retract_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :rt/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :rt/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :rt/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :rt/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :rt/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :rt/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :rt/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"rm\" :db/ident :rt/refs :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        ).expect("retract schema");
    }

    fn create_entity() -> i64 {
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :rt/name \"Entity\" :rt/val 42 :rt/flag true :rt/dbl 3.14 :rt/kw :active}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        j["tempids"]["e"].as_i64().expect("eid")
    }

    // ========================================================================
    // db/retract - String
    // ========================================================================

    #[pg_test]
    fn test_rt_retract_string() {
        setup();
        setup_retract_schema();
        let eid = create_entity();
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :rt/name \"Entity\"]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :rt/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    // ========================================================================
    // db/retract - Long
    // ========================================================================

    #[pg_test]
    fn test_rt_retract_long() {
        setup();
        setup_retract_schema();
        let eid = create_entity();
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :rt/val 42]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :rt/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    // ========================================================================
    // db/retract - Boolean
    // ========================================================================

    #[pg_test]
    fn test_rt_retract_boolean_true() {
        setup();
        setup_retract_schema();
        let eid = create_entity();
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :rt/flag true]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :rt/flag ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    // ========================================================================
    // db/retract - Double
    // ========================================================================

    #[pg_test]
    fn test_rt_retract_double() {
        setup();
        setup_retract_schema();
        let eid = create_entity();
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :rt/dbl 3.14]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :rt/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    // ========================================================================
    // db/retract - Keyword
    // ========================================================================

    #[pg_test]
    fn test_rt_retract_keyword() {
        setup();
        setup_retract_schema();
        let eid = create_entity();
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :rt/kw :active]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :rt/kw ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    // ========================================================================
    // db/retract - Cardinality many
    // ========================================================================

    #[pg_test]
    fn test_rt_retract_one_from_many() {
        setup();
        setup_retract_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :rt/name \"tagged\"]
                [:db/add \"e\" :rt/tags \"a\"]
                [:db/add \"e\" :rt/tags \"b\"]
                [:db/add \"e\" :rt/tags \"c\"]
            ]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :rt/tags \"b\"]]'::TEXT)",
            eid
        ))
        .expect("retract one");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :rt/tags ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let arr = v["result"].as_array().expect("arr");
        assert_eq!(arr.len(), 2);
        let strs: Vec<&str> = arr.iter().map(|v| v.as_str().expect("s")).collect();
        assert!(strs.contains(&"a"));
        assert!(strs.contains(&"c"));
        assert!(!strs.contains(&"b"));
    }

    #[pg_test]
    fn test_rt_retract_all_from_many() {
        setup();
        setup_retract_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :rt/name \"all-gone\"]
                [:db/add \"e\" :rt/tags \"x\"]
                [:db/add \"e\" :rt/tags \"y\"]
            ]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[
                [:db/retract {} :rt/tags \"x\"]
                [:db/retract {} :rt/tags \"y\"]
            ]'::TEXT)",
            eid, eid
        ))
        .expect("retract all");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :rt/tags ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    // ========================================================================
    // db/retractEntity
    // ========================================================================

    #[pg_test]
    fn test_rt_retract_entity_basic() {
        setup();
        setup_retract_schema();
        let eid = create_entity();
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retractEntity");

        // All attributes should be gone
        for attr in &[":rt/name", ":rt/val", ":rt/flag", ":rt/dbl", ":rt/kw"] {
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} {} ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
                eid, attr
            ))
            .expect("q")
            .expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert!(
                v["result"].is_null(),
                "Attr {} should be null after retractEntity",
                attr
            );
        }
    }

    #[pg_test]
    fn test_rt_retract_entity_with_many_attrs() {
        setup();
        setup_retract_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"e\" :rt/name \"Full\" :rt/val 99 :rt/flag true :rt/dbl 9.9 :rt/kw :doomed}
                [:db/add \"e\" :rt/tags \"t1\"]
                [:db/add \"e\" :rt/tags \"t2\"]
                [:db/add \"e\" :rt/tags \"t3\"]
            ]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retractEntity");

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = false",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert!(
            count >= 7,
            "Should have at least 7 retraction datoms, got {}",
            count
        );
    }

    #[pg_test]
    fn test_rt_retract_entity_with_refs() {
        setup();
        setup_retract_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"target\" :rt/name \"target\"]
                [:db/add \"s0\" :rt/name \"spoke0\"]
                [:db/add \"s1\" :rt/name \"spoke1\"]
                [:db/add \"e\" :rt/name \"doomed\"]
                [:db/add \"e\" :rt/ref \"target\"]
                [:db/add \"e\" :rt/refs \"s0\"]
                [:db/add \"e\" :rt/refs \"s1\"]
            ]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retractEntity");

        // Ref should be gone
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :rt/ref ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    // ========================================================================
    // Retract then re-add
    // ========================================================================

    #[pg_test]
    fn test_rt_retract_then_readd_string() {
        setup();
        setup_retract_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :rt/name \"original\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :rt/name \"original\"]]'::TEXT)",
            eid
        ))
        .expect("retract");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :rt/name \"revived\"]]'::TEXT)",
            eid
        ))
        .expect("re-add");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :rt/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "revived");
    }

    #[pg_test]
    fn test_rt_retract_entity_then_create_new() {
        setup();
        setup_retract_schema();
        let eid = create_entity();
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retractEntity");

        // Create a completely new entity (old eid should not be reused)
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"new\" :rt/name \"Fresh\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let new_eid = j["tempids"]["new"].as_i64().expect("new eid");
        assert_ne!(new_eid, eid, "New entity should get a different ID");
    }

    // ========================================================================
    // Retract and add in same tx
    // ========================================================================

    #[pg_test]
    fn test_rt_retract_and_add_same_tx() {
        setup();
        setup_retract_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :rt/val 10]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[
                [:db/retract {} :rt/val 10]
                [:db/add {} :rt/val 20]
            ]'::TEXT)",
            eid, eid
        ))
        .expect("retract+add");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :rt/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 20);
    }

    // ========================================================================
    // Retraction creates history datoms
    // ========================================================================

    #[pg_test]
    fn test_rt_retraction_creates_history() {
        setup();
        setup_retract_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :rt/val 42]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :rt/val 42]]'::TEXT)",
            eid
        ))
        .expect("retract");

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND added = false",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert!(count >= 1, "Retraction should create history datom");
    }

    // ========================================================================
    // Batch retractions
    // ========================================================================

    #[pg_test]
    fn test_rt_batch_retract_10_entities() {
        setup();
        setup_retract_schema();

        // Create 10 entities
        let mut ops = Vec::new();
        for i in 0..10 {
            ops.push(format!("[:db/add \"e{i}\" :rt/name \"entity-{i}\"]", i = i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");

        // Retract all 10
        let mut retract_ops = Vec::new();
        for i in 0..10 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            retract_ops.push(format!("[:db/retractEntity {}]", eid));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            retract_ops.join("\n")
        ))
        .expect("batch retract");

        // Verify all retracted
        for i in 0..10 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :rt/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert!(v["result"].is_null(), "Entity {} should be retracted", i);
        }
    }
}
