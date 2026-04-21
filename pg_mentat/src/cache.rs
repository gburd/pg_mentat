use pgrx::datum::DatumWithOid;
use pgrx::spi::Spi;
use std::collections::HashMap;
use std::sync::RwLock;

/// Attribute metadata cache entry
#[derive(Clone, Debug)]
pub struct AttributeInfo {
    pub value_type: String,
    pub cardinality: String,
    pub unique_constraint: Option<String>,
    pub fulltext: bool,
    pub indexed: bool,
}

/// Global caches for schema and ident lookups
pub struct SchemaCache {
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
            attrs_by_id: RwLock::new(HashMap::new()),
            idents_to_entid: RwLock::new(HashMap::new()),
            entids_to_ident: RwLock::new(HashMap::new()),
        }
    }

    /// Look up attribute metadata, loading from DB if not cached
    pub fn get_attribute(&self, attr_id: i64) -> Option<AttributeInfo> {
        // Try read lock first (fast path)
        {
            let cache = self.attrs_by_id.read().unwrap();
            if let Some(info) = cache.get(&attr_id) {
                return Some(info.clone());
            }
        }

        // Not in cache, query database
        let info = self.load_attribute_from_db(attr_id)?;

        // Store in cache
        {
            let mut cache = self.attrs_by_id.write().unwrap();
            cache.insert(attr_id, info.clone());
        }

        Some(info)
    }

    /// Load attribute metadata from database
    fn load_attribute_from_db(&self, attr_id: i64) -> Option<AttributeInfo> {
        let value_type = Spi::get_one_with_args::<String>(
            "SELECT value_type::TEXT FROM mentat.schema WHERE entid = $1",
            &[DatumWithOid::from(attr_id)],
        )
        .ok()
        .flatten()?;

        let cardinality = Spi::get_one_with_args::<String>(
            "SELECT cardinality::TEXT FROM mentat.schema WHERE entid = $1",
            &[DatumWithOid::from(attr_id)],
        )
        .ok()
        .flatten()?;

        let unique_constraint = Spi::get_one_with_args::<String>(
            "SELECT unique_constraint::TEXT FROM mentat.schema WHERE entid = $1",
            &[DatumWithOid::from(attr_id)],
        )
        .ok()
        .flatten();

        let fulltext = Spi::get_one_with_args::<bool>(
            "SELECT fulltext FROM mentat.schema WHERE entid = $1",
            &[DatumWithOid::from(attr_id)],
        )
        .ok()
        .flatten()
        .unwrap_or(false);

        let indexed = Spi::get_one_with_args::<bool>(
            "SELECT indexed FROM mentat.schema WHERE entid = $1",
            &[DatumWithOid::from(attr_id)],
        )
        .ok()
        .flatten()
        .unwrap_or(false);

        Some(AttributeInfo {
            value_type,
            cardinality,
            unique_constraint,
            fulltext,
            indexed,
        })
    }

    /// Look up entid by ident string, loading from DB if not cached
    pub fn resolve_ident(&self, ident: &str) -> Option<i64> {
        // Try read lock first (fast path)
        {
            let cache = self.idents_to_entid.read().unwrap();
            if let Some(&entid) = cache.get(ident) {
                return Some(entid);
            }
        }

        // Not in cache, query database
        let entid = Spi::get_one_with_args::<i64>(
            "SELECT mentat.resolve_ident($1)",
            &[DatumWithOid::from(ident)],
        )
        .ok()
        .flatten()?;

        // Store in cache (both directions)
        {
            let mut idents_cache = self.idents_to_entid.write().unwrap();
            let mut entids_cache = self.entids_to_ident.write().unwrap();
            idents_cache.insert(ident.to_string(), entid);
            entids_cache.insert(entid, ident.to_string());
        }

        Some(entid)
    }

    /// Look up ident by entid, loading from DB if not cached
    pub fn get_ident(&self, entid: i64) -> Option<String> {
        // Try read lock first (fast path)
        {
            let cache = self.entids_to_ident.read().unwrap();
            if let Some(ident) = cache.get(&entid) {
                return Some(ident.clone());
            }
        }

        // Not in cache, query database
        let ident = Spi::get_one_with_args::<String>(
            "SELECT ident FROM mentat.idents WHERE entid = $1",
            &[DatumWithOid::from(entid)],
        )
        .ok()
        .flatten()?;

        // Store in cache (both directions)
        {
            let mut idents_cache = self.idents_to_entid.write().unwrap();
            let mut entids_cache = self.entids_to_ident.write().unwrap();
            idents_cache.insert(ident.clone(), entid);
            entids_cache.insert(entid, ident.clone());
        }

        Some(ident)
    }

    /// Invalidate all caches (call after schema changes)
    pub fn invalidate(&self) {
        let mut attrs = self.attrs_by_id.write().unwrap();
        let mut idents = self.idents_to_entid.write().unwrap();
        let mut entids = self.entids_to_ident.write().unwrap();
        attrs.clear();
        idents.clear();
        entids.clear();
    }
}

/// Global schema cache instance
static SCHEMA_CACHE: once_cell::sync::Lazy<SchemaCache> =
    once_cell::sync::Lazy::new(|| SchemaCache::new());

/// Get the global schema cache
pub fn get_cache() -> &'static SchemaCache {
    &SCHEMA_CACHE
}
