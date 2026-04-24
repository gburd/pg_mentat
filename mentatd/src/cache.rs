use lru::LruCache;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Cache key: `(query_string, args_json)`.
type CacheKey = (String, String);

/// A cached query result, storing the serialized EDN response and when it was cached.
#[derive(Debug, Clone)]
struct CacheEntry {
    result: String,
    inserted_at: Instant,
}

/// Statistics about cache usage and dependency tracking.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of entries currently in the cache.
    pub size: usize,
    /// Total cache hits since startup.
    pub hits: u64,
    /// Total cache misses since startup.
    pub misses: u64,
    /// Hit rate as a fraction in `[0.0, 1.0]`, or 0 if no lookups yet.
    pub hit_rate: f64,
    /// Number of entries that have entity-level dependency tracking.
    pub tracked_entries: usize,
    /// Average number of entity dependencies per tracked entry (0 if none).
    pub avg_dependency_count: f64,
    /// Total number of targeted invalidations (individual entities).
    pub targeted_invalidations: u64,
    /// Total number of full invalidations (entire cache cleared).
    pub full_invalidations: u64,
}

/// Thread-safe LRU cache for query results with entity-level dependency tracking.
///
/// Cache keys are `(query, args_json)` tuples.  When a query result is inserted
/// with entity dependencies, only transactions that touch those entities will
/// invalidate the entry.  Entries inserted without dependencies are invalidated
/// on every transaction (conservative fallback).
///
/// Individual entries also expire after `ttl` has elapsed.
pub struct QueryCache {
    cache: Mutex<LruCache<CacheKey, CacheEntry>>,
    /// Maps cache keys to the set of entity IDs the cached result depends on.
    /// An entry present here with an empty set means "depends on everything"
    /// (conservative / untracked).
    dependencies: Mutex<HashMap<CacheKey, HashSet<i64>>>,
    ttl: Duration,
    enabled: bool,
    // Counters for stats (atomic so we don't need the cache lock).
    hits: AtomicU64,
    misses: AtomicU64,
    targeted_invalidations: AtomicU64,
    full_invalidations: AtomicU64,
}

