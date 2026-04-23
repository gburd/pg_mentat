use pgrx::spi::Spi;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

/// Attribute metadata cache entry
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeInfo {
    pub value_type: String,
    pub cardinality: String,
    pub unique_constraint: Option<String>,
    pub fulltext: bool,
    pub indexed: bool,
}

/// Global caches for schema and ident lookups.
///
/// On first access the cache bulk-loads every row from `mentat.schema` and
/// `mentat.idents` in two queries, so subsequent attribute / ident lookups are
/// pure in-memory HashMap reads (O(1)).  After `invalidate()` the next access
/// triggers a fresh bulk load.
pub struct SchemaCache {
    /// True once the initial bulk load has completed.
    warmed: AtomicBool,
    /// Map from attribute entid to attribute metadata
    attrs_by_id: RwLock<HashMap<i64, AttributeInfo>>,
    /// Map from ident string (e.g., ":person/name") to entid
    idents_to_entid: RwLock<HashMap<String, i64>>,
    /// Map from entid to ident string
    entids_to_ident: RwLock<HashMap<i64, String>>,
}

impl SchemaCache {
    pub fn new() -> Self {
        Self {
            warmed: AtomicBool::new(false),
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

        // Load all attributes from mentat.schema in a single query
        let _ = Spi::connect(|client| {
            let rows = client.select(
                "SELECT entid, ident, value_type::TEXT, cardinality::TEXT, \
                 unique_constraint::TEXT, fulltext, indexed \
                 FROM mentat.schema",
                None,
                &[],
            )?;

            let mut attrs = self
                .attrs_by_id
                .write()
                .expect("RwLock poisoned - schema cache corrupted");
            let mut idents = self
                .idents_to_entid
                .write()
                .expect("RwLock poisoned - ident cache corrupted");
            let mut entids = self
                .entids_to_ident
                .write()
                .expect("RwLock poisoned - ident cache corrupted");

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

                attrs.insert(
                    entid,
                    AttributeInfo {
                        value_type,
                        cardinality,
                        unique_constraint,
                        fulltext,
                        indexed,
                    },
                );

                idents.insert(ident.clone(), entid);
                entids.insert(entid, ident);
            }

            Ok::<_, pgrx::spi::SpiError>(())
        });

        // Also load idents that are not in mentat.schema (e.g., bootstrap
        // entries that only live in mentat.idents).
        let _ = Spi::connect(|client| {
            let rows = client.select("SELECT ident, entid FROM mentat.idents", None, &[])?;

            let mut idents = self
                .idents_to_entid
                .write()
                .expect("RwLock poisoned - ident cache corrupted");
            let mut entids = self
                .entids_to_ident
                .write()
                .expect("RwLock poisoned - ident cache corrupted");

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

        self.warmed.store(true, Ordering::Release);
    }

    /// Ensure the cache is warmed, then return true if already warm.
    fn ensure_warm(&self) {
        if !self.warmed.load(Ordering::Acquire) {
            self.warm();
        }
    }

    /// Look up attribute metadata.
    ///
    /// On first call, bulk-loads all schema data.  Subsequent calls are pure
    /// HashMap lookups behind a read lock.
    pub fn get_attribute(&self, attr_id: i64) -> Option<AttributeInfo> {
        self.ensure_warm();

        let cache = self
            .attrs_by_id
            .read()
            .expect("RwLock poisoned - schema cache corrupted");
        cache.get(&attr_id).cloned()
    }

    /// Look up entid by ident string.
    ///
    /// On first call, bulk-loads all schema data.  Subsequent calls are pure
    /// HashMap lookups behind a read lock.
    pub fn resolve_ident(&self, ident: &str) -> Option<i64> {
        self.ensure_warm();

        let cache = self
            .idents_to_entid
            .read()
            .expect("RwLock poisoned - ident cache corrupted");
        cache.get(ident).copied()
    }

    /// Look up ident by entid.
    pub fn get_ident(&self, entid: i64) -> Option<String> {
        self.ensure_warm();

        let cache = self
            .entids_to_ident
            .read()
            .expect("RwLock poisoned - ident cache corrupted");
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
        let mut attrs = self
            .attrs_by_id
            .write()
            .expect("RwLock poisoned - schema cache corrupted");
        let mut idents = self
            .idents_to_entid
            .write()
            .expect("RwLock poisoned - ident cache corrupted");
        let mut entids = self
            .entids_to_ident
            .write()
            .expect("RwLock poisoned - ident cache corrupted");
        attrs.clear();
        idents.clear();
        entids.clear();
        self.warmed.store(false, Ordering::Release);
    }
}

/// Global schema cache instance
static SCHEMA_CACHE: once_cell::sync::Lazy<SchemaCache> =
    once_cell::sync::Lazy::new(|| SchemaCache::new());

/// Get the global schema cache
pub fn get_cache() -> &'static SchemaCache {
    &SCHEMA_CACHE
}
