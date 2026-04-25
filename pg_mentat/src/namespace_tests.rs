// Namespace tests: systematic testing of attribute namespace organization,
// cross-namespace operations, and namespace-based queries.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod namespace_tests {
    use pgrx::prelude::*;

    fn setup() {
        Spi::run("SELECT mentat.bootstrap_schema()").expect("bootstrap_schema failed");
    }

    // ========================================================================
    // Namespace creation (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_ns_single_namespace() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :ns.test/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("ns.test/name"));
    }

    #[pg_test]
    fn test_ns_two_namespaces() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :user/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/id \"b\" :db/ident :product/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("user/name"));
        assert!(s.contains("product/name"));
    }

    #[pg_test]
    fn test_ns_five_namespaces() {
        setup();
        let nss = vec!["alpha", "beta", "gamma", "delta", "epsilon"];
        for ns in &nss {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"a\" :db/ident :{}/attr :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}]'::TEXT)", ns
            )).expect("schema");
        }
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        for ns in &nss {
            assert!(s.contains(&format!("{}/attr", ns)));
        }
    }

    #[pg_test]
    fn test_ns_deep_namespace() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :ns.deep.nested.path/attr :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("ns.deep.nested.path/attr"));
    }

    #[pg_test]
    fn test_ns_hyphenated_namespace() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :my-app/user-name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        assert!(s.contains("my-app/user-name"));
    }

    // ========================================================================
    // Cross-namespace data operations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_ns_cross_ns_entity() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/id \"b\" :db/ident :person/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one} {:db/id \"c\" :db/ident :job/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :person/name \"Alice\" :person/age 30 :job/title \"Engineer\"}]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?a ?t :where [?e :person/name ?n] [?e :person/age ?a] [?e :job/title ?t]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_ns_query_filter_by_ns() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :dept/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/id \"b\" :db/ident :dept/budget :db/valueType :db.type/long :db/cardinality :db.cardinality/one} {:db/id \"c\" :db/ident :emp/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} {:db/id \"d\" :db/ident :emp/dept :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[{:db/id \"d1\" :dept/name \"Engineering\" :dept/budget 1000000} {:db/id \"d2\" :dept/name \"Sales\" :dept/budget 500000} {:db/id \"e1\" :emp/name \"Alice\" :emp/dept \"d1\"} {:db/id \"e2\" :emp/name \"Bob\" :emp/dept \"d2\"}]'::TEXT)").expect("data");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?en ?dn :where [?e :emp/name ?en] [?e :emp/dept ?d] [?d :dept/name ?dn]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 2);
    }

    #[pg_test]
    fn test_ns_multiple_attrs_per_ns() {
        setup();
        let attrs = vec![
            ("item", "name", "string"), ("item", "price", "long"),
            ("item", "weight", "double"), ("item", "active", "boolean"),
            ("item", "category", "keyword"),
        ];
        let mut ops = Vec::new();
        for (i, (ns, name, vtype)) in attrs.iter().enumerate() {
            ops.push(format!(
                "{{:db/id \"a{i}\" :db/ident :{ns}/{name} :db/valueType :db.type/{vtype} :db/cardinality :db.cardinality/one}}",
                i = i, ns = ns, name = name, vtype = vtype
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("schema");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :item/name \"Widget\" :item/price 999 :item/weight 2.5 :item/active true :item/category :electronics}]'::TEXT)").expect("data");

        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n ?p ?w ?a ?c :where [?e :item/name ?n] [?e :item/price ?p] [?e :item/weight ?w] [?e :item/active ?a] [?e :item/category ?c]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_ns_same_attr_name_different_ns() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :ns.a/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one} {:db/id \"b\" :db/ident :ns.b/val :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[{:db/id \"ea\" :ns.a/val 10} {:db/id \"eb\" :ns.b/val 20}]'::TEXT)").expect("data");
        let qa = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :ns.a/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let va: serde_json::Value = serde_json::from_str(&qa).expect("parse");
        assert_eq!(va["result"].as_array().expect("arr").len(), 1);

        let qb = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :ns.b/val ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let vb: serde_json::Value = serde_json::from_str(&qb).expect("parse");
        assert_eq!(vb["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_ns_10_attrs_same_ns() {
        setup();
        let mut ops = Vec::new();
        for i in 0..10 {
            ops.push(format!(
                "{{:db/id \"a{i}\" :db/ident :wide/attr-{i} :db/valueType :db.type/string :db/cardinality :db.cardinality/one}}", i = i
            ));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("schema");
        let s = Spi::get_one::<String>("SELECT mentat_schema()::TEXT").expect("schema").expect("NULL");
        for i in 0..10 {
            assert!(s.contains(&format!("wide/attr-{}", i)));
        }
    }

    // ========================================================================
    // Namespace with special characters (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_ns_numeric_suffix() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :ns123/attr :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :ns123/attr \"test\"]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [_ :ns123/attr ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "test");
    }

    #[pg_test]
    fn test_ns_underscore_style() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :my-ns/my-attr :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :my-ns/my-attr 42]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [_ :my-ns/my-attr ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 42);
    }

    #[pg_test]
    fn test_ns_short_name() {
        setup();
        Spi::run("SELECT mentat_transact('[{:db/id \"a\" :db/ident :x/y :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'::TEXT)").expect("schema");
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :x/y \"short\"]]'::TEXT)").expect("data");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [_ :x/y ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "short");
    }

    // ========================================================================
    // Namespace organization patterns (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_ns_domain_model_pattern() {
        setup();
        // Model: user -> order -> item
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :user/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            {:db/id \"b\" :db/ident :user/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            {:db/id \"c\" :db/ident :order/date :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            {:db/id \"d\" :db/ident :order/user :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
            {:db/id \"e\" :db/ident :order/total :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            {:db/id \"f\" :db/ident :item/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            {:db/id \"g\" :db/ident :item/price :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
            {:db/id \"h\" :db/ident :order/items :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
        ]'::TEXT)").expect("schema");

        // Create domain data
        Spi::run("SELECT mentat_transact('[
            {:db/id \"u\" :user/name \"Alice\" :user/email \"alice@test.com\"}
            {:db/id \"i1\" :item/name \"Widget\" :item/price 10}
            {:db/id \"i2\" :item/name \"Gadget\" :item/price 25}
            {:db/id \"o\" :order/date \"2024-01-15\" :order/user \"u\" :order/total 60 :order/items \"i1\" :order/items \"i2\"}
        ]'::TEXT)").expect("data");

        // Cross-namespace query
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?un ?od ?ot :where [?o :order/user ?u] [?u :user/name ?un] [?o :order/date ?od] [?o :order/total ?ot]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_ns_tag_taxonomy_pattern() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :tag/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            {:db/id \"b\" :db/ident :tag/parent :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
            {:db/id \"c\" :db/ident :article/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            {:db/id \"d\" :db/ident :article/tags :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
        ]'::TEXT)").expect("schema");

        Spi::run("SELECT mentat_transact('[
            {:db/id \"t1\" :tag/name \"technology\"}
            {:db/id \"t2\" :tag/name \"programming\" :tag/parent \"t1\"}
            {:db/id \"t3\" :tag/name \"rust\" :tag/parent \"t2\"}
            {:db/id \"a1\" :article/title \"Learning Rust\" :article/tags \"t3\"}
        ]'::TEXT)").expect("data");

        // Navigate tag hierarchy
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?top . :where [?a :article/title \"Learning Rust\"] [?a :article/tags ?t] [?t :tag/parent ?p] [?p :tag/parent ?tp] [?tp :tag/name ?top]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "technology");
    }

    #[pg_test]
    fn test_ns_event_sourcing_pattern() {
        setup();
        Spi::run("SELECT mentat_transact('[
            {:db/id \"a\" :db/ident :event/type :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
            {:db/id \"b\" :db/ident :event/entity :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
            {:db/id \"c\" :db/ident :event/data :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            {:db/id \"d\" :db/ident :account/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            {:db/id \"e\" :db/ident :account/balance :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
        ]'::TEXT)").expect("schema");

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"acct\" :account/name \"Alice\" :account/balance 100}]'::TEXT)"
        ).expect("create").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let acct = j["tempids"]["acct"].as_i64().expect("acct");

        // Record events
        Spi::run(&format!(
            "SELECT mentat_transact('[{{:db/id \"ev1\" :event/type :deposit :event/entity {} :event/data \"50\"}}]'::TEXT)", acct
        )).expect("event");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :account/balance 150]]'::TEXT)", acct)).expect("update");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :account/balance ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", acct
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 150);
    }
}
