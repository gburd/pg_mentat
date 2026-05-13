// Exhaustive upsert tests: unique/identity resolution, merge behavior,
// conflict detection, and edge cases.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._test_raises_error(stmt TEXT) RETURNS BOOLEAN
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN false;
             EXCEPTION WHEN OTHERS THEN
                 RETURN true;
             END;
             $$"
        ).expect("create helper");
    }

    fn raises_error(sql: &str) -> bool {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<bool>(&format!(
            "SELECT mentat._test_raises_error('{}')", escaped
        )).expect("raises_error call").unwrap_or(false)
    }

    fn setup_upsert_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"ui\" :db/ident :up/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"uv\" :db/ident :up/code :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/value}
                {:db/id \"n\"  :db/ident :up/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\"  :db/ident :up/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"t\"  :db/ident :up/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"r\"  :db/ident :up/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"f\"  :db/ident :up/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("upsert schema");
    }

    // ========================================================================
    // Basic upsert via unique/identity
    // ========================================================================

    #[pg_test]
    fn test_up_basic_upsert() {
        setup(); setup_upsert_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :up/uid \"U1\" :up/name \"Alice\" :up/val 10}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :up/uid \"U1\" :up/val 20}]'::TEXT)").expect("upsert");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :up/uid \"U1\"] [?e :up/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_i64().expect("v"), 20);
    }

    #[pg_test]
    fn test_up_upsert_preserves_unmentioned_attrs() {
        setup(); setup_upsert_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :up/uid \"U2\" :up/name \"Bob\" :up/val 10 :up/flag true}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :up/uid \"U2\" :up/val 20}]'::TEXT)").expect("upsert");

        // Name and flag should still be there
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?f :where [?e :up/uid \"U2\"] [?e :up/name ?n] [?e :up/flag ?f]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let r = j["results"].as_array().expect("arr");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0][0].as_str().expect("n"), "Bob");
        assert_eq!(r[0][1].as_bool().expect("f"), true);
    }

    #[pg_test]
    fn test_up_upsert_entity_count_stable() {
        setup(); setup_upsert_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :up/uid \"U3\" :up/name \"Carol\"}]'::TEXT)").expect("create");

        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"u\" :up/uid \"U3\" :up/val {}}}]'::TEXT)", i
            )).expect("upsert");
        }

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':up/uid')
             AND v_text = 'U3' AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 1, "10 upserts should not create additional entities");
    }

    // ========================================================================
    // Multiple upserts in same tx
    // ========================================================================

    #[pg_test]
    fn test_up_two_upserts_same_tx() {
        setup(); setup_upsert_schema();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"e1\" :up/uid \"MA\" :up/name \"Alice\"}
            {:db/id \"e2\" :up/uid \"MB\" :up/name \"Bob\"}
        ]'::TEXT)").expect("create");

        Spi::run("SELECT mentat_transact('[
            {:db/id \"u1\" :up/uid \"MA\" :up/val 100}
            {:db/id \"u2\" :up/uid \"MB\" :up/val 200}
        ]'::TEXT)").expect("upsert both");

        let qa = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :up/uid \"MA\"] [?e :up/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let ja: serde_json::Value = serde_json::from_str(&qa).expect("parse");
        assert_eq!(ja["result"].as_i64().expect("v"), 100);

        let qb = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :up/uid \"MB\"] [?e :up/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let jb: serde_json::Value = serde_json::from_str(&qb).expect("parse");
        assert_eq!(jb["result"].as_i64().expect("v"), 200);
    }

    // ========================================================================
    // Upsert with cardinality-many
    // ========================================================================

    #[pg_test]
    fn test_up_upsert_adds_to_many() {
        setup(); setup_upsert_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :up/uid \"UT1\" :up/name \"Tagged\"}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :up/uid \"UT1\"} [:db/add \"e\" :up/tags \"tag1\"] [:db/add \"e\" :up/tags \"tag2\"]]'::TEXT)").expect("upsert tags");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :up/uid \"UT1\"] [?e :up/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Unique/value vs unique/identity
    // ========================================================================

    #[pg_test]
    fn test_up_unique_value_rejects_duplicate() {
        setup(); setup_upsert_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :up/code \"C1\"]]'::TEXT)").expect("first");
        assert!(
            raises_error("SELECT mentat_transact('[[:db/add \"e2\" :up/code \"C1\"]]'::TEXT)"),
            "unique/value should reject duplicate"
        );
    }

    #[pg_test]
    fn test_up_unique_identity_upserts() {
        setup(); setup_upsert_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e1\" :up/uid \"I1\" :up/name \"First\"}]'::TEXT)").expect("first");
        Spi::run("SELECT mentat_transact('[{:db/id \"e2\" :up/uid \"I1\" :up/name \"Second\"}]'::TEXT)").expect("upsert");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :up/uid \"I1\"] [?e :up/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_str().expect("n"), "Second");
    }

    // ========================================================================
    // Upsert with new entity creation
    // ========================================================================

    #[pg_test]
    fn test_up_new_uid_creates_entity() {
        setup(); setup_upsert_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :up/uid \"NEW1\" :up/name \"New Entity\"}]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e"].as_i64().is_some(), "New UID should create new entity");
    }

    // ========================================================================
    // Batch upsert
    // ========================================================================

    #[pg_test]
    fn test_up_batch_upsert_10() {
        setup(); setup_upsert_schema();

        // Create 10 entities
        let mut create_ops = Vec::new();
        for i in 0..10 {
            create_ops.push(format!(
                "{{:db/id \"e{i}\" :up/uid \"BATCH-{i}\" :up/name \"Entity-{i}\" :up/val {i}}}",
                i = i
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", create_ops.join("\n"))).expect("create");

        // Upsert all 10
        let mut upsert_ops = Vec::new();
        for i in 0..10 {
            upsert_ops.push(format!(
                "{{:db/id \"u{i}\" :up/uid \"BATCH-{i}\" :up/val {}}}",
                100 + i
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", upsert_ops.join("\n"))).expect("upsert");

        // Verify entity count
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':up/uid')
             AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 10, "Batch upsert should not create duplicates");

        // Spot check one value
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :up/uid \"BATCH-5\"] [?e :up/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_i64().expect("v"), 105);
    }
}
