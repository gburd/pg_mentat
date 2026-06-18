use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// A cached database snapshot with its basis-t and creation time.
#[derive(Debug, Clone)]
pub struct DbSnapshot {
    pub db_id: String,
    pub basis_t: i64,
    pub created_at: Instant,
}

/// Thread-safe cache for database snapshots used in batch queries.
///
/// Each snapshot captures a point-in-time view of the database (basis-t)
/// and can be reused across multiple queries to avoid HTTP overhead.
pub struct DbValueCache {
    snapshots: Arc<RwLock<HashMap<String, DbSnapshot>>>,
    ttl: Duration,
}

impl DbValueCache {
    /// Create a new db value cache with the specified TTL.
    ///
    /// Snapshots expire after the TTL and are cleaned up periodically.
    pub fn new(ttl: Duration) -> Self {
        Self {
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }

    /// Create a new database snapshot with the given basis-t.
    ///
    /// Returns a unique db_id that can be used to reference this snapshot.
    pub fn create_snapshot(&self, basis_t: i64) -> String {
        let db_id = Uuid::new_v4().to_string();
        let snapshot = DbSnapshot {
            db_id: db_id.clone(),
            basis_t,
            created_at: Instant::now(),
        };

        let mut snapshots = self.snapshots.write().unwrap();
        snapshots.insert(db_id.clone(), snapshot);
        db_id
    }

    /// Get a full snapshot clone for a given db_id.
    ///
    /// Returns None if the snapshot doesn't exist or has expired.
    pub fn get_snapshot(&self, db_id: &str) -> Option<DbSnapshot> {
        {
            let snapshots = self.snapshots.read().unwrap();
            if let Some(snapshot) = snapshots.get(db_id) {
                if snapshot.created_at.elapsed() < self.ttl {
                    return Some(snapshot.clone());
                }
            } else {
                return None;
            }
        }

        // If we get here, the snapshot expired and needs removal
        let mut snapshots = self.snapshots.write().unwrap();
        if let Some(snapshot) = snapshots.get(db_id) {
            if snapshot.created_at.elapsed() < self.ttl {
                return Some(snapshot.clone());
            }
            snapshots.remove(db_id);
        }
        None
    }

    /// Get the basis-t for a given db_id.
    ///
    /// Returns None if the snapshot doesn't exist or has expired.
    pub fn get_basis_t(&self, db_id: &str) -> Option<i64> {
        // First try with a read lock (common case)
        {
            let snapshots = self.snapshots.read().unwrap();
            if let Some(snapshot) = snapshots.get(db_id) {
                if snapshot.created_at.elapsed() < self.ttl {
                    return Some(snapshot.basis_t);
                }
                // Expired, need write lock to remove
            } else {
                return None; // Not found
            }
        }

        // If we get here, the snapshot expired and needs removal
        let mut snapshots = self.snapshots.write().unwrap();

        // Double-check it's still expired
        if let Some(snapshot) = snapshots.get(db_id) {
            if snapshot.created_at.elapsed() < self.ttl {
                // Race condition: another thread might have updated it
                return Some(snapshot.basis_t);
            }
            // Remove expired snapshot
            snapshots.remove(db_id);
        }
        None
    }

    /// Clean up expired snapshots.
    ///
    /// This should be called periodically by a background task.
    pub fn cleanup_expired(&self) {
        let mut snapshots = self.snapshots.write().unwrap();
        let now = Instant::now();

        snapshots.retain(|_, snapshot| now.duration_since(snapshot.created_at) < self.ttl);
    }

    /// Get the number of active snapshots.
    pub fn len(&self) -> usize {
        self.snapshots.read().unwrap().len()
    }

    /// Clear all snapshots.
    pub fn clear(&self) {
        self.snapshots.write().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_create_and_get_snapshot() {
        let cache = DbValueCache::new(Duration::from_secs(60));

        let db_id = cache.create_snapshot(1000);
        assert_eq!(cache.get_basis_t(&db_id), Some(1000));
    }

    #[test]
    fn test_snapshot_expiration() {
        let cache = DbValueCache::new(Duration::from_millis(50));

        let db_id = cache.create_snapshot(1000);
        assert_eq!(cache.get_basis_t(&db_id), Some(1000));

        thread::sleep(Duration::from_millis(100));
        assert_eq!(cache.get_basis_t(&db_id), None);
    }

    #[test]
    fn test_cleanup_expired() {
        let cache = DbValueCache::new(Duration::from_millis(50));

        let db_id1 = cache.create_snapshot(1000);
        let db_id2 = cache.create_snapshot(2000);
        assert_eq!(cache.len(), 2);

        thread::sleep(Duration::from_millis(100));

        cache.cleanup_expired();
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.get_basis_t(&db_id1), None);
        assert_eq!(cache.get_basis_t(&db_id2), None);
    }

    #[test]
    fn test_multiple_snapshots() {
        let cache = DbValueCache::new(Duration::from_secs(60));

        let db_id1 = cache.create_snapshot(1000);
        let db_id2 = cache.create_snapshot(2000);
        let db_id3 = cache.create_snapshot(3000);

        assert_eq!(cache.get_basis_t(&db_id1), Some(1000));
        assert_eq!(cache.get_basis_t(&db_id2), Some(2000));
        assert_eq!(cache.get_basis_t(&db_id3), Some(3000));
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn test_invalid_db_id() {
        let cache = DbValueCache::new(Duration::from_secs(60));

        assert_eq!(cache.get_basis_t("nonexistent"), None);
    }

    #[test]
    fn test_clear() {
        let cache = DbValueCache::new(Duration::from_secs(60));

        cache.create_snapshot(1000);
        cache.create_snapshot(2000);
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;

        let cache = Arc::new(DbValueCache::new(Duration::from_secs(60)));
        let mut handles = Vec::new();

        // Create snapshots concurrently
        for i in 0..10 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                let db_id = c.create_snapshot(i * 1000);
                assert_eq!(c.get_basis_t(&db_id), Some(i * 1000));
            }));
        }

        for h in handles {
            h.join().expect("thread should not panic");
        }

        assert!(cache.len() >= 10);
    }

    // ---- Database value / snapshot isolation tests ----

    #[test]
    fn test_get_snapshot_returns_full_details() {
        let cache = DbValueCache::new(Duration::from_secs(60));

        let db_id = cache.create_snapshot(42);
        let snapshot = cache.get_snapshot(&db_id);
        assert!(snapshot.is_some());

        let snapshot = snapshot.unwrap();
        assert_eq!(snapshot.db_id, db_id);
        assert_eq!(snapshot.basis_t, 42);
    }

    #[test]
    fn test_get_snapshot_expired() {
        let cache = DbValueCache::new(Duration::from_millis(50));

        let db_id = cache.create_snapshot(42);
        thread::sleep(Duration::from_millis(100));

        assert!(cache.get_snapshot(&db_id).is_none());
    }

    #[test]
    fn test_get_snapshot_nonexistent() {
        let cache = DbValueCache::new(Duration::from_secs(60));
        assert!(cache.get_snapshot("nonexistent-id").is_none());
    }

    #[test]
    fn test_snapshot_immutability() {
        // Verify that a snapshot's basis-t is immutable: creating new snapshots
        // with different basis-t values does not affect existing snapshots.
        let cache = DbValueCache::new(Duration::from_secs(60));

        let db_id_1 = cache.create_snapshot(1000);
        let db_id_2 = cache.create_snapshot(2000);
        let db_id_3 = cache.create_snapshot(3000);

        // Each snapshot retains its original basis-t
        assert_eq!(cache.get_basis_t(&db_id_1), Some(1000));
        assert_eq!(cache.get_basis_t(&db_id_2), Some(2000));
        assert_eq!(cache.get_basis_t(&db_id_3), Some(3000));

        // Verify via get_snapshot too
        let s1 = cache.get_snapshot(&db_id_1).unwrap();
        assert_eq!(s1.basis_t, 1000);
    }

    #[test]
    fn test_snapshot_isolation_simulated() {
        // Simulate the Datomic pattern:
        //   1. Take snapshot (basis-t = 100)
        //   2. "New transaction" occurs (basis-t = 200 in new snapshot)
        //   3. Old snapshot still reads basis-t = 100
        let cache = DbValueCache::new(Duration::from_secs(60));

        // Step 1: Client calls d/db, gets snapshot at t=100
        let db_before = cache.create_snapshot(100);
        assert_eq!(cache.get_basis_t(&db_before), Some(100));

        // Step 2: Another transaction happens, a new snapshot would have t=200
        let db_after = cache.create_snapshot(200);
        assert_eq!(cache.get_basis_t(&db_after), Some(200));

        // Step 3: The old snapshot is unaffected -- this is the immutability guarantee
        assert_eq!(cache.get_basis_t(&db_before), Some(100));
    }

    #[test]
    fn test_snapshot_unique_ids() {
        // Each snapshot should get a unique UUID-based db_id
        let cache = DbValueCache::new(Duration::from_secs(60));

        let id1 = cache.create_snapshot(100);
        let id2 = cache.create_snapshot(100);
        let id3 = cache.create_snapshot(100);

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_snapshot_ttl_boundary() {
        // Snapshot should be valid just before TTL and invalid just after
        let cache = DbValueCache::new(Duration::from_millis(200));

        let db_id = cache.create_snapshot(500);

        // Should be valid immediately
        assert!(cache.get_snapshot(&db_id).is_some());

        // Still valid partway through TTL
        thread::sleep(Duration::from_millis(100));
        assert!(cache.get_snapshot(&db_id).is_some());

        // Expired after TTL
        thread::sleep(Duration::from_millis(150));
        assert!(cache.get_snapshot(&db_id).is_none());
    }

    #[test]
    fn test_concurrent_snapshot_and_cleanup() {
        use std::sync::Arc;

        let cache = Arc::new(DbValueCache::new(Duration::from_millis(50)));
        let mut handles = Vec::new();

        // Writers creating snapshots
        for i in 0..5 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for j in 0..20 {
                    c.create_snapshot(i * 100 + j);
                }
            }));
        }

        // Concurrent cleanup thread
        {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for _ in 0..10 {
                    thread::sleep(Duration::from_millis(10));
                    c.cleanup_expired();
                }
            }));
        }

        for h in handles {
            h.join().expect("thread should not panic");
        }

        // Cache should still be operational after concurrent operations
        let final_id = cache.create_snapshot(9999);
        assert_eq!(cache.get_basis_t(&final_id), Some(9999));
    }
}
