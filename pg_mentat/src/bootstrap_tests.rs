// Bootstrap tests: verifying the bootstrap_schema() function,
// initial state, and core system attributes.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    // ========================================================================
    // Bootstrap function (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_bs_bootstrap_succeeds() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap should succeed");
    }

    #[pg_test]
    fn test_bs_bootstrap_idempotent() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("first");
        Spi::run("SELECT bootstrap_schema()").expect("second");
    }

    #[pg_test]
    fn test_bs_bootstrap_triple_call() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("first");
        Spi::run("SELECT bootstrap_schema()").expect("second");
        Spi::run("SELECT bootstrap_schema()").expect("third");
    }

    #[pg_test]
    fn test_bs_schema_returns_json() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        let _: serde_json::Value = serde_json::from_str(&s).expect("valid JSON");
    }

    #[pg_test]
    fn test_bs_schema_has_db_ident() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("db/ident"));
    }

    #[pg_test]
    fn test_bs_schema_has_db_type() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("db/valueType"));
    }

    #[pg_test]
    fn test_bs_schema_has_db_cardinality() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("db/cardinality"));
    }

    #[pg_test]
    fn test_bs_idents_table_exists() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.idents").expect("q").expect("NULL");
        assert!(count > 0);
    }

    #[pg_test]
    fn test_bs_datoms_table_exists() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.datoms").expect("q").expect("NULL");
        assert!(count >= 0);
    }

    #[pg_test]
    fn test_bs_transactions_table_exists() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.transactions").expect("q").expect("NULL");
        assert!(count >= 0);
    }

    // ========================================================================
    // System idents (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_bs_ident_db_ident_exists() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.idents WHERE ident = ':db/ident'").expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_bs_ident_db_value_type_exists() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.idents WHERE ident = ':db/valueType'").expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_bs_ident_db_cardinality_exists() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.idents WHERE ident = ':db/cardinality'").expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_bs_ident_db_unique_exists() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.idents WHERE ident = ':db/unique'").expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_bs_ident_db_doc_exists() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.idents WHERE ident = ':db/doc'").expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_bs_ident_db_tx_instant_exists() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        let count = Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.idents WHERE ident = ':db/txInstant'").expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    // ========================================================================
    // Post-bootstrap transact (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_bs_transact_after_bootstrap() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :bs/test :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema tx");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :bs/test \"hello\"]]'::TEXT)").expect("data tx");
    }

    #[pg_test]
    fn test_bs_query_after_bootstrap() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :bs/q :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :bs/q \"test\"]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :bs/q ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("query").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "test");
    }

    #[pg_test]
    fn test_bs_pull_after_bootstrap() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :bs/p :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :bs/p \"pull-test\"]]'::TEXT)",
        ).expect("data").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let p = Spi::get_one::<String>(&format!(
            "SELECT mentat_pull('{}', '[:bs/p]')::TEXT", eid
        )).expect("pull").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&p).expect("parse");
        assert_eq!(v[":bs/p"].as_str().expect("s"), "pull-test");
    }

    #[pg_test]
    fn test_bs_schema_func_after_user_attrs() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :bs/user :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("bs/user"));
        assert!(s.contains("db/ident"));
    }

    #[pg_test]
    fn test_bs_10_txs_after_bootstrap() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :bs/seq :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        for i in 0..10 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add \"e{i}\" :bs/seq {i}]]'::TEXT)", i = i)).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :bs/seq ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("query").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }
}
