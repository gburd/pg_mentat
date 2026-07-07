// Entity-id partition-collision diagnostics + repair tests.
//
// Reproduces the pre-1.5.6 failure mode -- partition sequences that overflowed
// their bands into a shared id space, producing an entid used as BOTH a
// transaction and a user entity -- and verifies mentat.entid_collision_report /
// _count detect it and mentat.repair_entid_collisions renumbers the non-tx
// side without losing data or moving the transaction id.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    /// A fresh, healthy store has no entid collisions (the new disjoint,
    /// bounded layout keeps the genesis-tx sentinel and the user band apart).
    #[pg_test]
    fn test_ec_healthy_store_reports_zero() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[{:db/ident :ec/name :db/valueType :db.type/string \
             :db/cardinality :db.cardinality/one}]'::TEXT)",
        )
        .expect("schema");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :ec/name \"Alice\"}]'::TEXT)")
            .expect("data");

        let n = Spi::get_one::<i64>("SELECT mentat.entid_collision_count()")
            .expect("count")
            .expect("NULL");
        assert_eq!(n, 0, "healthy store must report 0 entid collisions");
    }

    /// Forge a collision (a user entity's id is also a transaction id), then
    /// verify detection, dry-run (no change), real repair (0 after), and that
    /// the user entity's data survives while the transaction keeps its id.
    #[pg_test]
    fn test_ec_detect_and_repair() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[{:db/ident :ec/tag :db/valueType :db.type/string \
             :db/cardinality :db.cardinality/one}]'::TEXT)",
        )
        .expect("schema");
        let report = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"u\" :ec/tag \"needle\"}]'::TEXT)",
        )
        .expect("data")
        .expect("NULL");
        let report: serde_json::Value = serde_json::from_str(&report).expect("parse");
        let uid = report["tempids"]["u"].as_i64().expect("uid");

        // Forge: make that user entid ALSO a transaction (as an overflowed
        // sequence would have done).
        Spi::run(&format!(
            "INSERT INTO mentat.transactions (tx, tx_instant) VALUES ({}, NOW()) \
             ON CONFLICT DO NOTHING",
            uid
        ))
        .expect("forge tx row");
        Spi::run(&format!(
            "INSERT INTO mentat.datoms_instant_new (store_id, e, a, v, tx, added) \
             VALUES (0, {uid}, 50, NOW(), {uid}, true)",
            uid = uid
        ))
        .expect("forge txInstant datom");

        // Detected.
        assert_eq!(
            Spi::get_one::<i64>("SELECT mentat.entid_collision_count()")
                .expect("c")
                .expect("NULL"),
            1,
            "forged collision must be detected"
        );

        // Dry-run changes nothing.
        let dry = Spi::get_one::<i64>("SELECT mentat.repair_entid_collisions(true)")
            .expect("dry")
            .expect("NULL");
        assert_eq!(dry, 1, "dry-run reports the count that would be remapped");
        assert_eq!(
            Spi::get_one::<i64>("SELECT mentat.entid_collision_count()")
                .expect("c")
                .expect("NULL"),
            1,
            "dry-run must not change anything"
        );

        // Real repair.
        let remapped = Spi::get_one::<i64>("SELECT mentat.repair_entid_collisions(false)")
            .expect("repair")
            .expect("NULL");
        assert_eq!(remapped, 1);

        // No collisions remain.
        assert_eq!(
            Spi::get_one::<i64>("SELECT mentat.entid_collision_count()")
                .expect("c")
                .expect("NULL"),
            0,
            "repair must clear all collisions"
        );

        // The transaction keeps its id; the non-tx datoms moved off it.
        let non_tx = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.datoms WHERE e = {} AND a <> 50 AND added",
            uid
        ))
        .expect("q")
        .expect("NULL");
        assert_eq!(non_tx, 0, "non-tx datoms must have moved off the tx id");
        let still_tx = Spi::get_one::<i64>(&format!(
            "SELECT count(*) FROM mentat.transactions WHERE tx = {}",
            uid
        ))
        .expect("q")
        .expect("NULL");
        assert_eq!(still_tx, 1, "transaction id is preserved");

        // The user entity's data survives (queryable under its new id).
        let found = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v :where [?e :ec/tag ?v] [(= ?v \"needle\")]]'::TEXT, \
             '{}'::jsonb)::TEXT",
        )
        .expect("q")
        .expect("NULL");
        assert!(
            found.contains("needle"),
            "moved entity data must survive: {}",
            found
        );
    }
}
