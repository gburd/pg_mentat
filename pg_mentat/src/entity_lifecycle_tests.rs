// Entity lifecycle tests: comprehensive coverage of entity creation,
// modification, retraction, and querying through various lifecycle stages.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_el_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :el/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :el/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :el/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :el/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :el/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :el/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :el/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"rs\" :db/ident :el/refs :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"u\" :db/ident :el/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            ]'::TEXT)",
        ).expect("el schema");
    }

    // ========================================================================
    // Entity creation patterns (12 tests)
    // ========================================================================

    #[pg_test]
    fn test_el_create_single_attr() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :el/name \"alice\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e"].as_i64().expect("eid") > 0);
    }

    #[pg_test]
    fn test_el_create_two_attrs() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :el/name \"alice\"] [:db/add \"e\" :el/val 42]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :el/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 42);
    }

    #[pg_test]
    fn test_el_create_all_attr_types() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :el/name \"full\" :el/val 42 :el/dbl 3.14 :el/flag true :el/status :active}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n ?v ?d ?f :where [{} :el/name ?n] [{} :el/val ?v] [{} :el/dbl ?d] [{} :el/flag ?f]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid, eid, eid, eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let row = v["result"].as_array().expect("arr");
        assert_eq!(row.len(), 1);
    }

    #[pg_test]
    fn test_el_create_with_many_values() {
        setup();
        setup_el_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :el/name \"tagged\"] [:db/add \"e\" :el/tags \"a\"] [:db/add \"e\" :el/tags \"b\"] [:db/add \"e\" :el/tags \"c\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :el/name \"tagged\"] [?e :el/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_el_create_with_ref() {
        setup();
        setup_el_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"parent\" :el/name \"parent\"] [:db/add \"child\" :el/name \"child\"] [:db/add \"child\" :el/ref \"parent\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?c :el/name \"child\"] [?c :el/ref ?p] [?p :el/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "parent");
    }

    #[pg_test]
    fn test_el_create_batch_10() {
        setup();
        setup_el_schema();
        let mut ops = vec![];
        for i in 0..10 {
            ops.push(format!(
                "{{:db/id \"e{}\" :el/name \"item-{}\" :el/val {}}}",
                i, i, i
            ));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 10);
    }

    #[pg_test]
    fn test_el_create_batch_50() {
        setup();
        setup_el_schema();
        let mut ops = vec![];
        for i in 0..50 {
            ops.push(format!(
                "{{:db/id \"e{}\" :el/name \"item-{}\" :el/val {} :el/flag {}}}",
                i,
                i,
                i,
                if i % 2 == 0 { "true" } else { "false" }
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :el/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 50);
    }

    #[pg_test]
    fn test_el_create_via_upsert() {
        setup();
        setup_el_schema();
        Spi::run("SELECT mentat_transact('[{:el/uid \"bob\" :el/val 100}]'::TEXT)")
            .expect("create");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :el/uid \"bob\"] [?e :el/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 100);
    }

    #[pg_test]
    fn test_el_create_interlinked_graph() {
        setup();
        setup_el_schema();
        Spi::run(
            "SELECT mentat_transact('[
            {:db/id \"a\" :el/name \"A\"}
            {:db/id \"b\" :el/name \"B\" :el/ref \"a\"}
            {:db/id \"c\" :el/name \"C\" :el/ref \"b\"}
            {:db/id \"d\" :el/name \"D\" :el/ref \"c\"}
        ]'::TEXT)",
        )
        .expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?d :el/name \"D\"] [?d :el/ref ?c] [?c :el/ref ?b] [?b :el/ref ?a] [?a :el/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "A");
    }

    #[pg_test]
    fn test_el_create_with_many_refs() {
        setup();
        setup_el_schema();
        let mut ops = vec!["[:db/add \"hub\" :el/name \"hub\"]".to_string()];
        for i in 0..10 {
            ops.push(format!("[:db/add \"t{}\" :el/name \"target-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :el/refs \"t{}\"]", i));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?h :el/name \"hub\"] [?h :el/refs ?r] [?r :el/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_el_create_sequential_10() {
        setup();
        setup_el_schema();
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :el/name \"seq-{}\"]]'::TEXT)",
                i, i
            ))
            .expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :el/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_el_create_map_and_vector_forms() {
        setup();
        setup_el_schema();
        // Map form
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e1\" :el/name \"map-form\" :el/val 1}]'::TEXT)",
        )
        .expect("map");
        // Vector form
        Spi::run("SELECT mentat_transact('[[:db/add \"e2\" :el/name \"vector-form\"] [:db/add \"e2\" :el/val 2]]'::TEXT)").expect("vec");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :el/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Entity modification patterns (12 tests)
    // ========================================================================

    #[pg_test]
    fn test_el_update_string() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :el/name \"before\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :el/name \"after\"]]'::TEXT)",
            eid
        ))
        .expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :el/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "after");
    }

    #[pg_test]
    fn test_el_update_long() {
        setup();
        setup_el_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :el/val 10]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :el/val 99]]'::TEXT)",
            eid
        ))
        .expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :el/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 99);
    }

    #[pg_test]
    fn test_el_update_boolean() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :el/flag true]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :el/flag false]]'::TEXT)",
            eid
        ))
        .expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?f . :where [{} :el/flag ?f]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_el_update_keyword() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :el/status :draft]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :el/status :published]]'::TEXT)",
            eid
        ))
        .expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?s . :where [{} :el/status ?s]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("published"));
    }

    #[pg_test]
    fn test_el_update_preserves_other_attrs() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :el/name \"alice\" :el/val 42 :el/flag true}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :el/val 99]]'::TEXT)",
            eid
        ))
        .expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :el/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "alice");
    }

    #[pg_test]
    fn test_el_add_tag_to_existing() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :el/name \"tagged\"] [:db/add \"e\" :el/tags \"initial\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :el/tags \"new-tag\"]]'::TEXT)",
            eid
        ))
        .expect("add tag");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :el/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_el_update_ref_target() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"t1\" :el/name \"target1\"] [:db/add \"t2\" :el/name \"target2\"] [:db/add \"src\" :el/name \"source\"] [:db/add \"src\" :el/ref \"t1\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let src = j["tempids"]["src"].as_i64().expect("eid");
        let t2 = j["tempids"]["t2"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :el/ref {}]]'::TEXT)",
            src, t2
        ))
        .expect("update ref");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :el/ref ?t] [?t :el/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", src
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "target2");
    }

    #[pg_test]
    fn test_el_update_via_upsert() {
        setup();
        setup_el_schema();
        Spi::run("SELECT mentat_transact('[{:el/uid \"alice\" :el/val 100}]'::TEXT)")
            .expect("create");
        Spi::run("SELECT mentat_transact('[{:el/uid \"alice\" :el/val 200}]'::TEXT)")
            .expect("upsert");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :el/uid \"alice\"] [?e :el/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 200);
    }

    #[pg_test]
    fn test_el_update_multiple_attrs_one_tx() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :el/name \"old\" :el/val 1 :el/flag false}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :el/name \"new\"] [:db/add {} :el/val 999] [:db/add {} :el/flag true]]'::TEXT)", eid, eid, eid
        )).expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n ?v ?f :where [{} :el/name ?n] [{} :el/val ?v] [{} :el/flag ?f]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid, eid, eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let row = &v["result"].as_array().expect("arr")[0];
        assert_eq!(row[0].as_str().expect("n"), "new");
        assert_eq!(row[1].as_i64().expect("v"), 999);
        assert_eq!(row[2].as_bool().expect("f"), true);
    }

    #[pg_test]
    fn test_el_update_20_sequential() {
        setup();
        setup_el_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :el/val 0]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :el/val {}]]'::TEXT)",
                eid, i
            ))
            .expect("update");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :el/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 20);
    }

    #[pg_test]
    fn test_el_batch_update_20_entities() {
        setup();
        setup_el_schema();
        let mut create_ops = vec![];
        for i in 0..20 {
            create_ops.push(format!(
                "{{:db/id \"e{}\" :el/name \"item-{}\" :el/val 0}}",
                i, i
            ));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            create_ops.join("\n")
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let mut update_ops = vec![];
        for i in 0..20 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            update_ops.push(format!("[:db/add {} :el/val {}]", eid, (i + 1) * 100));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            update_ops.join("\n")
        ))
        .expect("batch update");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :el/val ?v] [(> ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_el_add_many_refs_incrementally() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"hub\" :el/name \"hub\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let hub = j["tempids"]["hub"].as_i64().expect("eid");
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"t{}\" :el/name \"target-{}\"] [:db/add {} :el/refs \"t{}\"]]'::TEXT)", i, i, hub, i
            )).expect("add ref");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?n ...] :where [{} :el/refs ?r] [?r :el/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", hub
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    // ========================================================================
    // Entity retraction patterns (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_el_retract_single_attr() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :el/name \"alice\" :el/val 42}]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :el/val 42]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :el/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_el_retract_entity() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :el/name \"doomed\" :el/val 42 :el/flag true}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :el/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_el_retract_many_value() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :el/tags \"a\"] [:db/add \"e\" :el/tags \"b\"] [:db/add \"e\" :el/tags \"c\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :el/tags \"b\"]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :el/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_el_retract_doesnt_affect_others() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e1\" :el/name \"keep\"] [:db/add \"e2\" :el/name \"remove\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let e2 = j["tempids"]["e2"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            e2
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :el/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_el_batch_retract_10() {
        setup();
        setup_el_schema();
        let mut ops = vec![];
        for i in 0..10 {
            ops.push(format!("{{:db/id \"e{}\" :el/name \"item-{}\"}}", i, i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let mut retracts = vec![];
        for i in 0..10 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            retracts.push(format!("[:db/retractEntity {}]", eid));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            retracts.join("\n")
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :el/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_el_retract_then_readd() {
        setup();
        setup_el_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :el/val 42]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :el/val 42]]'::TEXT)",
            eid
        ))
        .expect("retract");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :el/val 99]]'::TEXT)",
            eid
        ))
        .expect("readd");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :el/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 99);
    }

    #[pg_test]
    fn test_el_retract_ref() {
        setup();
        setup_el_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"p\" :el/name \"parent\"] [:db/add \"c\" :el/name \"child\"] [:db/add \"c\" :el/ref \"p\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let child = j["tempids"]["c"].as_i64().expect("eid");
        let parent = j["tempids"]["p"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :el/ref {}]]'::TEXT)",
            child, parent
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?p . :where [{} :el/ref ?p]]'::TEXT, '{{}}'::jsonb)::TEXT",
            child
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_el_retract_many_ref() {
        setup();
        setup_el_schema();
        let mut ops = vec!["[:db/add \"hub\" :el/name \"hub\"]".to_string()];
        for i in 0..5 {
            ops.push(format!("[:db/add \"t{}\" :el/name \"t{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :el/refs \"t{}\"]", i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let hub = j["tempids"]["hub"].as_i64().expect("eid");
        let t0 = j["tempids"]["t0"].as_i64().expect("eid");
        let t1 = j["tempids"]["t1"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :el/refs {}] [:db/retract {} :el/refs {}]]'::TEXT)", hub, t0, hub, t1
        )).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?r ...] :where [{} :el/refs ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", hub
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_el_retract_entity_with_many_values() {
        setup();
        setup_el_schema();
        let mut ops = vec!["[:db/add \"e\" :el/name \"doomed\"]".to_string()];
        for i in 0..20 {
            ops.push(format!("[:db/add \"e\" :el/tags \"tag-{}\"]", i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :el/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_el_retract_half_of_batch() {
        setup();
        setup_el_schema();
        let mut ops = vec![];
        for i in 0..20 {
            ops.push(format!(
                "{{:db/id \"e{}\" :el/name \"item-{}\" :el/val {}}}",
                i, i, i
            ));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let mut retracts = vec![];
        for i in 0..10 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            retracts.push(format!("[:db/retractEntity {}]", eid));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            retracts.join("\n")
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :el/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }
}
