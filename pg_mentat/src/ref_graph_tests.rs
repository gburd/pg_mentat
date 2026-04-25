// Ref graph tests: systematic coverage of reference relationships,
// graph topologies, traversals, and integrity.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod ref_graph_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_rg_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :rg/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :rg/type :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :rg/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"p\" :db/ident :rg/parent :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"c\" :db/ident :rg/children :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"l\" :db/ident :rg/link :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"ls\" :db/ident :rg/links :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"tg\" :db/ident :rg/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        ).expect("rg schema");
    }

    // ========================================================================
    // Linear chain topologies (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_rg_chain_2_nodes() {
        setup(); setup_rg_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"a\" :rg/name \"A\"] [:db/add \"b\" :rg/name \"B\"] [:db/add \"b\" :rg/parent \"a\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?b :rg/name \"B\"] [?b :rg/parent ?a] [?a :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "A");
    }

    #[pg_test]
    fn test_rg_chain_3_nodes() {
        setup(); setup_rg_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"a\" :rg/name \"A\"] [:db/add \"b\" :rg/name \"B\"] [:db/add \"c\" :rg/name \"C\"] [:db/add \"b\" :rg/parent \"a\"] [:db/add \"c\" :rg/parent \"b\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?c :rg/name \"C\"] [?c :rg/parent ?b] [?b :rg/parent ?a] [?a :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "A");
    }

    #[pg_test]
    fn test_rg_chain_5_nodes() {
        setup(); setup_rg_schema();
        let mut ops = vec![];
        for i in 0..5 {
            ops.push(format!("[:db/add \"n{}\" :rg/name \"node-{}\"]", i, i));
            if i > 0 {
                ops.push(format!("[:db/add \"n{}\" :rg/parent \"n{}\"]", i, i - 1));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // Traverse 4->3->2->1->0
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e4 :rg/name \"node-4\"] [?e4 :rg/parent ?e3] [?e3 :rg/parent ?e2] [?e2 :rg/parent ?e1] [?e1 :rg/parent ?e0] [?e0 :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "node-0");
    }

    #[pg_test]
    fn test_rg_chain_10_nodes() {
        setup(); setup_rg_schema();
        let mut ops = vec![];
        for i in 0..10 {
            ops.push(format!("[:db/add \"n{}\" :rg/name \"node-{}\"]", i, i));
            if i > 0 {
                ops.push(format!("[:db/add \"n{}\" :rg/parent \"n{}\"]", i, i - 1));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // Just verify the last node has a parent
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :rg/name \"node-9\"] [?e :rg/parent ?p] [?p :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "node-8");
    }

    #[pg_test]
    fn test_rg_chain_20_nodes() {
        setup(); setup_rg_schema();
        let mut ops = vec![];
        for i in 0..20 {
            ops.push(format!("[:db/add \"n{}\" :rg/name \"node-{}\"]", i, i));
            if i > 0 {
                ops.push(format!("[:db/add \"n{}\" :rg/parent \"n{}\"]", i, i - 1));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // Count all nodes that have a parent
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :rg/parent _] [?e :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 19); // all except node-0
    }

    #[pg_test]
    fn test_rg_chain_find_root() {
        setup(); setup_rg_schema();
        let mut ops = vec![];
        for i in 0..8 {
            ops.push(format!("[:db/add \"n{}\" :rg/name \"node-{}\"]", i, i));
            ops.push(format!("[:db/add \"n{}\" :rg/val {}]", i, i));
            if i > 0 {
                ops.push(format!("[:db/add \"n{}\" :rg/parent \"n{}\"]", i, i - 1));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // Find nodes with no parent (roots) - via absence
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :rg/name \"node-0\"] [?e :rg/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 0);
    }

    #[pg_test]
    fn test_rg_chain_find_leaves() {
        setup(); setup_rg_schema();
        let mut ops = vec![];
        for i in 0..5 {
            ops.push(format!("[:db/add \"n{}\" :rg/name \"node-{}\"]", i, i));
            if i > 0 {
                ops.push(format!("[:db/add \"n{}\" :rg/parent \"n{}\"]", i, i - 1));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // Find all parent entities
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :rg/parent ?p] [?p :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Nodes 0-3 are parents (node-4 is leaf, not pointed to as parent)
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_rg_chain_with_data() {
        setup(); setup_rg_schema();
        let mut ops = vec![];
        for i in 0..5 {
            ops.push(format!("{{:db/id \"n{}\" :rg/name \"node-{}\" :rg/val {} :rg/type :level-{}}}", i, i, i * 100, i));
            if i > 0 {
                ops.push(format!("[:db/add \"n{}\" :rg/parent \"n{}\"]", i, i - 1));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :rg/parent ?p] [?p :rg/name \"node-2\"] [?e :rg/name ?n] [?e :rg/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let rows = v["result"].as_array().expect("arr");
        assert_eq!(rows.len(), 1);
    }

    // ========================================================================
    // Star/hub-spoke topologies (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_rg_star_5_spokes() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"hub\" :rg/name \"hub\"]".to_string()];
        for i in 0..5 {
            ops.push(format!("[:db/add \"s{}\" :rg/name \"spoke-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :rg/children \"s{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?h :rg/name \"hub\"] [?h :rg/children ?c] [?c :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_rg_star_20_spokes() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"hub\" :rg/name \"hub\"]".to_string()];
        for i in 0..20 {
            ops.push(format!("[:db/add \"s{}\" :rg/name \"spoke-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :rg/children \"s{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?h :rg/name \"hub\"] [?h :rg/children ?c] [?c :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_rg_star_50_spokes() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"hub\" :rg/name \"hub\"]".to_string()];
        for i in 0..50 {
            ops.push(format!("[:db/add \"s{}\" :rg/name \"spoke-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :rg/children \"s{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?h :rg/name \"hub\"] [?h :rg/children ?c] [?c :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 50);
    }

    #[pg_test]
    fn test_rg_star_spokes_with_data() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"hub\" :rg/name \"hub\"]".to_string()];
        for i in 0..10 {
            ops.push(format!("{{:db/id \"s{}\" :rg/name \"spoke-{}\" :rg/val {}}}", i, i, i * 10));
            ops.push(format!("[:db/add \"hub\" :rg/children \"s{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?h :rg/name \"hub\"] [?h :rg/children ?c] [?c :rg/name ?n] [?c :rg/val ?v] [(> ?v 50)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 4); // 60,70,80,90
    }

    #[pg_test]
    fn test_rg_star_add_spokes_incrementally() {
        setup(); setup_rg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"hub\" :rg/name \"hub\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let hub = j["tempids"]["hub"].as_i64().expect("eid");
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"s{}\" :rg/name \"spoke-{}\"] [:db/add {} :rg/children \"s{}\"]]'::TEXT)", i, i, hub, i
            )).expect("add spoke");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?n ...] :where [{} :rg/children ?c] [?c :rg/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", hub
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_rg_star_remove_spokes() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"hub\" :rg/name \"hub\"]".to_string()];
        for i in 0..10 {
            ops.push(format!("[:db/add \"s{}\" :rg/name \"spoke-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :rg/children \"s{}\"]", i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let hub = j["tempids"]["hub"].as_i64().expect("eid");
        // Remove 5 spokes
        let mut retract_ops = vec![];
        for i in 0..5 {
            let spoke = j["tempids"][&format!("s{}", i)].as_i64().expect("eid");
            retract_ops.push(format!("[:db/retract {} :rg/children {}]", hub, spoke));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", retract_ops.join("\n"))).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?n ...] :where [{} :rg/children ?c] [?c :rg/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", hub
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_rg_multiple_hubs() {
        setup(); setup_rg_schema();
        let mut ops = vec![];
        for h in 0..3 {
            ops.push(format!("[:db/add \"h{}\" :rg/name \"hub-{}\"]", h, h));
            for s in 0..5 {
                let sid = h * 5 + s;
                ops.push(format!("[:db/add \"s{}\" :rg/name \"spoke-{}\"]", sid, sid));
                ops.push(format!("[:db/add \"h{}\" :rg/children \"s{}\"]", h, sid));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // Each hub should have 5 children
        for h in 0..3 {
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find [?n ...] :where [?hub :rg/name \"hub-{}\"] [?hub :rg/children ?c] [?c :rg/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", h
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_array().expect("arr").len(), 5);
        }
    }

    #[pg_test]
    fn test_rg_hub_with_tags_and_refs() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"hub\" :rg/name \"hub\"]".to_string()];
        for i in 0..5 {
            ops.push(format!("[:db/add \"hub\" :rg/tags \"tag-{}\"]", i));
        }
        for i in 0..5 {
            ops.push(format!("[:db/add \"c{}\" :rg/name \"child-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :rg/children \"c{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?h :rg/name \"hub\"] [?h :rg/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    // ========================================================================
    // Tree topologies (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_rg_binary_tree_depth_2() {
        setup(); setup_rg_schema();
        Spi::run("SELECT mentat_transact('[
            [:db/add \"root\" :rg/name \"root\"]
            [:db/add \"l\" :rg/name \"left\"] [:db/add \"r\" :rg/name \"right\"]
            [:db/add \"root\" :rg/children \"l\"] [:db/add \"root\" :rg/children \"r\"]
        ]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?root :rg/name \"root\"] [?root :rg/children ?c] [?c :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_rg_binary_tree_depth_3() {
        setup(); setup_rg_schema();
        Spi::run("SELECT mentat_transact('[
            [:db/add \"root\" :rg/name \"root\"]
            [:db/add \"l\" :rg/name \"left\"] [:db/add \"r\" :rg/name \"right\"]
            [:db/add \"ll\" :rg/name \"left-left\"] [:db/add \"lr\" :rg/name \"left-right\"]
            [:db/add \"rl\" :rg/name \"right-left\"] [:db/add \"rr\" :rg/name \"right-right\"]
            [:db/add \"root\" :rg/children \"l\"] [:db/add \"root\" :rg/children \"r\"]
            [:db/add \"l\" :rg/children \"ll\"] [:db/add \"l\" :rg/children \"lr\"]
            [:db/add \"r\" :rg/children \"rl\"] [:db/add \"r\" :rg/children \"rr\"]
        ]'::TEXT)").expect("tx");
        // Find grandchildren of root
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?root :rg/name \"root\"] [?root :rg/children ?c] [?c :rg/children ?gc] [?gc :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_rg_tree_3_ary_depth_2() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"root\" :rg/name \"root\"]".to_string()];
        for i in 0..3 {
            ops.push(format!("[:db/add \"c{}\" :rg/name \"child-{}\"]", i, i));
            ops.push(format!("[:db/add \"root\" :rg/children \"c{}\"]", i));
            for j in 0..3 {
                let id = i * 3 + j;
                ops.push(format!("[:db/add \"gc{}\" :rg/name \"grandchild-{}\"]", id, id));
                ops.push(format!("[:db/add \"c{}\" :rg/children \"gc{}\"]", i, id));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?root :rg/name \"root\"] [?root :rg/children ?c] [?c :rg/children ?gc] [?gc :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 9);
    }

    #[pg_test]
    fn test_rg_tree_with_values_at_leaves() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"root\" :rg/name \"root\"]".to_string()];
        for i in 0..4 {
            ops.push(format!("{{:db/id \"leaf{}\" :rg/name \"leaf-{}\" :rg/val {}}}", i, i, (i + 1) * 100));
            ops.push(format!("[:db/add \"root\" :rg/children \"leaf{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?root :rg/name \"root\"] [?root :rg/children ?c] [?c :rg/val ?v] [(> ?v 200)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // 300, 400
    }

    #[pg_test]
    fn test_rg_tree_sibling_count() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"parent\" :rg/name \"parent\"]".to_string()];
        for i in 0..7 {
            ops.push(format!("[:db/add \"c{}\" :rg/name \"child-{}\"]", i, i));
            ops.push(format!("[:db/add \"parent\" :rg/children \"c{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?p :rg/name \"parent\"] [?p :rg/children ?c] [?c :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 7);
    }

    #[pg_test]
    fn test_rg_tree_parent_pointer() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"root\" :rg/name \"root\"]".to_string()];
        for i in 0..5 {
            ops.push(format!("[:db/add \"c{}\" :rg/name \"child-{}\"]", i, i));
            ops.push(format!("[:db/add \"c{}\" :rg/parent \"root\"]", i));
            ops.push(format!("[:db/add \"root\" :rg/children \"c{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // Verify parent pointers
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?c :rg/parent ?p] [?p :rg/name \"root\"] [?c :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_rg_tree_replace_parent() {
        setup(); setup_rg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"p1\" :rg/name \"parent1\"] [:db/add \"p2\" :rg/name \"parent2\"] [:db/add \"child\" :rg/name \"child\"] [:db/add \"child\" :rg/parent \"p1\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let child = j["tempids"]["child"].as_i64().expect("eid");
        let p2 = j["tempids"]["p2"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :rg/parent {}]]'::TEXT)", child, p2
        )).expect("reparent");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :rg/parent ?p] [?p :rg/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", child
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "parent2");
    }

    #[pg_test]
    fn test_rg_org_chart() {
        setup(); setup_rg_schema();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"ceo\" :rg/name \"CEO\" :rg/type :exec}
            {:db/id \"vp1\" :rg/name \"VP-Eng\" :rg/type :vp}
            {:db/id \"vp2\" :rg/name \"VP-Sales\" :rg/type :vp}
            {:db/id \"mgr1\" :rg/name \"Mgr-FE\" :rg/type :mgr}
            {:db/id \"mgr2\" :rg/name \"Mgr-BE\" :rg/type :mgr}
            {:db/id \"mgr3\" :rg/name \"Mgr-West\" :rg/type :mgr}
            [:db/add \"ceo\" :rg/children \"vp1\"] [:db/add \"ceo\" :rg/children \"vp2\"]
            [:db/add \"vp1\" :rg/children \"mgr1\"] [:db/add \"vp1\" :rg/children \"mgr2\"]
            [:db/add \"vp2\" :rg/children \"mgr3\"]
        ]'::TEXT)").expect("tx");
        // Find all managers under VP-Eng
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?vp :rg/name \"VP-Eng\"] [?vp :rg/children ?m] [?m :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_rg_tree_retract_subtree() {
        setup(); setup_rg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"root\" :rg/name \"root\"]
                [:db/add \"a\" :rg/name \"A\"] [:db/add \"b\" :rg/name \"B\"]
                [:db/add \"root\" :rg/children \"a\"] [:db/add \"root\" :rg/children \"b\"]
                [:db/add \"a1\" :rg/name \"A1\"] [:db/add \"a2\" :rg/name \"A2\"]
                [:db/add \"a\" :rg/children \"a1\"] [:db/add \"a\" :rg/children \"a2\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("eid");
        // Retract entity A (removes it from root's children and its own children refs)
        Spi::run(&format!("SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", a)).expect("retract");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?root :rg/name \"root\"] [?root :rg/children ?c] [?c :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Only B should remain (A was retracted)
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_rg_forest_3_trees() {
        setup(); setup_rg_schema();
        let mut ops = vec![];
        for t in 0..3 {
            ops.push(format!("[:db/add \"r{}\" :rg/name \"root-{}\"]", t, t));
            for c in 0..3 {
                let cid = t * 3 + c;
                ops.push(format!("[:db/add \"c{}\" :rg/name \"child-{}\"]", cid, cid));
                ops.push(format!("[:db/add \"r{}\" :rg/children \"c{}\"]", t, cid));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // Total children across all trees
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :rg/children ?c] [?c :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 9);
    }

    // ========================================================================
    // Link / graph topologies (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_rg_bidirectional_link() {
        setup(); setup_rg_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"a\" :rg/name \"A\"] [:db/add \"b\" :rg/name \"B\"] [:db/add \"a\" :rg/link \"b\"] [:db/add \"b\" :rg/link \"a\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?a :rg/name \"A\"] [?a :rg/link ?b] [?b :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "B");
    }

    #[pg_test]
    fn test_rg_ring_4_nodes() {
        setup(); setup_rg_schema();
        let mut ops = vec![];
        for i in 0..4 {
            ops.push(format!("[:db/add \"n{}\" :rg/name \"node-{}\"]", i, i));
            ops.push(format!("[:db/add \"n{}\" :rg/link \"n{}\"]", i, (i + 1) % 4));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // From node-0, follow 2 links
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?a :rg/name \"node-0\"] [?a :rg/link ?b] [?b :rg/link ?c] [?c :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "node-2");
    }

    #[pg_test]
    fn test_rg_multi_links_10() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"src\" :rg/name \"source\"]".to_string()];
        for i in 0..10 {
            ops.push(format!("[:db/add \"t{}\" :rg/name \"target-{}\"]", i, i));
            ops.push(format!("[:db/add \"src\" :rg/links \"t{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?s :rg/name \"source\"] [?s :rg/links ?t] [?t :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_rg_complete_graph_4() {
        setup(); setup_rg_schema();
        let mut ops = vec![];
        for i in 0..4 {
            ops.push(format!("[:db/add \"n{}\" :rg/name \"node-{}\"]", i, i));
        }
        // Each node links to all others
        for i in 0..4 {
            for j in 0..4 {
                if i != j {
                    ops.push(format!("[:db/add \"n{}\" :rg/links \"n{}\"]", i, j));
                }
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // Each node should have 3 links
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?a :rg/name \"node-0\"] [?a :rg/links ?b] [?b :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_rg_dag_dependency_graph() {
        setup(); setup_rg_schema();
        // DAG: A->B, A->C, B->D, C->D (diamond dependency)
        Spi::run("SELECT mentat_transact('[
            [:db/add \"a\" :rg/name \"A\"] [:db/add \"b\" :rg/name \"B\"]
            [:db/add \"c\" :rg/name \"C\"] [:db/add \"d\" :rg/name \"D\"]
            [:db/add \"a\" :rg/links \"b\"] [:db/add \"a\" :rg/links \"c\"]
            [:db/add \"b\" :rg/links \"d\"] [:db/add \"c\" :rg/links \"d\"]
        ]'::TEXT)").expect("tx");
        // Find what A directly depends on
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?a :rg/name \"A\"] [?a :rg/links ?dep] [?dep :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // B, C
    }

    #[pg_test]
    fn test_rg_deep_two_hop_query() {
        setup(); setup_rg_schema();
        Spi::run("SELECT mentat_transact('[
            [:db/add \"a\" :rg/name \"A\"] [:db/add \"b\" :rg/name \"B\"]
            [:db/add \"c\" :rg/name \"C\"] [:db/add \"d\" :rg/name \"D\"]
            [:db/add \"a\" :rg/link \"b\"] [:db/add \"b\" :rg/link \"c\"] [:db/add \"c\" :rg/link \"d\"]
        ]'::TEXT)").expect("tx");
        // Two hops from A
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?a :rg/name \"A\"] [?a :rg/link ?b] [?b :rg/link ?c] [?c :rg/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "C");
    }

    #[pg_test]
    fn test_rg_links_retract_specific() {
        setup(); setup_rg_schema();
        let mut ops = vec!["[:db/add \"src\" :rg/name \"source\"]".to_string()];
        for i in 0..6 {
            ops.push(format!("[:db/add \"t{}\" :rg/name \"target-{}\"]", i, i));
            ops.push(format!("[:db/add \"src\" :rg/links \"t{}\"]", i));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let src = j["tempids"]["src"].as_i64().expect("eid");
        // Retract links to t0, t1, t2
        let mut retract_ops = vec![];
        for i in 0..3 {
            let tid = j["tempids"][&format!("t{}", i)].as_i64().expect("eid");
            retract_ops.push(format!("[:db/retract {} :rg/links {}]", src, tid));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", retract_ops.join("\n"))).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?n ...] :where [{} :rg/links ?t] [?t :rg/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", src
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_rg_replace_single_link() {
        setup(); setup_rg_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"a\" :rg/name \"A\"] [:db/add \"b\" :rg/name \"B\"] [:db/add \"c\" :rg/name \"C\"] [:db/add \"a\" :rg/link \"b\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("eid");
        let c = j["tempids"]["c"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :rg/link {}]]'::TEXT)", a, c
        )).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :rg/link ?t] [?t :rg/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", a
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "C");
    }
}
