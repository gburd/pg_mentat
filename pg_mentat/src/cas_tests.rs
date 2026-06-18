// Compare-and-swap (CAS) tests: thorough testing of :db/cas operations
// for optimistic concurrency control patterns.

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
             $$",
        )
        .expect("create helper");
    }

    fn raises_error(sql: &str) -> bool {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<bool>(&format!("SELECT mentat._test_raises_error('{}')", escaped))
            .expect("raises_error call")
            .unwrap_or(false)
    }

    fn setup_cas_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :cas/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"v\" :db/ident :cas/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :cas/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"b\" :db/ident :cas/flag :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :cas/status :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("cas schema");
    }

    // ========================================================================
    // CAS success cases (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_cas_string_success() {
        setup();
        setup_cas_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cas/name \"old\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/name \"old\" \"new\"]]'::TEXT)",
            eid
        ))
        .expect("cas");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cas/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "new");
    }

    #[pg_test]
    fn test_cas_long_success() {
        setup();
        setup_cas_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :cas/val 10]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/val 10 20]]'::TEXT)",
            eid
        ))
        .expect("cas");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cas/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 20);
    }

    #[pg_test]
    fn test_cas_boolean_success() {
        setup();
        setup_cas_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cas/flag false]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/flag false true]]'::TEXT)",
            eid
        ))
        .expect("cas");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cas/flag ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), true);
    }

    #[pg_test]
    fn test_cas_keyword_success() {
        setup();
        setup_cas_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cas/status :draft]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/status :draft :published]]'::TEXT)",
            eid
        ))
        .expect("cas");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cas/status ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("published"));
    }

    #[pg_test]
    fn test_cas_sequential_3_steps() {
        setup();
        setup_cas_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :cas/val 1]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/val 1 2]]'::TEXT)",
            eid
        ))
        .expect("cas 1->2");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/val 2 3]]'::TEXT)",
            eid
        ))
        .expect("cas 2->3");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/val 3 4]]'::TEXT)",
            eid
        ))
        .expect("cas 3->4");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cas/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 4);
    }

    #[pg_test]
    fn test_cas_sequential_10_steps() {
        setup();
        setup_cas_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :cas/val 0]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/cas {} :cas/val {} {}]]'::TEXT)",
                eid,
                i,
                i + 1
            ))
            .expect("cas");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cas/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 10);
    }

    #[pg_test]
    fn test_cas_from_nil_string() {
        setup();
        setup_cas_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :cas/val 0]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        // CAS from nil (attribute not set) to a value
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/name nil \"first\"]]'::TEXT)",
            eid
        ))
        .expect("cas from nil");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cas/name ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "first");
    }

    #[pg_test]
    fn test_cas_from_nil_long() {
        setup();
        setup_cas_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cas/name \"test\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/val nil 42]]'::TEXT)",
            eid
        ))
        .expect("cas from nil");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cas/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 42);
    }

    // ========================================================================
    // CAS failure cases (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_cas_string_wrong_old_fails() {
        setup();
        setup_cas_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cas/name \"current\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        assert!(
            raises_error(&format!(
                "SELECT mentat_transact('[[:db/cas {} :cas/name \"wrong\" \"new\"]]'::TEXT)",
                eid
            )),
            "CAS with wrong old value should fail"
        );
    }

    #[pg_test]
    fn test_cas_long_wrong_old_fails() {
        setup();
        setup_cas_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :cas/val 42]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        assert!(
            raises_error(&format!(
                "SELECT mentat_transact('[[:db/cas {} :cas/val 99 100]]'::TEXT)",
                eid
            )),
            "CAS with wrong old value should fail"
        );
    }

    #[pg_test]
    fn test_cas_bool_wrong_old_fails() {
        setup();
        setup_cas_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cas/flag true]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        assert!(
            raises_error(&format!(
                "SELECT mentat_transact('[[:db/cas {} :cas/flag false true]]'::TEXT)",
                eid
            )),
            "CAS with wrong old boolean should fail"
        );
    }

    #[pg_test]
    fn test_cas_nil_but_has_value_fails() {
        setup();
        setup_cas_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :cas/val 42]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        assert!(
            raises_error(&format!(
                "SELECT mentat_transact('[[:db/cas {} :cas/val nil 99]]'::TEXT)",
                eid
            )),
            "CAS from nil should fail when value exists"
        );
    }

    #[pg_test]
    fn test_cas_failure_preserves_value() {
        setup();
        setup_cas_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :cas/val 42]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        // Isolate the failing CAS in a subtransaction so its error does not
        // poison the outer transaction; the value must remain 42.
        assert!(
            raises_error(&format!(
                "SELECT mentat_transact('[[:db/cas {} :cas/val 99 100]]'::TEXT)",
                eid
            )),
            "CAS with wrong old value should fail"
        );
        // Value should remain 42
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cas/val ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 42);
    }

    // ========================================================================
    // CAS with other operations (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_cas_with_add_same_tx() {
        setup();
        setup_cas_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cas/val 10 :cas/name \"test\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        // CAS val and add name in same tx
        Spi::run(&format!("SELECT mentat_transact('[[:db/cas {} :cas/val 10 20] [:db/add {} :cas/name \"updated\"]]'::TEXT)", eid, eid)).expect("mixed");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v ?n :where [{e} :cas/val ?v] [{e} :cas/name ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", e = eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_cas_status_machine() {
        setup();
        setup_cas_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cas/status :draft]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/status :draft :review]]'::TEXT)",
            eid
        ))
        .expect("cas");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/status :review :approved]]'::TEXT)",
            eid
        ))
        .expect("cas");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/cas {} :cas/status :approved :published]]'::TEXT)",
            eid
        ))
        .expect("cas");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cas/status ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("published"));
    }
}
