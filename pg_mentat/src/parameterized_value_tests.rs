// Parameterized value tests: systematic coverage of value operations
// across types, cardinalities, and operation modes.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_pv_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"s1\" :db/ident :pv/str :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"s2\" :db/ident :pv/strs :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"l1\" :db/ident :pv/lng :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"l2\" :db/ident :pv/lngs :db/valueType :db.type/long :db/cardinality :db.cardinality/many}
                {:db/id \"d1\" :db/ident :pv/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"d2\" :db/ident :pv/dbls :db/valueType :db.type/double :db/cardinality :db.cardinality/many}
                {:db/id \"b1\" :db/ident :pv/boo :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k1\" :db/ident :pv/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"k2\" :db/ident :pv/kws :db/valueType :db.type/keyword :db/cardinality :db.cardinality/many}
                {:db/id \"r1\" :db/ident :pv/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"r2\" :db/ident :pv/refs :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"u1\" :db/ident :pv/uid :db/valueType :db.type/uuid :db/cardinality :db.cardinality/one}
                {:db/id \"i1\" :db/ident :pv/inst :db/valueType :db.type/instant :db/cardinality :db.cardinality/one}
                {:db/id \"n1\" :db/ident :pv/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
            ]'::TEXT)",
        ).expect("pv schema");
    }

    // ========================================================================
    // String value operations (20 tests)
    // ========================================================================

    #[pg_test]
    fn test_pv_str_empty() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "");
    }

    #[pg_test]
    fn test_pv_str_single_char() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"x\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "x");
    }

    #[pg_test]
    fn test_pv_str_spaces() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"  hello  world  \"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "  hello  world  ");
    }

    #[pg_test]
    fn test_pv_str_newlines() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"line1\\nline2\\nline3\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("line1"));
    }

    #[pg_test]
    fn test_pv_str_unicode_emoji() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"hello 🌍\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("🌍"));
    }

    #[pg_test]
    fn test_pv_str_unicode_cjk() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"日本語テスト\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "日本語テスト");
    }

    #[pg_test]
    fn test_pv_str_unicode_arabic() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"مرحبا\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "مرحبا");
    }

    #[pg_test]
    fn test_pv_str_replace_shorter() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"long string here\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/str \"short\"]]'::TEXT)", eid)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "short");
    }

    #[pg_test]
    fn test_pv_str_replace_longer() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"hi\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let long_str = "a".repeat(5000);
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/str \"{}\"]]'::TEXT)", eid, long_str)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s").len(), 5000);
    }

    #[pg_test]
    fn test_pv_str_retract_then_query() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"gone\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :pv/str \"gone\"]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_pv_strs_many_accumulate_5() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"holder\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 0..5 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/strs \"tag-{}\"]]'::TEXT)", eid, i)).expect("add");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :pv/strs ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_pv_strs_many_retract_one() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :pv/str \"h\"] [:db/add \"e\" :pv/strs \"a\"] [:db/add \"e\" :pv/strs \"b\"] [:db/add \"e\" :pv/strs \"c\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :pv/strs \"b\"]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :pv/strs ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let arr = v["result"].as_array().expect("arr");
        assert_eq!(arr.len(), 2);
    }

    // ========================================================================
    // Long value operations (20 tests)
    // ========================================================================

    #[pg_test]
    fn test_pv_lng_zero() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/lng 0]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 0);
    }

    #[pg_test]
    fn test_pv_lng_positive_1() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/lng 1]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 1);
    }

    #[pg_test]
    fn test_pv_lng_negative_1() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/lng -1]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), -1);
    }

    #[pg_test]
    fn test_pv_lng_i32_max() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/lng 2147483647]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 2147483647);
    }

    #[pg_test]
    fn test_pv_lng_i32_min() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/lng -2147483648]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), -2147483648);
    }

    #[pg_test]
    fn test_pv_lng_large_positive() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/lng 999999999999]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 999999999999);
    }

    #[pg_test]
    fn test_pv_lng_large_negative() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/lng -999999999999]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), -999999999999);
    }

    #[pg_test]
    fn test_pv_lng_replace_small_with_large() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/lng 1]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/lng 9999999]]'::TEXT)", eid)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 9999999);
    }

    #[pg_test]
    fn test_pv_lng_replace_positive_with_negative() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/lng 100]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/lng -100]]'::TEXT)", eid)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), -100);
    }

    #[pg_test]
    fn test_pv_lng_retract() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/lng 42]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :pv/lng 42]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_pv_lngs_many_accumulate() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"h\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 0..10 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/lngs {}]]'::TEXT)", eid, i * 100)).expect("add");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :pv/lngs ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_pv_lngs_many_retract_one() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :pv/str \"h\"] [:db/add \"e\" :pv/lngs 10] [:db/add \"e\" :pv/lngs 20] [:db/add \"e\" :pv/lngs 30]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :pv/lngs 20]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :pv/lngs ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 2);
    }

    // ========================================================================
    // Double value operations (15 tests)
    // ========================================================================

    #[pg_test]
    fn test_pv_dbl_zero() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/dbl 0.0]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/dbl ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") - 0.0).abs() < 0.001);
    }

    #[pg_test]
    fn test_pv_dbl_pi() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/dbl 3.14159265]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/dbl ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") - 3.14159265).abs() < 0.0001);
    }

    #[pg_test]
    fn test_pv_dbl_negative() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/dbl -273.15]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/dbl ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") - (-273.15)).abs() < 0.01);
    }

    #[pg_test]
    fn test_pv_dbl_very_small() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/dbl 0.000001]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/dbl ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") - 0.000001).abs() < 0.0000001);
    }

    #[pg_test]
    fn test_pv_dbl_very_large() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/dbl 999999999.999]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/dbl ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") - 999999999.999).abs() < 0.01);
    }

    #[pg_test]
    fn test_pv_dbl_replace() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/dbl 1.0]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/dbl 2.0]]'::TEXT)", eid)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") - 2.0).abs() < 0.001);
    }

    #[pg_test]
    fn test_pv_dbl_retract() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/dbl 5.5]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :pv/dbl 5.5]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_pv_dbls_many_accumulate() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"h\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 0..8 {
            // {:?} formats integral f64 with a decimal point (0 -> "0.0") so EDN
            // parses it as a double rather than an integer (type-check failure).
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/dbls {:?}]]'::TEXT)", eid, (i as f64) * 1.1)).expect("add");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :pv/dbls ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 8);
    }

    // ========================================================================
    // Boolean value operations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_pv_boo_true() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/boo true]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/boo ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), true);
    }

    #[pg_test]
    fn test_pv_boo_false() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/boo false]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/boo ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_pv_boo_toggle_true_to_false() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/boo true]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/boo false]]'::TEXT)", eid)).expect("toggle");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/boo ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_pv_boo_toggle_false_to_true() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/boo false]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/boo true]]'::TEXT)", eid)).expect("toggle");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/boo ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), true);
    }

    #[pg_test]
    fn test_pv_boo_retract_true() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/boo true]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :pv/boo true]]'::TEXT)", eid)).expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/boo ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_pv_boo_idempotent_true() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/boo true]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for _ in 0..5 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/boo true]]'::TEXT)", eid)).expect("idem");
        }
        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':pv/boo') AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    // ========================================================================
    // Keyword value operations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_pv_kw_simple() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/kw :active]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/kw ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("active"));
    }

    #[pg_test]
    fn test_pv_kw_namespaced() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/kw :status/active]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/kw ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("status/active"));
    }

    #[pg_test]
    fn test_pv_kw_replace() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/kw :draft]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/kw :published]]'::TEXT)", eid)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/kw ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("published"));
    }

    #[pg_test]
    fn test_pv_kws_many_accumulate() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :pv/str \"h\"]]'::TEXT)").expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/kws :tag-a] [:db/add {} :pv/kws :tag-b] [:db/add {} :pv/kws :tag-c]]'::TEXT)", eid, eid, eid)).expect("add");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :pv/kws ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    // ========================================================================
    // Ref value operations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_pv_ref_basic() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"parent\" :pv/str \"Parent\"] [:db/add \"child\" :pv/str \"Child\"] [:db/add \"child\" :pv/ref \"parent\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let parent = j["tempids"]["parent"].as_i64().expect("parent");
        let child = j["tempids"]["child"].as_i64().expect("child");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :pv/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", child
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("r"), parent);
    }

    #[pg_test]
    fn test_pv_ref_replace() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"a\" :pv/str \"A\"] [:db/add \"b\" :pv/str \"B\"] [:db/add \"c\" :pv/str \"C\"] [:db/add \"c\" :pv/ref \"a\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let b = j["tempids"]["b"].as_i64().expect("b");
        let c = j["tempids"]["c"].as_i64().expect("c");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/ref {}]]'::TEXT)", c, b)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :pv/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", c
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("r"), b);
    }

    #[pg_test]
    fn test_pv_refs_many_accumulate() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"hub\" :pv/str \"Hub\"] [:db/add \"s1\" :pv/str \"S1\"] [:db/add \"s2\" :pv/str \"S2\"] [:db/add \"s3\" :pv/str \"S3\"] [:db/add \"hub\" :pv/refs \"s1\"] [:db/add \"hub\" :pv/refs \"s2\"] [:db/add \"hub\" :pv/refs \"s3\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let hub = j["tempids"]["hub"].as_i64().expect("hub");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?r ...] :where [{} :pv/refs ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", hub
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_pv_ref_navigate_two_hops() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"gp\" :pv/str \"Grandparent\"] [:db/add \"p\" :pv/str \"Parent\"] [:db/add \"p\" :pv/ref \"gp\"] [:db/add \"c\" :pv/str \"Child\"] [:db/add \"c\" :pv/ref \"p\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let gp = j["tempids"]["gp"].as_i64().expect("gp");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?name . :where [?c :pv/str \"Child\"] [?c :pv/ref ?p] [?p :pv/ref ?gp] [?gp :pv/str ?name]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "Grandparent");
    }

    // ========================================================================
    // UUID operations (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_pv_uuid_basic() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/uid #uuid \"550e8400-e29b-41d4-a716-446655440000\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/uid ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("550e8400"));
    }

    #[pg_test]
    fn test_pv_uuid_replace() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :pv/uid #uuid \"550e8400-e29b-41d4-a716-446655440000\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :pv/uid #uuid \"660e8400-e29b-41d4-a716-446655440001\"]]'::TEXT)", eid
        )).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/uid ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("660e8400"));
    }

    // ========================================================================
    // Instant operations (5 tests)
    // ========================================================================

    #[pg_test]
    fn test_pv_inst_basic() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :pv/inst #inst \"2024-01-15T10:30:00.000Z\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/inst ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("2024"));
    }

    #[pg_test]
    fn test_pv_inst_replace() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :pv/inst #inst \"2024-01-01T00:00:00.000Z\"]]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :pv/inst #inst \"2025-06-15T12:00:00.000Z\"]]'::TEXT)", eid
        )).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/inst ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("2025"));
    }

    // ========================================================================
    // Upsert operations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_pv_upsert_create() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pv/name \"unique-1\" :pv/str \"hello\"}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e"].as_i64().is_some());
    }

    #[pg_test]
    fn test_pv_upsert_update() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :pv/name \"unique-2\" :pv/str \"v1\"}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :pv/name \"unique-2\" :pv/str \"v2\"}]'::TEXT)").expect("upsert");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/name \"unique-2\"] [?e :pv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "v2");
    }

    #[pg_test]
    fn test_pv_upsert_preserves_other_attrs() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :pv/name \"unique-3\" :pv/str \"hello\" :pv/lng 42}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[{:db/id \"e\" :pv/name \"unique-3\" :pv/str \"updated\"}]'::TEXT)").expect("upsert");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :pv/name \"unique-3\"] [?e :pv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 42);
    }

    #[pg_test]
    fn test_pv_upsert_5x_same() {
        setup(); setup_pv_schema();
        for _ in 0..5 {
            Spi::run("SELECT mentat_transact('[{:db/id \"e\" :pv/name \"unique-4\" :pv/lng 99}]'::TEXT)").expect("upsert");
        }
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':pv/name') AND v_text = 'unique-4' AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_pv_upsert_two_different() {
        setup(); setup_pv_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e1\" :pv/name \"unique-5a\" :pv/lng 1}]'::TEXT)").expect("u1");
        Spi::run("SELECT mentat_transact('[{:db/id \"e2\" :pv/name \"unique-5b\" :pv/lng 2}]'::TEXT)").expect("u2");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':pv/name') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 2);
    }

    // ========================================================================
    // Multi-attribute entity operations (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_pv_entity_all_types() {
        setup(); setup_pv_schema();
        Spi::run(
            "SELECT mentat_transact('[{:db/id \"e\" :pv/str \"test\" :pv/lng 42 :pv/dbl 3.14 :pv/boo true :pv/kw :active}]'::TEXT)"
        ).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?s ?l ?d ?b ?k :where [?e :pv/str ?s] [?e :pv/lng ?l] [?e :pv/dbl ?d] [?e :pv/boo ?b] [?e :pv/kw ?k]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_pv_entity_partial_update() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pv/str \"original\" :pv/lng 1 :pv/boo false}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        // Update only str
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :pv/str \"changed\"]]'::TEXT)", eid)).expect("update");
        // Verify lng and boo unchanged
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?l ?b :where [{e} :pv/lng ?l] [{e} :pv/boo ?b]]'::TEXT, '{{}}'::jsonb)::TEXT", e = eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["results"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_pv_entity_retract_one_attr() {
        setup(); setup_pv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[{:db/id \"e\" :pv/str \"test\" :pv/lng 42}]'::TEXT)"
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/retract {} :pv/lng 42]]'::TEXT)", eid)).expect("retract");
        // str should still exist
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :pv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "test");
    }

    #[pg_test]
    fn test_pv_batch_20_entities_3_attrs() {
        setup(); setup_pv_schema();
        let mut ops = Vec::new();
        for i in 0..20 {
            ops.push(format!(
                "{{:db/id \"e{i}\" :pv/str \"entity-{i}\" :pv/lng {i} :pv/boo {b}}}",
                i = i, b = if i % 2 == 0 { "true" } else { "false" }
            ));
        }
        let r = Spi::get_one::<String>(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 20);
    }

    #[pg_test]
    fn test_pv_batch_50_entities_str_only() {
        setup(); setup_pv_schema();
        let mut ops = Vec::new();
        for i in 0..50 {
            ops.push(format!("[:db/add \"e{i}\" :pv/str \"str-{i}\"]", i = i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :pv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 50);
    }
}
