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

    // -- Native SQL view access ------------------------------------------

    /// Query the facts view for human-readable EAVT data.
    ///
    /// Returns rows as JSON arrays. Optionally filter by entity_id and/or
    /// attribute ident.
    pub async fn facts(
        &self,
        entity_id: Option<i64>,
        attribute: Option<&str>,
        store: &str,
    ) -> Result<Vec<Value>, Box<dyn Error>> {
        let schema = Self::schema_for_store(store);
        let mut sql = format!(
            "SELECT row_to_json(f) FROM (SELECT entity_id, attribute, value, value_type, tx, tx_time FROM {}.facts",
            schema
        );
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
        let mut wheres = Vec::new();

        if let Some(eid) = entity_id {
            params.push(Box::new(eid));
            wheres.push(format!("entity_id = ${}", params.len()));
        }
        if let Some(attr) = attribute {
            params.push(Box::new(attr.to_string()));
            wheres.push(format!("attribute = ${}", params.len()));
        }
        if !wheres.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&wheres.join(" AND "));
        }
        sql.push_str(" ORDER BY entity_id, attribute) f");

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = self.client.query(&sql, &param_refs).await?;
        Ok(rows.iter().map(|r| r.get::<_, Value>(0)).collect())
    }

    /// Query entity_references view for relationship navigation.
    pub async fn entity_references(
        &self,
        source: Option<i64>,
        target: Option<i64>,
        store: &str,
    ) -> Result<Vec<Value>, Box<dyn Error>> {
        let schema = Self::schema_for_store(store);
        let mut sql = format!(
            "SELECT row_to_json(r) FROM (SELECT source_entity, attribute, target_entity, target_ident, tx FROM {}.entity_references",
            schema
        );
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
        let mut wheres = Vec::new();

        if let Some(s) = source {
            params.push(Box::new(s));
            wheres.push(format!("source_entity = ${}", params.len()));
        }
        if let Some(t) = target {
            params.push(Box::new(t));
            wheres.push(format!("target_entity = ${}", params.len()));
        }
        if !wheres.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&wheres.join(" AND "));
        }
        sql.push_str(") r");

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = self.client.query(&sql, &param_refs).await?;
        Ok(rows.iter().map(|r| r.get::<_, Value>(0)).collect())
    }

    /// Query entity_history view for temporal data.
    pub async fn entity_history(
        &self,
        entity_id: Option<i64>,
        store: &str,
    ) -> Result<Vec<Value>, Box<dyn Error>> {
        let schema = Self::schema_for_store(store);
        let mut sql = format!(
            "SELECT row_to_json(h) FROM (SELECT entity_id, attribute, value, value_type, tx, tx_time, operation FROM {}.entity_history",
            schema
        );
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();

        if let Some(eid) = entity_id {
            params.push(Box::new(eid));
            sql.push_str(&format!(" WHERE entity_id = ${}", params.len()));
        }
        sql.push_str(" ORDER BY tx DESC) h");

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = self.client.query(&sql, &param_refs).await?;
        Ok(rows.iter().map(|r| r.get::<_, Value>(0)).collect())
    }

    /// Query tx_log view for transaction history.
    pub async fn tx_log(
        &self,
        limit: i64,
        store: &str,
    ) -> Result<Vec<Value>, Box<dyn Error>> {
        let schema = Self::schema_for_store(store);
        let sql = format!(
            "SELECT row_to_json(t) FROM (SELECT tx, tx_time, datom_count FROM {}.tx_log ORDER BY tx DESC LIMIT $1) t",
            schema
        );
        let rows = self.client.query(&sql, &[&limit]).await?;
        Ok(rows.iter().map(|r| r.get::<_, Value>(0)).collect())
    }

    /// Find entities by attribute value using the lookup_entity function.
    pub async fn lookup_entity(
        &self,
        attribute: &str,
        value: &str,
        store: &str,
    ) -> Result<Vec<i64>, Box<dyn Error>> {
        let schema = Self::schema_for_store(store);
        let sql = format!(
            "SELECT entity_id FROM {}.lookup_entity($1, $2)",
            schema
        );
        let rows = self
            .client
            .query(&sql, &[&attribute, &value])
            .await?;
        Ok(rows.iter().map(|r| r.get::<_, i64>(0)).collect())
    }

    /// Get a single attribute value for an entity, returned as text.
    pub async fn entity_value(
        &self,
        entity_id: i64,
        attribute: &str,
        store: &str,
    ) -> Result<Option<String>, Box<dyn Error>> {
        let schema = Self::schema_for_store(store);
        let sql = format!("SELECT {}.entity_value($1, $2)", schema);
        let row = self
            .client
            .query_one(&sql, &[&entity_id, &attribute])
            .await?;
        Ok(row.get::<_, Option<String>>(0))
    }

    /// Full-text search across all text values.
    pub async fn find_text(
        &self,
        search_query: &str,
        store: &str,
    ) -> Result<Vec<Value>, Box<dyn Error>> {
        let schema = Self::schema_for_store(store);
        let sql = format!(
            "SELECT row_to_json(r) FROM (SELECT entity_id, attribute, value, rank FROM {}.find_text($1)) r",
            schema
        );
        let rows = self.client.query(&sql, &[&search_query]).await?;
        Ok(rows.iter().map(|r| r.get::<_, Value>(0)).collect())
    }

    /// Derive the PostgreSQL schema name for a store.
    fn schema_for_store(store: &str) -> String {
        if store == "default" {
            "mentat".to_string()
        } else {
            format!("mentat_{}", store)
        }
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
