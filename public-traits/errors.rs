// Copyright 2016 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

#![allow(dead_code)]

use std; // To refer to std::result::Result.

use std::collections::BTreeSet;
use std::error::Error;

use rusqlite;
use uuid;

use edn;

use core_traits::{Attribute, ValueType};

use db_traits::errors::DbError;
use query_algebrizer_traits::errors::AlgebrizerError;
use query_projector_traits::errors::ProjectorError;
use query_pull_traits::errors::PullError;
use sql_traits::errors::SQLError;

#[cfg(feature = "syncable")]
use tolstoy_traits::errors::TolstoyError;

#[cfg(feature = "syncable")]
use hyper;

#[cfg(feature = "syncable")]
use serde_json;

pub type Result<T> = std::result::Result<T, MentatError>;

#[derive(Debug, thiserror::Error)]
pub enum MentatError {
    #[error("bad uuid {0}")]
    BadUuid(String),

    #[error("path {0} already exists")]
    PathAlreadyExists(String),

    #[error("variables {:?} unbound at query execution time", 0)]
    UnboundVariables(BTreeSet<String>),

    #[error("invalid argument name: '{0}'")]
    InvalidArgumentName(String),

    #[error("unknown attribute: '{0}'")]
    UnknownAttribute(String),

    #[error("invalid vocabulary version")]
    InvalidVocabularyVersion,

    #[error(
        "vocabulary {}/version {} already has attribute {}, and the requested definition differs",
        _0, 1, 2
    )]
    ConflictingAttributeDefinitions(String, u32, String, Attribute, Attribute),

    #[error(
        "existing vocabulary {} too new: wanted version {}, got version {}",
        _0, 1, 2
    )]
    ExistingVocabularyTooNew(String, u32, u32),

    #[error("core schema: wanted version {0}, got version {1:?}")]
    UnexpectedCoreSchema(u32, Option<u32>),

    #[error("Lost the transact() race!")]
    UnexpectedLostTransactRace,

    #[error("missing core attribute {0}")]
    MissingCoreVocabulary(edn::query::Keyword),

    #[error("schema changed since query was prepared")]
    PreparedQuerySchemaMismatch,

    #[error(
        "provided value of type {} doesn't match attribute value type {}",
        _0, 1
    )]
    ValueTypeMismatch(ValueType, ValueType),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    /// We're just not done yet.  Message that the feature is recognized but not yet
    /// implemented.
    #[error("not yet implemented: {0}")]
    NotYetImplemented(String),

    // It would be better to capture the underlying `rusqlite::Error`, but that type doesn't
    // implement many useful traits, including `Clone`, `Eq`, and `PartialEq`.
    #[error("SQL error: {0}, cause: {1}")]
    RusqliteError(String, String),

    #[error(transparent)]
    EdnParseError(#[from] edn::ParseError),

    #[error(transparent)]
    DbError(#[from] DbError),

    #[error(transparent)]
    AlgebrizerError(#[from] AlgebrizerError),

    #[error(transparent)]
    ProjectorError(#[from] ProjectorError),

    #[error(transparent)]
    PullError(#[from] PullError),

    #[error(transparent)]
    SQLError(#[from] SQLError),

    #[error(transparent)]
    UuidError(#[from] uuid::Error),

    #[cfg(feature = "syncable")]
    #[error(transparent)]
    TolstoyError(#[from] TolstoyError),

    #[cfg(feature = "syncable")]
    #[error(transparent)]
    NetworkError(#[from] hyper::Error),

    #[cfg(feature = "syncable")]
    #[error(transparent)]
    UriError(#[from] http::uri::InvalidUri),

    #[cfg(feature = "syncable")]
    #[error(transparent)]
    SerializationError(#[from] serde_json::Error),
}

impl From<rusqlite::Error> for MentatError {
    fn from(error: rusqlite::Error) -> Self {
        let cause = match error.source() {
            Some(e) => e.to_string(),
            None => "".to_string(),
        };
        MentatError::RusqliteError(error.to_string(), cause)
    }
}
