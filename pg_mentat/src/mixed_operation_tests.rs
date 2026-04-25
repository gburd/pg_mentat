// Mixed operation tests: testing combinations of add, retract, CAS,
// upsert, retractEntity, and schema operations in various sequences.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod mixed_operation_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_mx_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :mx/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :mx/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :mx/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :mx/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :mx/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"u\" :db/ident :mx/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"b\" :db/ident :mx/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :mx/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("mx schema");
    }

    // ========================================================================
    // Add + Retract in same TX (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_mx_add_and_retract_different_attrs() {
        setup(); setup_mx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :mx/name \"test\" :mx/val 10}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :mx/name \"updated\"] [:db/retract {} :mx/val 10]]'::TEXT)", eid, eid
        )).expect("mixed");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :mx/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "updated");
    }

    #[pg_test]
    fn test_mx_add_new_and_retract_old_many() {
        setup(); setup_mx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :mx/name \"h\"] [:db/add \"e\" :mx/tags \"old1\"] [:db/add \"e\" :mx/tags \"old2\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :mx/tags \"old1\"] [:db/add {} :mx/tags \"new1\"]]'::TEXT)", eid, eid
        )).expect("mixed");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :mx/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tags: Vec<&str> = v["result"].as_array().expect("arr").iter().map(|t| t.as_str().expect("s")).collect();
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"old2"));
        assert!(tags.contains(&"new1"));
    }

    #[pg_test]
    fn test_mx_create_and_retract_entity_same_tx_batch() {
        setup(); setup_mx_schema();
        // Create entity, add tags, retract one tag - all in sequence
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :mx/name \"test\" :mx/tags \"a\" :mx/tags \"b\" :mx/tags \"c\"}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :mx/tags \"b\"] [:db/add {} :mx/tags \"d\"]]'::TEXT)", eid, eid
        )).expect("mixed");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :mx/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3); // a, c, d
    }

    // ========================================================================
    // Multi-entity operations in same TX (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_mx_create_two_entities_link() {
        setup(); setup_mx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"p\" :mx/name \"Parent\" :mx/val 100} {:db/id \"c\" :mx/name \"Child\" :mx/val 50 :mx/ref \"p\"}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let parent = j["tempids"]["p"].as_i64().expect("p");
        let child = j["tempids"]["c"].as_i64().expect("c");
        assert_ne!(parent, child);
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :mx/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", child
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("r"), parent);
    }

    #[pg_test]
    fn test_mx_update_two_entities_same_tx() {
        setup(); setup_mx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"a\" :mx/name \"A\" :mx/val 1} {:db/id \"b\" :mx/name \"B\" :mx/val 2}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("a");
        let b = j["tempids"]["b"].as_i64().expect("b");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :mx/val 10] [:db/add {} :mx/val 20]]'::TEXT)", a, b
        )).expect("update both");
        let qa = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :mx/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", a
        )).expect("q").expect("NULL");
        let va: serde_json::Value = serde_json::from_str(&qa).expect("parse");
        assert_eq!(va["result"].as_i64().expect("v"), 10);
    }

    #[pg_test]
    fn test_mx_create_and_update_in_sequence() {
        setup(); setup_mx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :mx/name \"init\" :mx/val 0}]'::TEXT)"
        ).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // 5 updates
        for i in 1..=5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :mx/val {}] [:db/add {} :mx/name \"v{}\"]]'::TEXT)", eid, i * 10, eid, i
            )).expect("update");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n ?v :where [{e} :mx/name ?n] [{e} :mx/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", e = eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let results = v["results"].as_array().expect("arr");
        assert_eq!(results.len(), 1);
    }

    // ========================================================================
    // Upsert with other operations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_mx_upsert_then_add_tags() {
        setup(); setup_mx_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :mx/uid \"MU1\" :mx/name \"Alice\"}]'::TEXT)").expect("create");
        // Upsert and add tags
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :mx/uid \"MU1\" :mx/tags \"tag1\"}]'::TEXT)").expect("upsert+tags");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :mx/uid \"MU1\"] [?e :mx/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_mx_upsert_then_ref() {
        setup(); setup_mx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"target\" :mx/name \"Target\"} {:db/id \"e\" :mx/uid \"MU2\" :mx/name \"Source\"}]'::TEXT)"
        ).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let target = j["tempids"]["target"].as_i64().expect("target");
        Spi::run(&format!(
            "SELECT mentat_transact('[{{:db/id \"e\" :mx/uid \"MU2\" :mx/ref {}}}]'::TEXT)", target
        )).expect("upsert+ref");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?rn . :where [?e :mx/uid \"MU2\"] [?e :mx/ref ?r] [?r :mx/name ?rn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Target");
    }

    #[pg_test]
    fn test_mx_upsert_with_bool_and_kw() {
        setup(); setup_mx_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :mx/uid \"MU3\" :mx/flag true :mx/status :draft}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :mx/uid \"MU3\" :mx/flag false :mx/status :published}]'::TEXT)").expect("upsert");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?f ?s :where [?e :mx/uid \"MU3\"] [?e :mx/flag ?f] [?e :mx/status ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    // ========================================================================
    // Schema + data mixed operations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_mx_define_attr_then_use_immediately() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :mx.new/attr1 :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :mx.new/attr1 \"works\"]]'::TEXT)").expect("use");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [_ :mx.new/attr1 ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "works");
    }

    #[pg_test]
    fn test_mx_define_5_attrs_use_all() {
        setup();
        for i in 0..5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"a\" :db/ident :mx.dyn/a{} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}]'::TEXT)", i
            )).expect("schema");
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :mx.dyn/a{} \"val-{}\"]]'::TEXT)", i, i, i
            )).expect("use");
        }
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        for i in 0..5 {
            assert!(s.contains(&format!("mx.dyn/a{}", i)));
        }
    }

    #[pg_test]
    fn test_mx_interleave_schema_data_10_rounds() {
        setup();
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"a\" :db/ident :mx.il/r{i} :db/valueType :db.type/long :db/cardinality :db.cardinality/one}}]'::TEXT)", i = i
            )).expect("schema");
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{i}\" :mx.il/r{i} {i}]]'::TEXT)", i = i
            )).expect("data");
        }
        // Verify last one
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [_ :mx.il/r9 ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 9);
    }

    // ========================================================================
    // Batch mixed operations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_mx_batch_add_retract_mixed() {
        setup(); setup_mx_schema();
        // Create 20 entities
        let mut ops = Vec::new();
        for i in 0..20 {
            ops.push(format!("{{:db/id \"e{i}\" :mx/name \"ent-{i}\" :mx/val {i}}}", i = i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");

        // Update first 10, retract last 10
        let mut mixed_ops = Vec::new();
        for i in 0..10 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            mixed_ops.push(format!("[:db/add {} :mx/val {}]", eid, (i + 1) * 100));
        }
        for i in 10..20 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            mixed_ops.push(format!("[:db/retractEntity {}]", eid));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", mixed_ops.join("\n"))).expect("mixed batch");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':mx/name') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 10);
    }

    #[pg_test]
    fn test_mx_batch_50_creates_then_updates() {
        setup(); setup_mx_schema();
        let mut ops = Vec::new();
        for i in 0..50 {
            ops.push(format!("{{:db/id \"e{i}\" :mx/name \"ent-{i}\" :mx/val 0}}", i = i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");

        let mut updates = Vec::new();
        for i in 0..50 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            updates.push(format!("[:db/add {} :mx/val {}]", eid, i * 2));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", updates.join("\n"))).expect("batch update");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :mx/val ?v] [(> ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() >= 49);
    }

    #[pg_test]
    fn test_mx_rapid_fire_20_txs() {
        setup(); setup_mx_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :mx/name \"rapid\" :mx/val 0}]'::TEXT)"
        ).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :mx/val {}] [:db/add {} :mx/name \"r-{}\"]]'::TEXT)", eid, i, eid, i
            )).expect("rapid");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n ?v :where [{e} :mx/name ?n] [{e} :mx/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", e = eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    // ========================================================================
    // End-to-end workflow (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_mx_full_lifecycle() {
        setup(); setup_mx_schema();
        // Create
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :mx/name \"lifecycle\" :mx/val 0 :mx/flag false :mx/status :new}]'::TEXT)"
        ).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        // Update
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :mx/val 100] [:db/add {} :mx/flag true] [:db/add {} :mx/status :active]]'::TEXT)", eid, eid, eid)).expect("update");
        // Add tags
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :mx/tags \"important\"] [:db/add {} :mx/tags \"reviewed\"]]'::TEXT)", eid, eid)).expect("tags");
        // Query
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n ?v ?f :where [{e} :mx/name ?n] [{e} :mx/val ?v] [{e} :mx/flag ?f]]'::TEXT, '{{}}'::jsonb)::TEXT", e = eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
        // Retract
        Spi::run(&format!("SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid)).expect("delete");
        // Verify gone
        let q2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :mx/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert!(v2["result"].is_null());
    }

    #[pg_test]
    fn test_mx_project_management_workflow() {
        setup(); setup_mx_schema();
        // Create project
        let r1 = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"proj\" :mx/name \"Big Project\" :mx/status :planning :mx/val 0}]'::TEXT)"
        ).expect("create").expect("NULL");
        let j1: serde_json::Value = serde_json::from_str(&r1).expect("parse");
        let proj = j1["tempids"]["proj"].as_i64().expect("proj");

        // Create tasks
        let r2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{{:db/id \"t1\" :mx/name \"Task 1\" :mx/status :todo :mx/ref {}}} {{:db/id \"t2\" :mx/name \"Task 2\" :mx/status :todo :mx/ref {}}} {{:db/id \"t3\" :mx/name \"Task 3\" :mx/status :todo :mx/ref {}}}]'::TEXT)", proj, proj, proj
        )).expect("tasks").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        let t1 = j2["tempids"]["t1"].as_i64().expect("t1");
        let t2 = j2["tempids"]["t2"].as_i64().expect("t2");

        // Complete tasks
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :mx/status :done] [:db/add {} :mx/status :in-progress]]'::TEXT)", t1, t2)).expect("progress");

        // Update project status
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :mx/status :active] [:db/add {} :mx/val 33]]'::TEXT)", proj, proj)).expect("project update");

        // Query pending tasks
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?n ...] :where [?t :mx/ref {}] [?t :mx/name ?n] [?t :mx/status :todo]]'::TEXT, '{{}}'::jsonb)::TEXT", proj
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1); // Only Task 3
    }
}
