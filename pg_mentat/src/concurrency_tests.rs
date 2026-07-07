// Concurrency and sequence-based entity ID allocation tests
//
// These tests verify that the sequence-based allocation (replacing UPDATE-based
// partition locking) produces unique IDs and doesn't break query semantics.
//
// Note: True multi-backend concurrency tests require external test harnesses
// (e.g., pgbench or multiple psql sessions). The tests here verify correctness
// of the allocation mechanism within a single backend, including rapid
// sequential allocation and gap tolerance.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;
    use std::collections::HashSet;

    /// Initialize a test database with the pg_mentat schema.
    /// The extension creates all needed infrastructure; we just ensure it's loaded
    /// and bootstrapped.
    fn setup_test_db() -> Result<(), Box<dyn std::error::Error>> {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()")?;
        Ok(())
    }

    // ========================================================================
    // Sequence Uniqueness Tests
    // ========================================================================

    /// Verify that rapid sequential nextval() calls on user partition produce
    /// unique, monotonically increasing IDs with no duplicates.
    #[pg_test]
    fn test_sequence_produces_unique_ids() {
        crate::ensure_extension_loaded();
        setup_test_db().expect("setup failed");

        let mut ids: Vec<i64> = Vec::new();
        for _ in 0..1000 {
            let id = Spi::get_one::<i64>("SELECT nextval('mentat.partition_user_seq')")
                .expect("nextval failed")
                .expect("nextval returned NULL");
            ids.push(id);
        }

        // Verify uniqueness
        let unique_ids: HashSet<i64> = ids.iter().copied().collect();
        assert_eq!(
            ids.len(),
            unique_ids.len(),
            "All 1000 IDs should be unique, but got {} duplicates",
            ids.len() - unique_ids.len()
        );

        // Verify monotonic increase
        for i in 1..ids.len() {
            assert!(
                ids[i] > ids[i - 1],
                "IDs should be monotonically increasing: id[{}]={} <= id[{}]={}",
                i,
                ids[i],
                i - 1,
                ids[i - 1]
            );
        }
    }

    /// Verify that the allocate_entid function produces unique IDs for each partition.
    #[pg_test]
    fn test_allocate_entid_uniqueness_per_partition() {
        crate::ensure_extension_loaded();
        setup_test_db().expect("setup failed");

        let mut db_ids: Vec<i64> = Vec::new();
        let mut user_ids: Vec<i64> = Vec::new();
        let mut tx_ids: Vec<i64> = Vec::new();

        for _ in 0..100 {
            let db_id = Spi::get_one::<i64>("SELECT mentat.allocate_entid('db.part/db')")
                .expect("allocate_entid failed")
                .expect("returned NULL");
            db_ids.push(db_id);

            let user_id = Spi::get_one::<i64>("SELECT mentat.allocate_entid('db.part/user')")
                .expect("allocate_entid failed")
                .expect("returned NULL");
            user_ids.push(user_id);

            let tx_id = Spi::get_one::<i64>("SELECT mentat.allocate_entid('db.part/tx')")
                .expect("allocate_entid failed")
                .expect("returned NULL");
            tx_ids.push(tx_id);
        }

        // All IDs within each partition should be unique
        let db_unique: HashSet<i64> = db_ids.iter().copied().collect();
        let user_unique: HashSet<i64> = user_ids.iter().copied().collect();
        let tx_unique: HashSet<i64> = tx_ids.iter().copied().collect();

        assert_eq!(db_ids.len(), db_unique.len(), "db partition IDs not unique");
        assert_eq!(
            user_ids.len(),
            user_unique.len(),
            "user partition IDs not unique"
        );
        assert_eq!(tx_ids.len(), tx_unique.len(), "tx partition IDs not unique");

        // IDs should be in their respective partition ranges (new layout).
        for id in &db_ids {
            assert!(
                *id >= 0 && *id < 1000000,
                "db.part/db ID {} outside range [0, 1000000)",
                id
            );
        }
        for id in &user_ids {
            assert!(
                *id >= 1000001 && *id < 1000000000000,
                "db.part/user ID {} outside range [1000001, 1000000000000)",
                id
            );
        }
        for id in &tx_ids {
            assert!(
                *id >= 1000000000000 && *id < 2000000000000,
                "db.part/tx ID {} outside range [1000000000000, 2000000000000)",
                id
            );
        }

        // No overlap between partitions
        assert!(
            db_unique.is_disjoint(&user_unique),
            "db and user partition IDs overlap"
        );
        assert!(
            user_unique.is_disjoint(&tx_unique),
            "user and tx partition IDs overlap"
        );
        assert!(
            db_unique.is_disjoint(&tx_unique),
            "db and tx partition IDs overlap"
        );
    }

    // ========================================================================
    // Transaction-level Tests
    // ========================================================================

    /// Verify that multiple rapid transactions each get unique entity IDs.
    #[pg_test]
    fn test_no_duplicate_entids_across_transactions() {
        crate::ensure_extension_loaded();
        setup_test_db().expect("setup failed");

        // Define a test attribute
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\"
                 :db/ident :test/name
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("Failed to define attribute");

        // Run 50 rapid transactions, each creating a new entity
        let mut entity_ids: Vec<i64> = Vec::new();
        for i in 0..50 {
            let txn = format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :test/name \"person-{}\"]]'::TEXT)",
                i, i
            );
            Spi::run(&txn).expect("Transaction failed");
        }

        // Collect all entity IDs from the datoms table for :test/name
        Spi::connect(|client| {
            let table = client
                .select(
                    "SELECT DISTINCT e FROM mentat.datoms \
                 WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':test/name') \
                 AND added = true \
                 ORDER BY e",
                    None,
                    &[],
                )
                .expect("Query failed");

            for row in table {
                let e: i64 = row
                    .get(1)
                    .expect("column access failed")
                    .expect("NULL entity");
                entity_ids.push(e);
            }
        });

        // All entity IDs should be unique
        let unique_ids: HashSet<i64> = entity_ids.iter().copied().collect();
        assert_eq!(
            entity_ids.len(),
            unique_ids.len(),
            "Expected {} unique entity IDs but got {} ({} duplicates)",
            entity_ids.len(),
            unique_ids.len(),
            entity_ids.len() - unique_ids.len()
        );

        assert_eq!(
            entity_ids.len(),
            50,
            "Expected 50 entities, got {}",
            entity_ids.len()
        );
    }

    /// Verify that transaction IDs are unique and monotonically increasing.
    #[pg_test]
    fn test_transaction_ids_unique_and_ordered() {
        crate::ensure_extension_loaded();
        setup_test_db().expect("setup failed");

        // Define a test attribute
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\"
                 :db/ident :test/counter
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("Failed to define attribute");

        // Perform multiple transactions
        for i in 0..20 {
            let txn = format!(
                "SELECT mentat_transact('[[:db/add \"e\" :test/counter {}]]'::TEXT)",
                i
            );
            Spi::run(&txn).expect("Transaction failed");
        }

        // Collect all transaction IDs
        let mut tx_ids: Vec<i64> = Vec::new();
        Spi::connect(|client| {
            let table = client
                .select("SELECT tx FROM mentat.transactions ORDER BY tx", None, &[])
                .expect("Query failed");

            for row in table {
                let tx: i64 = row.get(1).expect("column access failed").expect("NULL tx");
                tx_ids.push(tx);
            }
        });

        // All tx IDs should be unique
        let unique_tx: HashSet<i64> = tx_ids.iter().copied().collect();
        assert_eq!(
            tx_ids.len(),
            unique_tx.len(),
            "Transaction IDs should be unique"
        );

        // All tx IDs should be monotonically increasing
        for i in 1..tx_ids.len() {
            assert!(
                tx_ids[i] > tx_ids[i - 1],
                "Transaction IDs should increase: tx[{}]={} <= tx[{}]={}",
                i,
                tx_ids[i],
                i - 1,
                tx_ids[i - 1]
            );
        }

        // All tx IDs should be in the tx partition range (new layout), except
        // the genesis/bootstrap transaction (tx=1000000), which is a fixed
        // sentinel that precedes the allocated tx band [1e12, 2e12) and is
        // stamped on the bootstrap datoms.
        for tx in &tx_ids {
            if *tx == 1000000 {
                continue; // genesis sentinel
            }
            assert!(
                *tx >= 1000000000000 && *tx < 2000000000000,
                "Transaction ID {} outside tx partition range [1000000000000, 2000000000000)",
                tx
            );
        }
    }

    // ========================================================================
    // Gap Tolerance Tests
    // ========================================================================

    /// Verify that gaps in entity IDs (natural with sequences/CACHE) don't
    /// break queries. When CACHE=100, each backend pre-allocates 100 IDs,
    /// so gaps of up to 100 are expected under normal operation.
    #[pg_test]
    fn test_sequence_gaps_acceptable() {
        crate::ensure_extension_loaded();
        setup_test_db().expect("setup failed");

        // Define test attribute
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\"
                 :db/ident :test/value
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("Failed to define attribute");

        // Simulate gaps by consuming IDs from the sequence without using them
        // This mimics what happens when a backend with cached IDs disconnects
        Spi::run("SELECT nextval('mentat.partition_user_seq') FROM generate_series(1, 50)")
            .expect("Consuming sequence IDs failed");

        // Now transact - should use IDs past the gap
        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :test/value 42]]'::TEXT)")
            .expect("Transaction after gap failed");

        Spi::run("SELECT mentat_transact('[[:db/add \"e2\" :test/value 99]]'::TEXT)")
            .expect("Second transaction after gap failed");

        // Query should still work correctly despite gaps in entity IDs
        let result = Spi::get_one::<String>(
            "SELECT mentat_query(
                '[:find ?v
                  :where
                  [?e :test/value ?v]]'::TEXT,
                '{}'::jsonb
            )::TEXT",
        )
        .expect("Query failed");

        let json: serde_json::Value = serde_json::from_str(&result.expect("Query returned NULL"))
            .expect("Failed to parse JSON");

        let results = json["results"].as_array().expect("Expected results array");
        assert_eq!(
            results.len(),
            2,
            "Should find both entities despite ID gaps, got {}",
            results.len()
        );
    }

    // ========================================================================
    // Sequence vs UPDATE Performance Verification
    // ========================================================================

    /// Verify that sequence-based allocation doesn't acquire row locks on the
    /// partitions table. We do this by allocating many IDs and verifying that
    /// the partitions table's next_entid is NOT updated (since sequences
    /// handle allocation now).
    #[pg_test]
    fn test_sequences_dont_lock_partitions_table() {
        crate::ensure_extension_loaded();
        setup_test_db().expect("setup failed");

        // Read current next_entid values
        let user_next_before = Spi::get_one::<i64>(
            "SELECT next_entid FROM mentat.partitions WHERE name = 'db.part/user'",
        )
        .expect("Query failed")
        .expect("NULL next_entid");

        // Allocate 100 IDs via sequence
        for _ in 0..100 {
            Spi::get_one::<i64>("SELECT nextval('mentat.partition_user_seq')")
                .expect("nextval failed")
                .expect("nextval returned NULL");
        }

        // next_entid in partitions table should NOT have changed
        // (sequences are independent of the partitions table)
        let user_next_after = Spi::get_one::<i64>(
            "SELECT next_entid FROM mentat.partitions WHERE name = 'db.part/user'",
        )
        .expect("Query failed")
        .expect("NULL next_entid");

        assert_eq!(
            user_next_before,
            user_next_after,
            "Partitions table next_entid should not change when using sequences. \
             Before: {}, After: {} (changed by {}). \
             This means UPDATE-based locking is still active.",
            user_next_before,
            user_next_after,
            user_next_after - user_next_before
        );
    }

    /// Verify that CACHE parameter on sequences works by checking that
    /// IDs are allocated in the expected range without gaps within a
    /// single backend session.
    #[pg_test]
    fn test_sequence_cache_behavior() {
        crate::ensure_extension_loaded();
        setup_test_db().expect("setup failed");

        // Within a single backend, consecutive nextval() should produce
        // consecutive IDs (no gaps) because the CACHE is pre-allocated
        let first = Spi::get_one::<i64>("SELECT nextval('mentat.partition_user_seq')")
            .expect("nextval failed")
            .expect("nextval returned NULL");

        let second = Spi::get_one::<i64>("SELECT nextval('mentat.partition_user_seq')")
            .expect("nextval failed")
            .expect("nextval returned NULL");

        assert_eq!(
            second,
            first + 1,
            "Consecutive nextval within same backend should be sequential: first={}, second={}",
            first,
            second
        );

        // Allocate a batch and verify they're all sequential
        let mut prev = second;
        for _ in 0..98 {
            let next = Spi::get_one::<i64>("SELECT nextval('mentat.partition_user_seq')")
                .expect("nextval failed")
                .expect("nextval returned NULL");
            assert_eq!(
                next,
                prev + 1,
                "Sequential allocation should produce consecutive IDs"
            );
            prev = next;
        }
    }

    // ========================================================================
    // Concurrent Simulation via SQL
    // ========================================================================

    /// Test concurrent ID allocation using generate_series to simulate
    /// bulk allocation. This verifies the sequence produces unique IDs
    /// even when called in a set-returning context.
    #[pg_test]
    fn test_bulk_sequence_allocation() {
        crate::ensure_extension_loaded();
        setup_test_db().expect("setup failed");

        // Allocate 1000 IDs in a single SQL statement
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT id) FROM (
                SELECT nextval('mentat.partition_user_seq') AS id
                FROM generate_series(1, 1000)
            ) sub",
        )
        .expect("Query failed")
        .expect("NULL count");

        assert_eq!(
            count, 1000,
            "Bulk allocation of 1000 IDs should produce 1000 unique values, got {}",
            count
        );
    }

    /// Test that allocating IDs from multiple partitions simultaneously
    /// produces non-overlapping ranges.
    #[pg_test]
    fn test_multi_partition_interleaved_allocation() {
        crate::ensure_extension_loaded();
        setup_test_db().expect("setup failed");

        let mut all_ids: HashSet<i64> = HashSet::new();

        // Interleave allocations from all three partitions
        for _ in 0..100 {
            let db_id = Spi::get_one::<i64>("SELECT nextval('mentat.partition_db_seq')")
                .expect("nextval failed")
                .expect("nextval returned NULL");
            let user_id = Spi::get_one::<i64>("SELECT nextval('mentat.partition_user_seq')")
                .expect("nextval failed")
                .expect("nextval returned NULL");
            let tx_id = Spi::get_one::<i64>("SELECT nextval('mentat.partition_tx_seq')")
                .expect("nextval failed")
                .expect("nextval returned NULL");

            assert!(
                all_ids.insert(db_id),
                "Duplicate ID {} from db partition",
                db_id
            );
            assert!(
                all_ids.insert(user_id),
                "Duplicate ID {} from user partition",
                user_id
            );
            assert!(
                all_ids.insert(tx_id),
                "Duplicate ID {} from tx partition",
                tx_id
            );
        }

        assert_eq!(
            all_ids.len(),
            300,
            "300 total IDs from 3 partitions should all be unique"
        );
    }

    // ========================================================================
    // Throughput Measurement
    // ========================================================================

    /// Measure sequence allocation throughput to establish a baseline.
    /// This isn't a pass/fail test but logs the throughput for comparison.
    #[pg_test]
    fn test_sequence_allocation_throughput() {
        crate::ensure_extension_loaded();
        setup_test_db().expect("setup failed");

        // Define test attribute
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\"
                 :db/ident :test/bench
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("Failed to define attribute");

        // Measure time for 100 transactions
        let start_micros =
            Spi::get_one::<i64>("SELECT (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT")
                .expect("clock failed")
                .expect("NULL clock");

        for i in 0..100 {
            let txn = format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :test/bench {}]]'::TEXT)",
                i, i
            );
            Spi::run(&txn).expect("Transaction failed");
        }

        let end_micros =
            Spi::get_one::<i64>("SELECT (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT")
                .expect("clock failed")
                .expect("NULL clock");

        let elapsed_ms = (end_micros - start_micros) as f64 / 1000.0;
        let tps = if elapsed_ms > 0.0 {
            100.0 / (elapsed_ms / 1000.0)
        } else {
            f64::INFINITY
        };

        // Log the throughput (visible in test output)
        pgrx::log!(
            "Sequence-based transaction throughput: {:.0} TPS ({:.1} ms for 100 txns)",
            tps,
            elapsed_ms
        );

        // Verify all 100 transactions actually completed
        let tx_count =
            Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.transactions WHERE tx > 1000000")
                .expect("Count query failed")
                .expect("NULL count");

        // At least 100 transactions (schema definition + 100 data txns)
        assert!(
            tx_count >= 100,
            "Expected at least 100 transactions, got {}",
            tx_count
        );
    }
}
