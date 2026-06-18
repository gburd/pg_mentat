// Boundary value tests: systematic testing of edge values for every type,
// combinations of min/max values, and boundary arithmetic.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_bv_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"s\" :db/ident :bv/str :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"l\" :db/ident :bv/lng :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"d\" :db/ident :bv/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"b\" :db/ident :bv/bool :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k\" :db/ident :bv/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"sm\" :db/ident :bv/strs :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"lm\" :db/ident :bv/lngs :db/valueType :db.type/long :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        ).expect("bv schema");
    }

    // ========================================================================
    // Long boundary values
    // ========================================================================

    #[pg_test]
    fn test_bv_long_zero() {
        setup();
        setup_bv_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :bv/lng 0]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 0);
    }

    #[pg_test]
    fn test_bv_long_one() {
        setup();
        setup_bv_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :bv/lng 1]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 1);
    }

    #[pg_test]
    fn test_bv_long_neg_one() {
        setup();
        setup_bv_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :bv/lng -1]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), -1);
    }

    #[pg_test]
    fn test_bv_long_max_i32() {
        setup();
        setup_bv_schema();
        let val = i32::MAX as i64;
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/lng {}]]'::TEXT)",
            val
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), val);
    }

    #[pg_test]
    fn test_bv_long_min_i32() {
        setup();
        setup_bv_schema();
        let val = i32::MIN as i64;
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/lng {}]]'::TEXT)",
            val
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), val);
    }

    #[pg_test]
    fn test_bv_long_max_i64() {
        setup();
        setup_bv_schema();
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/lng {}]]'::TEXT)",
            i64::MAX
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), i64::MAX);
    }

    #[pg_test]
    fn test_bv_long_min_i64() {
        setup();
        setup_bv_schema();
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/lng {}]]'::TEXT)",
            i64::MIN
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), i64::MIN);
    }

    #[pg_test]
    fn test_bv_long_powers_of_10() {
        setup();
        setup_bv_schema();
        for exp in 0..18u32 {
            let val: i64 = 10i64.pow(exp);
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"p{exp}\" :bv/lng {val}]]'::TEXT)",
                exp = exp,
                val = val
            ))
            .expect("tx")
            .expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let eid = j["tempids"][&format!("p{}", exp)].as_i64().expect("eid");
            let q = Spi::get_one::<String>(&format!("SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid)).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(
                v["result"].as_i64().expect("v"),
                val,
                "10^{} should roundtrip",
                exp
            );
        }
    }

    #[pg_test]
    fn test_bv_long_negative_powers_of_10() {
        setup();
        setup_bv_schema();
        for exp in 0..18u32 {
            let val: i64 = -(10i64.pow(exp));
            let label = format!("n{}", exp);
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"{}\" :bv/lng {}]]'::TEXT)",
                label, val
            ))
            .expect("tx")
            .expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let eid = j["tempids"][&label].as_i64().expect("eid");
            let q = Spi::get_one::<String>(&format!("SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid)).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_i64().expect("v"), val);
        }
    }

    #[pg_test]
    fn test_bv_long_around_zero() {
        setup();
        setup_bv_schema();
        for val in -10i64..=10 {
            let label = if val >= 0 {
                format!("p{}", val)
            } else {
                format!("n{}", val.unsigned_abs())
            };
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"{}\" :bv/lng {}]]'::TEXT)",
                label, val
            ))
            .expect("tx")
            .expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let eid = j["tempids"][&label].as_i64().expect("eid");
            let q = Spi::get_one::<String>(&format!("SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid)).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(v["result"].as_i64().expect("v"), val);
        }
    }

    // ========================================================================
    // Double boundary values
    // ========================================================================

    #[pg_test]
    fn test_bv_dbl_zero() {
        setup();
        setup_bv_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :bv/dbl 0.0]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v")).abs() < 1e-10);
    }

    #[pg_test]
    fn test_bv_dbl_one() {
        setup();
        setup_bv_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :bv/dbl 1.0]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") - 1.0).abs() < 1e-10);
    }

    #[pg_test]
    fn test_bv_dbl_neg_one() {
        setup();
        setup_bv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/dbl -1.0]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") + 1.0).abs() < 1e-10);
    }

    #[pg_test]
    fn test_bv_dbl_epsilon() {
        setup();
        setup_bv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/dbl 0.000000001]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let d = v["result"].as_f64().expect("v");
        assert!(d > 0.0 && d < 0.00001);
    }

    #[pg_test]
    fn test_bv_dbl_large() {
        setup();
        setup_bv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/dbl 1.0e15]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("v") - 1.0e15).abs() < 1.0);
    }

    #[pg_test]
    fn test_bv_dbl_fractional_progression() {
        setup();
        setup_bv_schema();
        // Test 0.1, 0.2, ..., 0.9
        for i in 1..=9 {
            let val = (i as f64) * 0.1;
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"f{}\" :bv/dbl {}]]'::TEXT)",
                i, val
            ))
            .expect("tx")
            .expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let eid = j["tempids"][&format!("f{}", i)].as_i64().expect("eid");
            let q = Spi::get_one::<String>(&format!("SELECT mentat_query('[:find ?v . :where [{} :bv/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid)).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert!(
                (v["result"].as_f64().expect("v") - val).abs() < 0.01,
                "0.{} should roundtrip",
                i
            );
        }
    }

    // ========================================================================
    // String boundary values
    // ========================================================================

    #[pg_test]
    fn test_bv_str_empty() {
        setup();
        setup_bv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/str \"\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "");
    }

    #[pg_test]
    fn test_bv_str_single_char() {
        setup();
        setup_bv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/str \"a\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "a");
    }

    #[pg_test]
    fn test_bv_str_lengths_1_to_100() {
        setup();
        setup_bv_schema();
        for len in [1, 2, 5, 10, 25, 50, 100] {
            let s: String = (0..len).map(|i| (b'a' + (i % 26) as u8) as char).collect();
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"l{}\" :bv/str \"{}\"]]'::TEXT)",
                len, s
            ))
            .expect("tx")
            .expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let eid = j["tempids"][&format!("l{}", len)].as_i64().expect("eid");
            let q = Spi::get_one::<String>(&format!("SELECT mentat_query('[:find ?v . :where [{} :bv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid)).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(
                v["result"].as_str().expect("s").len(),
                len,
                "len {} roundtrip",
                len
            );
        }
    }

    #[pg_test]
    fn test_bv_str_1000_chars() {
        setup();
        setup_bv_schema();
        let s: String = (0..1000).map(|i| (b'A' + (i % 26) as u8) as char).collect();
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/str \"{}\"]]'::TEXT)",
            s
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s").len(), 1000);
    }

    #[pg_test]
    fn test_bv_str_10000_chars() {
        setup();
        setup_bv_schema();
        let s: String = (0..10000)
            .map(|i| (b'a' + (i % 26) as u8) as char)
            .collect();
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/str \"{}\"]]'::TEXT)",
            s
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s").len(), 10000);
    }

    #[pg_test]
    fn test_bv_str_whitespace_variations() {
        setup();
        setup_bv_schema();
        let cases = vec![
            ("ws1", " "),
            ("ws2", "  "),
            ("ws3", "   "),
            ("ws4", " a "),
            ("ws5", "  a  b  "),
        ];
        for (label, val) in cases {
            let r = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"{}\" :bv/str \"{}\"]]'::TEXT)",
                label, val
            ))
            .expect("tx")
            .expect("NULL");
            let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
            let eid = j["tempids"][label].as_i64().expect("eid");
            let q = Spi::get_one::<String>(&format!("SELECT mentat_query('[:find ?v . :where [{} :bv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid)).expect("q").expect("NULL");
            let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
            assert_eq!(
                v["result"].as_str().expect("s"),
                val,
                "{} should preserve whitespace",
                label
            );
        }
    }

    // ========================================================================
    // Cardinality-many boundary values
    // ========================================================================

    #[pg_test]
    fn test_bv_many_zero_values() {
        setup();
        setup_bv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/str \"holder\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let q = Spi::get_one::<String>(&format!("SELECT mentat_query('[:find [?v ...] :where [{} :bv/strs ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid)).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    #[pg_test]
    fn test_bv_many_one_value() {
        setup();
        setup_bv_schema();
        Spi::run("SELECT mentat_transact('[[:db/add \"e\" :bv/str \"h\"] [:db/add \"e\" :bv/strs \"only\"]]'::TEXT)").expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :bv/str \"h\"] [?e :bv/strs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    #[pg_test]
    fn test_bv_many_50_values() {
        setup();
        setup_bv_schema();
        let mut ops = vec!["[:db/add \"e\" :bv/str \"m50\"]".to_string()];
        for i in 0..50 {
            ops.push(format!("[:db/add \"e\" :bv/strs \"tag-{}\"]", i));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :bv/str \"m50\"] [?e :bv/strs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 50);
    }

    #[pg_test]
    fn test_bv_many_100_values() {
        setup();
        setup_bv_schema();
        let mut ops = vec!["[:db/add \"e\" :bv/str \"m100\"]".to_string()];
        for i in 0..100 {
            ops.push(format!("[:db/add \"e\" :bv/strs \"val-{}\"]", i));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :bv/str \"m100\"] [?e :bv/strs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 100);
    }

    #[pg_test]
    fn test_bv_many_longs_boundary_values() {
        setup();
        setup_bv_schema();
        let boundary_vals = vec![i64::MIN, -1, 0, 1, i64::MAX];
        let mut ops = vec!["[:db/add \"e\" :bv/str \"lbounds\"]".to_string()];
        for v in &boundary_vals {
            ops.push(format!("[:db/add \"e\" :bv/lngs {}]", v));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :bv/str \"lbounds\"] [?e :bv/lngs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(
            v["result"].as_array().expect("arr").len(),
            boundary_vals.len()
        );
    }

    // ========================================================================
    // Batch size boundaries
    // ========================================================================

    #[pg_test]
    fn test_bv_batch_1_entity() {
        setup();
        setup_bv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :bv/str \"one\"]]'::TEXT)",
        )
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 1);
    }

    #[pg_test]
    fn test_bv_batch_10_entities() {
        setup();
        setup_bv_schema();
        let mut ops = Vec::new();
        for i in 0..10 {
            ops.push(format!("[:db/add \"b{}\" :bv/str \"e-{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 10);
    }

    #[pg_test]
    fn test_bv_batch_50_entities() {
        setup();
        setup_bv_schema();
        let mut ops = Vec::new();
        for i in 0..50 {
            ops.push(format!("[:db/add \"b{}\" :bv/str \"e-{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 50);
    }

    #[pg_test]
    fn test_bv_batch_100_entities() {
        setup();
        setup_bv_schema();
        let mut ops = Vec::new();
        for i in 0..100 {
            ops.push(format!("[:db/add \"b{}\" :bv/str \"e-{}\"]", i, i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx")
        .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 100);
    }

    #[pg_test]
    fn test_bv_batch_500_entities() {
        setup();
        setup_bv_schema();
        let mut ops = Vec::new();
        for i in 0..500 {
            ops.push(format!("[:db/add \"b{}\" :bv/str \"e-{}\"]", i, i));
        }
        Spi::run(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)",
            ops.join("\n")
        ))
        .expect("tx");
        let count = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT e) FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':bv/str') AND added = true",
        ).expect("q").expect("NULL");
        assert_eq!(count, 500);
    }

    // ========================================================================
    // Update count boundaries
    // ========================================================================

    #[pg_test]
    fn test_bv_update_100_times() {
        setup();
        setup_bv_schema();
        let r =
            Spi::get_one::<String>("SELECT mentat_transact('[[:db/add \"e\" :bv/lng 0]]'::TEXT)")
                .expect("tx")
                .expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=100 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :bv/lng {}]]'::TEXT)",
                eid, i
            ))
            .expect("update");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :bv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT",
            eid
        ))
        .expect("q")
        .expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 100);
    }
}
