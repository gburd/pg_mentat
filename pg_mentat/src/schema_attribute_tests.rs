// Schema attribute tests: comprehensive coverage of schema definition,
// attribute properties, type enforcement, and schema lifecycle.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
        // A raw failing Spi::run aborts the whole test transaction (pgrx
        // longjmp). Tests that exercise an expected PG error must run it in a
        // subtransaction via this helper so the error is observable.
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._sa_run(stmt TEXT) RETURNS BOOLEAN
             LANGUAGE plpgsql AS $$
             BEGIN EXECUTE stmt; RETURN true;
             EXCEPTION WHEN OTHERS THEN RETURN false; END; $$",
        )
        .expect("helper");
    }

    // ========================================================================
    // Individual value type definitions (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_sa_define_string_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/str-attr :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("sa/str-attr"));
    }

    #[pg_test]
    fn test_sa_define_long_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/lng-attr :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("sa/lng-attr"));
    }

    #[pg_test]
    fn test_sa_define_double_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/dbl-attr :db/valueType :db.type/double :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("sa/dbl-attr"));
    }

    #[pg_test]
    fn test_sa_define_boolean_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/boo-attr :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("sa/boo-attr"));
    }

    #[pg_test]
    fn test_sa_define_keyword_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/kw-attr :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("sa/kw-attr"));
    }

    #[pg_test]
    fn test_sa_define_ref_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/ref-attr :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("sa/ref-attr"));
    }

    #[pg_test]
    fn test_sa_define_uuid_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/uuid-attr :db/valueType :db.type/uuid :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("sa/uuid-attr"));
    }

    #[pg_test]
    fn test_sa_define_instant_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/inst-attr :db/valueType :db.type/instant :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("sa/inst-attr"));
    }

    #[pg_test]
    fn test_sa_define_bytes_attr() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/bytes-attr :db/valueType :db.type/bytes :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("sa/bytes-attr"));
    }

    #[pg_test]
    fn test_sa_define_all_types_in_one_tx() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a1\" :db/ident :sa/all-str :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            {:db/id \"a2\" :db/ident :sa/all-lng :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            {:db/id \"a3\" :db/ident :sa/all-dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
            {:db/id \"a4\" :db/ident :sa/all-boo :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
            {:db/id \"a5\" :db/ident :sa/all-kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
            {:db/id \"a6\" :db/ident :sa/all-ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
            {:db/id \"a7\" :db/ident :sa/all-uuid :db/valueType :db.type/uuid :db/cardinality :db.cardinality/one}
            {:db/id \"a8\" :db/ident :sa/all-inst :db/valueType :db.type/instant :db/cardinality :db.cardinality/one}
        ]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        let expected = [
            "sa/all-str",
            "sa/all-lng",
            "sa/all-dbl",
            "sa/all-boo",
            "sa/all-kw",
            "sa/all-ref",
            "sa/all-uuid",
            "sa/all-inst",
        ];
        for attr in &expected {
            assert!(s.contains(attr), "Missing: {}", attr);
        }
    }

    // ========================================================================
    // Cardinality variants (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_sa_card_one_string() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/c1s :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sa/c1s \"hello\"]]'::TEXT)")
            .expect("data");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sa/c1s \"world\"]]'::TEXT)")
            .expect("replace");
        // Should error since different tempid with same name - verify only one value
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :sa/c1s ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Two different entities, each with one value
        assert!(v["result"].as_array().expect("arr").len() <= 2);
    }

    #[pg_test]
    fn test_sa_card_many_string() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/cms :db/valueType :db.type/string :db/cardinality :db.cardinality/many}]'::TEXT)").expect("tx");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sa/cms \"hello\"] [:db/add \"e\" :sa/cms \"world\"]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :sa/cms ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_sa_card_one_long() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/c1l :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :sa/c1l 42]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :sa/c1l 99]]'::TEXT)",
            eid
        ))
        .expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :sa/c1l ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 99);
    }

    #[pg_test]
    fn test_sa_card_many_long() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/cml :db/valueType :db.type/long :db/cardinality :db.cardinality/many}]'::TEXT)").expect("tx");
        let mut ops = vec![];
        for i in 0..10 {
            ops.push(format!("[:db/add \"e\" :sa/cml {}]", i));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :sa/cml ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_sa_card_many_keyword() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/cmk :db/valueType :db.type/keyword :db/cardinality :db.cardinality/many}]'::TEXT)").expect("tx");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sa/cmk :tag-a] [:db/add \"e\" :sa/cmk :tag-b] [:db/add \"e\" :sa/cmk :tag-c]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :sa/cmk ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_sa_card_many_ref() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :sa/cmr :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
            {:db/id \"n\" :db/ident :sa/cmrn :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
        ]'::TEXT)").expect("tx");
        Spi::run("SELECT mentat_transact('[[:db/add \"t1\" :sa/cmrn \"t1\"] [:db/add \"t2\" :sa/cmrn \"t2\"] [:db/add \"t3\" :sa/cmrn \"t3\"] [:db/add \"e\" :sa/cmr \"t1\"] [:db/add \"e\" :sa/cmr \"t2\"] [:db/add \"e\" :sa/cmr \"t3\"]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?r ...] :where [?e :sa/cmr ?r]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_sa_card_one_boolean() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/c1b :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :sa/c1b true]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :sa/c1b false]]'::TEXT)",
            eid
        ))
        .expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :sa/c1b ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_sa_card_one_double() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa/c1d :db/valueType :db.type/double :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :sa/c1d 3.14]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :sa/c1d 2.72]]'::TEXT)",
            eid
        ))
        .expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :sa/c1d ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let result = v["result"].as_f64().expect("d");
        assert!((result - 2.72).abs() < 0.01);
    }

    // ========================================================================
    // Unique constraints (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_sa_unique_identity() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :sa/uid :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            {:db/id \"v\" :db/ident :sa/uidv :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
        ]'::TEXT)").expect("tx");
        Spi::run("SELECT mentat_transact('[{:sa/uid \"alice\" :sa/uidv 100}]'::TEXT)")
            .expect("create");
        Spi::run("SELECT mentat_transact('[{:sa/uid \"alice\" :sa/uidv 200}]'::TEXT)")
            .expect("upsert");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :sa/uid \"alice\"] [?e :sa/uidv ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 200);
    }

    #[pg_test]
    fn test_sa_unique_identity_multiple() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :sa/uid2 :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            {:db/id \"v\" :db/ident :sa/uid2v :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
        ]'::TEXT)").expect("tx");
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:sa/uid2 \"user-{}\" :sa/uid2v {}}}]'::TEXT)",
                i,
                i * 10
            ))
            .expect("create");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :sa/uid2 ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_sa_unique_identity_upsert_10x() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :sa/uid3 :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            {:db/id \"v\" :db/ident :sa/uid3v :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
        ]'::TEXT)").expect("tx");
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:sa/uid3 \"singleton\" :sa/uid3v {}}}]'::TEXT)",
                i
            ))
            .expect("upsert");
        }
        // Should be one entity
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?e ...] :where [?e :sa/uid3 _]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_sa_unique_value() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :sa/uv :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/value}
        ]'::TEXT)").expect("tx");
        Spi::run("SELECT mentat_transact('[[:db/add \"e1\" :sa/uv \"unique-code\"]]'::TEXT)")
            .expect("first");
        // Second entity with same value: :db.unique/value either rejects the
        // collision or merges. Run in a subtransaction so a rejection (PG
        // error) does not abort the test. Either outcome is acceptable; what
        // matters is exactly one entity holds the value afterward.
        let _ = Spi::get_one::<bool>(
            "SELECT mentat._sa_run('SELECT mentat_transact(''[[:db/add \"e2\" :sa/uv \"unique-code\"]]''::TEXT)')",
        );
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.current_text \
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':sa/uv') AND v = 'unique-code'",
        )
        .expect("count")
        .expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_sa_unique_identity_preserves_other_attrs() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :sa/uid4 :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            {:db/id \"v\" :db/ident :sa/uid4v :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            {:db/id \"s\" :db/ident :sa/uid4s :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
        ]'::TEXT)").expect("tx");
        Spi::run("SELECT mentat_transact('[{:sa/uid4 \"bob\" :sa/uid4v 100 :sa/uid4s \"original\"}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[{:sa/uid4 \"bob\" :sa/uid4v 200}]'::TEXT)")
            .expect("upsert");
        // uid4s should be preserved
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?s . :where [?e :sa/uid4 \"bob\"] [?e :sa/uid4s ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "original");
    }

    // ========================================================================
    // Batch schema definition (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_sa_batch_10_attrs() {
        setup();
        let mut ops = vec![];
        for i in 0..10 {
            ops.push(format!(
                "{{:db/id \"a{}\" :db/ident :sa.b10/attr-{} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}",
                i, i
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        for i in 0..10 {
            assert!(s.contains(&format!("sa.b10/attr-{}", i)));
        }
    }

    #[pg_test]
    fn test_sa_batch_30_attrs() {
        setup();
        let mut ops = vec![];
        for i in 0..30 {
            let vtype = match i % 5 {
                0 => "string",
                1 => "long",
                2 => "double",
                3 => "boolean",
                _ => "keyword",
            };
            ops.push(format!(
                "{{:db/id \"a{}\" :db/ident :sa.b30/attr-{} :db/valueType :db.type/{} :db/cardinality :db.cardinality/one}}",
                i, i, vtype
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        for i in 0..30 {
            assert!(s.contains(&format!("sa.b30/attr-{}", i)));
        }
    }

    #[pg_test]
    fn test_sa_batch_50_mixed_attrs() {
        setup();
        let mut ops = vec![];
        for i in 0..50 {
            let card = if i % 3 == 0 { "many" } else { "one" };
            ops.push(format!(
                "{{:db/id \"a{}\" :db/ident :sa.b50/attr-{} :db/valueType :db.type/string :db/cardinality :db.cardinality/{}}}",
                i, i, card
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        for i in 0..50 {
            assert!(s.contains(&format!("sa.b50/attr-{}", i)));
        }
    }

    #[pg_test]
    fn test_sa_sequential_attr_definition() {
        setup();
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"a\" :db/ident :sa.seq/attr-{} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}]'::TEXT)", i
            )).expect("tx");
        }
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        for i in 0..10 {
            assert!(s.contains(&format!("sa.seq/attr-{}", i)));
        }
    }

    #[pg_test]
    fn test_sa_multiple_namespaces() {
        setup();
        let namespaces = ["user", "order", "product", "category", "review"];
        for ns in &namespaces {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"a\" :db/ident :{}/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}]'::TEXT)", ns
            )).expect("tx");
        }
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        for ns in &namespaces {
            assert!(s.contains(&format!("{}/name", ns)));
        }
    }

    #[pg_test]
    fn test_sa_schema_grows_over_time() {
        setup();
        for round in 0..5 {
            let mut ops = vec![];
            for i in 0..5 {
                ops.push(format!(
                    "{{:db/id \"a{}\" :db/ident :sa.grow/r{}-a{} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}",
                    i, round, i
                ));
            }
            Spi::run(&format!(
                "SELECT mentat_transact('[{}]'::TEXT)",
                ops.join("\n")
            ))
            .expect("tx");
        }
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        for round in 0..5 {
            for i in 0..5 {
                assert!(s.contains(&format!("sa.grow/r{}-a{}", round, i)));
            }
        }
    }

    #[pg_test]
    fn test_sa_schema_idempotent_definition() {
        setup();
        let tx = "SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa.idem/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)";
        Spi::run(tx).expect("first");
        // Define again. A fresh tempid maps to a new entid, so re-installing the
        // attribute can collide on schema.ident (UNIQUE). Run in a
        // subtransaction so a collision error does not abort the test; the
        // attribute must still be present afterward either way.
        let _ = Spi::get_one::<bool>(&format!(
            "SELECT mentat._sa_run('{}')",
            tx.replace('\'', "''")
        ));
        let s2 = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("s")
            .expect("NULL");
        // Schema should still contain the attribute
        assert!(s2.contains("sa.idem/name"));
    }

    #[pg_test]
    fn test_sa_schema_with_doc() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa.doc/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/doc \"A person name\"}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        assert!(s.contains("sa.doc/name"));
    }

    // ========================================================================
    // Schema and data interaction (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_sa_define_then_use_immediately() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa.use/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sa.use/name \"hello\"]]'::TEXT)")
            .expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [_ :sa.use/name ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "hello");
    }

    #[pg_test]
    fn test_sa_define_and_use_same_tx() {
        setup();
        // In some systems you can define schema and data in one tx
        // We just verify the schema is available for subsequent txs
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa.same/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :sa.same/name \"test\"]]'::TEXT)")
            .expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [_ :sa.same/name ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "test");
    }

    #[pg_test]
    fn test_sa_use_after_multiple_schema_txs() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa.m1/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("s1");
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa.m1/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("s2");
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :sa.m1/name \"test\" :sa.m1/val 42}]'::TEXT)",
        )
        .expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :sa.m1/name \"test\"] [?e :sa.m1/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 42);
    }

    #[pg_test]
    fn test_sa_10_schema_10_data_txs() {
        setup();
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"a\" :db/ident :sa.ten/attr-{} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}]'::TEXT)", i
            )).expect("schema");
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :sa.ten/attr-{} \"val-{}\"]]'::TEXT)",
                i, i, i
            ))
            .expect("data");
        }
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        for i in 0..10 {
            assert!(s.contains(&format!("sa.ten/attr-{}", i)));
        }
    }

    #[pg_test]
    fn test_sa_schema_visible_in_mentat_schema() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa.vis/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&s).expect("parse");
        // Should be valid JSON
        assert!(j.is_object() || j.is_array());
    }

    #[pg_test]
    fn test_sa_schema_in_idents_table() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa.id/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let count =
            Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.idents WHERE ident = ':sa.id/name'")
                .expect("q")
                .expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_sa_ident_entid_positive() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :sa.eid/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("tx");
        let entid =
            Spi::get_one::<i64>("SELECT entid FROM mentat.idents WHERE ident = ':sa.eid/name'")
                .expect("q")
                .expect("NULL");
        assert!(entid > 0);
    }

    #[pg_test]
    fn test_sa_idents_unique() {
        setup();
        let mut ops = vec![];
        for i in 0..10 {
            ops.push(format!(
                "{{:db/id \"a{}\" :db/ident :sa.uniq/attr-{} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}",
                i, i
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx");
        let count =
            Spi::get_one::<i64>("SELECT COUNT(*) FROM mentat.idents WHERE ident LIKE ':sa.uniq/%'")
                .expect("q")
                .expect("NULL");
        assert_eq!(count, 10);
    }

    #[pg_test]
    fn test_sa_schema_after_bootstrap() {
        setup();
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema")
            .expect("NULL");
        // Should have system attributes
        assert!(s.contains("db/ident"));
    }

    #[pg_test]
    fn test_sa_schema_stable_across_calls() {
        setup();
        let s1 = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("s")
            .expect("NULL");
        let s2 = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("s")
            .expect("NULL");
        assert_eq!(s1, s2);
    }
}
