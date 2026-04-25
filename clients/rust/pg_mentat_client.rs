//! pg_mentat Rust client -- Direct PostgreSQL access.
//!
//! No mentatd daemon required. Connect directly to PostgreSQL and call
//! the pg_mentat extension functions via standard SQL.
//!
//! # Dependencies (Cargo.toml)
//!
//! ```toml
//! [dependencies]
//! tokio-postgres = "0.7"
//! serde_json = "1"
//! tokio = { version = "1", features = ["full"] }
//! deadpool-postgres = "0.14"  # optional, for connection pooling
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use pg_mentat_client::MentatClient;
//!
//! #[tokio::main]
//! async fn main() {
//!     let client = MentatClient::connect("host=localhost dbname=postgres").await.unwrap();
//!     client.transact(r#"[{:db/ident :person/name ...}]"#).await.unwrap();
//!     let results = client.query("[:find ?name :where [?e :person/name ?name]]", None).await.unwrap();
//! }
//! ```

use serde_json::Value;
use std::error::Error;
use std::fmt;
use tokio_postgres::{Client, NoTls};

/// Error from a pg_mentat SQL function call.
#[derive(Debug)]
pub struct MentatError {
    pub message: String,
}

impl fmt::Display for MentatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pg_mentat: {}", self.message)
    }
}

impl Error for MentatError {}

/// Direct PostgreSQL client for pg_mentat.
///
/// Calls mentat_transact(), mentat_query(), mentat_pull(), mentat_entity(),
/// and mentat_schema() as SQL functions -- no HTTP daemon needed.
pub struct MentatClient {
    client: Client,
    // The connection task handle; must be kept alive.
    _handle: tokio::task::JoinHandle<()>,
}

impl MentatClient {
    /// Connect to PostgreSQL.
    ///
    /// `config` is a libpq-style connection string, e.g.
    /// `"host=localhost dbname=postgres"`.
    pub async fn connect(config: &str) -> Result<Self, Box<dyn Error>> {
        let (client, connection) = tokio_postgres::connect(config, NoTls).await?;
        let handle = tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("pg_mentat: connection error: {}", e);
            }
        });
        Ok(Self {
            client,
            _handle: handle,
        })
    }

    /// Execute an EDN transaction. Returns the transaction report as JSON.
    pub async fn transact(&self, edn_tx: &str) -> Result<Value, Box<dyn Error>> {
        let row = self
            .client
            .query_one("SELECT mentat_transact($1)", &[&edn_tx])
            .await?;
        let result: String = row.get(0);
        Ok(serde_json::from_str(&result)?)
    }

    /// Execute a Datalog query.
    pub async fn query(
        &self,
        datalog: &str,
        inputs: Option<&Value>,
    ) -> Result<Value, Box<dyn Error>> {
        let empty = serde_json::json!({});
        let inputs_json = serde_json::to_string(inputs.unwrap_or(&empty))?;
        let row = self
            .client
            .query_one(
                "SELECT mentat_query($1, $2::jsonb)",
                &[&datalog, &inputs_json],
            )
            .await?;
        let result: Value = row.get(0);
        Ok(result)
    }

    /// Pull attributes for an entity.
    pub async fn pull(
        &self,
        pattern: &str,
        entity_id: i64,
    ) -> Result<Value, Box<dyn Error>> {
        let row = self
            .client
            .query_one("SELECT mentat_pull($1, $2)", &[&pattern, &entity_id])
            .await?;
        let result: Value = row.get(0);
        Ok(result)
    }

    /// Pull attributes for multiple entities.
    pub async fn pull_many(
        &self,
        pattern: &str,
        entity_ids: &[i64],
    ) -> Result<Value, Box<dyn Error>> {
        let row = self
            .client
            .query_one(
                "SELECT mentat_pull_many($1, $2)",
                &[&pattern, &entity_ids],
            )
            .await?;
        let result: Value = row.get(0);
        Ok(result)
    }

    /// Get all attributes of an entity.
    pub async fn entity(&self, entity_id: i64) -> Result<Value, Box<dyn Error>> {
        let row = self
            .client
            .query_one("SELECT mentat_entity($1)", &[&entity_id])
            .await?;
        let result: Value = row.get(0);
        Ok(result)
    }

    /// Return the current schema.
    pub async fn schema(&self) -> Result<Value, Box<dyn Error>> {
        let row = self.client.query_one("SELECT mentat_schema()", &[]).await?;
        let result: Value = row.get(0);
        Ok(result)
    }

    /// Return the query execution plan.
    pub async fn explain(
        &self,
        datalog: &str,
        inputs: Option<&Value>,
    ) -> Result<Value, Box<dyn Error>> {
        let empty = serde_json::json!({});
        let inputs_json = serde_json::to_string(inputs.unwrap_or(&empty))?;
        let row = self
            .client
            .query_one(
                "SELECT mentat_explain($1, $2::jsonb)",
                &[&datalog, &inputs_json],
            )
            .await?;
        let result: Value = row.get(0);
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Example usage
// ---------------------------------------------------------------------------
#[cfg(feature = "example")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let dsn = std::env::var("PG_MENTAT_DSN")
        .unwrap_or_else(|_| "host=localhost dbname=postgres".into());

    let client = MentatClient::connect(&dsn).await?;

    // Define schema
    client
        .transact(
            r#"[
      {:db/ident :person/name
       :db/valueType :db.type/string
       :db/cardinality :db.cardinality/one}
      {:db/ident :person/email
       :db/valueType :db.type/string
       :db/cardinality :db.cardinality/one
       :db/unique :db.unique/identity}
    ]"#,
        )
        .await?;

    // Transact data
    client
        .transact(
            r#"[
      {:person/name "Alice" :person/email "alice@example.com"}
      {:person/name "Bob"   :person/email "bob@example.com"}
    ]"#,
        )
        .await?;

    // Query
    let results = client
        .query(
            r#"[:find ?name ?email
                :where
                [?e :person/name ?name]
                [?e :person/email ?email]]"#,
            None,
        )
        .await?;
    println!("Query results: {}", serde_json::to_string_pretty(&results)?);

    // Schema
    let schema = client.schema().await?;
    println!("Schema: {}", serde_json::to_string_pretty(&schema)?);

    Ok(())
}
