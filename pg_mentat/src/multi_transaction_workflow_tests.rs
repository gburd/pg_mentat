// Multi-transaction workflow tests: realistic multi-step scenarios
// that exercise create/read/update/delete across multiple transactions.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod multi_transaction_workflow_tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_wf_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :wf/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"e\" :db/ident :wf/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"v\" :db/ident :wf/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :wf/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :wf/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :wf/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("wf schema");
    }

    // ========================================================================
    // CRUD lifecycle
    // ========================================================================

    #[pg_test]
    fn test_wf_create_read_update_delete() {
        setup(); setup_wf_schema();

        // CREATE
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :wf/name \"CRUD\" :wf/val 0 :wf/status :pending}]'::TEXT)",
        ).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // READ
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n ?v ?s :where [{e} :wf/name ?n] [{e} :wf/val ?v] [{e} :wf/status ?s]]'::TEXT, '{{}}'::jsonb)::TEXT", e = eid
        )).expect("read").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);

        // UPDATE
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :wf/val 100] [:db/add {} :wf/status :active]]'::TEXT)", eid, eid
        )).expect("update");

        let q2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :wf/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("read").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_i64().expect("v"), 100);

        // DELETE
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", eid
        )).expect("delete");

        let q3 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :wf/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("read").expect("NULL");
        let v3: serde_json::Value = serde_json::from_str(&q3).expect("parse");
        assert!(v3["result"].is_null());
    }

    // ========================================================================
    // Status machine workflow
    // ========================================================================

    #[pg_test]
    fn test_wf_status_machine() {
        setup(); setup_wf_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"order\" :wf/name \"Order-001\" :wf/status :created :wf/val 0}]'::TEXT)",
        ).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["order"].as_i64().expect("eid");

        // created -> pending
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :wf/status :pending]]'::TEXT)", eid)).expect("transition");
        // pending -> processing
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :wf/status :processing]]'::TEXT)", eid)).expect("transition");
        // processing -> shipped
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :wf/status :shipped]]'::TEXT)", eid)).expect("transition");
        // shipped -> delivered
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :wf/status :delivered]]'::TEXT)", eid)).expect("transition");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?s . :where [{} :wf/status ?s]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("read").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let status = v["result"].as_str().expect("s");
        assert!(status.contains("delivered"), "Final status should be :delivered, got {}", status);
    }

    // ========================================================================
    // Tagging workflow
    // ========================================================================

    #[pg_test]
    fn test_wf_progressive_tagging() {
        setup(); setup_wf_schema();

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"doc\" :wf/name \"Document\"}]'::TEXT)",
        ).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["doc"].as_i64().expect("eid");

        // Add tags progressively
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :wf/tags \"draft\"]]'::TEXT)", eid)).expect("tag");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :wf/tags \"reviewed\"]]'::TEXT)", eid)).expect("tag");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :wf/tags \"approved\"]]'::TEXT)", eid)).expect("tag");
        // Remove draft tag
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :wf/tags \"draft\"]]'::TEXT)", eid)).expect("untag");
        // Add published
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :wf/tags \"published\"]]'::TEXT)", eid)).expect("tag");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?t ...] :where [{} :wf/tags ?t]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("read").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let tags: Vec<&str> = v["result"].as_array().expect("arr").iter().map(|t| t.as_str().expect("s")).collect();
        assert_eq!(tags.len(), 3);
        assert!(!tags.contains(&"draft"));
        assert!(tags.contains(&"reviewed"));
        assert!(tags.contains(&"approved"));
        assert!(tags.contains(&"published"));
    }

    // ========================================================================
    // Multi-entity creation and linking
    // ========================================================================

    #[pg_test]
    fn test_wf_create_then_link() {
        setup(); setup_wf_schema();

        // TX 1: Create project
        let r1 = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"proj\" :wf/name \"Project X\" :wf/status :active}]'::TEXT)",
        ).expect("create").expect("NULL");
        let j1: serde_json::Value = serde_json::from_str(&r1).expect("parse");
        let proj = j1["tempids"]["proj"].as_i64().expect("proj");

        // TX 2: Create tasks linked to project
        let r2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[
                {{:db/id \"t1\" :wf/name \"Task 1\" :wf/status :pending :wf/ref {}}}
                {{:db/id \"t2\" :wf/name \"Task 2\" :wf/status :pending :wf/ref {}}}
                {{:db/id \"t3\" :wf/name \"Task 3\" :wf/status :pending :wf/ref {}}}
            ]'::TEXT)", proj, proj, proj
        )).expect("create tasks").expect("NULL");
        let j2: serde_json::Value = serde_json::from_str(&r2).expect("parse");
        assert_eq!(j2["tempids"].as_object().expect("t").len(), 3);

        // TX 3: Update task statuses
        let t1 = j2["tempids"]["t1"].as_i64().expect("t1");
        let t2 = j2["tempids"]["t2"].as_i64().expect("t2");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :wf/status :done] [:db/add {} :wf/status :in-progress]]'::TEXT)", t1, t2
        )).expect("update");

        // Query: tasks for project that are pending
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?n ...] :where [?t :wf/ref {}] [?t :wf/name ?n] [?t :wf/status :pending]]'::TEXT, '{{}}'::jsonb)::TEXT", proj
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1); // Only Task 3 is still pending
    }

    // ========================================================================
    // Upsert-based sync workflow
    // ========================================================================

    #[pg_test]
    fn test_wf_upsert_sync() {
        setup(); setup_wf_schema();

        // Initial sync
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"u1\" :wf/email \"alice@test.com\" :wf/name \"Alice\" :wf/val 100}
                {:db/id \"u2\" :wf/email \"bob@test.com\" :wf/name \"Bob\" :wf/val 200}
            ]'::TEXT)",
        ).expect("sync 1");

        // Second sync: update Alice, add Carol
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"s1\" :wf/email \"alice@test.com\" :wf/val 150}
                {:db/id \"s2\" :wf/email \"carol@test.com\" :wf/name \"Carol\" :wf/val 300}
            ]'::TEXT)",
        ).expect("sync 2");

        // Alice should be updated
        let qa = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :wf/email \"alice@test.com\"] [?e :wf/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let ja: serde_json::Value = serde_json::from_str(&qa).expect("parse");
        assert_eq!(ja["result"].as_i64().expect("v"), 150);

        // Carol should exist
        let qc = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :wf/email \"carol@test.com\"] [?e :wf/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let jc: serde_json::Value = serde_json::from_str(&qc).expect("parse");
        assert_eq!(jc["result"].as_str().expect("n"), "Carol");

        // Total: 3 entities with emails
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':wf/email') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 3);
    }

    // ========================================================================
    // Counter pattern
    // ========================================================================

    #[pg_test]
    fn test_wf_counter_increment_20x() {
        setup(); setup_wf_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"c\" :wf/name \"Counter\" :wf/val 0}]'::TEXT)",
        ).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["c"].as_i64().expect("eid");

        for i in 1..=20 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :wf/val {}]]'::TEXT)", eid, i)).expect("increment");
        }

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :wf/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 20);
    }

    // ========================================================================
    // Batch workflow: create, query, update batch, query again
    // ========================================================================

    #[pg_test]
    fn test_wf_batch_crud_30_entities() {
        setup(); setup_wf_schema();

        // Create 30 entities
        let mut ops = Vec::new();
        for i in 0..30 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :wf/name \"Entity-{i}\" :wf/val 0 :wf/status :new}}",
                i = i
            ));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");

        // Query all
        let q1 = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :wf/status :new] [?e :wf/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v1: serde_json::Value = serde_json::from_str(&q1).expect("parse");
        assert_eq!(v1["result"].as_array().expect("arr").len(), 30);

        // Update first 15 to :active
        let mut updates = Vec::new();
        for i in 0..15 {
            let eid = j["tempids"][&format!("e{}", i)].as_i64().expect("eid");
            updates.push(format!("[:db/add {} :wf/status :active]", eid));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", updates.join("\n"))).expect("update");

        // Query: 15 new, 15 active
        let qnew = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :wf/status :new] [?e :wf/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let vnew: serde_json::Value = serde_json::from_str(&qnew).expect("parse");
        assert_eq!(vnew["result"].as_array().expect("arr").len(), 15);

        let qactive = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :wf/status :active] [?e :wf/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let vactive: serde_json::Value = serde_json::from_str(&qactive).expect("parse");
        assert_eq!(vactive["result"].as_array().expect("arr").len(), 15);
    }
}
