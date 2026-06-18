use mentatd::db_cache::DbValueCache;
use std::time::Duration;

#[tokio::test]
async fn test_db_value_caching() {
    // Create a cache with 1 hour TTL
    let cache = DbValueCache::new(Duration::from_secs(3600));

    // Create a db snapshot
    let db_id = cache.create_snapshot(1000);
    assert!(!db_id.is_empty());

    // Retrieve the basis-t
    let basis_t = cache.get_basis_t(&db_id);
    assert_eq!(basis_t, Some(1000));

    // Try with invalid db_id
    let invalid_basis_t = cache.get_basis_t("invalid-id");
    assert_eq!(invalid_basis_t, None);
}

#[tokio::test]
async fn test_db_snapshot_expiration() {
    // Create a cache with very short TTL
    let cache = DbValueCache::new(Duration::from_millis(50));

    // Create a snapshot
    let db_id = cache.create_snapshot(2000);

    // Should be valid immediately
    assert_eq!(cache.get_basis_t(&db_id), Some(2000));

    // Wait for expiration
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should be expired now
    assert_eq!(cache.get_basis_t(&db_id), None);
}

#[tokio::test]
async fn test_multiple_snapshots() {
    let cache = DbValueCache::new(Duration::from_secs(3600));

    // Create multiple snapshots
    let db_id1 = cache.create_snapshot(1000);
    let db_id2 = cache.create_snapshot(2000);
    let db_id3 = cache.create_snapshot(3000);

    // All should be retrievable
    assert_eq!(cache.get_basis_t(&db_id1), Some(1000));
    assert_eq!(cache.get_basis_t(&db_id2), Some(2000));
    assert_eq!(cache.get_basis_t(&db_id3), Some(3000));

    // They should have different IDs
    assert_ne!(db_id1, db_id2);
    assert_ne!(db_id2, db_id3);
    assert_ne!(db_id1, db_id3);
}

#[tokio::test]
async fn test_cleanup_expired() {
    let cache = DbValueCache::new(Duration::from_millis(50));

    // Create snapshots
    let db_id1 = cache.create_snapshot(1000);
    let db_id2 = cache.create_snapshot(2000);

    assert_eq!(cache.len(), 2);

    // Wait for expiration
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Clean up expired
    cache.cleanup_expired();

    // Should be empty now
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.get_basis_t(&db_id1), None);
    assert_eq!(cache.get_basis_t(&db_id2), None);
}

#[tokio::test]
async fn test_snapshot_isolation() {
    let cache = DbValueCache::new(Duration::from_secs(3600));

    // Simulate different points in time
    let db_snapshot_t1 = cache.create_snapshot(1000);
    let db_snapshot_t2 = cache.create_snapshot(2000);

    // Each snapshot should preserve its own basis-t
    assert_eq!(cache.get_basis_t(&db_snapshot_t1), Some(1000));
    assert_eq!(cache.get_basis_t(&db_snapshot_t2), Some(2000));

    // Even after time passes, the basis-t should remain the same
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(cache.get_basis_t(&db_snapshot_t1), Some(1000));
    assert_eq!(cache.get_basis_t(&db_snapshot_t2), Some(2000));
}

// ---- Database value / Datomic d/db compatibility tests ----

#[tokio::test]
async fn test_datomic_db_pattern_immutable_snapshot() {
    // Simulate the Datomic pattern:
    //   (let [db (d/db conn)]    ;; snapshot with basis-t
    //     (d/q query db)         ;; query against that snapshot
    //     ;; even if new transactions happen, db still returns old data
    //     (d/q query db))        ;; same result
    let cache = DbValueCache::new(Duration::from_secs(300));

    // Step 1: Client calls d/db -> server creates snapshot at current basis-t
    let basis_t_at_snapshot = 1000;
    let db_id = cache.create_snapshot(basis_t_at_snapshot);

    // Step 2: Query uses db_id to get basis-t for as-of filtering
    assert_eq!(cache.get_basis_t(&db_id), Some(1000));

    // Step 3: New transaction happens (basis-t advances to 1001)
    // This creates a NEW snapshot, but doesn't affect the old one
    let _new_db_id = cache.create_snapshot(1001);

    // Step 4: Original db_id still returns original basis-t
    assert_eq!(cache.get_basis_t(&db_id), Some(1000));
}

#[tokio::test]
async fn test_datomic_db_value_ttl_5_minutes() {
    // Verify that with a 5-minute TTL, snapshots expire correctly
    let cache = DbValueCache::new(Duration::from_millis(100)); // 100ms as proxy for 5min

    let db_id = cache.create_snapshot(500);
    assert_eq!(cache.get_basis_t(&db_id), Some(500));

    // Wait for expiry
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Expired snapshot should return None (not an error)
    assert_eq!(cache.get_basis_t(&db_id), None);
}

#[tokio::test]
async fn test_get_snapshot_full_details() {
    let cache = DbValueCache::new(Duration::from_secs(300));

    let db_id = cache.create_snapshot(42);
    let snapshot = cache.get_snapshot(&db_id);
    assert!(snapshot.is_some());

    let snapshot = snapshot.unwrap();
    assert_eq!(snapshot.db_id, db_id);
    assert_eq!(snapshot.basis_t, 42);
    // created_at should be very recent
    assert!(snapshot.created_at.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn test_concurrent_db_snapshot_creation() {
    use std::sync::Arc;

    let cache = Arc::new(DbValueCache::new(Duration::from_secs(300)));
    let mut handles = Vec::new();

    // Simulate multiple clients calling d/db concurrently
    for i in 0..20 {
        let c = Arc::clone(&cache);
        handles.push(tokio::spawn(async move {
            let db_id = c.create_snapshot(i * 100);
            assert_eq!(c.get_basis_t(&db_id), Some(i * 100));
            db_id
        }));
    }

    let mut db_ids = Vec::new();
    for h in handles {
        db_ids.push(h.await.unwrap());
    }

    // All snapshots should be unique
    let unique: std::collections::HashSet<_> = db_ids.iter().collect();
    assert_eq!(unique.len(), 20);

    // All should still be accessible
    assert_eq!(cache.len(), 20);
}

#[tokio::test]
async fn test_snapshot_cleanup_preserves_active() {
    let cache = DbValueCache::new(Duration::from_millis(100));

    // Create an old snapshot
    let old_id = cache.create_snapshot(100);
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Create a fresh snapshot
    let fresh_id = cache.create_snapshot(200);

    // Cleanup should remove old but keep fresh
    cache.cleanup_expired();

    assert_eq!(cache.get_basis_t(&old_id), None);
    assert_eq!(cache.get_basis_t(&fresh_id), Some(200));
    assert_eq!(cache.len(), 1);
}
