use crate::functions::store_management::get_schema_for_store;
use pgrx::spi::Spi;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use parking_lot::RwLock;

/// Attribute metadata cache entry
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeInfo {
    pub value_type: String,
    pub cardinality: String,
    pub unique_constraint: Option<String>,
    pub fulltext: bool,
    pub indexed: bool,
    pub component: bool,
}

/// Per-store schema and ident cache.
///
/// On first access the cache bulk-loads every row from `<schema>.schema` and
/// `<schema>.idents` in two queries, so subsequent attribute / ident lookups are
/// pure in-memory HashMap reads (O(1)).  After `invalidate()` the next access
/// triggers a fresh bulk load.
pub struct SchemaCache {
    /// The PostgreSQL schema name this cache loads from (e.g. "mentat", "mentat_my_store").
    db_schema: String,
    /// The logical store name (e.g. "default", "my_store") for generation lookups.
    store_name: String,
    /// True once the initial bulk load has completed.
    warmed: AtomicBool,
    /// Last known cache generation (from mentat.cache_generation table).
    /// Used for cross-backend invalidation: if the remote gen exceeds this,
    /// we know another backend modified the schema and must reload.
    local_gen: AtomicI64,
    /// Map from attribute entid to attribute metadata
    attrs_by_id: RwLock<HashMap<i64, AttributeInfo>>,
    /// Map from ident string (e.g., ":person/name") to entid
    idents_to_entid: RwLock<HashMap<String, i64>>,
    /// Map from entid to ident string
    entids_to_ident: RwLock<HashMap<i64, String>>,
}

impl SchemaCache {
    pub fn new(db_schema: String, store_name: String) -> Self {
        Self {
            db_schema,
            store_name,
            warmed: AtomicBool::new(false),
            local_gen: AtomicI64::new(0),
            attrs_by_id: RwLock::new(HashMap::new()),
            idents_to_entid: RwLock::new(HashMap::new()),
            entids_to_ident: RwLock::new(HashMap::new()),
        }
    }

    /// Bulk-load all schema attributes and idents from the database.
    ///
    /// Called automatically on the first cache miss.  Uses two queries to
    /// populate all three maps so that every subsequent lookup is O(1).
    fn warm(&self) {
        if self.warmed.load(Ordering::Acquire) {
            return;
        }

        let schema = &self.db_schema;

        // Load all attributes from <schema>.schema in a single query
        let _ = Spi::connect(|client| {
            let query = format!(
                "SELECT entid, ident, value_type::TEXT, cardinality::TEXT, \
                 unique_constraint::TEXT, fulltext, indexed, component \
                 FROM {}.schema",
                schema
            );
            let rows = client.select(&query, None, &[])?;

            let mut attrs = self.attrs_by_id.write();
            let mut idents = self.idents_to_entid.write();
            let mut entids = self.entids_to_ident.write();

            for row in rows {
                let entid: i64 = match row.get(1) {
                    Ok(Some(v)) => v,
                    _ => continue,
                };
                let ident: String = match row.get(2) {
                    Ok(Some(v)) => v,
                    _ => continue,
                };
                let value_type: String = match row.get(3) {
                    Ok(Some(v)) => v,
                    _ => continue,
                };
                let cardinality: String = match row.get(4) {
                    Ok(Some(v)) => v,
                    _ => continue,
                };
                let unique_constraint: Option<String> = row.get(5).ok().flatten();
                let fulltext: bool = row.get(6).ok().flatten().unwrap_or(false);
                let indexed: bool = row.get(7).ok().flatten().unwrap_or(false);
                let component: bool = row.get(8).ok().flatten().unwrap_or(false);

                attrs.insert(
                    entid,
                    AttributeInfo {
                        value_type,
                        cardinality,
                        unique_constraint,
                        fulltext,
                        indexed,
                        component,
                    },
                );

                idents.insert(ident.clone(), entid);
                entids.insert(entid, ident);
            }

            Ok::<_, pgrx::spi::SpiError>(())
        });

        // Also load idents that are not in <schema>.schema (e.g., bootstrap
        // entries that only live in <schema>.idents).
        let _ = Spi::connect(|client| {
            let query = format!("SELECT ident, entid FROM {}.idents", schema);
            let rows = client.select(&query, None, &[])?;

            let mut idents = self.idents_to_entid.write();
            let mut entids = self.entids_to_ident.write();

            for row in rows {
                let ident: String = match row.get(1) {
                    Ok(Some(v)) => v,
                    _ => continue,
                };
                let entid: i64 = match row.get(2) {
                    Ok(Some(v)) => v,
                    _ => continue,
                };

                // Only insert if not already present from the schema load
                idents.entry(ident.clone()).or_insert(entid);
                entids.entry(entid).or_insert(ident);
            }

            Ok::<_, pgrx::spi::SpiError>(())
        });

        // Record the current generation so we can detect future bumps
        let gen = Spi::get_one::<i64>(&format!(
            "SELECT gen FROM mentat.cache_generation WHERE store_name = '{}'",
            self.store_name
        ))
        .ok()
        .flatten()
        .unwrap_or(1);
        self.local_gen.store(gen, Ordering::Release);

        self.warmed.store(true, Ordering::Release);
    }

    /// Ensure the cache is fresh: warm it if never loaded, or reload if a
    /// remote backend has bumped the generation counter (cross-backend
    /// invalidation).
    fn ensure_fresh(&self) {
        // If never warmed, warm unconditionally
        if !self.warmed.load(Ordering::Acquire) {
            self.warm();
            return;
        }
        // Check remote generation (cheap: 1-row table, always in shared_buffers).
        // If the remote gen exceeds our local gen, another backend modified the
        // schema and we must reload.
        let remote_gen = Spi::get_one::<i64>(&format!(
            "SELECT gen FROM mentat.cache_generation WHERE store_name = '{}'",
            self.store_name
        ))
        .ok()
        .flatten()
        .unwrap_or(1);

        if remote_gen > self.local_gen.load(Ordering::Acquire) {
            self.invalidate();
            self.warm();
        }
    }

