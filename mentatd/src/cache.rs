use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A cached query result, storing the serialized EDN response and when it was cached.
#[derive(Debug, Clone)]
struct CacheEntry {
    result: String,
    inserted_at: Instant,
}

/// Thread-safe LRU cache for query results.
///
/// Cache keys are `(query, args_json)` tuples. The entire cache is invalidated
/// on any transaction, since transactions may change the data that queries read.
/// Individual entries also expire after `ttl` has elapsed.
pub struct QueryCache {
    cache: Mutex<LruCache<(String, String), CacheEntry>>,
    ttl: Duration,
    enabled: bool,
}

impl QueryCache {
    /// Create a new cache with the given capacity and TTL.
    ///
    /// A capacity of 0 disables caching. The TTL controls how long entries remain
    /// valid even without explicit invalidation.
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        let enabled = capacity > 0;
        // LruCache requires NonZeroUsize; use 1 as minimum (but `enabled` guards all access)
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap_or(NonZeroUsize::MIN);
        Self {
            cache: Mutex::new(LruCache::new(cap)),
            ttl,
            enabled,
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
        let mut cache = self.cache.lock().ok()?;
        let entry = cache.get(&key)?;
        if entry.inserted_at.elapsed() > self.ttl {
            // Entry expired; remove it
            let key_clone = key;
            cache.pop(&key_clone);
            None
        } else {
            Some(entry.result.clone())
        }
    }

    /// Store a query result in the cache.
    pub fn insert(&self, query: &str, args_json: &str, result: String) {
        if !self.enabled {
            return;
        }
        let key = (query.to_owned(), args_json.to_owned());
        if let Ok(mut cache) = self.cache.lock() {
            cache.put(
                key,
                CacheEntry {
                    result,
                    inserted_at: Instant::now(),
                },
            );
        }
    }

    /// Clear all cached entries. Called after every transaction to ensure
    /// queries never return stale data.
    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Return the number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.cache.lock().map_or(0, |c| c.len())
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
}
