// Regression tests for the PgQue integration:
//   mentat.has_pgque() / pgque_emit_tx / pgque_disable_tx /
//   pgque_register_consumer + the deferred constraint trigger
//   that emits one event per pg_mentat tx.
//
// PgQue is OPTIONAL and is not a PG extension — it's a pure-SQL
// schema installed via `\i sql/pgque.sql`. Tests skip when the
// schema isn't present in the test cluster.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._pgque_capture(stmt TEXT) RETURNS TEXT
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN '';
             EXCEPTION WHEN OTHERS THEN
                 RETURN SQLERRM;
             END;
             $$",
        )
        .expect("error-capture helper");
    }

    fn capture_error(sql: &str) -> String {
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<String>(&format!("SELECT mentat._pgque_capture('{}')", escaped))
            .expect("capture")
            .unwrap_or_default()
    }

    fn has_pgque() -> bool {
        Spi::get_one::<bool>("SELECT mentat.has_pgque()")
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    /// Detection helper returns boolean and doesn't crash regardless of
    /// whether PgQue is installed.
    #[pg_test]
    fn pg_test_pgque_has_pgque_returns_bool() {
        setup();
        let _ = Spi::get_one::<bool>("SELECT mentat.has_pgque()")
            .expect("call")
            .unwrap_or(false);
    }

    /// Calling pgque_emit_tx without PgQue installed surfaces the
    /// correct error.
    #[pg_test]
    fn pg_test_pgque_emit_tx_without_pgque_errors() {
        setup();
        if has_pgque() {
            return; // can't test the missing-extension path when it IS present
        }
        let err = capture_error("SELECT mentat.pgque_emit_tx('q')");
        assert!(
            err.contains(":db.error/missing-extension") && err.contains("PgQue"),
            "expected missing-extension PgQue error, got: {}",
            err,
        );
    }

    /// pgque_disable_tx is callable without PgQue and returns false (no
    /// trigger to drop). Useful for cleanup scripts that don't want to
    /// branch on has_pgque().
    #[pg_test]
    fn pg_test_pgque_disable_tx_safe_when_not_installed() {
        setup();
        if has_pgque() {
            return;
        }
        let dropped = Spi::get_one::<bool>("SELECT mentat.pgque_disable_tx('q')")
            .expect("call")
            .expect("NULL");
        assert!(!dropped);
    }

    /// _pgque_build_tx_payload returns a well-formed JSON envelope for
    /// a known tx. Doesn't require PgQue to be installed — only depends
    /// on the typed datom tables.
    #[pg_test]
    fn pg_test_pgque_build_tx_payload_shape() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :p/n :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :p/n \"Alice\"}
            ]'::TEXT)",
        )
        .expect("data tx");

        // Pull the latest tx number.
        let tx: i64 =
            Spi::get_one::<i64>("SELECT MAX(tx) FROM mentat.transactions WHERE tx >= 1000000")
                .expect("max tx")
                .expect("NULL");
        let payload = Spi::get_one::<String>(&format!(
            "SELECT mentat._pgque_build_tx_payload({})::TEXT",
            tx
        ))
        .expect("build payload")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&payload).expect("parse");
        assert_eq!(j["tx"].as_i64(), Some(tx));
        assert!(j["tx_instant"].is_string());
        assert!(
            j["datom_count"].as_i64().unwrap_or(0) >= 1,
            "datom_count > 0"
        );
        assert!(j["datoms"].is_array(), "datoms is array");
        let datoms = j["datoms"].as_array().expect("array");
        // Every datom has the expected envelope keys.
        for d in datoms {
            assert!(d["e"].is_number());
            assert!(d["a"].is_number());
            assert!(d["v"].is_string());
            assert!(d["vt"].is_string());
            assert!(d["tx"].as_i64() == Some(tx));
            assert!(d["added"].is_boolean());
        }
    }

    /// End-to-end happy path. Requires PgQue installed; skips otherwise.
    /// Verifies that:
    ///   1. pgque_emit_tx is idempotent (two calls produce the same
    ///      effective state).
    ///   2. After three transactions, the queue's underlying event table
    ///      contains three events with ev_type = 'mentat.tx'.
    ///   3. pgque_disable_tx returns true the first call, false the second.
    #[pg_test]
    fn pg_test_pgque_emit_tx_end_to_end() {
        setup();
        if !has_pgque() {
            return;
        }

        let q1 = Spi::get_one::<String>("SELECT mentat.pgque_emit_tx('mentat_events_test')")
            .expect("emit 1")
            .expect("NULL");
        let q2 = Spi::get_one::<String>("SELECT mentat.pgque_emit_tx('mentat_events_test')")
            .expect("emit 2")
            .expect("NULL");
        assert_eq!(q1, q2, "idempotent");

        // Three transactions.
        Spi::run(
            "SELECT mentat_transact('[
                {:db/ident :pq/n :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :pq/n \"A\"}]'::TEXT)").expect("tx 2");
        Spi::run("SELECT mentat_transact('[{:db/id \"b\" :pq/n \"B\"}]'::TEXT)").expect("tx 3");

        // Force a tick so events become consumer-visible. This is a
        // PgQue API call (no pg_mentat wrapper).
        Spi::run("SELECT pgque.force_next_tick('mentat_events_test')").expect("force_tick");

        // Inspect the queue's current event table directly. The exact
        // table name (event_<qid>_<rotation>) varies; query through the
        // public-facing view-by-aggregation pattern.
        let count: i64 = Spi::get_one::<i64>(
            "SELECT count(*)::BIGINT FROM (
                 SELECT pgque.batch_event_tables(
                     COALESCE(pgque.next_batch('mentat_events_test', '__test_consumer'), 0)
                 ) AS t
             ) tt",
        )
        .ok()
        .flatten()
        .unwrap_or(0);
        // Either a fresh consumer sees 0 events (registered after the
        // tick boundary) or events are present in the underlying table;
        // we don't assert >= 3 here because consumer registration timing
        // affects visibility per PgQue semantics. We DO assert that the
        // events actually landed in pgque.event_1 (the first rotation
        // event table for queue id 1).
        let _ = count;

        // Read the event table directly. ev_type = 'mentat.tx' for
        // every emit; we expect at least 3 events (the three
        // transactions) — possibly more if other tests ran first.
        let n_events: i64 = Spi::get_one::<i64>(
            "SELECT count(*)::BIGINT FROM pgque.event_1
             WHERE ev_type = 'mentat.tx'",
        )
        .expect("count")
        .expect("NULL");
        assert!(
            n_events >= 3,
            "expected at least 3 mentat.tx events, got {}",
            n_events
        );

        // ev_data is a valid JSON envelope.
        let any_data: String = Spi::get_one::<String>(
            "SELECT ev_data FROM pgque.event_1
             WHERE ev_type = 'mentat.tx' LIMIT 1",
        )
        .expect("ev_data")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&any_data).expect("parse");
        assert!(j["tx"].is_number());
        assert!(j["datoms"].is_array());

        // Disable round trip.
        let dropped1 = Spi::get_one::<bool>("SELECT mentat.pgque_disable_tx('mentat_events_test')")
            .expect("disable 1")
            .expect("NULL");
        let dropped2 = Spi::get_one::<bool>("SELECT mentat.pgque_disable_tx('mentat_events_test')")
            .expect("disable 2")
            .expect("NULL");
        assert!(dropped1, "first disable should report existed=true");
        assert!(!dropped2, "second disable should report existed=false");
    }
}
