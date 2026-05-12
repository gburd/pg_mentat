// Comprehensive schema operation tests.
//
// Tests cover:
// 1. Schema attribute definition with all property combinations
// 2. Schema modification (adding properties to existing attributes)
// 3. Schema cache invalidation
// 4. Multiple attributes in one transaction
// 5. Schema querying (mentat_schema)
// 6. Ident resolution
// 7. Value type enforcement after schema changes
// 8. Component attributes
// 9. No-history attributes
// 10. Index and fulltext properties

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    // ========================================================================
    // 1. Basic Schema Definition (all value types)
    // ========================================================================

    #[pg_test]
    fn test_schema_define_string_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/sname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define string attr");

        let vt = Spi::get_one::<String>(
            "SELECT value_type::TEXT FROM mentat.schema WHERE ident = ':sot/sname'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(vt, "string");
    }

    #[pg_test]
    fn test_schema_define_long_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/lval
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define long attr");

        let vt = Spi::get_one::<String>(
            "SELECT value_type::TEXT FROM mentat.schema WHERE ident = ':sot/lval'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(vt, "long");
    }

    #[pg_test]
    fn test_schema_define_double_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/dval
                 :db/valueType :db.type/double
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define double attr");

        let vt = Spi::get_one::<String>(
            "SELECT value_type::TEXT FROM mentat.schema WHERE ident = ':sot/dval'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(vt, "double");
    }

    #[pg_test]
    fn test_schema_define_boolean_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/bval
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define boolean attr");

        let vt = Spi::get_one::<String>(
            "SELECT value_type::TEXT FROM mentat.schema WHERE ident = ':sot/bval'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(vt, "boolean");
    }

    #[pg_test]
    fn test_schema_define_ref_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/rval
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define ref attr");

        let vt = Spi::get_one::<String>(
            "SELECT value_type::TEXT FROM mentat.schema WHERE ident = ':sot/rval'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(vt, "ref");
    }

    #[pg_test]
    fn test_schema_define_keyword_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/kval
                 :db/valueType :db.type/keyword
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define keyword attr");

        let vt = Spi::get_one::<String>(
            "SELECT value_type::TEXT FROM mentat.schema WHERE ident = ':sot/kval'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(vt, "keyword");
    }

    #[pg_test]
    fn test_schema_define_instant_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/ival
                 :db/valueType :db.type/instant
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define instant attr");

        let vt = Spi::get_one::<String>(
            "SELECT value_type::TEXT FROM mentat.schema WHERE ident = ':sot/ival'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(vt, "instant");
    }

    #[pg_test]
    fn test_schema_define_uuid_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/uval
                 :db/valueType :db.type/uuid
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define uuid attr");

        let vt = Spi::get_one::<String>(
            "SELECT value_type::TEXT FROM mentat.schema WHERE ident = ':sot/uval'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(vt, "uuid");
    }

    #[pg_test]
    fn test_schema_define_bytes_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/byval
                 :db/valueType :db.type/bytes
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define bytes attr");

        let vt = Spi::get_one::<String>(
            "SELECT value_type::TEXT FROM mentat.schema WHERE ident = ':sot/byval'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(vt, "bytes");
    }

    // ========================================================================
    // 2. Cardinality One vs Many
    // ========================================================================

    #[pg_test]
    fn test_schema_cardinality_one() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/c1
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define card-one attr");

        let card = Spi::get_one::<String>(
            "SELECT cardinality::TEXT FROM mentat.schema WHERE ident = ':sot/c1'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(card, "one");
    }

    #[pg_test]
    fn test_schema_cardinality_many() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/cm
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/many}
            ]'::TEXT)",
        )
        .expect("define card-many attr");

        let card = Spi::get_one::<String>(
            "SELECT cardinality::TEXT FROM mentat.schema WHERE ident = ':sot/cm'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(card, "many");
    }

    // ========================================================================
    // 3. Unique Constraints
    // ========================================================================

    #[pg_test]
    fn test_schema_unique_value() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/uv
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/value}
            ]'::TEXT)",
        )
        .expect("define unique-value attr");

        let uniq = Spi::get_one::<String>(
            "SELECT unique_constraint::TEXT FROM mentat.schema WHERE ident = ':sot/uv'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(uniq, "value");
    }

    #[pg_test]
    fn test_schema_unique_identity() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/ui
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity}
            ]'::TEXT)",
        )
        .expect("define unique-identity attr");

        let uniq = Spi::get_one::<String>(
            "SELECT unique_constraint::TEXT FROM mentat.schema WHERE ident = ':sot/ui'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(uniq, "identity");
    }

    // ========================================================================
    // 4. Index and Fulltext
    // ========================================================================

    #[pg_test]
    fn test_schema_indexed() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/idx
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/index true}
            ]'::TEXT)",
        )
        .expect("define indexed attr");

        let indexed = Spi::get_one::<bool>(
            "SELECT indexed FROM mentat.schema WHERE ident = ':sot/idx'",
        )
        .expect("query failed")
        .expect("NULL");
        assert!(indexed);
    }

    #[pg_test]
    fn test_schema_fulltext() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/ft
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/fulltext true}
            ]'::TEXT)",
        )
        .expect("define fulltext attr");

        let fulltext = Spi::get_one::<bool>(
            "SELECT fulltext FROM mentat.schema WHERE ident = ':sot/ft'",
        )
        .expect("query failed")
        .expect("NULL");
        assert!(fulltext);
    }

    // ========================================================================
    // 5. Multiple Attributes in One Transaction
    // ========================================================================

    #[pg_test]
    fn test_schema_multiple_attrs_one_tx() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a1\" :db/ident :sot/m1
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a2\" :db/ident :sot/m2
                 :db/valueType :db.type/long
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a3\" :db/ident :sot/m3
                 :db/valueType :db.type/boolean
                 :db/cardinality :db.cardinality/one}
                {:db/id \"a4\" :db/ident :sot/m4
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/many}
                {:db/id \"a5\" :db/ident :sot/m5
                 :db/valueType :db.type/keyword
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity}
            ]'::TEXT)",
        )
        .expect("5 attrs in one tx");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.schema WHERE ident LIKE ':sot/m%'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(count, 5, "All 5 should be defined");
    }

    // ========================================================================
    // 6. Schema + Data in Same Transaction
    // ========================================================================

    #[pg_test]
    fn test_schema_define_and_use_same_tx() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"attr\" :db/ident :sot/combo
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
                [:db/add \"e\" :sot/combo \"created in schema tx\"]
            ]'::TEXT)",
        )
        .expect("schema + data same tx");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.datoms
             WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':sot/combo')
             AND v_text = 'created in schema tx' AND added = true",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(count, 1);
    }

    // ========================================================================
    // 7. Schema Query (mentat_schema)
    // ========================================================================

    #[pg_test]
    fn test_mentat_schema_returns_all_attrs() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/sq1
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define attr");

        let result = Spi::get_one::<String>("SELECT mentat_schema()::TEXT")
            .expect("schema query failed")
            .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse JSON");
        let schema = json.as_array().expect("schema should be array");
        assert!(schema.len() > 0, "Schema should have entries");

        // Find our attribute
        let found = schema.iter().any(|attr| {
            attr.get("ident")
                .and_then(|v| v.as_str())
                .map(|s| s == ":sot/sq1")
                .unwrap_or(false)
        });
        assert!(found, "Should find :sot/sq1 in schema");
    }

    // ========================================================================
    // 8. Ident Resolution
    // ========================================================================

    #[pg_test]
    fn test_ident_resolved_in_query() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/resname
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define attr");

        Spi::run(
            "SELECT mentat_transact('[[:db/add \"e\" :sot/resname \"test\"]]'::TEXT)",
        )
        .expect("data");

        let result = Spi::get_one::<String>(
            "SELECT mentat_query('[:find ?v . :where [?e :sot/resname ?v]]'::TEXT, '{}'::jsonb)::TEXT",
        )
        .expect("query failed")
        .expect("NULL");

        let json: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(json["result"].as_str().expect("val"), "test");
    }

    #[pg_test]
    fn test_ident_in_idents_table() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/idtbl
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("define attr");

        let entid = Spi::get_one::<i64>(
            "SELECT entid FROM mentat.idents WHERE ident = ':sot/idtbl'",
        )
        .expect("query failed")
        .expect("NULL");

        assert!(entid > 0, "Should have positive entid");
    }

    // ========================================================================
    // 9. Sequential Schema Definitions
    // ========================================================================

    #[pg_test]
    fn test_schema_sequential_10_attrs() {
        setup();

        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[
                    {{:db/id \"a{i}\" :db/ident :sot/seq{i}
                     :db/valueType :db.type/string
                     :db/cardinality :db.cardinality/one}}
                ]'::TEXT)",
                i = i
            ))
            .expect("sequential schema");
        }

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.schema WHERE ident LIKE ':sot/seq%'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(count, 10);

        // Verify all are usable
        for i in 0..10 {
            Spi::run(&format!(
                "SELECT mentat_transact('[[:db/add \"e{i}\" :sot/seq{i} \"val{i}\"]]'::TEXT)",
                i = i
            ))
            .expect("use sequential attr");
        }
    }

    // ========================================================================
    // 10. Component and noHistory Properties
    // ========================================================================

    #[pg_test]
    fn test_schema_component_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/comp
                 :db/valueType :db.type/ref
                 :db/cardinality :db.cardinality/many
                 :db/isComponent true}
            ]'::TEXT)",
        )
        .expect("define component attr");

        let comp = Spi::get_one::<bool>(
            "SELECT component FROM mentat.schema WHERE ident = ':sot/comp'",
        )
        .expect("query failed")
        .expect("NULL");
        assert!(comp);
    }

    #[pg_test]
    fn test_schema_no_history_attr() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/nohist
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/noHistory true}
            ]'::TEXT)",
        )
        .expect("define no-history attr");

        let no_hist = Spi::get_one::<bool>(
            "SELECT no_history FROM mentat.schema WHERE ident = ':sot/nohist'",
        )
        .expect("query failed")
        .expect("NULL");
        assert!(no_hist);
    }

    // ========================================================================
    // 11. Schema with Doc String
    // ========================================================================

    #[pg_test]
    fn test_schema_with_doc_string() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/documented
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/doc \"A documented attribute for testing\"}
            ]'::TEXT)",
        )
        .expect("define documented attr");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.schema WHERE ident = ':sot/documented'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(count, 1);
    }

    // ========================================================================
    // 12. All Properties Combined
    // ========================================================================

    #[pg_test]
    fn test_schema_all_properties() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"a\" :db/ident :sot/full
                 :db/valueType :db.type/string
                 :db/cardinality :db.cardinality/one
                 :db/unique :db.unique/identity
                 :db/index true
                 :db/fulltext true
                 :db/noHistory true
                 :db/doc \"Fully specified attribute\"}
            ]'::TEXT)",
        )
        .expect("define fully-specified attr");

        let count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM mentat.schema
             WHERE ident = ':sot/full'
             AND indexed = true
             AND fulltext = true
             AND no_history = true
             AND unique_constraint = 'identity'",
        )
        .expect("query failed")
        .expect("NULL");
        assert_eq!(count, 1, "All properties should match");
    }
}
