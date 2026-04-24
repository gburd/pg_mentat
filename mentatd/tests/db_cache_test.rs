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