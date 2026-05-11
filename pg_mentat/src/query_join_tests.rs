// Query join tests: systematic coverage of entity joins, multi-hop
// navigation, cross-entity queries, and complex join patterns.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod query_join_tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_qj_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :qj/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :qj/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :qj/dept :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"r\" :db/ident :qj/mgr :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :qj/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"f\" :db/ident :qj/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"p\" :db/ident :qj/proj :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"ps\" :db/ident :qj/projs :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"s\" :db/ident :qj/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"pn\" :db/ident :qj/pname :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("qj schema");
    }

    fn setup_org_data() {
        Spi::run("SELECT mentat_transact('[
            {:db/id \"ceo\" :qj/name \"CEO\" :qj/dept \"exec\" :qj/val 200 :qj/flag true :qj/status :active}
            {:db/id \"vpe\" :qj/name \"VP-Eng\" :qj/dept \"eng\" :qj/val 180 :qj/flag true :qj/status :active :qj/mgr \"ceo\"}
            {:db/id \"vps\" :qj/name \"VP-Sales\" :qj/dept \"sales\" :qj/val 170 :qj/flag true :qj/status :active :qj/mgr \"ceo\"}
            {:db/id \"m1\" :qj/name \"Mgr-FE\" :qj/dept \"eng\" :qj/val 150 :qj/flag true :qj/status :active :qj/mgr \"vpe\"}
            {:db/id \"m2\" :qj/name \"Mgr-BE\" :qj/dept \"eng\" :qj/val 150 :qj/flag false :qj/status :active :qj/mgr \"vpe\"}
            {:db/id \"m3\" :qj/name \"Mgr-West\" :qj/dept \"sales\" :qj/val 140 :qj/flag true :qj/status :inactive :qj/mgr \"vps\"}
            {:db/id \"e1\" :qj/name \"Alice\" :qj/dept \"eng\" :qj/val 120 :qj/flag true :qj/status :active :qj/mgr \"m1\"}
            {:db/id \"e2\" :qj/name \"Bob\" :qj/dept \"eng\" :qj/val 110 :qj/flag false :qj/status :active :qj/mgr \"m1\"}
            {:db/id \"e3\" :qj/name \"Carol\" :qj/dept \"eng\" :qj/val 115 :qj/flag true :qj/status :pending :qj/mgr \"m2\"}
            {:db/id \"e4\" :qj/name \"Dave\" :qj/dept \"sales\" :qj/val 100 :qj/flag false :qj/status :active :qj/mgr \"m3\"}
            {:db/id \"e5\" :qj/name \"Eve\" :qj/dept \"sales\" :qj/val 105 :qj/flag true :qj/status :inactive :qj/mgr \"m3\"}
            {:db/id \"e6\" :qj/name \"Frank\" :qj/dept \"eng\" :qj/val 130 :qj/flag true :qj/status :active :qj/mgr \"m2\"}
        ]'::TEXT)").expect("org data");
    }

    // ========================================================================
    // Single-hop ref joins (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_qj_find_manager_name() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?mn . :where [?e :qj/name \"Alice\"] [?e :qj/mgr ?m] [?m :qj/name ?mn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Mgr-FE");
    }

    #[pg_test]
    fn test_qj_find_all_reports() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?m :qj/name \"Mgr-FE\"] [?e :qj/mgr ?m] [?e :qj/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // Alice, Bob
    }

    #[pg_test]
    fn test_qj_find_ceo_reports() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?c :qj/name \"CEO\"] [?e :qj/mgr ?c] [?e :qj/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // VP-Eng, VP-Sales
    }

    #[pg_test]
    fn test_qj_ref_with_predicate() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?m :qj/name \"Mgr-BE\"] [?e :qj/mgr ?m] [?e :qj/name ?n] [?e :qj/val ?v] [(> ?v 120)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1); // Frank (130)
    }

    #[pg_test]
    fn test_qj_ref_with_flag_filter() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?m :qj/name \"Mgr-FE\"] [?e :qj/mgr ?m] [?e :qj/name ?n] [?e :qj/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1); // Alice
    }

    #[pg_test]
    fn test_qj_ref_with_status_filter() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?m :qj/name \"VP-Eng\"] [?e :qj/mgr ?m] [?e :qj/name ?n] [?e :qj/status :active]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // Mgr-FE, Mgr-BE
    }

    #[pg_test]
    fn test_qj_all_people_with_managers() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qj/mgr _] [?e :qj/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 11); // all except CEO
    }

    #[pg_test]
    fn test_qj_employee_manager_pairs() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?en ?mn :where [?e :qj/name ?en] [?e :qj/mgr ?m] [?m :qj/name ?mn] [?e :qj/dept \"eng\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 3);
    }

    #[pg_test]
    fn test_qj_manager_dept_cross() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?en ?md :where [?e :qj/name ?en] [?e :qj/mgr ?m] [?m :qj/dept ?md] [?e :qj/dept \"eng\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    #[pg_test]
    fn test_qj_scalar_manager_of() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?mn . :where [?e :qj/name \"Carol\"] [?e :qj/mgr ?m] [?m :qj/name ?mn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Mgr-BE");
    }

    // ========================================================================
    // Two-hop ref joins (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_qj_grandmanager() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?gm . :where [?e :qj/name \"Alice\"] [?e :qj/mgr ?m] [?m :qj/mgr ?g] [?g :qj/name ?gm]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "VP-Eng");
    }

    #[pg_test]
    fn test_qj_three_hop_to_ceo() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?top . :where [?e :qj/name \"Alice\"] [?e :qj/mgr ?m] [?m :qj/mgr ?g] [?g :qj/mgr ?t] [?t :qj/name ?top]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "CEO");
    }

    #[pg_test]
    fn test_qj_all_grandreports() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?vp :qj/name \"VP-Eng\"] [?m :qj/mgr ?vp] [?e :qj/mgr ?m] [?e :qj/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Alice, Bob (under Mgr-FE), Carol, Frank (under Mgr-BE) = 4
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_qj_two_hop_with_predicate() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?vp :qj/name \"VP-Eng\"] [?m :qj/mgr ?vp] [?e :qj/mgr ?m] [?e :qj/name ?n] [?e :qj/val ?v] [(> ?v 115)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Alice (120), Frank (130) = 2
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_qj_two_hop_with_flag() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?vp :qj/name \"VP-Eng\"] [?m :qj/mgr ?vp] [?e :qj/mgr ?m] [?e :qj/name ?n] [?e :qj/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Alice (true), Carol (true), Frank (true) = 3
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_qj_two_hop_relation() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?en ?mn :where [?vp :qj/name \"VP-Eng\"] [?m :qj/mgr ?vp] [?m :qj/name ?mn] [?e :qj/mgr ?m] [?e :qj/name ?en]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let rows = v["result"].as_array().expect("arr");
        assert_eq!(rows.len(), 4);
        for row in rows {
            assert_eq!(row.as_array().expect("r").len(), 2);
        }
    }

    #[pg_test]
    fn test_qj_ceo_all_grandreports() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?c :qj/name \"CEO\"] [?vp :qj/mgr ?c] [?m :qj/mgr ?vp] [?m :qj/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Mgr-FE, Mgr-BE, Mgr-West = 3
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_qj_three_hop_count() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?c :qj/name \"CEO\"] [?vp :qj/mgr ?c] [?m :qj/mgr ?vp] [?e :qj/mgr ?m] [?e :qj/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // All individual contributors: Alice, Bob, Carol, Dave, Eve, Frank = 6
        assert_eq!(v["result"].as_array().expect("arr").len(), 6);
    }

    // ========================================================================
    // Same-entity multi-attribute joins (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_qj_two_attr_same_entity() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v :where [?e :qj/name ?n] [?e :qj/val ?v] [?e :qj/dept \"eng\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 3);
    }

    #[pg_test]
    fn test_qj_three_attr_same_entity() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v ?d :where [?e :qj/name ?n] [?e :qj/val ?v] [?e :qj/dept ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 12);
    }

    #[pg_test]
    fn test_qj_four_attr_same_entity() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?v ?d ?s :where [?e :qj/name ?n] [?e :qj/val ?v] [?e :qj/dept ?d] [?e :qj/status ?s] [?e :qj/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    #[pg_test]
    fn test_qj_name_and_dept() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qj/name ?n] [?e :qj/dept \"sales\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 4); // VP-Sales, Mgr-West, Dave, Eve
    }

    #[pg_test]
    fn test_qj_active_eng() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qj/name ?n] [?e :qj/dept \"eng\"] [?e :qj/status :active]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // VP-Eng, Mgr-FE, Mgr-BE, Alice, Bob, Frank = 6
        assert_eq!(v["result"].as_array().expect("arr").len(), 6);
    }

    #[pg_test]
    fn test_qj_flagged_active_eng() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qj/name ?n] [?e :qj/dept \"eng\"] [?e :qj/status :active] [?e :qj/flag true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // VP-Eng (true, active), Mgr-FE (true, active), Alice (true, active), Frank (true, active) = 4
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_qj_val_range_with_dept() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qj/name ?n] [?e :qj/dept \"eng\"] [?e :qj/val ?v] [(> ?v 115)] [(< ?v 160)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Alice (120), Frank (130), Mgr-FE (150), Mgr-BE (150) = 4
        assert_eq!(v["result"].as_array().expect("arr").len(), 4);
    }

    #[pg_test]
    fn test_qj_all_filters_combined() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qj/name ?n] [?e :qj/dept \"eng\"] [?e :qj/flag true] [?e :qj/status :active] [?e :qj/val ?v] [(> ?v 100)] [(< ?v 160)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // VP-Eng excluded (val=180 > 160), Mgr-FE (150, true, active, eng), Alice (120, true, active, eng), Frank (130, true, active, eng) = 3
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    // ========================================================================
    // Cross-entity same-attribute joins (6 tests)
    // ========================================================================

    #[pg_test]
    fn test_qj_same_dept_different_entities() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n1 ?n2 :where [?e1 :qj/name ?n1] [?e2 :qj/name ?n2] [?e1 :qj/dept ?d] [?e2 :qj/dept ?d] [?e1 :qj/name \"Alice\"] [(!= ?n1 ?n2)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 0);
    }

    #[pg_test]
    fn test_qj_same_manager() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?alice :qj/name \"Alice\"] [?alice :qj/mgr ?m] [?e :qj/mgr ?m] [?e :qj/name ?n] [(!= ?n \"Alice\")]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1); // Bob
    }

    #[pg_test]
    fn test_qj_same_status() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :qj/status :inactive] [?e :qj/status :inactive] [?e :qj/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // Mgr-West, Eve
    }

    #[pg_test]
    fn test_qj_higher_val_than_specific() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?alice :qj/name \"Alice\"] [?alice :qj/val ?av] [?e :qj/name ?n] [?e :qj/val ?v] [(> ?v ?av)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Everyone with val > 120: CEO(200), VP-Eng(180), VP-Sales(170), Mgr-FE(150), Mgr-BE(150), Mgr-West(140), Frank(130) = 7
        assert_eq!(v["result"].as_array().expect("arr").len(), 7);
    }

    #[pg_test]
    fn test_qj_peers_same_level() {
        setup(); setup_qj_schema(); setup_org_data();
        // Find people who share the same manager as Bob
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?bob :qj/name \"Bob\"] [?bob :qj/mgr ?m] [?e :qj/mgr ?m] [?e :qj/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2); // Alice, Bob
    }

    #[pg_test]
    fn test_qj_cross_dept_same_val_range() {
        setup(); setup_qj_schema(); setup_org_data();
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?d :where [?e :qj/name ?n] [?e :qj/dept ?d] [?e :qj/val ?v] [(>= ?v 100)] [(<= ?v 120)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Alice(120,eng), Bob(110,eng), Carol(115,eng), Dave(100,sales), Eve(105,sales) = 5
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    // ========================================================================
    // Dynamic data then query (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_qj_add_entity_then_join() {
        setup(); setup_qj_schema(); setup_org_data();
        Spi::run("SELECT mentat_transact('[{:db/id \"new\" :qj/name \"NewHire\" :qj/dept \"eng\" :qj/val 90 :qj/flag true :qj/status :active}]'::TEXT)").expect("add");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qj/dept \"eng\"] [?e :qj/name ?n] [?e :qj/val ?v] [(< ?v 100)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1); // NewHire
    }

    #[pg_test]
    fn test_qj_update_then_join() {
        setup(); setup_qj_schema(); setup_org_data();
        // Update Alice's val
        let q_eid = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :qj/name \"Alice\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q_eid).expect("parse");
        let eid = j["result"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :qj/val 999]]'::TEXT)", eid)).expect("update");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :qj/name ?n] [?e :qj/val ?v] [(> ?v 500)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Alice");
    }

    #[pg_test]
    fn test_qj_retract_then_join() {
        setup(); setup_qj_schema(); setup_org_data();
        let q_eid = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :qj/name \"Dave\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q_eid).expect("parse");
        let eid = j["result"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qj/dept \"sales\"] [?e :qj/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3); // VP-Sales, Mgr-West, Eve (Dave removed)
    }

    #[pg_test]
    fn test_qj_add_50_then_join() {
        setup(); setup_qj_schema(); setup_org_data();
        let mut ops = vec![];
        for i in 0..50 {
            ops.push(format!(
                "{{:db/id \"n{}\" :qj/name \"new-{}\" :qj/dept \"eng\" :qj/val {} :qj/flag {}}}",
                i, i, i + 200, if i % 2 == 0 { "true" } else { "false" }
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :qj/dept \"eng\"] [?e :qj/name ?n] [?e :qj/val ?v] [(> ?v 200)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() >= 49);
    }

    #[pg_test]
    fn test_qj_link_then_navigate() {
        setup(); setup_qj_schema();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"p1\" :qj/pname \"ProjectA\"}
            {:db/id \"p2\" :qj/pname \"ProjectB\"}
            {:db/id \"e1\" :qj/name \"Alice\" :qj/proj \"p1\"}
            {:db/id \"e2\" :qj/name \"Bob\" :qj/proj \"p1\"}
            {:db/id \"e3\" :qj/name \"Carol\" :qj/proj \"p2\"}
        ]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?p :qj/pname \"ProjectA\"] [?e :qj/proj ?p] [?e :qj/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_qj_multi_ref_navigate() {
        setup(); setup_qj_schema();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"p1\" :qj/pname \"ProjectA\"}
            {:db/id \"p2\" :qj/pname \"ProjectB\"}
            {:db/id \"p3\" :qj/pname \"ProjectC\"}
            {:db/id \"e\" :qj/name \"Alice\"}
            [:db/add \"e\" :qj/projs \"p1\"]
            [:db/add \"e\" :qj/projs \"p2\"]
            [:db/add \"e\" :qj/projs \"p3\"]
        ]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?pn ...] :where [?e :qj/name \"Alice\"] [?e :qj/projs ?p] [?p :qj/pname ?pn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_qj_bidirectional_ref() {
        setup(); setup_qj_schema(); setup_org_data();
        // Forward: who is Alice's manager?
        let q1 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?mn . :where [?a :qj/name \"Alice\"] [?a :qj/mgr ?m] [?m :qj/name ?mn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v1: serde_json::Value = serde_json::from_str(&q1).expect("parse");
        assert_eq!(v1["result"].as_str().expect("s"), "Mgr-FE");
        // Reverse: who reports to Mgr-FE?
        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?rn ...] :where [?m :qj/name \"Mgr-FE\"] [?r :qj/mgr ?m] [?r :qj/name ?rn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_qj_chain_query_results() {
        setup(); setup_qj_schema(); setup_org_data();
        // First query: find all managers
        let q1 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?mn ...] :where [_ :qj/mgr ?m] [?m :qj/name ?mn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v1: serde_json::Value = serde_json::from_str(&q1).expect("parse");
        let mgr_count = v1["result"].as_array().expect("arr").len();
        // Second query: count their reports
        let q2 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?en ...] :where [?e :qj/mgr _] [?e :qj/name ?en]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        let report_count = v2["result"].as_array().expect("arr").len();
        assert!(mgr_count > 0);
        assert!(report_count > mgr_count);
    }
}
