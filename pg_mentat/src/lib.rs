use pgrx::prelude::*;

pgrx::pg_module_magic!();

mod functions;
mod operators;
mod planner;
mod types;

pub use types::edn::EdnValue;

#[pg_schema]
mod mentat {
    use pgrx::prelude::*;

    /// Initialize the pg_mentat extension
    #[pg_extern]
    fn initialize_schema() -> Result<(), Box<dyn std::error::Error>> {
        Spi::run(
            r#"
            CREATE TABLE IF NOT EXISTS mentat_datoms (
                e BIGINT NOT NULL,
                a BIGINT NOT NULL,
                v mentat.EdnValue NOT NULL,
                tx BIGINT NOT NULL,
                added BOOLEAN NOT NULL DEFAULT TRUE
            );

            CREATE INDEX IF NOT EXISTS idx_mentat_eavt
                ON mentat_datoms (e, a, v, tx);
            CREATE INDEX IF NOT EXISTS idx_mentat_aevt
                ON mentat_datoms (a, e, v, tx);
            CREATE INDEX IF NOT EXISTS idx_mentat_avet
                ON mentat_datoms (a, v, e, tx);
            CREATE INDEX IF NOT EXISTS idx_mentat_vaet
                ON mentat_datoms (v, a, e, tx);
        "#,
        )?;
        Ok(())
    }
}

#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {
        // Initialize extension for testing
    }

    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![]
    }
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;

    #[pg_test]
    fn test_edn_roundtrip_boolean() {
        let result = Spi::get_one::<String>("SELECT mentat.edn_out(mentat.edn_in('true'))")
            .expect("Failed to execute query")
            .expect("Query returned NULL");
        assert!(result.contains("true"));
    }

    #[pg_test]
    fn test_edn_roundtrip_integer() {
        let result = Spi::get_one::<String>("SELECT mentat.edn_out(mentat.edn_in('42'))")
            .expect("Failed to execute query")
            .expect("Query returned NULL");
        assert!(result.contains("42"));
    }

    #[pg_test]
    fn test_edn_roundtrip_string() {
        let result =
            Spi::get_one::<String>("SELECT mentat.edn_out(mentat.edn_in('\"hello\"'))")
                .expect("Failed to execute query")
                .expect("Query returned NULL");
        assert!(result.contains("hello"));
    }

    #[pg_test]
    fn test_edn_roundtrip_vector() {
        let result = Spi::get_one::<String>("SELECT mentat.edn_out(mentat.edn_in('[1 2 3]'))")
            .expect("Failed to execute query")
            .expect("Query returned NULL");
        assert!(result.contains("1"));
    }

    #[pg_test]
    fn test_edn_roundtrip_map() {
        let result = Spi::get_one::<String>(
            "SELECT mentat.edn_out(mentat.edn_in('{:name \"Alice\" :age 30}'))",
        )
        .expect("Failed to execute query")
        .expect("Query returned NULL");
        assert!(result.contains("Alice"));
    }
}
