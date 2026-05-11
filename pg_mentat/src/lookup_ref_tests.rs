// Exhaustive lookup ref tests: resolution via unique attributes,
// error cases, usage in transactions and queries.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod lookup_ref_tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_lr_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e\" :db/ident :lr/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"c\" :db/ident :lr/code :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/value}
                {:db/id \"n\" :db/ident :lr/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :lr/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"r\" :db/ident :lr/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"t\" :db/ident :lr/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        ).expect("lr schema");
    }

    // ========================================================================
    // Basic lookup ref in transactions
    // ========================================================================

    #[pg_test]
    fn test_lr_add_via_identity_lookup() {
        setup(); setup_lr_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :lr/email \"alice@test.com\"] [:db/add \"e\" :lr/name \"Alice\"]]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[[:db/add [:lr/email \"alice@test.com\"] :lr/val 42]]'::TEXT)").expect("lookup ref add");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :lr/email \"alice@test.com\"] [?e :lr/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_i64().expect("v"), 42);
    }

    #[pg_test]
    fn test_lr_add_via_value_lookup() {
        setup(); setup_lr_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :lr/code \"C001\"] [:db/add \"e\" :lr/name \"CodeEntity\"]]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[[:db/add [:lr/code \"C001\"] :lr/val 99]]'::TEXT)").expect("lookup ref add");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :lr/code \"C001\"] [?e :lr/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_i64().expect("v"), 99);
    }

    // ========================================================================
    // Lookup ref for retraction
    // ========================================================================

    #[pg_test]
    fn test_lr_retract_via_lookup() {
        setup(); setup_lr_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :lr/email \"retract@test.com\" :lr/name \"Gone\" :lr/val 10}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[[:db/retract [:lr/email \"retract@test.com\"] :lr/val 10]]'::TEXT)").expect("retract via lookup");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :lr/email \"retract@test.com\"] [?e :lr/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(j["result"].is_null());
    }

    // ========================================================================
    // Error cases
    // ========================================================================

    #[pg_test]
    fn test_lr_nonexistent_entity_fails() {
        setup(); setup_lr_schema();
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add [:lr/email \"nobody@test.com\"] :lr/val 1]]'::TEXT)",
        );
        assert!(result.is_err(), "Lookup ref for nonexistent entity should fail");
    }

    #[pg_test]
    fn test_lr_non_unique_attr_fails() {
        setup(); setup_lr_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :lr/name \"Test\"]]'::TEXT)").expect("create");
        let result = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add [:lr/name \"Test\"] :lr/val 1]]'::TEXT)",
        );
        assert!(result.is_err(), "Lookup ref on non-unique attr should fail");
    }

    // ========================================================================
    // Multiple lookups in one tx
    // ========================================================================

    #[pg_test]
    fn test_lr_multiple_lookups_same_tx() {
        setup(); setup_lr_schema();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"e1\" :lr/email \"a@test.com\" :lr/name \"A\"}
            {:db/id \"e2\" :lr/email \"b@test.com\" :lr/name \"B\"}
        ]'::TEXT)").expect("create");

        Spi::run("SELECT mentat_transact('[
            [:db/add [:lr/email \"a@test.com\"] :lr/val 100]
            [:db/add [:lr/email \"b@test.com\"] :lr/val 200]
        ]'::TEXT)").expect("multi lookup");

        let qa = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :lr/email \"a@test.com\"] [?e :lr/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let ja: serde_json::Value = serde_json::from_str(&qa).expect("parse");
        assert_eq!(ja["result"].as_i64().expect("v"), 100);

        let qb = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :lr/email \"b@test.com\"] [?e :lr/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let jb: serde_json::Value = serde_json::from_str(&qb).expect("parse");
        assert_eq!(jb["result"].as_i64().expect("v"), 200);
    }

    // ========================================================================
    // Lookup ref with update
    // ========================================================================

    #[pg_test]
    fn test_lr_update_via_lookup() {
        setup(); setup_lr_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :lr/email \"update@test.com\" :lr/val 10}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[[:db/add [:lr/email \"update@test.com\"] :lr/val 20]]'::TEXT)").expect("update");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :lr/email \"update@test.com\"] [?e :lr/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_i64().expect("v"), 20);
    }

    // ========================================================================
    // Lookup ref with cardinality-many
    // ========================================================================

    #[pg_test]
    fn test_lr_add_many_via_lookup() {
        setup(); setup_lr_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :lr/email \"tags@test.com\" :lr/name \"Tagged\"}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[
            [:db/add [:lr/email \"tags@test.com\"] :lr/tags \"t1\"]
            [:db/add [:lr/email \"tags@test.com\"] :lr/tags \"t2\"]
        ]'::TEXT)").expect("add tags via lookup");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?t ...] :where [?e :lr/email \"tags@test.com\"] [?e :lr/tags ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(j["result"].as_array().expect("arr").len(), 2);
    }
}
