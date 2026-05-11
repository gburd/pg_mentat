// Cache performance tests
#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod cache_tests {
    use pgrx::prelude::*;

    /// Test that the cache bulk-loads all bootstrap attributes on first access
    #[pg_test]
    fn test_cache_warming() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        let cache = crate::cache::get_cache();
        cache.invalidate();

        // Cache should not be warmed yet
        assert!(
            !cache.is_warmed(),
            "Cache should start un-warmed after invalidation"
        );

        // First access triggers bulk load
        let entid = cache.resolve_ident(":db/ident");
        assert_eq!(entid, Some(1), "Should resolve :db/ident");
        assert!(
            cache.is_warmed(),
            "Cache should be warmed after first access"
        );

        // All bootstrap attributes should now be in cache without further DB queries
        assert!(cache.resolve_ident(":db/valueType").is_some());
        assert!(cache.resolve_ident(":db/cardinality").is_some());
        assert!(cache.resolve_ident(":db/unique").is_some());

        // Attribute metadata should also be loaded
        let attr = cache.get_attribute(1);
        assert!(
            attr.is_some(),
            "Attribute metadata should be cached after warming"
        );
    }

    /// Test cache hit behavior for attribute lookups
    #[pg_test]
    fn test_attribute_cache_hit() {
        crate::ensure_extension_loaded();
        // Bootstrap schema should be in place
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        let cache = crate::cache::get_cache();

        // First lookup (cache miss, triggers bulk load)
        let attr1 = cache.get_attribute(1); // :db/ident
        assert!(attr1.is_some(), "Should find :db/ident in schema");

        // Second lookup (cache hit, no DB query)
        let attr2 = cache.get_attribute(1);
        assert!(attr2.is_some(), "Should find cached :db/ident");
        assert_eq!(attr1, attr2, "Cached values should be identical");
    }

    /// Test cache hit behavior for ident resolution
    #[pg_test]
    fn test_ident_cache_hit() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        let cache = crate::cache::get_cache();

        // First lookup (triggers bulk load)
        let entid1 = cache.resolve_ident(":db/ident");
        assert_eq!(entid1, Some(1), "Should resolve :db/ident to entid 1");

        // Second lookup (cache hit)
        let entid2 = cache.resolve_ident(":db/ident");
        assert_eq!(entid2, Some(1), "Should find cached :db/ident");
    }

    /// Test bidirectional ident/entid cache consistency
    #[pg_test]
    fn test_bidirectional_cache() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        let cache = crate::cache::get_cache();

        // Resolve ident to entid
        let entid = cache.resolve_ident(":db/ident");
        assert_eq!(entid, Some(1));

        // Reverse lookup should be cached
        let ident = cache.get_ident(1);
        assert_eq!(ident, Some(":db/ident".to_string()));

        // Both directions should be in cache now
        let entid2 = cache.resolve_ident(":db/ident");
        assert_eq!(entid2, Some(1));
    }

    /// Test cache invalidation after schema changes
    #[pg_test]
    fn test_cache_invalidation() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        let cache = crate::cache::get_cache();

        // Populate cache
        let _ = cache.resolve_ident(":db/ident");
        let _ = cache.get_attribute(1);
        assert!(cache.is_warmed(), "Cache should be warmed");

        // Invalidate cache
        cache.invalidate();
        assert!(
            !cache.is_warmed(),
            "Cache should not be warmed after invalidation"
        );

        // Next lookup re-warms the cache
        let entid = cache.resolve_ident(":db/ident");
        assert_eq!(entid, Some(1), "Should still resolve after invalidation");
        assert!(cache.is_warmed(), "Cache should be re-warmed after access");

        let attr = cache.get_attribute(1);
        assert!(
            attr.is_some(),
            "Should still find attribute after invalidation"
        );
    }

    /// Test cache behavior with user-defined attributes
    #[pg_test]
    fn test_user_attribute_caching() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        // Define a new attribute
        let tx = r#"[{:db/ident :person/name
                      :db/valueType :db.type/string
                      :db/cardinality :db.cardinality/one}]"#;

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_transact('{}')",
            tx.replace('\'', "''")
        ));
        assert!(result.is_ok(), "Transaction should succeed");

        let cache = crate::cache::get_cache();

        // Resolve new attribute (should be in cache after transaction re-warm)
        let entid = cache.resolve_ident(":person/name");
        assert!(entid.is_some(), "Should resolve user-defined attribute");

        // Check attribute metadata
        if let Some(eid) = entid {
            let attr_info = cache.get_attribute(eid);
            assert!(attr_info.is_some(), "Should have attribute metadata");

            let info = attr_info.unwrap();
            assert_eq!(info.value_type, "string");
            assert_eq!(info.cardinality, "one");
        }
    }

    /// Test cache miss for non-existent attributes
    #[pg_test]
    fn test_cache_miss_nonexistent() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        let cache = crate::cache::get_cache();

        // Try to resolve non-existent ident
        let entid = cache.resolve_ident(":nonexistent/attribute");
        assert_eq!(entid, None, "Should return None for non-existent ident");

        // Try to get non-existent attribute (very high entid)
        let attr = cache.get_attribute(999999);
        assert_eq!(attr, None, "Should return None for non-existent attribute");
    }

    /// Test attribute metadata completeness
    #[pg_test]
    fn test_attribute_metadata_fields() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        // Create attribute with various properties
        let tx = r#"[{:db/ident :test/indexed
                      :db/valueType :db.type/string
                      :db/cardinality :db.cardinality/one
                      :db/unique :db.unique/value
                      :db/index true
                      :db/fulltext false}]"#;

        let result = Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_transact('{}')",
            tx.replace('\'', "''")
        ));
        assert!(result.is_ok(), "Transaction should succeed");

        let cache = crate::cache::get_cache();
        let entid = cache
            .resolve_ident(":test/indexed")
            .expect("Should resolve");
        let attr = cache.get_attribute(entid).expect("Should have metadata");

        // Verify all fields are present
        assert_eq!(attr.value_type, "string");
        assert_eq!(attr.cardinality, "one");
        assert_eq!(attr.unique_constraint, Some("value".to_string()));
        assert_eq!(attr.indexed, true);
        assert_eq!(attr.fulltext, false);
    }

    /// Test concurrent cache access (read-heavy workload)
    #[pg_test]
    fn test_concurrent_reads() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        let cache = crate::cache::get_cache();

        // Populate cache
        let _ = cache.resolve_ident(":db/ident");
        let _ = cache.get_attribute(1);

        // Multiple concurrent reads (in single-threaded test, just verify no panics)
        for _ in 0..100 {
            let _ = cache.resolve_ident(":db/ident");
            let _ = cache.get_ident(1);
            let _ = cache.get_attribute(1);
        }

        // Verify cache still works
        assert_eq!(cache.resolve_ident(":db/ident"), Some(1));
    }

    /// Test cache behavior across schema transactions
    #[pg_test]
    fn test_cache_across_transactions() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        let cache = crate::cache::get_cache();

        // Transaction 1: Add attribute
        let tx1 = r#"[{:db/ident :person/email
                       :db/valueType :db.type/string
                       :db/cardinality :db.cardinality/one
                       :db/unique :db.unique/identity}]"#;

        Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_transact('{}')",
            tx1.replace('\'', "''")
        ))
        .expect("Transaction 1 should succeed");

        // Verify attribute is resolvable
        let entid1 = cache.resolve_ident(":person/email");
        assert!(entid1.is_some(), "Should resolve after first transaction");

        // Transaction 2: Add another attribute
        let tx2 = r#"[{:db/ident :person/age
                       :db/valueType :db.type/long
                       :db/cardinality :db.cardinality/one}]"#;

        Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_transact('{}')",
            tx2.replace('\'', "''")
        ))
        .expect("Transaction 2 should succeed");

        // Both attributes should be resolvable
        let entid2 = cache.resolve_ident(":person/age");
        assert!(entid2.is_some(), "Should resolve after second transaction");

        // First attribute should still be cached
        let entid1_again = cache.resolve_ident(":person/email");
        assert_eq!(
            entid1, entid1_again,
            "First attribute should remain accessible"
        );
    }

    /// Test that repeated attribute resolution is instant (no DB queries after warming)
    #[pg_test]
    fn test_repeated_resolution_performance() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT mentat.bootstrap_schema()").expect("Failed to bootstrap schema");

        // Define a few attributes
        let tx = r#"[{:db/ident :perf/attr1
                      :db/valueType :db.type/string
                      :db/cardinality :db.cardinality/one}
                     {:db/ident :perf/attr2
                      :db/valueType :db.type/long
                      :db/cardinality :db.cardinality/one}
                     {:db/ident :perf/attr3
                      :db/valueType :db.type/boolean
                      :db/cardinality :db.cardinality/many}]"#;

        Spi::get_one::<String>(&format!(
            "SELECT mentat.mentat_transact('{}')",
            tx.replace('\'', "''")
        ))
        .expect("Transaction should succeed");

        let cache = crate::cache::get_cache();

        // First access warms the cache
        let e1 = cache.resolve_ident(":perf/attr1");
        assert!(e1.is_some());

        // 1000 repeated lookups should all be in-memory
        for _ in 0..1000 {
            assert!(cache.resolve_ident(":perf/attr1").is_some());
            assert!(cache.resolve_ident(":perf/attr2").is_some());
            assert!(cache.resolve_ident(":perf/attr3").is_some());
            assert!(cache.get_attribute(e1.unwrap()).is_some());
        }
    }
}
