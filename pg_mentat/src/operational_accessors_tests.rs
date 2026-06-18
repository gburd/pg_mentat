// Tests for the operational accessors added in response to production
// feedback: mentat.attr_id, mentat.current, mentat.attribute_health,
// plus a regression guard on the cardinality-one single-table fast path
// (find_current_value_for_ea_typed) introduced alongside them.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn install_person_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :person/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/ident :person/name  :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/ident :person/age   :db/valueType :db.type/long   :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
    }

    /// Resolve an entity id by a known unique value.
    fn entity_by_email(email: &str) -> i64 {
        Spi::get_one::<i64>(&format!(
            "SELECT e FROM mentat.datoms_text_new \
             WHERE a = mentat.attr_id(':person/email') AND v = '{}' AND added",
            email
        ))
        .expect("query")
        .expect("NULL")
    }

    #[pg_test]
    fn pg_test_ops_attr_id_resolves() {
        setup();
        install_person_schema();
        let id = Spi::get_one::<i64>("SELECT mentat.attr_id(':person/name')")
            .expect("call")
            .expect("NULL");
        assert!(id > 0, "attr_id should be a positive entid, got {}", id);

        // Unknown attribute returns NULL (not an error).
        let none = Spi::get_one::<i64>("SELECT mentat.attr_id(':no/such')").expect("call");
        assert!(none.is_none(), "unknown attr should be NULL");
    }

    #[pg_test]
    fn pg_test_ops_current_returns_latest_value() {
        setup();
        install_person_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"p\" :person/email \"a@x.io\" :person/name \"Alice\" :person/age 30}]'::TEXT)",
        )
        .expect("data tx");
        let e = entity_by_email("a@x.io");

        let name = Spi::get_one::<String>(&format!("SELECT mentat.current({}, ':person/name')", e))
            .expect("current name")
            .expect("NULL");
        assert_eq!(name, "Alice");

        let age = Spi::get_one::<String>(&format!("SELECT mentat.current({}, ':person/age')", e))
            .expect("current age")
            .expect("NULL");
        assert_eq!(age, "30");

        // After a replace, current() reflects the new value.
        Spi::run(&format!(
            "SELECT mentat_transact('[{{:db/id {} :person/name \"Alyce\"}}]'::TEXT)",
            e
        ))
        .expect("replace tx");
        let name2 =
            Spi::get_one::<String>(&format!("SELECT mentat.current({}, ':person/name')", e))
                .expect("current name2")
                .expect("NULL");
        assert_eq!(name2, "Alyce", "current() must reflect the replaced value");
    }

    #[pg_test]
    fn pg_test_ops_current_null_for_absent() {
        setup();
        install_person_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :person/email \"b@x.io\"}]'::TEXT)")
            .expect("data tx");
        let e = entity_by_email("b@x.io");
        // No :person/name asserted for this entity.
        let name = Spi::get_one::<String>(&format!("SELECT mentat.current({}, ':person/name')", e))
            .expect("call");
        assert!(
            name.is_none(),
            "current() should be NULL when no value exists"
        );
    }

    /// The cardinality-one single-table fast path must give identical
    /// results to the old UNION-ALL probe: replacing a value retracts the
    /// old one and leaves exactly one live datom.
    #[pg_test]
    fn pg_test_ops_cardinality_one_fast_path_correct() {
        setup();
        install_person_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"p\" :person/email \"c@x.io\" :person/name \"Carol\"}]'::TEXT)",
        )
        .expect("data tx");
        let e = entity_by_email("c@x.io");

        // Replace twice.
        Spi::run(&format!(
            "SELECT mentat_transact('[{{:db/id {} :person/name \"Caroline\"}}]'::TEXT)",
            e
        ))
        .expect("r1");
        Spi::run(&format!(
            "SELECT mentat_transact('[{{:db/id {} :person/name \"Carrie\"}}]'::TEXT)",
            e
        ))
        .expect("r2");

        // Exactly one current value (append-only model: the log retains the
        // full assert/retract history; "exactly one live value" is a property
        // of the current-state projection, not an `added=true` count of the
        // immutable log).
        let live = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::BIGINT FROM mentat.current_text \
             WHERE e = {} AND a = mentat.attr_id(':person/name')",
            e
        ))
        .expect("count")
        .expect("NULL");
        assert_eq!(
            live, 1,
            "cardinality-one must keep exactly one current value"
        );

        // And it is the latest.
        let name = Spi::get_one::<String>(&format!("SELECT mentat.current({}, ':person/name')", e))
            .expect("current")
            .expect("NULL");
        assert_eq!(name, "Carrie");
    }

    /// Idempotent re-assertion of the same value does not create a second
    /// live datom (the Skip path).
    #[pg_test]
    fn pg_test_ops_idempotent_reassert_no_new_live_datom() {
        setup();
        install_person_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"p\" :person/email \"d@x.io\" :person/name \"Dave\"}]'::TEXT)",
        )
        .expect("data tx");
        let e = entity_by_email("d@x.io");

        // Re-assert the identical value three times.
        for _ in 0..3 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id {} :person/name \"Dave\"}}]'::TEXT)",
                e
            ))
            .expect("reassert");
        }

        let live = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::BIGINT FROM mentat.datoms_text_new \
             WHERE e = {} AND a = mentat.attr_id(':person/name') AND added",
            e
        ))
        .expect("count")
        .expect("NULL");
        assert_eq!(live, 1, "idempotent re-assert must not add live datoms");

        // No new dead rows either: re-asserting the same value is a true
        // no-op for the datom tables (Skip path), so total rows stay at 1.
        let total = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::BIGINT FROM mentat.datoms_text_new \
             WHERE e = {} AND a = mentat.attr_id(':person/name')",
            e
        ))
        .expect("count")
        .expect("NULL");
        assert_eq!(total, 1, "idempotent re-assert must not churn the table");
    }

    #[pg_test]
    fn pg_test_ops_attribute_health_reports_counts() {
        setup();
        install_person_schema();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"p1\" :person/email \"e1@x.io\" :person/name \"E1\"}
                {:db/id \"p2\" :person/email \"e2@x.io\" :person/name \"E2\"}
            ]'::TEXT)",
        )
        .expect("data tx");

        // attribute_health returns a row per attribute with live counts.
        let name_live = Spi::get_one::<i64>(
            "SELECT live_datoms FROM mentat.attribute_health() \
             WHERE attr_ident = ':person/name'",
        )
        .expect("query")
        .expect("NULL");
        assert_eq!(name_live, 2, ":person/name should have 2 live datoms");

        // dead_pct column is present and numeric (0 on a fresh table).
        let dead = Spi::get_one::<pgrx::AnyNumeric>(
            "SELECT dead_pct FROM mentat.attribute_health() \
             WHERE attr_ident = ':person/name'",
        )
        .expect("query")
        .expect("NULL");
        let dead_f: f64 = dead.try_into().expect("numeric->f64");
        assert!(
            dead_f >= 0.0 && dead_f <= 100.0,
            "dead_pct in [0,100], got {}",
            dead_f
        );
    }
}
