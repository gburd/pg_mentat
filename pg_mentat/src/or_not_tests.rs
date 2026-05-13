// Regression tests for NOT clauses inside OR branches and inside (or (and ...)).
//
// Prior to the fix in this commit, the query compiler returned
// :db.error/unsupported-query for any (not ...) clause that appeared inside an
// or-join branch. These tests pin the now-supported behaviour so that
// `or` + `not` continues to compile to NOT EXISTS subqueries scoped to each arm.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;
    use std::collections::HashSet;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    /// Install schema and the five fixed person rows used by both tests.
    /// Returns the (p1..p5) entids resolved from the transaction's tempids.
    fn setup_or_not_data() -> [i64; 5] {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\" :db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
                {:db/id \"a\" :db/ident :person/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
                {:db/id \"b\" :db/ident :person/banned? :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"s\" :db/ident :person/superuser? :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        )
        .expect("schema tx");

        let r = Spi::get_one::<String>(
            "SELECT mentat_transact('[
                {:db/id \"p1\" :person/name \"alice\" :person/age 30 :person/banned? false :person/superuser? false}
                {:db/id \"p2\" :person/name \"bob\"   :person/age 25 :person/banned? true  :person/superuser? false}
                {:db/id \"p3\" :person/name \"carol\" :person/age 30 :person/banned? true  :person/superuser? false}
                {:db/id \"p4\" :person/name \"dave\"  :person/age 18 :person/banned? false :person/superuser? true}
                {:db/id \"p5\" :person/name \"eve\"   :person/age 17 :person/banned? false :person/superuser? false}
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

    /// Helper: run a `:find ?p` (relation form) query and return the set of
    /// entids in the first column.
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

    /// (or PATTERN (not PATTERN)) — NOT appears as the only clause in one
    /// branch of an or-join. The Datalog-canonical form needs a base pattern
    /// to bind ?p so the second arm has something to negate against; the
    /// implementation rejects an arm that consists solely of NOT (because
    /// such an arm has no FROM clause). The shared `[?p :person/name _]`
    /// supplies that binding for both arms.
    ///
    /// Expected: union of (age=30) and (not banned).
    ///   age=30   -> p1, p3
    ///   !banned  -> p1, p4, p5
    ///   union    -> p1, p3, p4, p5
    #[pg_test]
    fn pg_test_or_with_not_in_one_branch() {
        setup();
        let [p1, p2, p3, p4, p5] = setup_or_not_data();

        let q = "[:find ?p :where [?p :person/name _] \
                 (or [?p :person/age 30] \
                     (not [?p :person/banned? true]))]";
        let got = query_p_set(q);

        let expected: HashSet<i64> = [p1, p3, p4, p5].into_iter().collect();
        assert_eq!(
            got, expected,
            "or+not result mismatch. got={:?} expected={:?} (p2={} should be excluded: banned and age!=30)",
            got, expected, p2,
        );
    }

    /// (or (and PATTERN PRED (not PATTERN)) PATTERN) — NOT inside an `(and ...)`
    /// arm of an or-join. Verifies that arm-local NOT joins compose with
    /// arm-local patterns and predicates, and that other arms remain
    /// independent.
    ///
    /// Expected: union of
    ///   arm 1 (age >= 18 AND NOT banned): p1 (30, !banned), p4 (18, !banned)
    ///         excluded: p2 (banned), p3 (banned), p5 (17)
    ///   arm 2 (superuser=true): p4
    ///   union: p1, p4
    #[pg_test]
    fn pg_test_or_and_with_not() {
        setup();
        let [p1, p2, p3, p4, p5] = setup_or_not_data();

        let q = "[:find ?p :where \
                 (or (and [?p :person/age ?a] [(>= ?a 18)] (not [?p :person/banned? true])) \
                     [?p :person/superuser? true])]";
        let got = query_p_set(q);

        let expected: HashSet<i64> = [p1, p4].into_iter().collect();
        assert_eq!(
            got, expected,
            "or(and+not) result mismatch. got={:?} expected={:?} \
             (excluded: p2={} banned, p3={} banned, p5={} underage)",
            got, expected, p2, p3, p5,
        );
    }
}
