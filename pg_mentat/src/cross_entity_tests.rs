// Cross-entity relationship tests: multi-entity graphs, ref integrity,
// cascading operations, join queries across entities.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod cross_entity_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_ce_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :ce/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :ce/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"p\" :db/ident :ce/parent :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"c\" :db/ident :ce/children :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"t\" :db/ident :ce/type :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :ce/dept :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"m\" :db/ident :ce/manager :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"f\" :db/ident :ce/friends :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"s\" :db/ident :ce/score :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("ce schema");
    }

    // ========================================================================
    // Parent-child relationships
    // ========================================================================

    #[pg_test]
    fn test_ce_parent_child_basic() {
        setup(); setup_ce_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"parent\" :ce/name \"Parent\"]
                [:db/add \"child\" :ce/name \"Child\"]
                [:db/add \"child\" :ce/parent \"parent\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let parent = j["tempids"]["parent"].as_i64().expect("parent");
        let child = j["tempids"]["child"].as_i64().expect("child");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?p . :where [{} :ce/parent ?p]]'::TEXT, '{{}}'::jsonb)::TEXT", child
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("p"), parent);
    }

    #[pg_test]
    fn test_ce_one_parent_many_children() {
        setup(); setup_ce_schema();
        let mut ops = vec!["[:db/add \"root\" :ce/name \"Root\"]".to_string()];
        for i in 0..10 {
            ops.push(format!("[:db/add \"c{}\" :ce/name \"Child-{}\"]", i, i));
            ops.push(format!("[:db/add \"root\" :ce/children \"c{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?c ...] :where [?r :ce/name \"Root\"] [?r :ce/children ?c]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_ce_three_level_hierarchy() {
        setup(); setup_ce_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"gp\" :ce/name \"Grandparent\"]
                [:db/add \"p\" :ce/name \"Parent\"]
                [:db/add \"c\" :ce/name \"Child\"]
                [:db/add \"p\" :ce/parent \"gp\"]
                [:db/add \"c\" :ce/parent \"p\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let gp = j["tempids"]["gp"].as_i64().expect("gp");
        let c = j["tempids"]["c"].as_i64().expect("c");

        // Navigate child -> parent -> grandparent
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?gp . :where [{} :ce/parent ?p] [?p :ce/parent ?gp]]'::TEXT, '{{}}'::jsonb)::TEXT", c
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("gp"), gp);
    }

    // ========================================================================
    // Manager hierarchy
    // ========================================================================

    #[pg_test]
    fn test_ce_manager_hierarchy() {
        setup(); setup_ce_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"ceo\" :ce/name \"CEO\" :ce/type :executive :ce/dept \"C-Suite\"}
                {:db/id \"vp\" :ce/name \"VP\" :ce/type :executive :ce/dept \"Engineering\" :ce/manager \"ceo\"}
                {:db/id \"dir\" :ce/name \"Director\" :ce/type :manager :ce/dept \"Engineering\" :ce/manager \"vp\"}
                {:db/id \"e1\" :ce/name \"Alice\" :ce/type :ic :ce/dept \"Engineering\" :ce/manager \"dir\" :ce/val 100}
                {:db/id \"e2\" :ce/name \"Bob\" :ce/type :ic :ce/dept \"Engineering\" :ce/manager \"dir\" :ce/val 110}
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let ceo = j["tempids"]["ceo"].as_i64().expect("ceo");

        // Find all people who report (directly) to the Director
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?d :ce/name \"Director\"] [?e :ce/manager ?d] [?e :ce/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // Alice, Bob

        // Navigate from Alice up to CEO (3 levels)
        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?top . :where [?a :ce/name \"Alice\"] [?a :ce/manager ?m1] [?m1 :ce/manager ?m2] [?m2 :ce/manager ?top]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_i64().expect("top"), ceo);
    }

    // ========================================================================
    // Friend graph (many-to-many)
    // ========================================================================

    #[pg_test]
    fn test_ce_bidirectional_friends() {
        setup(); setup_ce_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"a\" :ce/name \"Alice\"]
                [:db/add \"b\" :ce/name \"Bob\"]
                [:db/add \"a\" :ce/friends \"b\"]
                [:db/add \"b\" :ce/friends \"a\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let a = j["tempids"]["a"].as_i64().expect("a");
        let b = j["tempids"]["b"].as_i64().expect("b");

        // Alice's friends include Bob
        let q1 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?f ...] :where [{} :ce/friends ?f]]'::TEXT, '{{}}'::jsonb)::TEXT", a
        )).expect("q").expect("NULL");
        let v1: serde_json::Value = serde_json::from_str(&q1).expect("parse");
        assert!(v1["result"].as_array().expect("arr").iter().any(|v| v.as_i64() == Some(b)));

        // Bob's friends include Alice
        let q2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?f ...] :where [{} :ce/friends ?f]]'::TEXT, '{{}}'::jsonb)::TEXT", b
        )).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert!(v2["result"].as_array().expect("arr").iter().any(|v| v.as_i64() == Some(a)));
    }

    #[pg_test]
    fn test_ce_friend_network_5_nodes() {
        setup(); setup_ce_schema();
        // Star topology: center connected to 4 others
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"center\" :ce/name \"Center\"]
                [:db/add \"n1\" :ce/name \"N1\"]
                [:db/add \"n2\" :ce/name \"N2\"]
                [:db/add \"n3\" :ce/name \"N3\"]
                [:db/add \"n4\" :ce/name \"N4\"]
                [:db/add \"center\" :ce/friends \"n1\"]
                [:db/add \"center\" :ce/friends \"n2\"]
                [:db/add \"center\" :ce/friends \"n3\"]
                [:db/add \"center\" :ce/friends \"n4\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let center = j["tempids"]["center"].as_i64().expect("center");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?f ...] :where [{} :ce/friends ?f]]'::TEXT, '{{}}'::jsonb)::TEXT", center
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    // ========================================================================
    // Cross-entity queries
    // ========================================================================

    #[pg_test]
    fn test_ce_join_by_department() {
        setup(); setup_ce_schema();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e1\" :ce/name \"Alice\" :ce/dept \"Engineering\" :ce/val 100}
                {:db/id \"e2\" :ce/name \"Bob\" :ce/dept \"Engineering\" :ce/val 110}
                {:db/id \"e3\" :ce/name \"Carol\" :ce/dept \"Design\" :ce/val 95}
                {:db/id \"e4\" :ce/name \"Dave\" :ce/dept \"Engineering\" :ce/val 120}
            ]'::TEXT)",
        ).expect("tx");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :ce/name ?n] [?e :ce/dept \"Engineering\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_ce_join_by_type() {
        setup(); setup_ce_schema();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e1\" :ce/name \"A\" :ce/type :engineer :ce/val 100}
                {:db/id \"e2\" :ce/name \"B\" :ce/type :designer :ce/val 90}
                {:db/id \"e3\" :ce/name \"C\" :ce/type :engineer :ce/val 110}
                {:db/id \"e4\" :ce/name \"D\" :ce/type :pm :ce/val 120}
                {:db/id \"e5\" :ce/name \"E\" :ce/type :engineer :ce/val 105}
            ]'::TEXT)",
        ).expect("tx");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :ce/name ?n] [?e :ce/type :engineer]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    // ========================================================================
    // Retract entity with refs
    // ========================================================================

    #[pg_test]
    fn test_ce_retract_parent_leaves_children() {
        setup(); setup_ce_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"p\" :ce/name \"Parent\"]
                [:db/add \"c1\" :ce/name \"Child1\"]
                [:db/add \"c2\" :ce/name \"Child2\"]
                [:db/add \"c1\" :ce/parent \"p\"]
                [:db/add \"c2\" :ce/parent \"p\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let p = j["tempids"]["p"].as_i64().expect("p");
        let c1 = j["tempids"]["c1"].as_i64().expect("c1");

        // Retract parent
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", p
        )).expect("retract parent");

        // Children should still exist
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :ce/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", c1
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("n"), "Child1");
    }

    // ========================================================================
    // Large entity graph
    // ========================================================================

    #[pg_test]
    fn test_ce_linear_chain_20_nodes() {
        setup(); setup_ce_schema();
        // Create chain: n0 -> n1 -> n2 -> ... -> n19
        let mut ops = Vec::new();
        for i in 0..20 {
            ops.push(format!("[:db/add \"n{}\" :ce/name \"Node-{}\"]", i, i));
            if i > 0 {
                ops.push(format!("[:db/add \"n{}\" :ce/parent \"n{}\"]", i, i - 1));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");

        // Count all nodes
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :ce/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_ce_star_graph_hub_and_50_spokes() {
        setup(); setup_ce_schema();
        let mut ops = vec!["[:db/add \"hub\" :ce/name \"Hub\"]".to_string()];
        for i in 0..50 {
            ops.push(format!("[:db/add \"s{}\" :ce/name \"Spoke-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :ce/children \"s{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?c ...] :where [?h :ce/name \"Hub\"] [?h :ce/children ?c]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 50);
    }

    // ========================================================================
    // Self-referencing entities
    // ========================================================================

    #[pg_test]
    fn test_ce_self_reference() {
        setup(); setup_ce_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :ce/name \"SelfRef\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :ce/parent {}]]'::TEXT)", eid, eid
        )).expect("self-ref");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?p . :where [{} :ce/parent ?p]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("p"), eid);
    }

    #[pg_test]
    fn test_ce_self_friend() {
        setup(); setup_ce_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :ce/name \"Narcissist\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :ce/friends {}]]'::TEXT)", eid, eid
        )).expect("self-friend");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?f ...] :where [{} :ce/friends ?f]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").iter().any(|v| v.as_i64() == Some(eid)));
    }

    // ========================================================================
    // Multi-attribute entity creation
    // ========================================================================

    #[pg_test]
    fn test_ce_full_entity_with_all_attrs() {
        setup(); setup_ce_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"target\" :ce/name \"Target\" :ce/val 99}
                {:db/id \"e\" :ce/name \"Full\" :ce/val 42 :ce/type :engineer :ce/dept \"Eng\" :ce/score 88.5 :ce/parent \"target\"}
                [:db/add \"e\" :ce/friends \"target\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(DISTINCT a) FROM mentat.datoms WHERE e = {} AND added = true", eid
        )).expect("q").expect("NULL");
        assert!(count >= 6, "Full entity should have at least 6 attributes, got {}", count);
    }
}
