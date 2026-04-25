pub mod parser;
pub mod serializer;
pub mod transit_parser;
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

    // Database snapshot for batch queries
    DbSnapshot,

    // Query
    Query {
        query: String,
        args: Vec<String>,
        timeout: Option<u64>,
        limit: Option<usize>,
        offset: Option<usize>,
        db_id: Option<String>,  // Optional db snapshot for batch queries
    },

    // Transaction
    Transact {
        connection_id: String,
        tx_data: String,
    },

    // Pull API
    Pull {
        pattern: String,
        entity_id: i64,
    },

    // Index access
    Datoms {
        index: DatomsIndex,
        components: Vec<String>,
    },

    // Time-travel queries
    AsOf {
        query: String,
        args: Vec<String>,
        t: i64,
    },
    Since {
        query: String,
        args: Vec<String>,
        t: i64,
    },
    History {
        query: String,
        args: Vec<String>,
    },

    // Transaction log
    TxRange {
        start: Option<i64>,
        end: Option<i64>,
    },

    // Speculative transaction (d/with)
    With {
        tx_data: String,
    },

    // Database filtering (d/filter)
    Filter {
        predicate: FilterPredicate,
        query: String,
        args: Vec<String>,
    },

    // Basis timestamp (d/basis-t)
    BasisT,

    // Health check
    Health,
}

/// Predicate for d/filter operations.
///
/// Filters restrict the datoms visible to subsequent queries by applying
/// a WHERE-clause predicate. Supported predicates:
///   - AttrEquals: only datoms with a specific attribute
///   - EntityEquals: only datoms for a specific entity
///   - Since: only datoms from transactions after a given t
///   - Custom: arbitrary SQL predicate expression (validated)
#[derive(Debug, Clone)]
pub enum FilterPredicate {
    AttrEquals(String),
    EntityEquals(i64),
    Since(i64),
    Custom(String),
}

#[derive(Debug, Clone, Copy)]
pub enum DatomsIndex {
    EAVT,
    AEVT,
    AVET,
    VAET,
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
    Float(f64),
    Keyword(String),
    /// An instant in time, stored as microseconds since Unix epoch.
    /// Serialized as `#inst "ISO-8601"` in EDN and `~m<millis>` in Transit.
    Instant(i64),
    /// A UUID value. Serialized as `#uuid "..."` in EDN and `~u...` in Transit.
    Uuid(String),
    List(Vec<ResponseValue>),
    Vector(Vec<ResponseValue>),
    Map(Vec<(ResponseValue, ResponseValue)>),
    DbSnapshot { db_id: String, basis_t: i64 },
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