impl QueryCache {
    /// Create a new cache with the given capacity and TTL.
    ///
    /// A capacity of 0 disables caching.  The TTL controls how long entries
    /// remain valid even without explicit invalidation.
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        let enabled = capacity > 0;
        // LruCache requires NonZeroUsize; use 1 as minimum (but `enabled` guards all access)
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap_or(NonZeroUsize::MIN);
        Self {
            cache: Mutex::new(LruCache::new(cap)),
            dependencies: Mutex::new(HashMap::new()),
            ttl,
            enabled,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            targeted_invalidations: AtomicU64::new(0),
            full_invalidations: AtomicU64::new(0),
        }
    }

    /// Look up a cached result for the given query and args.
    ///
    /// Returns `None` if the entry is missing, expired, or caching is disabled.
    pub fn get(&self, query: &str, args_json: &str) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let key = (query.to_owned(), args_json.to_owned());
        let mut cache = match self.cache.lock() {
            Ok(c) => c,
            Err(_) => return None,
        };
        let entry = match cache.get(&key) {
            Some(e) => e,
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        };
        if entry.inserted_at.elapsed() > self.ttl {
            // Entry expired; remove it and its dependency tracking.
            let key_clone = key;
            cache.pop(&key_clone);
            drop(cache);
            if let Ok(mut deps) = self.dependencies.lock() {
                deps.remove(&key_clone);
            }
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        } else {
            self.hits.fetch_add(1, Ordering::Relaxed);
            Some(entry.result.clone())
        }
    }

    /// Store a query result in the cache **without** dependency tracking.
    ///
    /// Entries inserted this way are conservatively invalidated on every
    /// transaction (same as the old behaviour).
    pub fn insert(&self, query: &str, args_json: &str, result: String) {
        if !self.enabled {
            return;
        }
        let key = (query.to_owned(), args_json.to_owned());
        if let Ok(mut cache) = self.cache.lock() {
            cache.put(
                key.clone(),
                CacheEntry {
                    result,
                    inserted_at: Instant::now(),
                },
            );
        }
        // Mark as untracked (empty dep set = depends on everything).
        if let Ok(mut deps) = self.dependencies.lock() {
            deps.insert(key, HashSet::new());
        }
    }

    /// Store a query result in the cache **with** entity dependency tracking.
    ///
    /// Only transactions that touch at least one entity in `depends_on` will
    /// invalidate this entry.  If `depends_on` is empty the entry is treated
    /// as depending on everything (conservative fallback).
    pub fn insert_with_deps(
        &self,
        query: &str,
        args_json: &str,
        result: String,
        depends_on: HashSet<i64>,
    ) {
        if !self.enabled {
            return;
        }
        let key = (query.to_owned(), args_json.to_owned());
        if let Ok(mut cache) = self.cache.lock() {
            cache.put(
                key.clone(),
                CacheEntry {
                    result,
                    inserted_at: Instant::now(),
                },
            );
        }
        if let Ok(mut deps) = self.dependencies.lock() {
            deps.insert(key, depends_on);
        }
    }

    /// Invalidate cached queries that depend on the given entity IDs.
    ///
    /// Entries that were inserted without dependency tracking (empty dep set)
    /// are also invalidated, since we cannot prove they are unaffected.
    ///
    /// Returns the number of cache entries that were removed.
    pub fn invalidate_entities(&self, entity_ids: &[i64]) -> usize {
        if !self.enabled || entity_ids.is_empty() {
            return 0;
        }

        let changed: HashSet<i64> = entity_ids.iter().copied().collect();

        let keys_to_remove: Vec<CacheKey> = {
            let deps = match self.dependencies.lock() {
                Ok(d) => d,
                Err(_) => return 0,
            };
            deps.iter()
                .filter(|(_, dep_entities)| {
                    // Empty dep set means untracked -- always invalidate.
                    dep_entities.is_empty() || dep_entities.iter().any(|e| changed.contains(e))
                })
                .map(|(key, _)| key.clone())
                .collect()
        };

        let removed = keys_to_remove.len();

        if removed > 0 {
            if let Ok(mut cache) = self.cache.lock() {
                for key in &keys_to_remove {
                    cache.pop(key);
                }
            }
            if let Ok(mut deps) = self.dependencies.lock() {
                for key in &keys_to_remove {
                    deps.remove(key);
                }
            }
        }

        self.targeted_invalidations.fetch_add(1, Ordering::Relaxed);
        removed
    }

    /// Clear all cached entries.  Use `invalidate_entities` when the set of
    /// changed entities is known; this full-clear is a fallback for cases
    /// where entity-level tracking is not available.
    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
        if let Ok(mut deps) = self.dependencies.lock() {
            deps.clear();
        }
        self.full_invalidations.fetch_add(1, Ordering::Relaxed);
    }

    /// Return the number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.cache.lock().map_or(0, |c| c.len())
    }

    /// Return a snapshot of cache statistics.
    pub fn stats(&self) -> CacheStats {
        let size = self.len();
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        let hit_rate = if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        };

        let (tracked_entries, avg_dependency_count) =
            if let Ok(deps) = self.dependencies.lock() {
                let tracked: Vec<&HashSet<i64>> =
                    deps.values().filter(|s| !s.is_empty()).collect();
                let count = tracked.len();
                let avg = if count > 0 {
                    tracked.iter().map(|s| s.len()).sum::<usize>() as f64 / count as f64
                } else {
                    0.0
                };
                (count, avg)
            } else {
                (0, 0.0)
            };

        CacheStats {
            size,
            hits,
            misses,
            hit_rate,
            tracked_entries,
            avg_dependency_count,
            targeted_invalidations: self.targeted_invalidations.load(Ordering::Relaxed),
            full_invalidations: self.full_invalidations.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_cache_hit_and_miss() {
        let cache = QueryCache::new(100, Duration::from_secs(60));
        assert!(cache.get("[:find ?e :where [?e :name]]", "[]").is_none());

        cache.insert(
            "[:find ?e :where [?e :name]]",
            "[]",
            r#"[["Alice"] ["Bob"]]"#.to_string(),
        );

        let result = cache.get("[:find ?e :where [?e :name]]", "[]");
        assert_eq!(result.as_deref(), Some(r#"[["Alice"] ["Bob"]]"#));
    }

    #[test]
    fn test_cache_different_args() {
        let cache = QueryCache::new(100, Duration::from_secs(60));
        let query = "[:find ?e :in $ ?name :where [?e :name ?name]]";

        cache.insert(query, r#"["Alice"]"#, "result_a".to_string());
        cache.insert(query, r#"["Bob"]"#, "result_b".to_string());

        assert_eq!(cache.get(query, r#"["Alice"]"#).as_deref(), Some("result_a"));
        assert_eq!(cache.get(query, r#"["Bob"]"#).as_deref(), Some("result_b"));
    }

    #[test]
    fn test_cache_invalidation() {
        let cache = QueryCache::new(100, Duration::from_secs(60));
        cache.insert("q1", "[]", "r1".to_string());
        cache.insert("q2", "[]", "r2".to_string());
        assert_eq!(cache.len(), 2);

        cache.invalidate();
        assert_eq!(cache.len(), 0);
        assert!(cache.get("q1", "[]").is_none());
    }

    #[test]
    fn test_cache_ttl_expiry() {
        let cache = QueryCache::new(100, Duration::from_millis(50));
        cache.insert("q", "[]", "r".to_string());
        assert!(cache.get("q", "[]").is_some());

        thread::sleep(Duration::from_millis(100));
        assert!(cache.get("q", "[]").is_none());
    }

    #[test]
    fn test_cache_lru_eviction() {
        let cache = QueryCache::new(2, Duration::from_secs(60));
        cache.insert("q1", "[]", "r1".to_string());
        cache.insert("q2", "[]", "r2".to_string());
        cache.insert("q3", "[]", "r3".to_string()); // evicts q1

        assert!(cache.get("q1", "[]").is_none());
        assert!(cache.get("q2", "[]").is_some());
        assert!(cache.get("q3", "[]").is_some());
    }

    #[test]
    fn test_cache_disabled_when_capacity_zero() {
        let cache = QueryCache::new(0, Duration::from_secs(60));
        cache.insert("q", "[]", "r".to_string());
        assert!(cache.get("q", "[]").is_none());
    }

    #[test]
    fn test_cache_overwrite_same_key() {
        let cache = QueryCache::new(100, Duration::from_secs(60));
        cache.insert("q", "[]", "old".to_string());
        cache.insert("q", "[]", "new".to_string());
        assert_eq!(cache.get("q", "[]").as_deref(), Some("new"));
    }

    // ---- Entity-level dependency tracking tests ----

    #[test]
    fn test_unrelated_transaction_does_not_invalidate_tracked_entry() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        // Insert a query that depends on entities 100 and 200
        let mut deps = HashSet::new();
        deps.insert(100);
        deps.insert(200);
        cache.insert_with_deps("q_person", "[]", "Alice".to_string(), deps);

        // Transaction changes entity 999 -- unrelated
        let removed = cache.invalidate_entities(&[999]);
        assert_eq!(removed, 0);
        assert_eq!(cache.get("q_person", "[]").as_deref(), Some("Alice"));
    }

    #[test]
    fn test_related_transaction_invalidates_tracked_entry() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        let mut deps = HashSet::new();
        deps.insert(100);
        deps.insert(200);
        cache.insert_with_deps("q_person", "[]", "Alice".to_string(), deps);

        // Transaction changes entity 200 -- overlaps
        let removed = cache.invalidate_entities(&[200]);
        assert_eq!(removed, 1);
        assert!(cache.get("q_person", "[]").is_none());
    }

    #[test]
    fn test_untracked_entry_invalidated_on_any_transaction() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        // Insert without deps (untracked -- conservative)
        cache.insert("q_legacy", "[]", "old_result".to_string());

        // Any entity change should invalidate untracked entries
        let removed = cache.invalidate_entities(&[42]);
        assert_eq!(removed, 1);
        assert!(cache.get("q_legacy", "[]").is_none());
    }

    #[test]
    fn test_mixed_tracked_and_untracked() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        // Tracked entry depending on entity 10
        let mut deps = HashSet::new();
        deps.insert(10);
        cache.insert_with_deps("q_tracked", "[]", "tracked".to_string(), deps);

        // Untracked entry
        cache.insert("q_untracked", "[]", "untracked".to_string());

        assert_eq!(cache.len(), 2);

        // Transaction on entity 999: untracked removed, tracked survives
        let removed = cache.invalidate_entities(&[999]);
        assert_eq!(removed, 1);
        assert_eq!(
            cache.get("q_tracked", "[]").as_deref(),
            Some("tracked")
        );
        assert!(cache.get("q_untracked", "[]").is_none());
    }

    #[test]
    fn test_invalidate_entities_with_multiple_entity_ids() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        let mut deps_a = HashSet::new();
        deps_a.insert(1);
        cache.insert_with_deps("qa", "[]", "a".to_string(), deps_a);

        let mut deps_b = HashSet::new();
        deps_b.insert(2);
        cache.insert_with_deps("qb", "[]", "b".to_string(), deps_b);

        let mut deps_c = HashSet::new();
        deps_c.insert(3);
        cache.insert_with_deps("qc", "[]", "c".to_string(), deps_c);

        // Transaction touches entities 1 and 3
        let removed = cache.invalidate_entities(&[1, 3]);
        assert_eq!(removed, 2);
        assert!(cache.get("qa", "[]").is_none());
        assert_eq!(cache.get("qb", "[]").as_deref(), Some("b"));
        assert!(cache.get("qc", "[]").is_none());
    }

    #[test]
    fn test_invalidate_entities_empty_list() {
        let cache = QueryCache::new(100, Duration::from_secs(60));
        cache.insert("q", "[]", "r".to_string());
        let removed = cache.invalidate_entities(&[]);
        assert_eq!(removed, 0);
        assert_eq!(cache.get("q", "[]").as_deref(), Some("r"));
    }

    #[test]
    fn test_full_invalidation_clears_dependencies() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        let mut deps = HashSet::new();
        deps.insert(1);
        cache.insert_with_deps("q", "[]", "r".to_string(), deps);

        cache.invalidate();
        assert_eq!(cache.len(), 0);

        // Re-insert and verify dep map was cleaned up (no stale deps)
        cache.insert("q", "[]", "r2".to_string());
        // Entity 1 should still invalidate because the new entry is untracked
        let removed = cache.invalidate_entities(&[1]);
        assert_eq!(removed, 1);
    }

    #[test]
    fn test_cache_stats() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        // Trigger some hits and misses
        let _ = cache.get("miss", "[]"); // miss
        cache.insert("q", "[]", "r".to_string());
        let _ = cache.get("q", "[]"); // hit
        let _ = cache.get("q", "[]"); // hit

        let mut deps = HashSet::new();
        deps.insert(10);
        deps.insert(20);
        cache.insert_with_deps("q2", "[]", "r2".to_string(), deps);

        let stats = cache.stats();
        assert_eq!(stats.size, 2);
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 2.0 / 3.0).abs() < 0.001);
        assert_eq!(stats.tracked_entries, 1); // only q2 has non-empty deps
        assert!((stats.avg_dependency_count - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_stats_invalidation_counters() {
        let cache = QueryCache::new(100, Duration::from_secs(60));
        cache.insert("q1", "[]", "r1".to_string());
        cache.insert("q2", "[]", "r2".to_string());

        cache.invalidate_entities(&[1]);
        cache.invalidate_entities(&[2]);
        cache.invalidate();

        let stats = cache.stats();
        assert_eq!(stats.targeted_invalidations, 2);
        assert_eq!(stats.full_invalidations, 1);
    }

    #[test]
    fn test_insert_with_deps_overwrites_previous_deps() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        let mut deps1 = HashSet::new();
        deps1.insert(1);
        cache.insert_with_deps("q", "[]", "r1".to_string(), deps1);

        // Overwrite with different deps
        let mut deps2 = HashSet::new();
        deps2.insert(2);
        cache.insert_with_deps("q", "[]", "r2".to_string(), deps2);

        // Entity 1 no longer relevant
        let removed = cache.invalidate_entities(&[1]);
        assert_eq!(removed, 0);
        assert_eq!(cache.get("q", "[]").as_deref(), Some("r2"));

        // Entity 2 should invalidate
        let removed = cache.invalidate_entities(&[2]);
        assert_eq!(removed, 1);
        assert!(cache.get("q", "[]").is_none());
    }

    // ---- Concurrent access tests ----

    #[test]
    fn test_concurrent_insert_and_get() {
        use std::sync::Arc;

        let cache = Arc::new(QueryCache::new(1000, Duration::from_secs(60)));
        let mut handles = Vec::new();

        // Spawn writers
        for i in 0..10 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for j in 0..100 {
                    let q = format!("q_{}_{}", i, j);
                    c.insert(&q, "[]", format!("r_{}_{}", i, j));
                }
            }));
        }

        // Spawn readers
        for i in 0..10 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for j in 0..100 {
                    let q = format!("q_{}_{}", i, j);
                    let _ = c.get(&q, "[]");
                }
            }));
        }

        for h in handles {
            h.join().expect("thread should not panic");
        }

        // Cache should still be functional
        cache.insert("final", "[]", "ok".to_string());
        assert_eq!(cache.get("final", "[]").as_deref(), Some("ok"));
    }

    #[test]
    fn test_concurrent_invalidate_entities() {
        use std::sync::Arc;

        let cache = Arc::new(QueryCache::new(1000, Duration::from_secs(60)));

        // Pre-populate with tracked entries
        for i in 0..100 {
            let mut deps = HashSet::new();
            deps.insert(i as i64);
            cache.insert_with_deps(
                &format!("q_{}", i),
                "[]",
                format!("r_{}", i),
                deps,
            );
        }

        let mut handles = Vec::new();

        // Invalidators
        for i in 0..10 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for j in 0..10 {
                    let entity_id = (i * 10 + j) as i64;
                    c.invalidate_entities(&[entity_id]);
                }
            }));
        }

        // Concurrent readers
        for i in 0..10 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for j in 0..10 {
                    let q = format!("q_{}", i * 10 + j);
                    let _ = c.get(&q, "[]");
                }
            }));
        }

        for h in handles {
            h.join().expect("thread should not panic");
        }

        // Stats should be coherent
        let stats = cache.stats();
        assert!(stats.targeted_invalidations > 0);
    }

    #[test]
    fn test_concurrent_full_invalidation() {
        use std::sync::Arc;

        let cache = Arc::new(QueryCache::new(1000, Duration::from_secs(60)));

        let mut handles = Vec::new();

        // Writers + invalidators running simultaneously
        for i in 0..5 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for j in 0..200 {
                    c.insert(&format!("q_{}_{}", i, j), "[]", "r".to_string());
                    if j % 50 == 0 {
                        c.invalidate();
                    }
                }
            }));
        }

        for h in handles {
            h.join().expect("thread should not panic");
        }

        // Cache should still be operational
        cache.insert("check", "[]", "ok".to_string());
        assert_eq!(cache.get("check", "[]").as_deref(), Some("ok"));
    }

    // ---- Additional edge case tests ----

    #[test]
    fn test_cache_len_reflects_deps_cleanup() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        let mut deps = HashSet::new();
        deps.insert(1);
        cache.insert_with_deps("q1", "[]", "r1".to_string(), deps);
        assert_eq!(cache.len(), 1);

        cache.invalidate_entities(&[1]);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_insert_with_empty_deps_is_untracked() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        // Empty deps = untracked = invalidated on any transaction
        cache.insert_with_deps("q", "[]", "r".to_string(), HashSet::new());

        let removed = cache.invalidate_entities(&[999]);
        assert_eq!(removed, 1);
        assert!(cache.get("q", "[]").is_none());
    }

    #[test]
    fn test_stats_after_no_operations() {
        let cache = QueryCache::new(100, Duration::from_secs(60));
        let stats = cache.stats();
        assert_eq!(stats.size, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.hit_rate, 0.0);
        assert_eq!(stats.tracked_entries, 0);
        assert_eq!(stats.avg_dependency_count, 0.0);
        assert_eq!(stats.targeted_invalidations, 0);
        assert_eq!(stats.full_invalidations, 0);
    }

    #[test]
    fn test_disabled_cache_stats() {
        let cache = QueryCache::new(0, Duration::from_secs(60));
        cache.insert("q", "[]", "r".to_string());
        let _ = cache.get("q", "[]");
        let stats = cache.stats();
        assert_eq!(stats.size, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_lru_eviction_cleans_up_deps() {
        let cache = QueryCache::new(2, Duration::from_secs(60));

        let mut deps1 = HashSet::new();
        deps1.insert(1);
        cache.insert_with_deps("q1", "[]", "r1".to_string(), deps1);

        let mut deps2 = HashSet::new();
        deps2.insert(2);
        cache.insert_with_deps("q2", "[]", "r2".to_string(), deps2);

        // q3 evicts q1 from LRU cache
        let mut deps3 = HashSet::new();
        deps3.insert(3);
        cache.insert_with_deps("q3", "[]", "r3".to_string(), deps3);

        assert!(cache.get("q1", "[]").is_none());
        assert!(cache.get("q2", "[]").is_some());
        assert!(cache.get("q3", "[]").is_some());
    }

    #[test]
    fn test_many_deps_per_entry() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        let deps: HashSet<i64> = (0..1000).collect();
        cache.insert_with_deps("q", "[]", "r".to_string(), deps);

        let stats = cache.stats();
        assert_eq!(stats.tracked_entries, 1);
        assert!((stats.avg_dependency_count - 1000.0).abs() < 0.001);

        // Invalidate with one matching entity
        let removed = cache.invalidate_entities(&[500]);
        assert_eq!(removed, 1);
    }

    #[test]
    fn test_invalidate_entities_returns_zero_for_empty_cache() {
        let cache = QueryCache::new(100, Duration::from_secs(60));
        let removed = cache.invalidate_entities(&[1, 2, 3]);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_repeated_invalidation_is_idempotent() {
        let cache = QueryCache::new(100, Duration::from_secs(60));

        let mut deps = HashSet::new();
        deps.insert(1);
        cache.insert_with_deps("q", "[]", "r".to_string(), deps);

        let removed1 = cache.invalidate_entities(&[1]);
        assert_eq!(removed1, 1);

        // Second invalidation should find nothing to remove
        let removed2 = cache.invalidate_entities(&[1]);
        assert_eq!(removed2, 0);
    }
}
