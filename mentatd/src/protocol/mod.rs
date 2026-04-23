pub mod parser;
pub mod serializer;
pub mod transit_serializer;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Request {
    pub op: Operation,
}

#[derive(Debug, Clone)]
pub enum Operation {
    // Database management
    ListDatabases,
    CreateDatabase {
        db_name: String,
    },
    DeleteDatabase {
        db_name: String,
    },

    // Connection
    Connect {
        db_name: String,
    },
    Db {
        connection_id: Uuid,
    },

    // Query
    Query {
        query: String,
        args: Vec<String>,
        timeout: Option<u64>,
        limit: Option<usize>,
        offset: Option<usize>,
    },

    // Transaction
    Transact {
        connection_id: String,
        tx_data: String,
    },

    // Health check
    Health,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbId {
    pub database_id: String,
    pub t: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_t: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<i64>,
    #[serde(default)]
    pub history: bool,
}

#[derive(Debug, Clone)]
pub enum Response {
    Success { result: ResponseValue },
    Error { anomaly: Anomaly },
}

#[derive(Debug, Clone)]
pub enum ResponseValue {
    Nil,
    String(String),
    Boolean(bool),
    Integer(i64),
    Keyword(String),
    List(Vec<ResponseValue>),
    Vector(Vec<ResponseValue>),
    Map(Vec<(ResponseValue, ResponseValue)>),
}

#[derive(Debug, Clone)]
pub struct Anomaly {
    pub category: AnomalyCategory,
    pub message: String,
    pub db_error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum AnomalyCategory {
    Incorrect,
    Forbidden,
    NotFound,
    Unavailable,
    Interrupted,
    Fault,
}

impl AnomalyCategory {
    pub fn as_keyword(&self) -> &'static str {
        match self {
            Self::Incorrect => ":cognitect.anomalies/incorrect",
            Self::Forbidden => ":cognitect.anomalies/forbidden",
            Self::NotFound => ":cognitect.anomalies/not-found",
            Self::Unavailable => ":cognitect.anomalies/unavailable",
            Self::Interrupted => ":cognitect.anomalies/interrupted",
            Self::Fault => ":cognitect.anomalies/fault",
        }
    }
}
