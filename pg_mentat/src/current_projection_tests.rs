// Tests for the current-state projection (item 3 of the 1.5.0 append-only
// work): the nine mentat.current_<type> tables, their incremental
// maintenance by the transact path, and the rebuild / verify helpers.
//
// The central invariant under test: after ANY sequence of transactions,
// mentat.verify_current_projection(0) returns 0 -- the incrementally
// maintained projection equals a fresh latest-tx-wins resolution of the
// append-only log, with no rebuild required. This is the safety gate that
// must hold before item (1) removes the in-place `added` flip.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/ident :p/name  :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/ident :p/age   :db/valueType :db.type/long   :db/cardinality :db.cardinality/one}
                {:db/ident :p/tag   :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/ident :p/score :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/ident :p/seen  :db/valueType :db.type/instant :db/cardinality :db.cardinality/one}
                {:db/ident :p/admin :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/ident :p/friend :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        )
        .expect("schema tx");
    }

    fn verify_clean() {
        let m = Spi::get_one::<i64>("SELECT mentat.verify_current_projection(0)")
            .expect("verify")
            .expect("NULL");
        assert_eq!(m, 0, "projection drifted from log: {} mismatches", m);
    }

    fn email_eid(email: &str) -> i64 {
        Spi::get_one::<i64>(&format!(
            "SELECT e FROM mentat.current_text WHERE v = '{}' AND a = mentat.attr_id(':p/email')",
            email
        ))
        .expect("q")
        .expect("NULL")
    }

    /// Fresh insert populates the projection and stays consistent.
    #[pg_test]
    fn pg_test_proj_fresh_insert_consistent() {
        setup();
        schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/name \"Alice\" :p/age 30 :p/score 9.5 :p/admin true :p/tag \"x\"}]'::TEXT)",
        )
        .expect("tx");
        verify_clean();
        let e = email_eid("a@x.io");
        // Each cardinality-one value present exactly once.
        assert_eq!(
            Spi::get_one::<String>(&format!("SELECT v FROM mentat.current_text WHERE e={} AND a=mentat.attr_id(':p/name')", e)).expect("q").expect("NULL"),
            "Alice"
        );
        assert_eq!(
            Spi::get_one::<i64>(&format!("SELECT v FROM mentat.current_long WHERE e={} AND a=mentat.attr_id(':p/age')", e)).expect("q").expect("NULL"),
            30
        );
        assert!(
            Spi::get_one::<bool>(&format!("SELECT v FROM mentat.current_boolean WHERE e={} AND a=mentat.attr_id(':p/admin')", e)).expect("q").expect("NULL")
        );
    }

    /// Cardinality-one replace leaves exactly one current value.
    #[pg_test]
    fn pg_test_proj_cardinality_one_replace() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/name \"Alice\"}]'::TEXT)").expect("tx");
        let e = email_eid("a@x.io");
        Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/name \"Alyce\"}}]'::TEXT)", e)).expect("r1");
        Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/name \"Alicia\"}}]'::TEXT)", e)).expect("r2");

        let cnt = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.current_text WHERE e={} AND a=mentat.attr_id(':p/name')", e
        )).expect("q").expect("NULL");
        assert_eq!(cnt, 1, "cardinality-one must have exactly one current value");
        let v = Spi::get_one::<String>(&format!(
            "SELECT v FROM mentat.current_text WHERE e={} AND a=mentat.attr_id(':p/name')", e
        )).expect("q").expect("NULL");
        assert_eq!(v, "Alicia");
        verify_clean();
    }

    /// Cardinality-many: multiple values coexist; retract removes exactly one.
    #[pg_test]
    fn pg_test_proj_cardinality_many_add_retract() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/tag \"x\"}]'::TEXT)").expect("tx");
        let e = email_eid("a@x.io");
        Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/tag \"y\"}} {{:db/id {} :p/tag \"z\"}}]'::TEXT)", e, e)).expect("add");
        // Three tags now.
        let cnt = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.current_text WHERE e={} AND a=mentat.attr_id(':p/tag')", e
        )).expect("q").expect("NULL");
        assert_eq!(cnt, 3);

        // Retract one.
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :p/tag \"y\"]]'::TEXT)", e)).expect("retract");
        let tags: Vec<String> = {
            let raw = Spi::get_one::<String>(&format!(
                "SELECT string_agg(v, ',' ORDER BY v) FROM mentat.current_text WHERE e={} AND a=mentat.attr_id(':p/tag')", e
            )).expect("q").expect("NULL");
            raw.split(',').map(|s| s.to_string()).collect()
        };
        assert_eq!(tags, vec!["x".to_string(), "z".to_string()], "y retracted, x and z remain");
        verify_clean();
    }

    /// Retract-then-reassert within the projection ends with the value live.
    #[pg_test]
    fn pg_test_proj_retract_then_reassert() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/tag \"x\"}]'::TEXT)").expect("tx");
        let e = email_eid("a@x.io");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :p/tag \"x\"]]'::TEXT)", e)).expect("retract");
        // After retract, gone from projection.
        let after_retract = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.current_text WHERE e={} AND a=mentat.attr_id(':p/tag') AND v='x'", e
        )).expect("q").expect("NULL");
        assert_eq!(after_retract, 0, "retracted tag must leave the projection");
        verify_clean();

        // Re-assert.
        Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/tag \"x\"}}]'::TEXT)", e)).expect("reassert");
        let after_reassert = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.current_text WHERE e={} AND a=mentat.attr_id(':p/tag') AND v='x'", e
        )).expect("q").expect("NULL");
        assert_eq!(after_reassert, 1, "re-asserted tag must return to the projection");
        verify_clean();
    }

    /// retractEntity clears all of an entity's projection rows.
    #[pg_test]
    fn pg_test_proj_retract_entity() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/name \"Alice\" :p/age 30 :p/tag \"x\"}]'::TEXT)").expect("tx");
        let e = email_eid("a@x.io");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retractEntity {}]]'::TEXT)", e)).expect("retractEntity");

        for tbl in ["current_text", "current_long"] {
            let cnt = Spi::get_one::<i64>(&format!(
                "SELECT count(*) FROM mentat.{} WHERE e={}", tbl, e
            )).expect("q").expect("NULL");
            assert_eq!(cnt, 0, "{} must be empty after retractEntity", tbl);
        }
        verify_clean();
    }

    /// txInstant is mirrored into current_instant and is queryable.
    #[pg_test]
    fn pg_test_proj_txinstant_present() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\"}]'::TEXT)").expect("tx");
        // Every transaction's txInstant datom should be in current_instant.
        let txinstant_rows = Spi::get_one::<i64>(
            "SELECT count(*) FROM mentat.current_instant WHERE a = 50",
        ).expect("q").expect("NULL");
        assert!(txinstant_rows >= 1, "txInstant datoms must be projected");
        verify_clean();
    }

    /// rebuild_current_projection reproduces the incrementally-maintained
    /// state exactly (verify stays 0 after a rebuild).
    #[pg_test]
    fn pg_test_proj_rebuild_idempotent() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/name \"Alice\" :p/tag \"x\" :p/tag \"y\"}]'::TEXT)").expect("tx");
        let e = email_eid("a@x.io");
        Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/name \"Alyce\"}}]'::TEXT)", e)).expect("replace");

        let before = Spi::get_one::<i64>("SELECT count(*) FROM mentat.current_text").expect("q").expect("NULL");
        Spi::get_one::<i64>("SELECT mentat.rebuild_current_projection(0)").expect("rebuild");
        let after = Spi::get_one::<i64>("SELECT count(*) FROM mentat.current_text").expect("q").expect("NULL");
        assert_eq!(before, after, "rebuild must reproduce the same row count");
        verify_clean();
    }

    /// A deliberately corrupted projection is detected by verify and healed
    /// by rebuild.
    #[pg_test]
    fn pg_test_proj_verify_detects_drift() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/name \"Alice\"}]'::TEXT)").expect("tx");
        verify_clean();

        // Corrupt: delete a projection row that the log says is live.
        Spi::run("DELETE FROM mentat.current_text WHERE v = 'Alice'").expect("corrupt");
        let m = Spi::get_one::<i64>("SELECT mentat.verify_current_projection(0)").expect("verify").expect("NULL");
        assert!(m >= 1, "verify must detect the deleted row");

        // Heal.
        Spi::get_one::<i64>("SELECT mentat.rebuild_current_projection(0)").expect("rebuild");
        verify_clean();
    }
}
