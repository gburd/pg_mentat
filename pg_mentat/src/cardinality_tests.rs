// Exhaustive cardinality tests: one vs many behavior across all operations.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_card_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"so\" :db/ident :cd/str-one :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"sm\" :db/ident :cd/str-many :db/valueType :db.type/string :db/cardinality :db.cardinality/many}
                {:db/id \"lo\" :db/ident :cd/long-one :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"lm\" :db/ident :cd/long-many :db/valueType :db.type/long :db/cardinality :db.cardinality/many}
                {:db/id \"do\" :db/ident :cd/dbl-one :db/valueType :db.type/double :db/cardinality :db.cardinality/one}
                {:db/id \"dm\" :db/ident :cd/dbl-many :db/valueType :db.type/double :db/cardinality :db.cardinality/many}
                {:db/id \"ro\" :db/ident :cd/ref-one :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
                {:db/id \"rm\" :db/ident :cd/ref-many :db/valueType :db.type/ref :db/cardinality :db.cardinality/many}
                {:db/id \"bo\" :db/ident :cd/bool-one :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"ko\" :db/ident :cd/kw-one :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"km\" :db/ident :cd/kw-many :db/valueType :db.type/keyword :db/cardinality :db.cardinality/many}
                {:db/id \"n\"  :db/ident :cd/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("card schema");
    }

    // ========================================================================
    // Cardinality ONE - String
    // ========================================================================

    #[pg_test]
    fn test_cd_one_string_replace() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cd/str-one \"first\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :cd/str-one \"second\"]]'::TEXT)", eid)).expect("replace");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cd/str-one ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "second");
    }

    #[pg_test]
    fn test_cd_one_string_idempotent() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cd/str-one \"same\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Re-add same value 5 times
        for _ in 0..5 {
            Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :cd/str-one \"same\"]]'::TEXT)", eid)).expect("idem");
        }

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':cd/str-one')
             AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_cd_one_string_replace_chain() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cd/str-one \"v0\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for i in 1..=20 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :cd/str-one \"v{}\"]]'::TEXT)", eid, i
            )).expect("replace");
        }

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cd/str-one ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "v20");
    }

    // ========================================================================
    // Cardinality MANY - String
    // ========================================================================

    #[pg_test]
    fn test_cd_many_string_accumulate() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cd/name \"holder\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :cd/str-many \"val-{}\"]]'::TEXT)", eid, i
            )).expect("add");
        }

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :cd/str-many ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 10);
    }

    #[pg_test]
    fn test_cd_many_string_no_duplicate() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cd/name \"dedup\"]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        for _ in 0..5 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add {} :cd/str-many \"same\"]]'::TEXT)", eid
            )).expect("add");
        }

        let count = Spi::get_one::<i64>(&format!(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE e = {} AND a = (SELECT entid FROM mentat.idents WHERE ident = ':cd/str-many')
             AND v_text = 'same' AND added = true", eid
        )).expect("q").expect("NULL");
        assert_eq!(count, 1, "Duplicate many adds should be idempotent");
    }

    #[pg_test]
    fn test_cd_many_string_retract_one() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :cd/name \"retr\"]
                [:db/add \"e\" :cd/str-many \"keep\"]
                [:db/add \"e\" :cd/str-many \"remove\"]
                [:db/add \"e\" :cd/str-many \"also-keep\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :cd/str-many \"remove\"]]'::TEXT)", eid
        )).expect("retract");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :cd/str-many ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        let arr = v["result"].as_array().expect("arr");
        assert_eq!(arr.len(), 2);
    }

    #[pg_test]
    fn test_cd_many_string_retract_all() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"e\" :cd/name \"empty\"]
                [:db/add \"e\" :cd/str-many \"a\"]
                [:db/add \"e\" :cd/str-many \"b\"]
                [:db/add \"e\" :cd/str-many \"c\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        Spi::run(&format!(
            "SELECT mentat_transact('[
                [:db/retract {} :cd/str-many \"a\"]
                [:db/retract {} :cd/str-many \"b\"]
                [:db/retract {} :cd/str-many \"c\"]
            ]'::TEXT)", eid, eid, eid
        )).expect("retract all");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :cd/str-many ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 0);
    }

    // ========================================================================
    // Cardinality ONE - Long
    // ========================================================================

    #[pg_test]
    fn test_cd_one_long_replace() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cd/long-one 10]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :cd/long-one 20]]'::TEXT)", eid)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cd/long-one ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("l"), 20);
    }

    // ========================================================================
    // Cardinality MANY - Long
    // ========================================================================

    #[pg_test]
    fn test_cd_many_long_accumulate() {
        setup(); setup_card_schema();
        let mut ops = vec!["[:db/add \"e\" :cd/name \"lnums\"]".to_string()];
        for i in 0..15 {
            ops.push(format!("[:db/add \"e\" :cd/long-many {}]", i * 10));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("add");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :cd/name \"lnums\"] [?e :cd/long-many ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 15);
    }

    // ========================================================================
    // Cardinality ONE - Double
    // ========================================================================

    #[pg_test]
    fn test_cd_one_double_replace() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cd/dbl-one 1.0]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :cd/dbl-one 2.0]]'::TEXT)", eid)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cd/dbl-one ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert!((v["result"].as_f64().expect("d") - 2.0).abs() < 0.01);
    }

    // ========================================================================
    // Cardinality MANY - Double
    // ========================================================================

    #[pg_test]
    fn test_cd_many_double_accumulate() {
        setup(); setup_card_schema();
        let mut ops = vec!["[:db/add \"e\" :cd/name \"dbls\"]".to_string()];
        for i in 0..8 {
            // {:?} keeps the decimal point (0 -> "0.0") so EDN reads a double.
            ops.push(format!("[:db/add \"e\" :cd/dbl-many {:?}]", (i as f64) * 0.5));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("add");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :cd/name \"dbls\"] [?e :cd/dbl-many ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 8);
    }

    // ========================================================================
    // Cardinality ONE - Ref
    // ========================================================================

    #[pg_test]
    fn test_cd_one_ref_replace() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"a\" :cd/name \"target-a\"]
                [:db/add \"b\" :cd/name \"target-b\"]
                [:db/add \"e\" :cd/name \"refholder\"]
                [:db/add \"e\" :cd/ref-one \"a\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let b = j["tempids"]["b"].as_i64().expect("b");
        let e = j["tempids"]["e"].as_i64().expect("e");

        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :cd/ref-one {}]]'::TEXT)", e, b)).expect("replace ref");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?r . :where [{} :cd/ref-one ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", e
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_i64().expect("ref"), b);
    }

    // ========================================================================
    // Cardinality MANY - Ref
    // ========================================================================

    #[pg_test]
    fn test_cd_many_ref_accumulate() {
        setup(); setup_card_schema();
        let mut ops = vec!["[:db/add \"hub\" :cd/name \"hub\"]".to_string()];
        for i in 0..5 {
            ops.push(format!("[:db/add \"s{}\" :cd/name \"spoke-{}\"]", i, i));
            ops.push(format!("[:db/add \"hub\" :cd/ref-many \"s{}\"]", i));
        }
        Spi::run(&format!("SELECT mentat_transact('[{}]'::TEXT)", ops.join("\n"))).expect("add");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?r ...] :where [?e :cd/name \"hub\"] [?e :cd/ref-many ?r]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 5);
    }

    #[pg_test]
    fn test_cd_many_ref_retract_one() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                [:db/add \"hub\" :cd/name \"hub2\"]
                [:db/add \"s0\" :cd/name \"keep\"]
                [:db/add \"s1\" :cd/name \"remove\"]
                [:db/add \"hub\" :cd/ref-many \"s0\"]
                [:db/add \"hub\" :cd/ref-many \"s1\"]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let hub = j["tempids"]["hub"].as_i64().expect("hub");
        let s1 = j["tempids"]["s1"].as_i64().expect("s1");

        Spi::run(&format!(
            "SELECT mentat_transact('[[:db/retract {} :cd/ref-many {}]]'::TEXT)", hub, s1
        )).expect("retract ref");

        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?r ...] :where [{} :cd/ref-many ?r]]'::TEXT, '{{}}'::jsonb)::TEXT", hub
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 1);
    }

    // ========================================================================
    // Cardinality ONE - Boolean
    // ========================================================================

    #[pg_test]
    fn test_cd_one_bool_replace() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[[:db/add \"e\" :cd/bool-one true]]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");
        Spi::run(&format!("SELECT mentat_transact('[[:db/add {} :cd/bool-one false]]'::TEXT)", eid)).expect("replace");
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?v . :where [{} :cd/bool-one ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_bool().expect("b"), false);
    }

    // ========================================================================
    // Cardinality MANY - Keyword
    // ========================================================================

    #[pg_test]
    fn test_cd_many_keyword_accumulate() {
        setup(); setup_card_schema();
        Spi::run(
            "SELECT mentat_transact('[
                [:db/add \"e\" :cd/name \"kwhold\"]
                [:db/add \"e\" :cd/kw-many :tag-a]
                [:db/add \"e\" :cd/kw-many :tag-b]
                [:db/add \"e\" :cd/kw-many :tag-c]
            ]'::TEXT)",
        ).expect("add");
        let q = Spi::get_one::<String>(
            "SELECT mentat_query('[:find [?v ...] :where [?e :cd/name \"kwhold\"] [?e :cd/kw-many ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        ).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_array().expect("arr").len(), 3);
    }

    // ========================================================================
    // Batch: mixed cardinalities
    // ========================================================================

    #[pg_test]
    fn test_cd_batch_mixed_cardinalities() {
        setup(); setup_card_schema();
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"e\"
                 :cd/name \"mixed\"
                 :cd/str-one \"only-one\"
                 :cd/long-one 42
                 :cd/bool-one true
                 :cd/kw-one :active}
                [:db/add \"e\" :cd/str-many \"a\"]
                [:db/add \"e\" :cd/str-many \"b\"]
                [:db/add \"e\" :cd/str-many \"c\"]
                [:db/add \"e\" :cd/long-many 1]
                [:db/add \"e\" :cd/long-many 2]
            ]'::TEXT)",
        ).expect("tx").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse");
        let eid = j["tempids"]["e"].as_i64().expect("eid");

        // Verify one attributes
        let q = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find ?s . :where [{} :cd/str-one ?s]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v: serde_json::Value = serde_json::from_str(&q).expect("parse");
        assert_eq!(v["result"].as_str().expect("s"), "only-one");

        // Verify many string attributes
        let q2 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :cd/str-many ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v2: serde_json::Value = serde_json::from_str(&q2).expect("parse");
        assert_eq!(v2["result"].as_array().expect("arr").len(), 3);

        // Verify many long attributes
        let q3 = Spi::get_one::<String>(&format!(
            "SELECT mentat_query('[:find [?v ...] :where [{} :cd/long-many ?v]]'::TEXT, '{{}}'::jsonb)::TEXT", eid
        )).expect("q").expect("NULL");
        let v3: serde_json::Value = serde_json::from_str(&q3).expect("parse");
        assert_eq!(v3["result"].as_array().expect("arr").len(), 2);
    }
}
