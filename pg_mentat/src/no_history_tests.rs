// Tests for :db/noHistory attributes (1.5.0 item 2).
//
// A :db/noHistory attribute keeps ONLY the current value: assertions do not
// accumulate an assert/retract history trail in the log. This is the
// structural fix for monotonic-attribute bloat (e.g. :last-seen / :observed-at
// timestamps re-asserted every sync). The current-state projection and
// current-time queries behave exactly as for a normal attribute.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn schema() {
        // :p/seen is noHistory cardinality-one; :p/name is normal (full history);
        // :p/tag is noHistory cardinality-many.
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/ident :p/seen  :db/valueType :db.type/long   :db/cardinality :db.cardinality/one :db/noHistory true}
                {:db/ident :p/name  :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/ident :p/tag   :db/valueType :db.type/string :db/cardinality :db.cardinality/many :db/noHistory true}
            ]'::TEXT)",
        )
        .expect("schema tx");
    }

    fn eid() -> i64 {
        Spi::get_one::<i64>(
            "SELECT e FROM mentat.current_text WHERE v = 'a@x.io' AND a = mentat.attr_id(':p/email')",
        )
        .expect("q")
        .expect("NULL")
    }

    fn verify_clean() {
        let m = Spi::get_one::<i64>("SELECT mentat.verify_current_projection(0)")
            .expect("verify")
            .expect("NULL");
        assert_eq!(m, 0, "projection drifted from log: {} mismatches", m);
    }

    /// noHistory cardinality-one keeps ONLY the current value in the log --
    /// no assert/retract trail accumulates across repeated updates.
    #[pg_test]
    fn pg_test_nh_cardinality_one_no_trail() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/seen 100}]'::TEXT)")
            .expect("tx");
        let e = eid();
        for v in 101..=110 {
            Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/seen {}}}]'::TEXT)", e, v))
                .expect("update");
        }
        // Exactly ONE log row for :p/seen (the current value), not 10+ history rows.
        let log_rows = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.datoms_long_new \
             WHERE e = {} AND a = mentat.attr_id(':p/seen')", e
        ))
        .expect("count")
        .expect("NULL");
        assert_eq!(log_rows, 1, "noHistory must keep exactly 1 log row, got {}", log_rows);

        // And it's the latest value.
        let v = Spi::get_one::<i64>(&format!(
            "SELECT v FROM mentat.datoms_long_new \
             WHERE e = {} AND a = mentat.attr_id(':p/seen')", e
        ))
        .expect("v")
        .expect("NULL");
        assert_eq!(v, 110);
        verify_clean();
    }

    /// A normal (history-keeping) attribute on the same entity still keeps
    /// its full trail -- noHistory is per-attribute, not global.
    #[pg_test]
    fn pg_test_nh_normal_attr_keeps_history() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/name \"Alice\"}]'::TEXT)")
            .expect("tx");
        let e = eid();
        Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/name \"Alyce\"}}]'::TEXT)", e))
            .expect("update");
        // Normal attr: full trail (Alice asserted, Alice retracted, Alyce asserted) = 3.
        let log_rows = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.datoms_text_new \
             WHERE e = {} AND a = mentat.attr_id(':p/name')", e
        ))
        .expect("count")
        .expect("NULL");
        assert_eq!(log_rows, 3, "normal attr keeps full history trail");
        verify_clean();
    }

    /// Current-time queries on a noHistory attribute return the current value.
    #[pg_test]
    fn pg_test_nh_query_returns_current() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/seen 1}]'::TEXT)")
            .expect("tx");
        let e = eid();
        Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/seen 42}}]'::TEXT)", e))
            .expect("update");
        let raw = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?s :where [{} :p/seen ?s]]'::TEXT, '{{}}'::jsonb)::TEXT", e
        ))
        .expect("query")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let results = j["results"].as_array().expect("results");
        assert_eq!(results.len(), 1, "exactly one current value");
        assert_eq!(results[0][0].as_i64(), Some(42));
        verify_clean();
    }

    /// Idempotent re-assertion of the same noHistory value is a no-op (no new
    /// log row, no churn).
    #[pg_test]
    fn pg_test_nh_idempotent_reassert() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/seen 7}]'::TEXT)")
            .expect("tx");
        let e = eid();
        for _ in 0..5 {
            Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/seen 7}}]'::TEXT)", e))
                .expect("reassert");
        }
        let log_rows = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.datoms_long_new \
             WHERE e = {} AND a = mentat.attr_id(':p/seen')", e
        ))
        .expect("count")
        .expect("NULL");
        assert_eq!(log_rows, 1, "idempotent noHistory re-assert must not churn");
        verify_clean();
    }

    /// noHistory cardinality-many: multiple current values coexist, and
    /// re-asserting a value doesn't accumulate a trail.
    #[pg_test]
    fn pg_test_nh_cardinality_many() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/tag \"x\"}]'::TEXT)")
            .expect("tx");
        let e = eid();
        // Add two more tags, then re-assert x several times.
        Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/tag \"y\"}} {{:db/id {} :p/tag \"z\"}}]'::TEXT)", e, e))
            .expect("add");
        for _ in 0..3 {
            Spi::run(&format!("SELECT mentat_transact('[{{:db/id {} :p/tag \"x\"}}]'::TEXT)", e))
                .expect("reassert x");
        }
        // 3 current tags, and each appears exactly once in the log (noHistory).
        let log_rows = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.datoms_text_new \
             WHERE e = {} AND a = mentat.attr_id(':p/tag')", e
        ))
        .expect("count")
        .expect("NULL");
        assert_eq!(log_rows, 3, "noHistory many: one log row per distinct value");

        let current = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.current_text \
             WHERE e = {} AND a = mentat.attr_id(':p/tag')", e
        ))
        .expect("count")
        .expect("NULL");
        assert_eq!(current, 3, "three current tags");
        verify_clean();
    }

    /// Retracting a noHistory value removes it from current state.
    #[pg_test]
    fn pg_test_nh_retract() {
        setup();
        schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"p\" :p/email \"a@x.io\" :p/tag \"x\"}]'::TEXT)")
            .expect("tx");
        let e = eid();
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :p/tag \"x\"]]'::TEXT)", e))
            .expect("retract");
        let current = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.current_text \
             WHERE e = {} AND a = mentat.attr_id(':p/tag') AND v = 'x'", e
        ))
        .expect("count")
        .expect("NULL");
        assert_eq!(current, 0, "retracted noHistory value gone from current state");
        verify_clean();
    }
}
