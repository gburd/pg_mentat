// Nesting-depth tests at every SQL entry point that parses EDN text.
//
// Before 1.6.2, deeply nested EDN overflowed the parser's stack. An
// unprivileged role could run
//   SELECT mentat_query('[:find ?e :where [?e :a/b <3000 nested vectors>]]', '{}')
// and the backend died with SIGSEGV; the postmaster then terminated every
// server process and ran crash recovery. The `edn` crate now rejects input
// nested deeper than `edn::MAX_NESTING` before the grammar runs, and the `edn`
// type's own 100-level limit is checked before parsing instead of after.
//
// Each test hands an entry point 100,000-deep input. A regression crashes the
// backend, which fails the whole pg_test run.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    const DEPTH: usize = 100_000;

    fn nested(n: usize) -> String {
        format!("{}{}", "[".repeat(n), "]".repeat(n))
    }

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    /// Run `sql` (which must raise) inside a subtransaction and return the
    /// error message. Uses a plpgsql EXCEPTION block so the outer test
    /// transaction survives the error.
    fn error_of(sql: &str) -> String {
        create_error_helper();
        let escaped = sql.replace('\'', "''");
        Spi::get_one::<String>(&format!("SELECT mentat._test_error_of('{escaped}')"))
            .expect("spi")
            .unwrap_or_else(|| panic!("expected an error from: {}", &sql[..sql.len().min(120)]))
    }

    fn create_error_helper() {
        if Spi::get_one::<bool>("SELECT to_regprocedure('mentat._test_error_of(text)') IS NOT NULL")
            .expect("spi")
            == Some(true)
        {
            return;
        }
        Spi::run(
            "CREATE OR REPLACE FUNCTION mentat._test_error_of(stmt TEXT) RETURNS TEXT
             LANGUAGE plpgsql AS $$
             BEGIN
                 EXECUTE stmt;
                 RETURN NULL;
             EXCEPTION WHEN OTHERS THEN
                 RETURN SQLERRM;
             END;
             $$",
        )
        .expect("helper");
    }

    fn assert_rejected(sql: &str) {
        let msg = error_of(sql);
        assert!(
            msg.contains("nesting"),
            "expected a nesting-depth error, got: {msg}"
        );
    }

    #[pg_test]
    fn deep_query_is_an_error_not_a_crash() {
        setup();
        assert_rejected(&format!(
            "SELECT mentat_query('[:find ?e :where [?e :a/b {}]]', '{{}}'::jsonb)",
            nested(DEPTH)
        ));
        // The backend is still serving.
        assert_eq!(Spi::get_one::<i32>("SELECT 1").unwrap(), Some(1));
    }

    #[pg_test]
    fn deep_transaction_is_an_error_not_a_crash() {
        setup();
        assert_rejected(&format!(
            "SELECT mentat_transact('[[:db/add 1 :a/b {}]]')",
            nested(DEPTH)
        ));
        assert_rejected(&format!("SELECT mentat_transact('{}')", nested(DEPTH)));
    }

    #[pg_test]
    fn deep_pull_pattern_is_an_error_not_a_crash() {
        setup();
        assert_rejected(&format!("SELECT mentat_pull('{}', 1)", nested(DEPTH)));
        assert_rejected(&format!(
            "SELECT mentat_pull_many('{}', ARRAY[1::bigint])",
            nested(DEPTH)
        ));
    }

    #[pg_test]
    fn deep_edn_type_input_is_an_error_not_a_crash() {
        setup();
        assert_rejected(&format!("SELECT '{}'::mentat.edn", nested(DEPTH)));
        // The edn type's own, stricter limit is enforced before parsing too.
        assert_rejected(&format!("SELECT '{}'::mentat.edn", nested(101)));
        assert!(
            Spi::get_one::<String>(&format!("SELECT '{}'::mentat.edn::text", nested(100)))
                .unwrap()
                .is_some()
        );
    }

    #[pg_test]
    fn ordinary_queries_are_unaffected() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[{:db/ident :nest/name :db/valueType :db.type/string
                                      :db/cardinality :db.cardinality/one}]')",
        )
        .expect("schema");
        Spi::run("SELECT mentat_transact('[{:nest/name \"x\"}]')").expect("data");
        let r = Spi::get_one::<pgrx::JsonB>(
            "SELECT mentat_query('[:find ?n :where [?e :nest/name ?n]]', '{}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        assert!(r.0.to_string().contains('x'), "{}", r.0);
    }

    /// Every pg_test runs as a superuser, so nothing else exercises the
    /// ordinary-role path. Before 1.6.2, `mentat_query` set
    /// `temp_file_limit` (a superuser-only parameter) on every call, so every
    /// query by a non-superuser failed with "permission denied to set
    /// parameter". The limit is now applied only when the caller may set it.
    #[pg_test]
    fn ordinary_role_can_query() {
        setup();
        Spi::run(
            "SELECT mentat_transact('[{:db/ident :nest/role :db/valueType :db.type/string
                                      :db/cardinality :db.cardinality/one}]')",
        )
        .expect("schema");
        Spi::run("SELECT mentat_transact('[{:nest/role \"y\"}]')").expect("data");
        Spi::run(
            "DO $$ BEGIN
               IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'pgm_plain_role') THEN
                 CREATE ROLE pgm_plain_role NOSUPERUSER;
               END IF;
             END $$;
             GRANT USAGE ON SCHEMA mentat TO pgm_plain_role;
             GRANT SELECT ON ALL TABLES IN SCHEMA mentat TO pgm_plain_role;",
        )
        .expect("role");
        create_error_helper();
        Spi::run("GRANT EXECUTE ON FUNCTION mentat._test_error_of(text) TO pgm_plain_role")
            .expect("grant");
        Spi::run("SET LOCAL ROLE pgm_plain_role").expect("set role");
        let r = Spi::get_one::<pgrx::JsonB>(
            "SELECT mentat_query('[:find ?n . :where [?e :nest/role ?n]]', '{}'::jsonb)",
        )
        .expect("a non-superuser can run mentat_query")
        .unwrap();
        assert_eq!(r.0, serde_json::json!({ "result": "y" }));
        // And deep input is still an error, not a crash, for an ordinary role.
        assert_rejected(&format!(
            "SELECT mentat_query('[:find ?e :where [?e :a/b {}]]', '{{}}'::jsonb)",
            nested(DEPTH)
        ));
        Spi::run("RESET ROLE").expect("reset role");
    }
}
