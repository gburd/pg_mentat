// Tests for the optional mino scripting layer (`mentat.store/*`, exposed via
// the `mentat_eval` SQL function). Gated behind the `script` feature.
//
// These mirror `mentat/tests/mino_script.rs`, proving the Datomic-a-like model
// against the live Postgres-resident store: `db` yields an immutable database
// VALUE carrying its basis-tx, reads take a db value, `with` is a pure
// speculative `db -> db'` that does not commit, and `as-of`/`since` produce db
// values whose reads reflect the basis. Unlike Mentat, pg_mentat backs full `q`
// on a historical basis (its `mentat_query` accepts asOf/since inputs).

#![cfg(feature = "script")]

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    /// Run a mino script through `mentat_eval`, returning its EDN result text.
    fn eval(script: &str) -> String {
        Spi::get_one_with_args::<String>(
            "SELECT mentat_eval($1)",
            &[pgrx::datum::DatumWithOid::from(script)],
        )
        .expect("mentat_eval SPI failed")
        .expect("mentat_eval returned NULL")
    }

    /// Seed: define :person/name and assert Alice.
    fn seed() {
        setup();
        eval(
            "(mentat.store/transact (mentat.store/open) \
               [{:db/ident :person/name \
                 :db/valueType :db.type/string \
                 :db/cardinality :db.cardinality/one}])",
        );
        eval("(mentat.store/transact (mentat.store/open) [{:person/name \"Alice\"}])");
    }

    /// The eid of the person named `name`.
    fn eid_of(name: &str) -> i64 {
        eval(&format!(
            "(mentat.store/q (mentat.store/db (mentat.store/open)) \
               '[:find ?e . :where [?e :person/name \"{name}\"]])"
        ))
        .parse()
        .expect("eid should parse")
    }

    #[pg_test]
    fn eval_basic_arithmetic() {
        setup();
        assert_eq!(eval("(+ 1 2)"), "3");
    }

    #[pg_test]
    fn prim_can_reach_postgres() {
        setup();
        // open -> conn handle 1 (there is exactly one database).
        assert_eq!(eval("(mentat.store/open)"), "1");
        // db reaches SPI for the current basis.
        let db = eval("(mentat.store/db (mentat.store/open))");
        assert!(db.contains(":mentat.store/db true"), "db value: {db}");
    }

    #[pg_test]
    fn db_is_an_immutable_value_not_the_conn() {
        seed();
        let db = eval("(mentat.store/db (mentat.store/open))");
        assert!(db.contains(":mentat.store/db true"), "db value: {db}");
        assert!(db.contains(":mentat.store/basis-tx"), "db value: {db}");
        assert_ne!(db, "1");

        let basis: i64 = eval("(:mentat.store/basis-tx (mentat.store/db (mentat.store/open)))")
            .parse()
            .unwrap();
        assert!(basis > 0, "basis-tx should be a real tx id, got {basis}");
    }

    #[pg_test]
    fn q_takes_a_db_value() {
        seed();
        let got = eval(
            "(mentat.store/q (mentat.store/db (mentat.store/open)) \
               '[:find ?n :where [?e :person/name ?n]])",
        );
        assert!(got.contains("Alice"), "q result: {got}");

        // A bare conn is accepted for ergonomics.
        let via_conn =
            eval("(mentat.store/q (mentat.store/open) '[:find ?n :where [?e :person/name ?n]])");
        assert!(via_conn.contains("Alice"), "q via conn: {via_conn}");
    }

    #[pg_test]
    fn pull_returns_a_map() {
        seed();
        let eid = eid_of("Alice");
        let pulled = eval(&format!(
            "(mentat.store/pull (mentat.store/db (mentat.store/open)) {eid} [:person/name])"
        ));
        assert!(pulled.contains(":person/name \"Alice\""), "pull: {pulled}");
    }

    #[pg_test]
    fn entity_returns_an_entity_map() {
        seed();
        let eid = eid_of("Alice");
        let ent = eval(&format!(
            "(mentat.store/entity (mentat.store/db (mentat.store/open)) {eid})"
        ));
        assert!(ent.contains(&format!(":db/id {eid}")), "entity: {ent}");
        assert!(ent.contains(":person/name \"Alice\""), "entity: {ent}");
    }

    #[pg_test]
    fn read_returns_the_scalar() {
        seed();
        let eid = eid_of("Alice");
        let v = eval(&format!(
            "(mentat.store/read (mentat.store/db (mentat.store/open)) {eid} :person/name)"
        ));
        assert_eq!(v, "\"Alice\"");
    }

    #[pg_test]
    fn with_is_speculative_and_does_not_commit() {
        seed();

        let with_result = eval(
            "(mentat.store/with (mentat.store/db (mentat.store/open)) \
               [{:person/name \"Bob\"}])",
        );
        assert!(
            with_result.contains(":mentat.store/db-after"),
            "with result: {with_result}"
        );
        assert!(
            with_result.contains(":mentat.store/tx-report"),
            "with result: {with_result}"
        );

        // CRUCIAL: the store is UNCHANGED. Bob was rolled back, never committed.
        let after = eval(
            "(mentat.store/q (mentat.store/db (mentat.store/open)) \
               '[:find ?n :where [?e :person/name ?n]])",
        );
        assert!(after.contains("Alice"), "with must keep Alice: {after}");
        assert!(!after.contains("Bob"), "with must not commit Bob: {after}");
    }

    #[pg_test]
    fn as_of_and_since_reflect_the_basis() {
        seed();
        let basis_alice: i64 =
            eval("(:mentat.store/basis-tx (mentat.store/db (mentat.store/open)))")
                .parse()
                .unwrap();

        eval("(mentat.store/transact (mentat.store/open) [{:person/name \"Bob\"}])");
        let basis_bob: i64 = eval("(:mentat.store/basis-tx (mentat.store/db (mentat.store/open)))")
            .parse()
            .unwrap();
        assert!(basis_bob > basis_alice);

        // Full q against an as-of basis: Alice only, not Bob (pg_mentat's edge
        // over Mentat -- real temporal query support).
        let as_of_alice = eval(&format!(
            "(mentat.store/q (mentat.store/as-of (mentat.store/db (mentat.store/open)) {basis_alice}) \
               '[:find ?n :where [?e :person/name ?n]])"
        ));
        assert!(as_of_alice.contains("Alice"), "as-of Alice: {as_of_alice}");
        assert!(
            !as_of_alice.contains("Bob"),
            "as-of should exclude Bob: {as_of_alice}"
        );

        // as-of / since return db values carrying the bound.
        let as_of_db = eval(&format!(
            "(mentat.store/as-of (mentat.store/db (mentat.store/open)) {basis_alice})"
        ));
        assert!(
            as_of_db.contains(&format!(":mentat.store/as-of {basis_alice}")),
            "as-of db: {as_of_db}"
        );
        let since_db = eval(&format!(
            "(mentat.store/since (mentat.store/db (mentat.store/open)) {basis_alice})"
        ));
        assert!(
            since_db.contains(&format!(":mentat.store/since {basis_alice}")),
            "since db: {since_db}"
        );
    }

    #[pg_test]
    fn inst_representation_emitted_on_the_output_path() {
        // The instant OUTPUT path is real: `datoms` reconstruction surfaces the
        // per-tx :db/txInstant (instant-typed) datom. pg_mentat renders it via
        // its JSON, and the layer maps that through unchanged. (Note: an #inst
        // LITERAL cannot be fed *in* through eval -- mino's evaluator expands it
        // to a calendar map, per the mino-rs v0.1.0 caveat -- so we assert the
        // output path, exactly as mentat/tests/mino_script.rs does.)
        seed();
        let ds = eval("(mentat.store/datoms (mentat.store/db (mentat.store/open)))");
        // The datom listing carries values for the transacted attrs; the tuple
        // vector shape is [e a v tx added].
        assert!(ds.starts_with('['), "datoms -> vector: {ds}");
        assert!(ds.contains("Alice"), "datoms should include Alice: {ds}");
    }

    #[pg_test]
    fn entities_returns_the_eids_with_an_attribute() {
        seed();
        eval("(mentat.store/transact (mentat.store/open) [{:person/name \"Bob\"}])");
        let s = eval("(mentat.store/entities (mentat.store/db (mentat.store/open)) :person/name)");
        // A set of two eids (Alice + Bob).
        assert!(s.starts_with("#{"), "entities -> set: {s}");
        let eid_alice = eid_of("Alice");
        let eid_bob = eid_of("Bob");
        assert!(
            s.contains(&eid_alice.to_string()) && s.contains(&eid_bob.to_string()),
            "entities set {s} should hold {eid_alice} and {eid_bob}"
        );
    }

    #[pg_test]
    fn close_is_a_noop_for_api_parity() {
        seed();
        assert_eq!(eval("(mentat.store/close (mentat.store/open))"), "nil");
    }

    #[pg_test]
    fn eid_resolves_from_an_ident_keyword() {
        seed();
        // :db/txInstant is a bootstrapped ident; entity by ident keyword works.
        let ent = eval("(mentat.store/entity (mentat.store/db (mentat.store/open)) :db/txInstant)");
        assert!(ent.contains(":db/id"), "entity by ident: {ent}");
    }

    #[pg_test]
    fn eid_resolves_from_a_lookup_ref() {
        setup();
        // A unique-identity attribute so [:attr val] identifies an entity.
        eval(
            "(mentat.store/transact (mentat.store/open) \
               [{:db/ident :person/email \
                 :db/valueType :db.type/string \
                 :db/unique :db.unique/identity \
                 :db/cardinality :db.cardinality/one}])",
        );
        eval(
            "(mentat.store/transact (mentat.store/open) \
               [{:person/email \"a@x.com\"}])",
        );
        // read via a lookup-ref eid.
        let v = eval(
            "(mentat.store/read (mentat.store/db (mentat.store/open)) \
               [:person/email \"a@x.com\"] :person/email)",
        );
        assert_eq!(v, "\"a@x.com\"");
    }

    #[pg_test]
    fn read_cardinality_many_returns_a_set() {
        setup();
        eval(
            "(mentat.store/transact (mentat.store/open) \
               [{:db/ident :person/alias \
                 :db/valueType :db.type/string \
                 :db/cardinality :db.cardinality/many}])",
        );
        eval(
            "(mentat.store/transact (mentat.store/open) \
               [{:db/id \"p\" :person/alias \"Ali\"} \
                {:db/id \"p\" :person/alias \"Al\"}])",
        );
        let eid: i64 = eval(
            "(mentat.store/q (mentat.store/db (mentat.store/open)) \
               '[:find ?e . :where [?e :person/alias \"Ali\"]])",
        )
        .parse()
        .unwrap();
        let s = eval(&format!(
            "(mentat.store/read (mentat.store/db (mentat.store/open)) {eid} :person/alias)"
        ));
        assert!(s.starts_with("#{"), "many -> set: {s}");
        assert!(s.contains("\"Ali\"") && s.contains("\"Al\""), "{s}");
    }

    #[pg_test]
    fn pull_wildcard_returns_all_attributes() {
        seed();
        let eid = eid_of("Alice");
        // The wildcard pattern must be quoted: `*` is not self-evaluating in
        // mino (it is the multiply prim), so `[*]` evaluated is invalid EDN.
        let pulled = eval(&format!(
            "(mentat.store/pull (mentat.store/db (mentat.store/open)) {eid} '[*])"
        ));
        assert!(
            pulled.contains(":person/name \"Alice\""),
            "wildcard pull: {pulled}"
        );
    }

    #[pg_test]
    fn pull_many_returns_a_vector() {
        seed();
        eval("(mentat.store/transact (mentat.store/open) [{:person/name \"Bob\"}])");
        let a = eid_of("Alice");
        let b = eid_of("Bob");
        let pulled = eval(&format!(
            "(mentat.store/pull (mentat.store/db (mentat.store/open)) [{a} {b}] [:person/name])"
        ));
        assert!(pulled.starts_with('['), "pull-many -> vector: {pulled}");
        assert!(
            pulled.contains("Alice") && pulled.contains("Bob"),
            "{pulled}"
        );
    }

    #[pg_test]
    fn q_scalar_and_coll_shapes() {
        seed();
        // Scalar find -> the bare value.
        let scalar = eval(
            "(mentat.store/q (mentat.store/db (mentat.store/open)) \
               '[:find ?n . :where [?e :person/name ?n]])",
        );
        assert_eq!(scalar, "\"Alice\"");
        // Collection find [?n ...] -> a vector.
        let coll = eval(
            "(mentat.store/q (mentat.store/db (mentat.store/open)) \
               '[:find [?n ...] :where [?e :person/name ?n]])",
        );
        assert!(coll.starts_with('['), "coll -> vector: {coll}");
        assert!(coll.contains("Alice"), "{coll}");
    }

    #[pg_test]
    fn datoms_lists_tuples_for_the_basis() {
        seed();
        let ds = eval("(mentat.store/datoms (mentat.store/db (mentat.store/open)))");
        assert!(ds.starts_with('['), "datoms -> vector: {ds}");
        // The Alice name assertion appears among the [e a v tx added] tuples.
        assert!(ds.contains("Alice"), "datoms should include Alice: {ds}");
    }

    #[pg_test]
    fn since_reflects_only_later_datoms() {
        seed();
        let basis_alice: i64 =
            eval("(:mentat.store/basis-tx (mentat.store/db (mentat.store/open)))")
                .parse()
                .unwrap();
        eval("(mentat.store/transact (mentat.store/open) [{:person/name \"Bob\"}])");
        // Full q with a `since` bound: Bob (asserted after) but not Alice.
        let since = eval(&format!(
            "(mentat.store/q (mentat.store/since (mentat.store/db (mentat.store/open)) {basis_alice}) \
               '[:find ?n :where [?e :person/name ?n]])"
        ));
        assert!(since.contains("Bob"), "since should include Bob: {since}");
        assert!(
            !since.contains("Alice"),
            "since should exclude Alice: {since}"
        );
    }

    #[pg_test]
    fn transact_report_carries_tempids_and_basis() {
        setup();
        eval(
            "(mentat.store/transact (mentat.store/open) \
               [{:db/ident :person/name \
                 :db/valueType :db.type/string \
                 :db/cardinality :db.cardinality/one}])",
        );
        let report = eval(
            "(mentat.store/transact (mentat.store/open) \
               [{:db/id \"newbie\" :person/name \"Zoe\"}])",
        );
        assert!(report.contains(":mentat.store/tx-id"), "report: {report}");
        assert!(report.contains(":mentat.store/tempids"), "report: {report}");
        assert!(
            report.contains("\"newbie\""),
            "tempid resolved in report: {report}"
        );
        assert!(
            report.contains(":mentat.store/db-after"),
            "report: {report}"
        );
    }
}
