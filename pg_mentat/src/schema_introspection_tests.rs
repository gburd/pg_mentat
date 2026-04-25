// Schema introspection tests: verifying mentat_schema() output,
// attribute metadata, and schema querying capabilities.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod schema_introspection_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    // ========================================================================
    // Bootstrap schema introspection (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_si_bootstrap_has_db_ident() {
        setup();
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("db/ident"));
    }

    #[pg_test]
    fn test_si_bootstrap_has_db_value_type() {
        setup();
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("db/valueType"));
    }

    #[pg_test]
    fn test_si_bootstrap_has_db_cardinality() {
        setup();
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("db/cardinality"));
    }

    #[pg_test]
    fn test_si_bootstrap_has_db_unique() {
        setup();
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("db/unique"));
    }

    #[pg_test]
    fn test_si_bootstrap_has_db_doc() {
        setup();
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("db/doc"));
    }

    #[pg_test]
    fn test_si_bootstrap_has_tx_instant() {
        setup();
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("db/txInstant"));
    }

    #[pg_test]
    fn test_si_bootstrap_is_valid_json() {
        setup();
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        let _: serde_json::Value = serde_json::from_str(&s).expect("should be valid JSON");
    }

    #[pg_test]
    fn test_si_schema_not_empty() {
        setup();
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.len() > 10);
    }

    // ========================================================================
    // User-defined attribute introspection (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_si_user_attr_visible() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("si/name"));
    }

    #[pg_test]
    fn test_si_multiple_user_attrs_visible() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si/attr1 :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/id \"b\" :db/ident :si/attr2 :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("si/attr1"));
        assert!(s.contains("si/attr2"));
    }

    #[pg_test]
    fn test_si_all_types_visible() {
        setup();
        let types = vec![
            ("si-s", "string"), ("si-l", "long"), ("si-d", "double"),
            ("si-b", "boolean"), ("si-k", "keyword"), ("si-r", "ref"),
            ("si-i", "instant"), ("si-u", "uuid"),
        ];
        for (name, vtype) in &types {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"a\" :db/ident :si.t/{} :db/valueType :db.type/{} :db/cardinality :db.cardinality/one}}]'::TEXT)",
                name, vtype
            )).expect("add attr");
        }
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        for (name, _) in &types {
            assert!(s.contains(&format!("si.t/{}", name)), "Missing {}", name);
        }
    }

    #[pg_test]
    fn test_si_unique_identity_visible() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("si/email"));
    }

    #[pg_test]
    fn test_si_unique_value_visible() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si/code :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/value}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("si/code"));
    }

    #[pg_test]
    fn test_si_cardinality_many_visible() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si/tags :db/valueType :db.type/string :db/cardinality :db.cardinality/many}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("si/tags"));
    }

    #[pg_test]
    fn test_si_20_attrs_visible() {
        setup();
        let mut ops = Vec::new();
        for i in 0..20 {
            ops.push(format!(
                "{{:db/id \"a{i}\" :db/ident :si.bulk/attr-{i} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}", i = i
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        for i in 0..20 {
            assert!(s.contains(&format!("si.bulk/attr-{}", i)), "Missing attr-{}", i);
        }
    }

    #[pg_test]
    fn test_si_schema_after_data() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si/data-name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :si/data-name \"test\"]]'::TEXT)").expect("data");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("si/data-name"));
    }

    #[pg_test]
    fn test_si_schema_multiple_namespaces() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :user/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/id \"b\" :db/ident :product/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/id \"c\" :db/ident :order/total :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("user/name"));
        assert!(s.contains("product/name"));
        assert!(s.contains("order/total"));
    }

    // ========================================================================
    // Idents table queries (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_si_idents_table_has_db_ident() {
        setup();
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.idents WHERE ident = ':db/ident'"
        ).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_si_idents_table_has_user_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si.idents/test :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.idents WHERE ident = ':si.idents/test'"
        ).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_si_idents_entid_positive() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si.idents/pos :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let entid = Spi::get_one::<i64>(
            "SELECT entid FROM mentat.idents WHERE ident = ':si.idents/pos'"
        ).expect("q").expect("NULL");
        assert!(entid > 0);
    }

    #[pg_test]
    fn test_si_idents_unique_per_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si.idents/uniq1 :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/id \"b\" :db/ident :si.idents/uniq2 :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let e1 = Spi::get_one::<i64>("SELECT entid FROM mentat.idents WHERE ident = ':si.idents/uniq1'").expect("q").expect("NULL");
        let e2 = Spi::get_one::<i64>("SELECT entid FROM mentat.idents WHERE ident = ':si.idents/uniq2'").expect("q").expect("NULL");
        assert_ne!(e1, e2);
    }

    // ========================================================================
    // Schema idempotency (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_si_schema_stable_across_calls() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si.stable/attr :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let s1 = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        let s2 = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert_eq!(s1, s2);
    }

    #[pg_test]
    fn test_si_schema_stable_10x() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si.s10/attr :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let mut results = Vec::new();
        for _ in 0..10 {
            let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
            results.push(s);
        }
        for i in 1..10 {
            assert_eq!(results[0], results[i], "Schema call {} differs", i);
        }
    }

    #[pg_test]
    fn test_si_schema_grows_with_new_attrs() {
        setup();
        let s1 = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :si.grow/new :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let s2 = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s2.len() >= s1.len());
        assert!(s2.contains("si.grow/new"));
    }
}
