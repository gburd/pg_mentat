use crate::config::Config;
use crate::pool::{DbPool, PoolError};
use crate::protocol::{
    parser::{parse_request, ParseError},
    serializer::serialize_response,
    Anomaly, AnomalyCategory, Operation, Response, ResponseValue,
};
use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
    Router,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("Database pool error: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),
    #[error("Database query error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<ServerError> for Anomaly {
    fn from(err: ServerError) -> Self {
        match err {
            ServerError::Parse(e) => e.into(),
            ServerError::Pool(_) | ServerError::Database(_) => Self {
                category: AnomalyCategory::Unavailable,
                message: "Database unavailable".to_string(),
                db_error: Some("db.error/unavailable".to_string()),
            },
            ServerError::Internal(msg) => Self {
                category: AnomalyCategory::Fault,
                message: msg,
                db_error: Some("db.error/fault".to_string()),
            },
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pool: DbPool,
    config: Arc<Config>,
}

impl AppState {
    pub fn new(pool: DbPool, config: Config) -> Self {
        Self {
            pool,
            config: Arc::new(config),
        }
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", post(handle_request))
        .route("/health", get(health_check))
        .with_state(state)
}

async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "mentatd ready")
}

async fn handle_request(
    State(state): State<AppState>,
    body: String,
) -> Result<AxumResponse, StatusCode> {
    info!("Received request: {}", body);

    let response = match parse_request(&body) {
        Ok(request) => match execute_operation(request.op, &state).await {
            Ok(result) => Response::Success { result },
            Err(e) => {
                error!("Operation failed: {}", e);
                Response::Error {
                    anomaly: e.into(),
                }
            }
        },
        Err(e) => {
            error!("Parse failed: {}", e);
            Response::Error {
                anomaly: e.into(),
            }
        }
    };

    let edn_response = serialize_response(&response);
    info!("Sending response: {}", edn_response);

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/edn")],
        edn_response,
    )
        .into_response())
}

async fn execute_operation(
    op: Operation,
    state: &AppState,
) -> Result<ResponseValue, ServerError> {
    match op {
        Operation::Health => Ok(ResponseValue::String("healthy".to_string())),

        Operation::ListDatabases => {
            let client = state.pool.get().await?;

            let rows = client
                .query(
                    "SELECT datname FROM pg_database WHERE datistemplate = false",
                    &[],
                )
                .await?;

            let databases: Vec<String> = rows
                .iter()
                .map(|row| row.get::<_, String>(0))
                .collect();

            Ok(ResponseValue::List(databases))
        }

        Operation::CreateDatabase { db_name } => {
            if !is_valid_db_name(&db_name) {
                return Err(ServerError::Internal(
                    "Invalid database name".to_string(),
                ));
            }

            let client = state.pool.get().await?;

            client
                .execute(&format!("CREATE DATABASE {}", db_name), &[])
                .await?;

            Ok(ResponseValue::Boolean(true))
        }

        Operation::DeleteDatabase { db_name } => {
            if !is_valid_db_name(&db_name) {
                return Err(ServerError::Internal(
                    "Invalid database name".to_string(),
                ));
            }

            let client = state.pool.get().await?;

            client
                .execute(&format!("DROP DATABASE {}", db_name), &[])
                .await?;

            Ok(ResponseValue::Boolean(true))
        }

        Operation::Connect { db_name } => {
            let client = state.pool.get().await?;

            let exists = client
                .query_one(
                    "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)",
                    &[&db_name],
                )
                .await?
                .get::<_, bool>(0);

            if !exists {
                return Err(ServerError::Internal(format!(
                    "Database '{}' not found",
                    db_name
                )));
            }

            let connection_id = Uuid::new_v4().to_string();

            let mut result = BTreeMap::new();
            result.insert("connection-id".to_string(), connection_id);
            result.insert("db-name".to_string(), db_name);
            result.insert("status".to_string(), "connected".to_string());

            Ok(ResponseValue::Map(result))
        }

        Operation::Db { connection_id } => {
            let mut result = BTreeMap::new();
            result.insert("connection-id".to_string(), connection_id.to_string());
            result.insert("status".to_string(), "active".to_string());

            Ok(ResponseValue::Map(result))
        }

        Operation::Query { query, args, .. } => {
            info!("Executing query: {} with args: {:?}", query, args);

            let client = state.pool.get().await?;

            let row_count = client
                .query("SELECT 1", &[])
                .await?
                .len();

            let result = vec![format!("query-result-{}", row_count)];

            Ok(ResponseValue::List(result))
        }

        Operation::Transact { connection_id, tx_data } => {
            info!("Executing transaction: {} with data: {}", connection_id, tx_data);

            let mut result = BTreeMap::new();
            result.insert("tx-id".to_string(), "123".to_string());
            result.insert("status".to_string(), "committed".to_string());

            Ok(ResponseValue::Map(result))
        }
    }
}

fn is_valid_db_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.chars().next().map_or(false, |c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_db_name() {
        assert!(is_valid_db_name("test_db"));
        assert!(is_valid_db_name("mydb123"));
        assert!(!is_valid_db_name("123db"));
        assert!(!is_valid_db_name("my-db"));
        assert!(!is_valid_db_name(""));
    }
}
