// Comprehensive upsert tests: exhaustive coverage of upsert behavior
// across value types, multi-attribute updates, and edge cases.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_cu_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :cu/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :cu/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :cu/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"b\" :db/ident :cu/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :cu/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :cu/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\" :db/ident :cu/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"u\" :db/ident :cu/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"e\" :db/ident :cu/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            ]'::TEXT)",
        ).expect("cu schema");
    }

    // ========================================================================
    // Basic upsert operations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_cu_first_upsert_creates() {
        setup();
        setup_cu_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U1\" :cu/name \"Alice\"}]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e"].as_i64().is_some());
    }

    #[pg_test]
    fn test_cu_second_upsert_updates() {
        setup();
        setup_cu_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U2\" :cu/name \"v1\"}]'::TEXT)")
            .expect("c1");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U2\" :cu/name \"v2\"}]'::TEXT)")
            .expect("c2");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :cu/uid \"U2\"] [?e :cu/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "v2");
    }

    #[pg_test]
    fn test_cu_upsert_preserves_unmentioned() {
        setup();
        setup_cu_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U3\" :cu/name \"Alice\" :cu/val 42}]'::TEXT)").expect("c");
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U3\" :cu/name \"updated\"}]'::TEXT)",
        )
        .expect("u");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :cu/uid \"U3\"] [?e :cu/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 42);
    }

    #[pg_test]
    fn test_cu_upsert_10x_same() {
        setup();
        setup_cu_schema();
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"e\" :cu/uid \"U4\" :cu/val {}}}]'::TEXT)",
                i
            ))
            .expect("upsert");
        }
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':cu/uid') AND v_text = 'U4' AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 1);
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :cu/uid \"U4\"] [?e :cu/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 9);
    }

    #[pg_test]
    fn test_cu_upsert_with_bool() {
        setup();
        setup_cu_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U5\" :cu/flag true}]'::TEXT)")
            .expect("c");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U5\" :cu/flag false}]'::TEXT)")
            .expect("u");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?f . :where [?e :cu/uid \"U5\"] [?e :cu/flag ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_cu_upsert_with_keyword() {
        setup();
        setup_cu_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U6\" :cu/kw :draft}]'::TEXT)")
            .expect("c");
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U6\" :cu/kw :published}]'::TEXT)",
        )
        .expect("u");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?k . :where [?e :cu/uid \"U6\"] [?e :cu/kw ?k]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("published"));
    }

    #[pg_test]
    fn test_cu_upsert_with_double() {
        setup();
        setup_cu_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U7\" :cu/dbl 1.0}]'::TEXT)")
            .expect("c");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"U7\" :cu/dbl 9.99}]'::TEXT)")
            .expect("u");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?d . :where [?e :cu/uid \"U7\"] [?e :cu/dbl ?d]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("d") - 9.99).abs() < 0.01);
    }

    // ========================================================================
    // Upsert with cardinality-many (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_cu_upsert_adds_to_many() {
        setup();
        setup_cu_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"UM1\" :cu/tags \"tag1\"}]'::TEXT)",
        )
        .expect("c");
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"UM1\" :cu/tags \"tag2\"}]'::TEXT)",
        )
        .expect("u");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :cu/uid \"UM1\"] [?e :cu/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_cu_upsert_many_idempotent() {
        setup();
        setup_cu_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"UM2\" :cu/tags \"dup\"}]'::TEXT)",
        )
        .expect("c");
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"UM2\" :cu/tags \"dup\"}]'::TEXT)",
        )
        .expect("u");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :cu/uid \"UM2\"] [?e :cu/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    // ========================================================================
    // Upsert with refs (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_cu_upsert_sets_ref() {
        setup();
        setup_cu_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"target\" :cu/name \"Target\"} {:db/id \"e\" :cu/uid \"UR1\" :cu/name \"Source\"}]'::TEXT)"
        ).expect("c").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let target = j["tempids"]["target"].as_i64().expect("t");
        Spi::run(&format!(
            "SELECT mentat_transact('[{{:db/id \"e\" :cu/uid \"UR1\" :cu/ref {}}}]'::TEXT)",
            target
        ))
        .expect("u");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?rn . :where [?e :cu/uid \"UR1\"] [?e :cu/ref ?r] [?r :cu/name ?rn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Target");
    }

    #[pg_test]
    fn test_cu_upsert_replaces_ref() {
        setup();
        setup_cu_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"t1\" :cu/name \"T1\"} {:db/id \"t2\" :cu/name \"T2\"} {:db/id \"e\" :cu/uid \"UR2\" :cu/ref \"t1\"}]'::TEXT)"
        ).expect("c").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let t2 = j["tempids"]["t2"].as_i64().expect("t2");
        Spi::run(&format!(
            "SELECT mentat_transact('[{{:db/id \"e\" :cu/uid \"UR2\" :cu/ref {}}}]'::TEXT)",
            t2
        ))
        .expect("u");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?rn . :where [?e :cu/uid \"UR2\"] [?e :cu/ref ?r] [?r :cu/name ?rn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "T2");
    }

    // ========================================================================
    // Multi-unique-attr scenarios (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_cu_two_unique_attrs_create() {
        setup();
        setup_cu_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"TU1\" :cu/email \"alice@test.com\" :cu/name \"Alice\"}]'::TEXT)").expect("c");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :cu/email \"alice@test.com\"] [?e :cu/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Alice");
    }

    #[pg_test]
    fn test_cu_upsert_by_uid() {
        setup();
        setup_cu_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"TU2\" :cu/email \"bob@test.com\" :cu/name \"Bob\"}]'::TEXT)").expect("c");
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"TU2\" :cu/name \"Bobby\"}]'::TEXT)",
        )
        .expect("u via uid");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :cu/email \"bob@test.com\"] [?e :cu/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Bobby");
    }

    #[pg_test]
    fn test_cu_upsert_by_email() {
        setup();
        setup_cu_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/uid \"TU3\" :cu/email \"carol@test.com\" :cu/name \"Carol\"}]'::TEXT)").expect("c");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :cu/email \"carol@test.com\" :cu/name \"Carolina\"}]'::TEXT)").expect("u via email");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :cu/uid \"TU3\"] [?e :cu/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Carolina");
    }

    // ========================================================================
    // Batch upsert (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_cu_batch_upsert_10_new() {
        setup();
        setup_cu_schema();
        let mut ops = Vec::new();
        for i in 0..10 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :cu/uid \"BU-{i}\" :cu/name \"ent-{i}\" :cu/val {i}}}",
                i = i
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch create");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':cu/uid') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 10);
    }

    #[pg_test]
    fn test_cu_batch_upsert_10_update() {
        setup();
        setup_cu_schema();
        // Create
        let mut ops = Vec::new();
        for i in 0..10 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :cu/uid \"BU2-{i}\" :cu/val 0}}",
                i = i
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("create");
        // Update
        let mut updates = Vec::new();
        for i in 0..10 {
            updates.push(format!(
                "{{:db/id \"e{i}\" :cu/uid \"BU2-{i}\" :cu/val {v}}}",
                i = i,
                v = (i + 1) * 100
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            updates.join("\n")
        ))
        .expect("batch update");
        // Verify
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :cu/uid \"BU2-5\"] [?e :cu/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 600);
    }

    #[pg_test]
    fn test_cu_sync_pattern() {
        setup();
        setup_cu_schema();
        // Simulate external sync: create 5 entities
        Spi::run(
            "SELECT mentat_transact('[
            {:db/id \"e1\" :cu/uid \"sync-1\" :cu/name \"Alice\" :cu/val 100}
            {:db/id \"e2\" :cu/uid \"sync-2\" :cu/name \"Bob\" :cu/val 200}
            {:db/id \"e3\" :cu/uid \"sync-3\" :cu/name \"Carol\" :cu/val 300}
            {:db/id \"e4\" :cu/uid \"sync-4\" :cu/name \"Dave\" :cu/val 400}
            {:db/id \"e5\" :cu/uid \"sync-5\" :cu/name \"Eve\" :cu/val 500}
        ]'::TEXT)",
        )
        .expect("sync 1");
        // Second sync: update 3, add 1
        Spi::run(
            "SELECT mentat_transact('[
            {:db/id \"s1\" :cu/uid \"sync-1\" :cu/val 150}
            {:db/id \"s3\" :cu/uid \"sync-3\" :cu/val 350}
            {:db/id \"s5\" :cu/uid \"sync-5\" :cu/val 550}
            {:db/id \"s6\" :cu/uid \"sync-6\" :cu/name \"Frank\" :cu/val 600}
        ]'::TEXT)",
        )
        .expect("sync 2");
        // Verify counts
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':cu/uid') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 6);
        // Verify update
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :cu/uid \"sync-1\"] [?e :cu/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 150);
    }
}