    /// Look up attribute metadata.
    ///
    /// On first call, bulk-loads all schema data.  Subsequent calls are pure
    /// HashMap lookups behind a read lock.
    pub fn get_attribute(&self, attr_id: i64) -> Option<AttributeInfo> {
        self.ensure_fresh();

        let cache = self.attrs_by_id.read();
        cache.get(&attr_id).cloned()
    }

    /// Look up entid by ident string.
    ///
    /// On first call, bulk-loads all schema data.  Subsequent calls are pure
    /// HashMap lookups behind a read lock.
    pub fn resolve_ident(&self, ident: &str) -> Option<i64> {
        self.ensure_fresh();

        let cache = self.idents_to_entid.read();
        cache.get(ident).copied()
    }

    /// Look up ident by entid.
    pub fn get_ident(&self, entid: i64) -> Option<String> {
        self.ensure_fresh();

        let cache = self.entids_to_ident.read();
        cache.get(&entid).cloned()
    }

    /// Look up attribute metadata by ident string.
    pub fn get_attribute_by_ident(&self, ident: &str) -> Option<AttributeInfo> {
        self.ensure_fresh();

        let ident_map = self.idents_to_entid.read();
        let entid = ident_map.get(ident).copied()?;
        drop(ident_map);

        let cache = self.attrs_by_id.read();
        cache.get(&entid).cloned()
    }

    /// Return true if the cache has been warmed (bulk-loaded).
    pub fn is_warmed(&self) -> bool {
        self.warmed.load(Ordering::Acquire)
    }

    /// Invalidate all caches (call after schema changes).
    ///
    /// The next access will trigger a fresh bulk load from the database.
    pub fn invalidate(&self) {
        let mut attrs = self.attrs_by_id.write();
        let mut idents = self.idents_to_entid.write();
        let mut entids = self.entids_to_ident.write();
        attrs.clear();
        idents.clear();
        entids.clear();
        self.warmed.store(false, Ordering::Release);
    }
}

/// Global store-aware schema cache map.
///
/// Maps store names (e.g. "default", "my_store") to their per-store SchemaCache.
/// Thread-safe via RwLock; each individual SchemaCache is also internally locked.
struct StoreCacheMap {
    caches: RwLock<HashMap<String, &'static SchemaCache>>,
}

impl StoreCacheMap {
    fn new() -> Self {
        Self {
            caches: RwLock::new(HashMap::new()),
        }
    }

    /// Get or create a SchemaCache for the given store name.
    fn get_or_create(&self, store_name: &str) -> &'static SchemaCache {
        // Fast path: read lock
        {
            let caches = self.caches.read();
            if let Some(cache) = caches.get(store_name) {
                return cache;
            }
        }

        // Slow path: write lock, create new cache
        let mut caches = self.caches.write();
        // Double-check after acquiring write lock
        if let Some(cache) = caches.get(store_name) {
            return cache;
        }

        let db_schema = get_schema_for_store(store_name);
        let cache = Box::leak(Box::new(SchemaCache::new(db_schema, store_name.to_string())));
        caches.insert(store_name.to_string(), cache);
        cache
    }

    /// Invalidate the cache for a specific store.
    fn invalidate_store(&self, store_name: &str) {
        let caches = self.caches.read();
        if let Some(cache) = caches.get(store_name) {
            cache.invalidate();
        }
    }

    /// Invalidate all store caches.
    #[allow(dead_code)] // Public API for future multi-store invalidation
    fn invalidate_all(&self) {
        let caches = self.caches.read();
        for cache in caches.values() {
            cache.invalidate();
        }
    }
}

/// Global store cache map instance
static STORE_CACHES: std::sync::LazyLock<StoreCacheMap> =
    std::sync::LazyLock::new(StoreCacheMap::new);

/// Get the schema cache for the default store.
///
/// This is backwards-compatible with code that used the old single-cache API.
pub fn get_cache() -> &'static SchemaCache {
    get_cache_for_store("default")
}

/// Get the schema cache for a named store.
///
/// Creates the cache on first access; subsequent calls return the same instance.
/// The cache is lazily warmed on first attribute/ident lookup.
pub fn get_cache_for_store(store_name: &str) -> &'static SchemaCache {
    STORE_CACHES.get_or_create(store_name)
}

/// Invalidate the schema cache for a specific store.
///
/// Call this after schema changes in a store so the cache is refreshed on next access.
pub fn invalidate_store_cache(store_name: &str) {
    STORE_CACHES.invalidate_store(store_name);
}

/// Invalidate all store schema caches.
///
/// Call this when a global event requires all caches to be refreshed.
#[allow(dead_code)] // Public API for future multi-store invalidation
pub fn invalidate_all_caches() {
    STORE_CACHES.invalidate_all();
}

/// Bump the generation counter for a store in the shared `cache_generation` table.
///
/// Called after a transaction modifies schema-defining attributes (`:db/valueType`,
/// `:db/cardinality`, `:db/unique`, `:db/ident`, `:db.install/attribute`).
/// Other backends will detect this bump on their next cache access and reload.
pub fn bump_store_generation(store_name: &str) {
    let _ = Spi::run(&format!(
        "UPDATE mentat.cache_generation SET gen = gen + 1 WHERE store_name = '{}'",
        store_name
    ));
}
