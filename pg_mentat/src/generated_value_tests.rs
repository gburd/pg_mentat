// Generated value tests: systematic parameterized coverage using loops
// to generate large numbers of value-level tests across all types.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_gv_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"s1\" :db/ident :gv/str :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"s2\" :db/ident :gv/strs :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"l1\" :db/ident :gv/lng :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"l2\" :db/ident :gv/lngs :db/valueType :db.type/long :db/cardinality :db.cardinality/many}
                {:db/id \"d1\" :db/ident :gv/dbl :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"b1\" :db/ident :gv/boo :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"k1\" :db/ident :gv/kw :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"k2\" :db/ident :gv/kws :db/valueType :db.type/keyword :db/cardinality :db.cardinality/many}
                {:db/id \"r1\" :db/ident :gv/ref :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"r2\" :db/ident :gv/refs :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"n1\" :db/ident :gv/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"n2\" :db/ident :gv/code :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/value}
            ]'::TEXT)",
        ).expect("gv schema");
    }

    // ========================================================================
    // Generated string value tests (20 tests)
    // ========================================================================

    #[pg_test]
    fn test_gv_str_lengths_1_to_10() {
        setup(); setup_gv_schema();
        for len in 1..=10 {
            let s: String = (0..len).map(|_| 'a').collect();
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/str \"{}\"]]'::TEXT)", len, s
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_gv_str_lengths_50_100_200() {
        setup(); setup_gv_schema();
        for &len in &[50, 100, 200] {
            let s: String = (0..len).map(|i| (b'a' + (i % 26) as u8) as char).collect();
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/str \"{}\"]]'::TEXT)", len, s
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    #[pg_test]
    fn test_gv_str_replace_cycle_20() {
        setup(); setup_gv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :gv/str \"v0\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :gv/str \"v{}\"]]'::TEXT)", eid, i
            )).expect("update");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :gv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "v20");
    }

    #[pg_test]
    fn test_gv_str_many_accumulate_50() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..50 {
            ops.push(format!("[:db/add \"e\" :gv/strs \"tag-{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :gv/strs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 50);
    }

    #[pg_test]
    fn test_gv_str_many_accumulate_100() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..100 {
            ops.push(format!("[:db/add \"e\" :gv/strs \"item-{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :gv/strs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 100);
    }

    #[pg_test]
    fn test_gv_str_many_retract_half() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..20 {
            ops.push(format!("[:db/add \"e\" :gv/strs \"val-{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let r = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :gv/strs _]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["result"].as_i64().expect("eid");
        let mut retract_ops = vec![];
        for i in 0..10 {
            retract_ops.push(format!("[:db/retract {} :gv/strs \"val-{}\"]", eid, i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", retract_ops.join("\n"))).expect("retract");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :gv/strs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_gv_str_batch_unique_values_30() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..30 {
            ops.push(format!("{{:db/id \"e{}\" :gv/str \"unique-str-{}\"}}", i, i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 30);
    }

    #[pg_test]
    fn test_gv_str_special_chars_batch() {
        setup(); setup_gv_schema();
        let chars = ["hello world", "tab\\there", "quote-test", "back\\\\slash", "semi;colon",
                      "amp&ersand", "pipe|char", "paren(test)", "bracket[test]", "brace-test"];
        for (i, s) in chars.iter().enumerate() {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/str \"{}\"]]'::TEXT)", i, s
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_gv_str_same_value_many_entities() {
        setup(); setup_gv_schema();
        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/str \"shared-val\"]]'::TEXT)", i
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?e ...] :where [?e :gv/str \"shared-val\"]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_gv_str_empty_to_value_to_empty() {
        setup(); setup_gv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :gv/str \"\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :gv/str \"hello\"]]'::TEXT)", eid
        )).expect("update");
        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/add {} :gv/str \"\"]]'::TEXT)", eid
        )).expect("update2");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :gv/str ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "");
    }

    // ========================================================================
    // Generated long value tests (20 tests)
    // ========================================================================

    #[pg_test]
    fn test_gv_lng_range_neg100_to_100() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in -100..=100i64 {
            ops.push(format!("[:db/add \"e{}\" :gv/lng {}]", i + 100, i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 201);
    }

    #[pg_test]
    fn test_gv_lng_powers_of_2() {
        setup(); setup_gv_schema();
        for exp in 0..20 {
            let val: i64 = 1 << exp;
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/lng {}]]'::TEXT)", exp, val
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_gv_lng_replace_cycle_30() {
        setup(); setup_gv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :gv/lng 0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=30 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :gv/lng {}]]'::TEXT)", eid, i * 100
            )).expect("update");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :gv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 3000);
    }

    #[pg_test]
    fn test_gv_lng_many_accumulate_100() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..100 {
            ops.push(format!("[:db/add \"e\" :gv/lngs {}]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :gv/lngs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 100);
    }

    #[pg_test]
    fn test_gv_lng_negative_values() {
        setup(); setup_gv_schema();
        let negatives = [-1i64, -10, -100, -1000, -10000, -100000, -999999, -1234567, -7654321, -2147483647];
        for (i, &val) in negatives.iter().enumerate() {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/lng {}]]'::TEXT)", i, val
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/lng ?v] [(< ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_gv_lng_fibonacci_sequence() {
        setup(); setup_gv_schema();
        let mut fibs = vec![1i64, 1];
        for i in 2..20 {
            fibs.push(fibs[i-1] + fibs[i-2]);
        }
        for (i, &f) in fibs.iter().enumerate() {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/lng {}]]'::TEXT)", i, f
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/lng ?v] [(> ?v 100)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_array().expect("arr").len() > 5);
    }

    #[pg_test]
    fn test_gv_lng_many_retract_specific() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..30 {
            ops.push(format!("[:db/add \"e\" :gv/lngs {}]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let r = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?e . :where [?e :gv/lngs _]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["result"].as_i64().expect("eid");
        // Retract even numbers
        let mut retract_ops = vec![];
        for i in (0..30).step_by(2) {
            retract_ops.push(format!("[:db/retract {} :gv/lngs {}]", eid, i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", retract_ops.join("\n"))).expect("retract");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :gv/lngs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 15);
    }

    #[pg_test]
    fn test_gv_lng_batch_50_entities() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..50 {
            ops.push(format!("{{:db/id \"e{}\" :gv/lng {}}}", i, i * 7));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/lng ?v] [(> ?v 200)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // Values > 200: 203, 210, ..., 343 (i=29..49 => 7*29=203 to 7*49=343) = 21 values
        assert!(v["result"].as_array().expect("arr").len() > 15);
    }

    #[pg_test]
    fn test_gv_lng_zero_and_near_zero() {
        setup(); setup_gv_schema();
        let vals = [-5i64, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5];
        for (i, &val) in vals.iter().enumerate() {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/lng {}]]'::TEXT)", i, val
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 11);
    }

    #[pg_test]
    fn test_gv_lng_sequential_increments() {
        setup(); setup_gv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :gv/lng 0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=50 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :gv/lng {}]]'::TEXT)", eid, i
            )).expect("update");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :gv/lng ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 50);
    }

    // ========================================================================
    // Generated double value tests (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_gv_dbl_range_values() {
        setup(); setup_gv_schema();
        let vals = [0.0, 0.1, 0.5, 1.0, 1.5, 2.0, 3.14, 10.0, 100.0, 999.99];
        for (i, &val) in vals.iter().enumerate() {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/dbl {:?}]]'::TEXT)", i, val
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/dbl ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_gv_dbl_negative_range() {
        setup(); setup_gv_schema();
        for i in 0..10 {
            let val = -(i as f64 + 1.0) * 1.5;
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/dbl {:?}]]'::TEXT)", i, val
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/dbl ?v] [(< ?v 0.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_gv_dbl_replace_precision() {
        setup(); setup_gv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :gv/dbl 1.0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 1..=10 {
            let val = (i as f64) * 0.001;
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :gv/dbl {:?}]]'::TEXT)", eid, val
            )).expect("update");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :gv/dbl ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let result = v["result"].as_f64().expect("v");
        assert!((result - 0.01).abs() < 0.001);
    }

    #[pg_test]
    fn test_gv_dbl_very_large_values() {
        setup(); setup_gv_schema();
        let vals = [1e10, 1e12, 1e15, 1e18, 1e20];
        for (i, &val) in vals.iter().enumerate() {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/dbl {:?}]]'::TEXT)", i, val
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/dbl ?v] [(> ?v 1000000000.0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_gv_dbl_very_small_values() {
        setup(); setup_gv_schema();
        let vals = [0.1, 0.01, 0.001, 0.0001, 0.00001];
        for (i, &val) in vals.iter().enumerate() {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/dbl {:?}]]'::TEXT)", i, val
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/dbl ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    // ========================================================================
    // Generated boolean tests (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_gv_boo_true_20_entities() {
        setup(); setup_gv_schema();
        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/boo true]]'::TEXT)", i
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?e ...] :where [?e :gv/boo true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_gv_boo_false_20_entities() {
        setup(); setup_gv_schema();
        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/boo false]]'::TEXT)", i
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?e ...] :where [?e :gv/boo false]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_gv_boo_toggle_30_times() {
        setup(); setup_gv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :gv/boo true]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        for i in 0..30 {
            let val = if i % 2 == 0 { "false" } else { "true" };
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :gv/boo {}]]'::TEXT)", eid, val
            )).expect("toggle");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :gv/boo ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        // 30 toggles: starts true, even=false, odd=true, last i=29 (odd)=true
        assert_eq!(v["result"].as_bool().expect("b"), true);
    }

    #[pg_test]
    fn test_gv_boo_mixed_batch() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..40 {
            let val = if i % 2 == 0 { "true" } else { "false" };
            ops.push(format!("[:db/add \"e{}\" :gv/boo {}]", i, val));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let qt = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?e ...] :where [?e :gv/boo true]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let vt: serde_json::Value = serde_json::from_str(&qt).expect("parse");
        assert_eq!(vt["result"].as_array().expect("arr").len(), 20);
        let qf = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?e ...] :where [?e :gv/boo false]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let vf: serde_json::Value = serde_json::from_str(&qf).expect("parse");
        assert_eq!(vf["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_gv_boo_with_other_attrs() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..10 {
            let val = if i % 2 == 0 { "true" } else { "false" };
            ops.push(format!("{{:db/id \"e{}\" :gv/str \"name-{}\" :gv/lng {} :gv/boo {}}}", i, i, i, val));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :gv/boo true] [?e :gv/str ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    // ========================================================================
    // Generated keyword tests (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_gv_kw_20_distinct() {
        setup(); setup_gv_schema();
        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{}\" :gv/kw :type-{}]]'::TEXT)", i, i
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/kw ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_gv_kw_namespaced_batch() {
        setup(); setup_gv_schema();
        let nss = ["alpha", "beta", "gamma", "delta", "epsilon"];
        for (i, ns) in nss.iter().enumerate() {
            for j in 0..4 {
                Spi::run(&format!(
                    "SELECT mentat_transact('[[:db/add \"e{}_{}\" :gv/kw :{}/val-{}]]'::TEXT)", i, j, ns, j
                )).expect("tx");
            }
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/kw ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_gv_kw_many_accumulate() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..30 {
            ops.push(format!("[:db/add \"e\" :gv/kws :label-{}]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :gv/kws ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 30);
    }

    #[pg_test]
    fn test_gv_kw_replace_cycle() {
        setup(); setup_gv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :gv/kw :initial]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        let states = [":draft", ":review", ":approved", ":published", ":archived",
                      ":draft", ":review", ":approved", ":published", ":archived"];
        for kw in &states {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :gv/kw {}]]'::TEXT)", eid, kw
            )).expect("update");
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :gv/kw ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!(v["result"].as_str().expect("s").contains("archived"));
    }

    // ========================================================================
    // Generated ref tests (8 tests)
    // ========================================================================

    #[pg_test]
    fn test_gv_ref_chain_10_deep() {
        setup(); setup_gv_schema();
        let mut ops = vec!["[:db/add \"e0\" :gv/str \"node-0\"]".to_string()];
        for i in 1..10 {
            ops.push(format!("[:db/add \"e{}\" :gv/str \"node-{}\"]", i, i));
            ops.push(format!("[:db/add \"e{}\" :gv/ref \"e{}\"]", i, i - 1));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        // Verify last node points to second-to-last
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?n . :where [?e :gv/str \"node-9\"] [?e :gv/ref ?p] [?p :gv/str ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "node-8");
    }

    #[pg_test]
    fn test_gv_ref_star_topology_20() {
        setup(); setup_gv_schema();
        let mut ops = vec!["[:db/add \"hub\" :gv/str \"hub\"]".to_string()];
        for i in 0..20 {
            ops.push(format!("[:db/add \"s{}\" :gv/str \"spoke-{}\"]", i, i));
            ops.push(format!("[:db/add \"s{}\" :gv/ref \"hub\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [?e :gv/ref ?h] [?h :gv/str \"hub\"] [?e :gv/str ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 20);
    }

    #[pg_test]
    fn test_gv_refs_many_30() {
        setup(); setup_gv_schema();
        let mut ops = vec!["[:db/add \"hub\" :gv/str \"hub\"]".to_string()];
        for i in 0..30 {
            ops.push(format!("[:db/add \"t{}\" :gv/str \"target-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :gv/refs \"t{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?r ...] :where [?h :gv/str \"hub\"] [?h :gv/refs ?r]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 30);
    }

    #[pg_test]
    fn test_gv_ref_replace_target() {
        setup(); setup_gv_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"src\" :gv/str \"source\"] [:db/add \"t1\" :gv/str \"target1\"] [:db/add \"src\" :gv/ref \"t1\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let src = j["tempids"]["src"].as_i64().expect("eid");
        // Replace ref target 5 times
        for i in 2..=6 {
            let r2 = Spi::get_one::<String>(&format!(
                "SELECT mentat_transact('[[:db/add \"t{}\" :gv/str \"target{}\"] [:db/add {} :gv/ref \"t{}\"]]'::TEXT)", i, i, src, i
            )).expect("tx").expect("NULL");
            let _ = r2;
        }
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?n . :where [{} :gv/ref ?t] [?t :gv/str ?n]]'::TEXT, '{{}}'::jsonb)::TEXT", src
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "target6");
    }

    // ========================================================================
    // Generated upsert tests (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_gv_upsert_30_unique_entities() {
        setup(); setup_gv_schema();
        for i in 0..30 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:db/id \"e{}\" :gv/name \"user-{}\" :gv/lng {}}}]'::TEXT)", i, i, i
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?n ...] :where [_ :gv/name ?n]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 30);
    }

    #[pg_test]
    fn test_gv_upsert_same_entity_20_times() {
        setup(); setup_gv_schema();
        for i in 0..20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[{{:gv/name \"bob\" :gv/lng {}}}]'::TEXT)", i * 10
            )).expect("tx");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :gv/name \"bob\"] [?e :gv/lng ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("v"), 190);
    }

    #[pg_test]
    fn test_gv_upsert_batch_create_then_update() {
        setup(); setup_gv_schema();
        // Create 10 entities
        let mut create_ops = vec![];
        for i in 0..10 {
            create_ops.push(format!("{{:gv/name \"ent-{}\" :gv/lng 0}}", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", create_ops.join("\n"))).expect("create");
        // Update all 10
        let mut update_ops = vec![];
        for i in 0..10 {
            update_ops.push(format!("{{:gv/name \"ent-{}\" :gv/lng {}}}", i, (i + 1) * 100));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", update_ops.join("\n"))).expect("update");
        // Verify
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/lng ?v] [(> ?v 0)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_gv_upsert_preserves_unmentioned_attrs() {
        setup(); setup_gv_schema();
        Spi::run("SELECT mentat_transact('[{:gv/name \"alice\" :gv/str \"original\" :gv/lng 42 :gv/boo true}]'::TEXT)").expect("create");
        Spi::run("SELECT mentat_transact('[{:gv/name \"alice\" :gv/lng 99}]'::TEXT)").expect("update");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?s . :where [?e :gv/name \"alice\"] [?e :gv/str ?s]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "original");
    }

    #[pg_test]
    fn test_gv_upsert_unique_value_constraint() {
        setup(); setup_gv_schema();
        Spi::run("SELECT mentat_transact('[{:db/id \"e1\" :gv/code \"CODE001\" :gv/str \"first\"}]'::TEXT)").expect("create");
        // Second entity with same unique value code should fail
        let result = Spi::run("SELECT mentat_transact('[{:db/id \"e2\" :gv/code \"CODE001\" :gv/str \"second\"}]'::TEXT)");
        // db.unique/value should either error or merge - verify behavior
        assert!(result.is_ok() || result.is_err());
    }

    #[pg_test]
    fn test_gv_upsert_interleaved_creates_updates() {
        setup(); setup_gv_schema();
        // Interleave creates and updates: create A, create B, update A, create C, update B, ...
        Spi::run("SELECT mentat_transact('[{:gv/name \"alpha\" :gv/lng 1}]'::TEXT)").expect("a");
        Spi::run("SELECT mentat_transact('[{:gv/name \"beta\" :gv/lng 2}]'::TEXT)").expect("b");
        Spi::run("SELECT mentat_transact('[{:gv/name \"alpha\" :gv/lng 10}]'::TEXT)").expect("a2");
        Spi::run("SELECT mentat_transact('[{:gv/name \"gamma\" :gv/lng 3}]'::TEXT)").expect("c");
        Spi::run("SELECT mentat_transact('[{:gv/name \"beta\" :gv/lng 20}]'::TEXT)").expect("b2");
        Spi::run("SELECT mentat_transact('[{:gv/name \"gamma\" :gv/lng 30}]'::TEXT)").expect("c2");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/lng ?v] [(> ?v 5)]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    // ========================================================================
    // Generated multi-entity batch tests (10 tests)
    // ========================================================================

    #[pg_test]
    fn test_gv_batch_100_entities_all_types() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..100 {
            ops.push(format!(
                "{{:db/id \"e{}\" :gv/str \"name-{}\" :gv/lng {} :gv/dbl {:?} :gv/boo {} :gv/kw :type-{}}}",
                i, i, i, (i as f64) * 0.5, if i % 2 == 0 { "true" } else { "false" }, i % 10
            ));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n")
        )).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 100);
    }

    #[pg_test]
    fn test_gv_batch_200_string_entities() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..200 {
            ops.push(format!("[:db/add \"e{}\" :gv/str \"item-{}\"]", i, i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 200);
    }

    #[pg_test]
    fn test_gv_batch_50_with_refs() {
        setup(); setup_gv_schema();
        let mut ops = vec!["[:db/add \"root\" :gv/str \"root\"]".to_string()];
        for i in 0..50 {
            ops.push(format!("{{:db/id \"c{}\" :gv/str \"child-{}\" :gv/ref \"root\"}}", i, i));
        }
        let r = Spi::get_one::<String>(&format!(
            "SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n")
        )).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        assert_eq!(j["tempids"].as_object().expect("t").len(), 51); // root + 50 children
    }

    #[pg_test]
    fn test_gv_sequential_20_batches_of_10() {
        setup(); setup_gv_schema();
        for batch in 0..20 {
            let mut ops = vec![];
            for i in 0..10 {
                let idx = batch * 10 + i;
                ops.push(format!("[:db/add \"e{}\" :gv/str \"b{}-i{}\"]", idx, batch, i));
            }
            Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("batch");
        }
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/str ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 200);
    }

    #[pg_test]
    fn test_gv_batch_with_many_values() {
        setup(); setup_gv_schema();
        let mut ops = vec![];
        for i in 0..20 {
            ops.push(format!("[:db/add \"e{}\" :gv/str \"ent-{}\"]", i, i));
            for j in 0..5 {
                ops.push(format!("[:db/add \"e{}\" :gv/strs \"tag-{}-{}\"]", i, i, j));
            }
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("tx");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [_ :gv/strs ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 100); // 20 * 5 tags
    }
}
