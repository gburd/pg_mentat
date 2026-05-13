// Regression tests for rule invocations inside OR branches and inside (or (and ...)).
//
// Prior to this fix, the query compiler returned :db.error/unsupported-query
// for any rule invocation `(rule-name ?args)` inside an or-join branch.
// These tests pin the now-supported behaviour: the engine emits CTE
// definitions for every unique rule referenced anywhere in the query, and
// each OR arm joins against the rules it actually invokes.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;
    use std::collections::HashSet;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    /// Install schema and the five-person dataset used by the basic tests.
    /// Returns (p1..p5) entids resolved from the transaction's tempids.
    fn setup_or_rule_data() -> [i64; 5] {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :person/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"r\" :db/ident :person/role :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :person/superuser? :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");

        // p1: 30, manager, not super       -> adult ✓, manager ✓
        // p2: 16, manager, not super       -> adult ✗, manager ✓
        // p3: 25, contributor, not super   -> adult ✓, manager ✗
        // p4: 12, contributor, super       -> adult ✗, manager ✗, superuser
        // p5: 50, contributor, super       -> adult ✓, manager ✗, superuser
        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"p1\" :person/name \"alice\" :person/age 30 :person/role :role/manager     :person/superuser? false}
                {:db/id \"p2\" :person/name \"bob\"   :person/age 16 :person/role :role/manager     :person/superuser? false}
                {:db/id \"p3\" :person/name \"carol\" :person/age 25 :person/role :role/contributor :person/superuser? false}
                {:db/id \"p4\" :person/name \"dave\"  :person/age 12 :person/role :role/contributor :person/superuser? true}
                {:db/id \"p5\" :person/name \"eve\"   :person/age 50 :person/role :role/contributor :person/superuser? true}
            ]'::TEXT)::TEXT",
        )
        .expect("data tx")
        .expect("NULL tx report");
        let j: serde_json::Value = serde_json::from_str(&r).expect("parse tx report");
        let p1 = j["tempids"]["p1"].as_i64().expect("p1");
        let p2 = j["tempids"]["p2"].as_i64().expect("p2");
        let p3 = j["tempids"]["p3"].as_i64().expect("p3");
        let p4 = j["tempids"]["p4"].as_i64().expect("p4");
        let p5 = j["tempids"]["p5"].as_i64().expect("p5");
        [p1, p2, p3, p4, p5]
    }

    /// Run a `:find ?p` (relation form) query and return the entid set.
    fn query_p_set(q: &str) -> HashSet<i64> {
        let sql = format!(
            "SELECT mentat_query('{}'::TEXT, '{{}}'::jsonb)::TEXT",
            q.replace('\'', "''")
        );
        let raw = Spi::get_one::<String>(&sql).expect("query").expect("NULL");
        let j: serde_json::Value = serde_json::from_str(&raw).expect("parse query result");
        let rows = j["results"].as_array().expect("results array");
        rows.iter()
            .map(|row| {
                row.as_array()
                    .expect("row is array")
                    .first()
                    .expect("row has first element")
                    .as_i64()
                    .expect("entid is i64")
            })
            .collect()
    }

    /// (or PATTERN (rule)) — rule invocation as one branch of an or-join.
    ///
    /// rule (adult ?p): ?p has :person/age >= 18.
    /// query: superuser? true OR adult.
    ///
    /// Expected: union of {superuser=true} and {adult}.
    ///   superuser=true  -> p4, p5
    ///   adult (>=18)    -> p1, p3, p5
    ///   union           -> p1, p3, p4, p5  (p2: 16, !super; excluded)
    #[pg_test]
    fn pg_test_or_with_rule_in_one_branch() {
        setup();
        let [p1, p2, p3, p4, p5] = setup_or_rule_data();

        let q = "[:find ?p \
                 :with [[(adult ?p) [?p :person/age ?a] [(>= ?a 18)]]] \
                 :where [?p :person/name _] \
                        (or [?p :person/superuser? true] \
                            (adult ?p))]";
        let got = query_p_set(q);

        let expected: HashSet<i64> = [p1, p3, p4, p5].into_iter().collect();
        assert_eq!(
            got, expected,
            "or+rule result mismatch. got={:?} expected={:?} (p2={} should be excluded: age 16 < 18 and not super)",
            got, expected, p2,
        );
    }

    /// (or (and PATTERN (rule)) PATTERN) — rule invocation inside an `(and ...)`
    /// arm.
    ///
    /// Arm 1: role = manager AND adult (>=18).  -> p1   (p2 excluded, age 16)
    /// Arm 2: superuser? = true.                -> p4, p5
    /// Union: p1, p4, p5
    #[pg_test]
    fn pg_test_or_and_with_rule() {
        setup();
        let [p1, p2, p3, p4, p5] = setup_or_rule_data();

        let q = "[:find ?p \
                 :with [[(adult ?p) [?p :person/age ?a] [(>= ?a 18)]]] \
                 :where (or (and [?p :person/role :role/manager] (adult ?p)) \
                            [?p :person/superuser? true])]";
        let got = query_p_set(q);

        let expected: HashSet<i64> = [p1, p4, p5].into_iter().collect();
        assert_eq!(
            got, expected,
            "or+(and+rule) mismatch. got={:?} expected={:?} (p2={}, p3={} excluded)",
            got, expected, p2, p3,
        );
    }

    /// Two branches each invoke a different rule (one rule each).
    /// Verifies that build_rule_ctes emits CTE definitions for both,
    /// and that each arm gets the right per-arm RuleCteInfo.
    ///
    /// rule (adult ?p):    age >= 18.
    /// rule (manager ?p):  role = :role/manager.
    /// query: adult OR manager.
    ///
    /// Expected:
    ///   adult           -> p1, p3, p5
    ///   manager         -> p1, p2
    ///   union           -> p1, p2, p3, p5  (p4 excluded: age 12, contributor)
    #[pg_test]
    fn pg_test_or_with_different_rule_per_branch() {
        setup();
        let [p1, p2, p3, p4, p5] = setup_or_rule_data();

        let q = "[:find ?p \
                 :with [[(adult ?p) [?p :person/age ?a] [(>= ?a 18)]] \
                              [(manager ?p) [?p :person/role :role/manager]]] \
                 :where [?p :person/name _] \
                        (or (adult ?p) (manager ?p))]";
        let got = query_p_set(q);

        let expected: HashSet<i64> = [p1, p2, p3, p5].into_iter().collect();
        assert_eq!(
            got, expected,
            "two-rules-in-or mismatch. got={:?} expected={:?} (p4={} excluded: contributor age 12)",
            got, expected, p4,
        );
    }
}
