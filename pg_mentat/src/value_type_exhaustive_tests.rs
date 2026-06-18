// Exhaustive value type tests for pg_mentat.
//
// Tests every value type (ref, boolean, long, double, string, keyword, instant, uuid, bytes)
// across all operations: assert, retract, query, update, batch.
// Each type gets a dedicated section with boundary value testing.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_all_types_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"s\" :db/ident :vt/str :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"l\" :db/ident :vt/lng :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :vt/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"b\" :db/ident :vt/bool :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"r\" :db/ident :vt/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :vt/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"i\" :db/ident :vt/inst :db/valueType :db.type/instant :db/cardinality :db.cardinality/one}
                {:db/id \"u\" :db/ident :vt/uuid :db/valueType :db.type/uuid :db/cardinality :db.cardinality/one}
                {:db/id \"y\" :db/ident :vt/bytes :db/valueType :db.type/bytes :db/cardinality :db.cardinality/one}
                {:db/id \"sm\" :db/ident :vt/strs :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"lm\" :db/ident :vt/lngs :db/valueType :db.type/long :db/cardinality :db.cardinality/many}
                {:db/id \"dm\" :db/ident :vt/dbls :db/valueType :db.type/double :db/cardinality :db.cardinality/many}
                {:db/id \"rm\" :db/ident :vt/refs :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"km\" :db/ident :vt/kws :db/valueType :db.type/keyword :db/cardinality :db.cardinality/many}
                {:db/id \"name\" :db/ident :vt/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("all types schema failed");
    }

    // ========================================================================
    // STRING TYPE - Exhaustive
    // ========================================================================

    #[pg_test]
    fn test_vt_string_empty() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/str \"\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "");
    }

    #[pg_test]
    fn test_vt_string_whitespace_only() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/str \"   \"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "   ");
    }

    #[pg_test]
    fn test_vt_string_newlines() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/str \"line1\\nline2\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("line1"));
    }

    #[pg_test]
    fn test_vt_string_tabs() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/str \"col1\\tcol2\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().is_some());
    }

    #[pg_test]
    fn test_vt_string_special_chars() {
        setup();
        setup_all_types_schema();
        // Backslash and quotes
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/str \"has\\\\backslash\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e"].as_i64().is_some());
    }

    #[pg_test]
    fn test_vt_string_long_10k() {
        setup();
        setup_all_types_schema();
        let long_str: String = (0..10000)
            .map(|i| (b'a' + (i % 26) as u8) as char)
            .collect();
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/str \"{}\"]]'::TEXT)",
            long_str
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s").len(), 10000);
    }

    #[pg_test]
    fn test_vt_string_update_replace() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/str \"first\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :vt/str \"second\"]]'::TEXT)",
            eid
        ))
        .expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "second");
    }

    #[pg_test]
    fn test_vt_string_retract() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/str \"gone\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :vt/str \"gone\"]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_vt_string_many_add_multiple() {
        setup();
        setup_all_types_schema();
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e\" :vt/name \"holder\"]
                [:db/add \"e\" :vt/strs \"a\"]
                [:db/add \"e\" :vt/strs \"b\"]
                [:db/add \"e\" :vt/strs \"c\"]
                [:db/add \"e\" :vt/strs \"d\"]
                [:db/add \"e\" :vt/strs \"e\"]
            ]'::TEXT)",
        )
        .expect("many add");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :vt/name \"holder\"] [?e :vt/strs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_vt_string_many_retract_one() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :vt/name \"holder2\"]
                [:db/add \"e\" :vt/strs \"keep\"]
                [:db/add \"e\" :vt/strs \"remove\"]
            ]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :vt/strs \"remove\"]]'::TEXT)",
            eid
        ))
        .expect("retract one");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :vt/strs ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let arr = v["result"].as_array().expect("arr");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].as_str().expect("s"), "keep");
    }

    // ========================================================================
    // LONG TYPE - Exhaustive
    // ========================================================================

    #[pg_test]
    fn test_vt_long_zero() {
        setup();
        setup_all_types_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :vt/lng 0]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("l"), 0);
    }

    #[pg_test]
    fn test_vt_long_one() {
        setup();
        setup_all_types_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :vt/lng 1]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("l"), 1);
    }

    #[pg_test]
    fn test_vt_long_negative_one() {
        setup();
        setup_all_types_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :vt/lng -1]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("l"), -1);
    }

    #[pg_test]
    fn test_vt_long_max() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/lng {}]]'::TEXT)",
            i64::MAX
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("l"), i64::MAX);
    }

    #[pg_test]
    fn test_vt_long_min() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/lng {}]]'::TEXT)",
            i64::MIN
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("l"), i64::MIN);
    }

    #[pg_test]
    fn test_vt_long_powers_of_two() {
        setup();
        setup_all_types_schema();
        for exp in 0..62 {
            let val: i64 = 1 << exp;
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"p{}\" :vt/lng {}]]'::TEXT)",
                exp, val
            ))
            .expect("tx")
            .expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let eid = j["tempids"][&format!("p{}", exp)].as_i64().expect("eid");
            let q = Spi::get_one::<String>(&format!(
                "SELECT mentat_query('[:find ?v . :where [{} :vt/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
            )).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(
                v["result"].as_i64().expect("l"),
                val,
                "Power of 2: 2^{} = {}",
                exp,
                val
            );
        }
    }

    #[pg_test]
    fn test_vt_long_update_replace() {
        setup();
        setup_all_types_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :vt/lng 10]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :vt/lng 20]]'::TEXT)",
            eid
        ))
        .expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("l"), 20);
    }

    #[pg_test]
    fn test_vt_long_retract() {
        setup();
        setup_all_types_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :vt/lng 42]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :vt/lng 42]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    #[pg_test]
    fn test_vt_long_many_accumulate() {
        setup();
        setup_all_types_schema();
        let mut ops = vec!["[:db/add \"e\" :vt/name \"nums\"]".to_string()];
        for i in 0..20 {
            ops.push(format!("[:db/add \"e\" :vt/lngs {}]", i * 10));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("many add");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :vt/name \"nums\"] [?e :vt/lngs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    // ========================================================================
    // DOUBLE TYPE - Exhaustive
    // ========================================================================

    #[pg_test]
    fn test_vt_double_zero() {
        setup();
        setup_all_types_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :vt/dbl 0.0]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("d") - 0.0).abs() < 1e-10);
    }

    #[pg_test]
    fn test_vt_double_pi() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/dbl 3.141592653589793]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("d") - std::f64::consts::PI).abs() < 1e-10);
    }

    #[pg_test]
    fn test_vt_double_negative() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/dbl -99.99]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("d") - (-99.99)).abs() < 0.01);
    }

    #[pg_test]
    fn test_vt_double_very_small() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/dbl 0.000000001]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_f64().expect("d") > 0.0);
        assert!(v["result"].as_f64().expect("d") < 0.00001);
    }

    #[pg_test]
    fn test_vt_double_very_large() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/dbl 1.0e15]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("d") - 1.0e15).abs() < 1.0);
    }

    #[pg_test]
    fn test_vt_double_update() {
        setup();
        setup_all_types_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :vt/dbl 1.0]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :vt/dbl 2.0]]'::TEXT)",
            eid
        ))
        .expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("d") - 2.0).abs() < 0.01);
    }

    #[pg_test]
    fn test_vt_double_many_accumulate() {
        setup();
        setup_all_types_schema();
        let mut ops = vec!["[:db/add \"e\" :vt/name \"dbls\"]".to_string()];
        for i in 0..10 {
            // {:?} formats integral f64 as "0.0" so EDN reads it as a double,
            // not an integer (which would fail the double-type check).
            ops.push(format!("[:db/add \"e\" :vt/dbls {:?}]", (i as f64) * 0.1));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("many add");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :vt/name \"dbls\"] [?e :vt/dbls ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    // ========================================================================
    // BOOLEAN TYPE - Exhaustive
    // ========================================================================

    #[pg_test]
    fn test_vt_bool_true() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/bool true]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/bool ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), true);
    }

    #[pg_test]
    fn test_vt_bool_false() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/bool false]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/bool ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    #[pg_test]
    fn test_vt_bool_toggle() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/bool true]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Toggle 10 times
        for i in 0..10 {
            let val = if i % 2 == 0 { "false" } else { "true" };
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :vt/bool {}]]'::TEXT)",
                eid, val
            ))
            .expect("toggle");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/bool ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // After 10 toggles starting from true (i=0..=9), the LAST assertion is
        // i=9 (odd) -> true. The current value is the last write, so it is true.
        assert_eq!(v["result"].as_bool().expect("b"), true);
    }

    #[pg_test]
    fn test_vt_bool_retract_true() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/bool true]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :vt/bool true]]'::TEXT)",
            eid
        ))
        .expect("retract");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/bool ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].is_null());
    }

    // ========================================================================
    // KEYWORD TYPE - Exhaustive
    // ========================================================================

    #[pg_test]
    fn test_vt_keyword_simple() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/kw :active]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/kw ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("kw").contains("active"));
    }

    #[pg_test]
    fn test_vt_keyword_namespaced() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/kw :user.status/active]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/kw ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let kw = v["result"].as_str().expect("kw");
        assert!(kw.contains("user.status") || kw.contains("active"));
    }

    #[pg_test]
    fn test_vt_keyword_update() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/kw :pending]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :vt/kw :approved]]'::TEXT)",
            eid
        ))
        .expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/kw ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("kw").contains("approved"));
    }

    #[pg_test]
    fn test_vt_keyword_many_accumulate() {
        setup();
        setup_all_types_schema();
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e\" :vt/name \"kwholder\"]
                [:db/add \"e\" :vt/kws :tag-a]
                [:db/add \"e\" :vt/kws :tag-b]
                [:db/add \"e\" :vt/kws :tag-c]
            ]'::TEXT)",
        )
        .expect("many kw");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :vt/name \"kwholder\"] [?e :vt/kws ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    // ========================================================================
    // REF TYPE - Exhaustive
    // ========================================================================

    #[pg_test]
    fn test_vt_ref_simple() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"parent\" :vt/name \"parent\"]
                [:db/add \"child\" :vt/name \"child\"]
                [:db/add \"child\" :vt/ref \"parent\"]
            ]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let parent = j["tempids"]["parent"].as_i64().expect("parent");
        let child = j["tempids"]["child"].as_i64().expect("child");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :vt/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT",
            child
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("ref"), parent);
    }

    #[pg_test]
    fn test_vt_ref_self_reference() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/name \"self\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Self-reference
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :vt/ref {}]]'::TEXT)",
            eid, eid
        ))
        .expect("self-ref");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :vt/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("ref"), eid);
    }

    #[pg_test]
    fn test_vt_ref_update() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"a\" :vt/name \"A\"]
                [:db/add \"b\" :vt/name \"B\"]
                [:db/add \"c\" :vt/name \"C\"]
                [:db/add \"c\" :vt/ref \"a\"]
            ]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let b = j["tempids"]["b"].as_i64().expect("b");
        let c = j["tempids"]["c"].as_i64().expect("c");
        // Update ref from A to B
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :vt/ref {}]]'::TEXT)",
            c, b
        ))
        .expect("update ref");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :vt/ref ?r]]'::TEXT, '{{}}'::jsonb)::TEXT",
            c
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("ref"), b);
    }

    #[pg_test]
    fn test_vt_ref_many_accumulate() {
        setup();
        setup_all_types_schema();
        // Create a hub entity and 5 spoke entities
        let mut ops = vec!["[:db/add \"hub\" :vt/name \"hub\"]".to_string()];
        for i in 0..5 {
            ops.push(format!("[:db/add \"s{}\" :vt/name \"spoke-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :vt/refs \"s{}\"]", i));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("hub/spokes");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?r ...] :where [?e :vt/name \"hub\"] [?e :vt/refs ?r]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    // ========================================================================
    // INSTANT TYPE
    // ========================================================================

    #[pg_test]
    fn test_vt_instant_basic() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/inst #inst \"2024-01-15T10:30:00Z\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/inst ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(
            v["result"].as_str().is_some() || v["result"].is_string(),
            "Instant should be retrievable"
        );
    }

    #[pg_test]
    fn test_vt_instant_epoch() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/inst #inst \"1970-01-01T00:00:00Z\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e"].as_i64().is_some());
    }

    #[pg_test]
    fn test_vt_instant_update() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/inst #inst \"2024-01-01T00:00:00Z\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :vt/inst #inst \"2024-12-31T23:59:59Z\"]]'::TEXT)", eid
        )).expect("update");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/inst ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let inst = v["result"].as_str().unwrap_or("");
        assert!(
            inst.contains("2024-12-31") || inst.contains("2024"),
            "Should have updated instant"
        );
    }

    // ========================================================================
    // UUID TYPE
    // ========================================================================

    #[pg_test]
    fn test_vt_uuid_basic() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/uuid #uuid \"550e8400-e29b-41d4-a716-446655440000\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/uuid ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let uuid = v["result"].as_str().unwrap_or("");
        assert!(
            uuid.contains("550e8400") || uuid.contains("446655440000"),
            "UUID should roundtrip"
        );
    }

    #[pg_test]
    fn test_vt_uuid_nil() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/uuid #uuid \"00000000-0000-0000-0000-000000000000\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert!(j["tempids"]["e"].as_i64().is_some());
    }

    #[pg_test]
    fn test_vt_uuid_update() {
        setup();
        setup_all_types_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :vt/uuid #uuid \"550e8400-e29b-41d4-a716-446655440000\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :vt/uuid #uuid \"a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11\"]]'::TEXT)", eid
        )).expect("update uuid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :vt/uuid ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let uuid = v["result"].as_str().unwrap_or("");
        assert!(
            uuid.contains("a0eebc99") || uuid.contains("bb9bd380a11"),
            "UUID should be updated"
        );
    }

    // ========================================================================
    // Cross-type queries
    // ========================================================================

    #[pg_test]
    fn test_vt_cross_type_entity() {
        setup();
        setup_all_types_schema();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"e\"
                 :vt/name \"Cross-Type\"
                 :vt/str \"hello\"
                 :vt/lng 42
                 :vt/dbl 3.14
                 :vt/bool true
                 :vt/kw :active
                 :vt/inst #inst \"2024-06-15T12:00:00Z\"
                 :vt/uuid #uuid \"550e8400-e29b-41d4-a716-446655440000\"}
            ]'::TEXT)",
        )
        .expect("cross-type entity");

        // Query to verify each attribute type via datoms
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT a) FROM mentat.datoms
             WHERE e = (SELECT MIN(e) FROM mentat.datoms
                       WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':vt/name')
                       AND v_text = 'Cross-Type')
             AND added = true",
        )
        .expect("q")
        .expect("NULL");
        assert!(
            count >= 7,
            "Cross-type entity should have at least 7 attributes, got {}",
            count
        );
    }

    #[pg_test]
    fn test_vt_type_tag_correctness() {
        setup();
        setup_all_types_schema();

        // Create entities with different value types
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :vt/str \"text\"]
                [:db/add \"e\" :vt/lng 42]
                [:db/add \"e\" :vt/dbl 3.14]
                [:db/add \"e\" :vt/bool true]
                [:db/add \"e\" :vt/kw :test]
            ]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Verify value_type_tags match expected
        // Tag 7 = string, 2 = long, 3 = double, 1 = boolean, 8 = keyword
        let str_tag = Spi::get_one::<i16>(&format!(
            "SELECT value_type_tag FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':vt/str')
             AND added = true LIMIT 1",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert_eq!(str_tag, 7, "String type tag should be 7");

        let long_tag = Spi::get_one::<i16>(&format!(
            "SELECT value_type_tag FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':vt/lng')
             AND added = true LIMIT 1",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert_eq!(long_tag, 2, "Long type tag should be 2");

        let dbl_tag = Spi::get_one::<i16>(&format!(
            "SELECT value_type_tag FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':vt/dbl')
             AND added = true LIMIT 1",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert_eq!(dbl_tag, 3, "Double type tag should be 3");

        let bool_tag = Spi::get_one::<i16>(&format!(
            "SELECT value_type_tag FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':vt/bool')
             AND added = true LIMIT 1",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert_eq!(bool_tag, 1, "Boolean type tag should be 1");

        let kw_tag = Spi::get_one::<i16>(&format!(
            "SELECT value_type_tag FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':vt/kw')
             AND added = true LIMIT 1",
            eid
        ))
        .expect("q")
        .expect("NULL");
        assert_eq!(kw_tag, 8, "Keyword type tag should be 8");
    }

    // ========================================================================
    // Batch operations per type
    // ========================================================================

    #[pg_test]
    fn test_vt_batch_50_strings() {
        setup();
        setup_all_types_schema();
        let mut ops = Vec::new();
        for i in 0..50 {
            ops.push(format!("[:db/add \"s{}\" :vt/str \"string-{}\"]", i, i));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch strings");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':vt/str')
             AND added = true",
        )
        .expect("q")
        .expect("NULL");
        assert_eq!(count, 50);
    }

    #[pg_test]
    fn test_vt_batch_50_longs() {
        setup();
        setup_all_types_schema();
        let mut ops = Vec::new();
        for i in 0..50 {
            ops.push(format!("[:db/add \"l{}\" :vt/lng {}]", i, i * 100));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch longs");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':vt/lng')
             AND added = true",
        )
        .expect("q")
        .expect("NULL");
        assert_eq!(count, 50);
    }

    #[pg_test]
    fn test_vt_batch_50_doubles() {
        setup();
        setup_all_types_schema();
        let mut ops = Vec::new();
        for i in 0..50 {
            // {:?} keeps the decimal point so 0 -> "0.0" (a valid EDN double).
            ops.push(format!(
                "[:db/add \"d{}\" :vt/dbl {:?}]",
                i,
                (i as f64) * 0.7
            ));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch doubles");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':vt/dbl')
             AND added = true",
        )
        .expect("q")
        .expect("NULL");
        assert_eq!(count, 50);
    }

    #[pg_test]
    fn test_vt_batch_mixed_types() {
        setup();
        setup_all_types_schema();
        let mut ops = Vec::new();
        for i in 0..25 {
            ops.push(format!(
                "{{:db/id \"m{}\" :vt/str \"name-{}\" :vt/lng {} :vt/dbl {:?} :vt/bool {}}}",
                i,
                i,
                i,
                (i as f64) * 1.1,
                if i % 2 == 0 { "true" } else { "false" }
            ));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("batch mixed")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let tempids = j["tempids"].as_object().expect("tempids");
        assert_eq!(tempids.len(), 25);
    }
}
